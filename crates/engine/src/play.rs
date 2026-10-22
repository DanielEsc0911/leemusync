//! Launcher mode, `leemusync play` (sync-model §Launcher mode): pull, lease, launch, renew while
//! the emulator runs, then quiet period, snapshot, upload, release.
//!
//! The emulator's blocking `wait` runs on `spawn_blocking` while this task keeps `&mut Engine`
//! and renews the lease on a tokio interval. A failed renewal is a warning and is retried; it
//! never touches the game. The lease is released on every path after it was taken (D6: a lease
//! left behind by a crash only goes stale). A failed upload after exit leaves the files and the
//! stream's base untouched, so the next sync sees them as changed and uploads them (B3).

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use leemusync_api::{
    API_VERSION, ConflictInfo, PlayEvent, PlayLease, PlayReport, StreamAction, StreamReport,
    Warning, WarningCode,
};
use leemusync_core::VersionId;
use leemusync_core::lease::RENEW_MS;

use crate::lease::{Acquire, LeaseHandle};
use crate::process::Launcher;
use crate::query::Target;
use crate::{Engine, EngineError, fsx, now_ms};

/// The player's answer when another device holds the stream's lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseChoice {
    /// Poll every 10 s until the lease is free.
    Wait,
    /// Play without the lease; a divergence becomes a conflict, never a loss (D6).
    PlayAnyway,
    /// Take over a stale lease. Refused for a live one.
    TakeOver,
}

/// Questions `play` asks the player. The CLI answers them; tests script them.
/// An `Err` (e.g. `LeaseHeld`, `NeedsDecision` when there is no terminal) stops `play` before launch.
pub trait PlayPrompt: Send + Sync {
    /// `by` holds the lease, last renewed `renewed_ago_s` ago; `stale` once it expired.
    ///
    /// # Errors
    /// Whatever stops `play`, returned as is.
    fn lease_held(
        &self,
        by: &str,
        renewed_ago_s: u64,
        stale: bool,
    ) -> Result<LeaseChoice, EngineError>;
    /// The stream has a conflict: the head to keep, or `None` to play this device's copy.
    ///
    /// # Errors
    /// Whatever stops `play`, returned as is.
    fn conflict(&self, c: &ConflictInfo) -> Result<Option<VersionId>, EngineError>;
    /// Live status for each step; ignored by default.
    fn event(&self, _e: PlayEvent) {}
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Timings {
    renew: Duration,
    poll: Duration,
    quiet: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            renew: Duration::from_millis(RENEW_MS.unsigned_abs()),
            poll: Duration::from_secs(10),
            quiet: Duration::from_secs(10),
        }
    }
}

/// Newest modification time under `dir` in Unix ms (symlinks are not followed).
fn newest_mtime(dir: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| {
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            let own = fsx::mtime_ms(&m);
            Some(if m.is_dir() {
                own.max(newest_mtime(&e.path()))
            } else {
                own
            })
        })
        .max()
        .unwrap_or(0)
}

/// Waits until no file under `dir` changed for `quiet` (sync-model §Snapshot rules).
async fn wait_quiet(dir: &Path, quiet: Duration) {
    let quiet_ms = i64::try_from(quiet.as_millis()).unwrap_or(i64::MAX);
    loop {
        let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
        let left = quiet_ms.saturating_sub(now.saturating_sub(newest_mtime(dir)));
        if left <= 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(left.unsigned_abs())).await;
    }
}

fn is_conflict(r: &StreamReport) -> bool {
    matches!(
        r.action,
        StreamAction::Conflict | StreamAction::UploadThenConflict
    )
}

impl Engine {
    /// Test-only: shortens the renew, lease-poll and quiet-period intervals of `play`.
    #[cfg(any(test, feature = "failpoints"))]
    pub fn set_play_timings(&mut self, renew: Duration, poll: Duration, quiet: Duration) {
        self.play_timings = Timings { renew, poll, quiet };
    }

    /// Launcher mode: resolve the stream, pull it (a conflict asks `prompt`), take the lease
    /// (held elsewhere asks `prompt`), launch `launch_argv` + `extra`, renew while it runs,
    /// then wait the quiet period, snapshot, upload and release.
    ///
    /// # Errors
    /// `EmulatorRunning` if the emulator is already open (I3); `Usage` for an unknown player,
    /// a mapping or install that is missing, or `TakeOver` of a live lease; launch, storage
    /// and file errors. The lease, once taken, is released before any error returns.
    #[allow(clippy::too_many_lines)] // the launcher-mode steps in order, each a few lines
    pub async fn play(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
        extra: &[OsString],
        prompt: &dyn PlayPrompt,
        launcher: &dyn Launcher,
    ) -> Result<PlayReport, EngineError> {
        let Target {
            stream,
            player: pid,
            account,
            profile,
            folder,
            names,
            ..
        } = self.target(player, emulator, game).await?;
        let game = game.to_lowercase();
        if self.env.processes.running(&names) {
            return Err(EngineError::EmulatorRunning(profile.name.clone()));
        }
        let mut argv = self.launch_argv(emulator, &game, &account)?;
        argv.extend(extra.iter().cloned());
        let mut warnings = Vec::new();

        prompt.event(PlayEvent::Pulling);
        let mut pulled = self.sync_one(pid, emulator, &game, &mut warnings).await?;
        if is_conflict(&pulled) {
            let (conflicts, w) = self.conflicts().await?;
            warnings.extend(w);
            let ours = conflicts
                .iter()
                .find(|c| c.player == player && c.emulator == emulator && c.game == game);
            let keep = match ours {
                Some(c) => prompt.conflict(c)?,
                None => None,
            };
            if let Some(keep) = keep {
                let (r, w) = self.resolve(player, emulator, &game, keep).await?;
                warnings.extend(w);
                pulled = r;
            }
        }

        let mut takeover = false;
        let (handle, lease) = loop {
            let (got, w) = self.lease_acquire(stream, pid, takeover).await?;
            warnings.extend(w);
            match got {
                Acquire::Ours(h) => {
                    break (
                        Some(h),
                        if takeover {
                            PlayLease::TookOver
                        } else {
                            PlayLease::Held
                        },
                    );
                }
                Acquire::HeldElsewhere {
                    device_name,
                    renewed_at_ms,
                    stale,
                    ..
                } => {
                    let ago = self.lease_now().saturating_sub(renewed_at_ms).max(0) / 1000;
                    match prompt.lease_held(&device_name, ago.unsigned_abs(), stale)? {
                        LeaseChoice::Wait => {
                            prompt.event(PlayEvent::WaitingForLease);
                            tokio::time::sleep(self.play_timings.poll).await;
                        }
                        LeaseChoice::PlayAnyway => break (None, PlayLease::PlayedAnyway),
                        LeaseChoice::TakeOver if stale => takeover = true,
                        LeaseChoice::TakeOver => {
                            return Err(EngineError::Usage("lease_live".into()));
                        }
                    }
                }
            }
        };

        let session = self
            .session(
                handle.clone(),
                &argv,
                &names,
                &folder,
                launcher,
                prompt,
                &mut warnings,
            )
            .await;
        if let Some(h) = handle
            && let Err(e) = self.lease_release(h).await
        {
            warnings.push(Warning {
                code: WarningCode::LeaseReleaseFailed,
                detail: e.code().to_owned(),
            });
        }
        let (exit_code, renewals) = session?;
        let uploaded = self.sync_one(pid, emulator, &game, &mut warnings).await?;
        Ok(PlayReport {
            api_version: API_VERSION,
            pulled,
            exit_code,
            uploaded,
            lease,
            renewals,
            warnings,
        })
    }

    /// Steps 4–6 up to the quiet period: launch, renew until exit, wait for quiet files.
    /// Returns the exit code and the number of renewals.
    #[allow(clippy::too_many_arguments)] // one session's inputs, passed once
    async fn session(
        &mut self,
        mut handle: Option<LeaseHandle>,
        argv: &[OsString],
        names: &[String],
        folder: &Path,
        launcher: &dyn Launcher,
        prompt: &dyn PlayPrompt,
        warnings: &mut Vec<Warning>,
    ) -> Result<(i32, u32), EngineError> {
        // Re-checked after the pull and the lease wait (I3).
        if self.env.processes.running(names) {
            return Err(EngineError::EmulatorRunning(
                names.first().cloned().unwrap_or_default(),
            ));
        }
        let mut child = launcher.spawn(argv)?;
        prompt.event(PlayEvent::Playing);
        let mut wait = tokio::task::spawn_blocking(move || child.wait());
        let renew = self.play_timings.renew;
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + renew, renew);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut renewals = 0u32;
        let exit = loop {
            tokio::select! {
                res = &mut wait => break res,
                _ = ticker.tick() => {
                    if let Some(h) = handle.as_mut() {
                        match self.lease_renew(h).await {
                            Ok(()) => renewals += 1,
                            Err(e) => warnings.push(Warning {
                                code: WarningCode::LeaseRenewFailed,
                                detail: e.code().to_owned(),
                            }),
                        }
                    }
                }
            }
        };
        let code = exit.map_err(|e| EngineError::Io(e.to_string()))??;
        prompt.event(PlayEvent::Uploading);
        wait_quiet(folder, self.play_timings.quiet).await;
        Ok((code, renewals))
    }
}
