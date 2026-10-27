//! Advisory per-stream leases: acquire, renew, release, status (sync-model §Leases).
//!
//! Leases are advisory (D6). Each device writes only its own `locks/<stream>/<device>` object.
//! A lease that doesn't decrypt or parse is ignored with a warning and never trusted (B2); any
//! race leases miss still ends as a conflict, never a lost save.

use lemusync_api::{LeaseInfo, Warning, WarningCode};
use lemusync_core::lease::{self, Lease, Liveness, Observed, SKEW_MS};
use lemusync_core::{DeviceId, PlayerId, SessionId, StreamId};

use crate::repo::{ListedLease, Repo};
use crate::{Engine, EngineError, new_version_id, now_ms, repo_config};

/// Our lease on one stream, as `lease_acquire` returned it.
#[derive(Clone, Debug)]
pub struct LeaseHandle {
    stream: StreamId,
    lease: Lease,
}

impl LeaseHandle {
    #[must_use]
    pub fn stream(&self) -> StreamId {
        self.stream
    }

    #[must_use]
    pub fn session(&self) -> SessionId {
        self.lease.session
    }
}

#[derive(Clone, Debug)]
pub enum Acquire {
    Ours(LeaseHandle),
    /// Another device holds the stream. `stale`: its lease expired; `takeover` may take it.
    HeldElsewhere {
        device: DeviceId,
        device_name: String,
        renewed_at_ms: i64,
        stale: bool,
    },
}

struct Seen {
    observed: Observed,
    live: bool,
}

fn observe(listed: Vec<ListedLease>, now: i64, warnings: &mut Vec<Warning>) -> Vec<Seen> {
    listed
        .into_iter()
        .filter_map(|l| {
            let Some(lease) = l.lease else {
                warnings.push(Warning {
                    code: WarningCode::LeaseUnreadable,
                    detail: l.device.to_string(),
                });
                return None;
            };
            let observed = Observed {
                lease,
                server_modified_ms: l.modified_ms,
            };
            let live = lease::liveness(&observed, now, SKEW_MS) == Liveness::Live;
            Some(Seen { observed, live })
        })
        .collect()
}

async fn held(repo: &Repo, o: &Observed, stale: bool) -> Acquire {
    let device = o.lease.device;
    Acquire::HeldElsewhere {
        device,
        device_name: repo
            .device_name(&device)
            .await
            .unwrap_or_else(|| device.to_string()),
        renewed_at_ms: o.lease.renewed_at_ms,
        stale,
    }
}

impl Engine {
    /// Device clock in Unix ms, shifted in tests.
    pub(crate) fn lease_now(&self) -> i64 {
        i64::try_from(now_ms())
            .unwrap_or(i64::MAX)
            .saturating_add(self.clock_shift_ms)
    }

    /// Test-only: moves this engine's lease clock by `ms`.
    #[cfg(any(test, feature = "failpoints"))]
    pub fn shift_lease_clock_ms(&mut self, ms: i64) {
        self.clock_shift_ms = ms;
    }

    /// Test-only: pauses `lease_acquire` once at `name` until `barrier` is full.
    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm_pause(&self, name: &'static str, barrier: std::sync::Arc<tokio::sync::Barrier>) {
        self.failpoints.arm_pause(name, barrier);
    }

    async fn leases_of(
        &self,
        stream: &StreamId,
        warnings: &mut Vec<Warning>,
    ) -> Result<Vec<Seen>, EngineError> {
        let listed = self.repo_ref()?.list_leases(stream).await?;
        Ok(observe(listed, self.lease_now(), warnings))
    }

    /// Lists leases; another live one → `HeldElsewhere`. A stale one → `HeldElsewhere { stale }`
    /// unless `takeover`. Otherwise writes ours and lists again: of all live leases the earliest
    /// `(acquired_at, device id)` wins, and a loser deletes its own lease. A takeover is recorded
    /// in the stream's next version (`meta.takeover_from`).
    ///
    /// # Errors
    /// Storage errors; `NotSetUp` without a repo.
    pub async fn lease_acquire(
        &mut self,
        stream: StreamId,
        player: PlayerId,
        takeover: bool,
    ) -> Result<(Acquire, Vec<Warning>), EngineError> {
        self.ensure_repo().await?;
        let me = self.device_id();
        let mut warnings = Vec::new();
        let others: Vec<Seen> = self
            .leases_of(&stream, &mut warnings)
            .await?
            .into_iter()
            .filter(|s| s.observed.lease.device != me)
            .collect();
        let repo = self.repo_ref()?;
        if let Some(live) = others.iter().find(|s| s.live) {
            return Ok((held(repo, &live.observed, false).await, warnings));
        }
        let stale = others
            .iter()
            .max_by_key(|s| s.observed.lease.renewed_at_ms)
            .map(|s| &s.observed);
        if let (Some(o), false) = (stale, takeover) {
            return Ok((held(repo, o, true).await, warnings));
        }
        let taken_from = stale.map(|o| o.lease.device);
        let now = self.lease_now();
        let ours = Lease {
            format: lease::FORMAT,
            device: me,
            player,
            session: SessionId(new_version_id()?),
            acquired_at_ms: now,
            renewed_at_ms: now,
            ttl_ms: self.config.leases.ttl_ms(),
        };
        repo.put_lease(&stream, &ours).await?;
        self.failpoints.hit("lease.after_write")?;
        self.failpoints.pause("lease.after_write").await;
        let mut again = Vec::new();
        let live: Vec<Observed> = self
            .leases_of(&stream, &mut again)
            .await?
            .into_iter()
            .filter(|s| s.live)
            .map(|s| s.observed)
            .collect();
        let repo = self.repo_ref()?;
        match lease::winner(&live) {
            Some(w) if w != me => {
                repo.delete_lease(&stream, &me).await?;
                let o = live.iter().find(|o| o.lease.device == w);
                let out = match o {
                    Some(o) => held(repo, o, false).await,
                    None => return Err(EngineError::Data("lease_winner".into())),
                };
                Ok((out, warnings))
            }
            _ => {
                if let Some(d) = taken_from
                    && let Ok(mut t) = self.takeovers.lock()
                {
                    t.insert(stream, d);
                }
                Ok((
                    Acquire::Ours(LeaseHandle {
                        stream,
                        lease: ours,
                    }),
                    warnings,
                ))
            }
        }
    }

    /// Rewrites our lease with a new `renewed_at` (every `lease::RENEW_MS`).
    ///
    /// # Errors
    /// Storage errors.
    pub async fn lease_renew(&mut self, h: &mut LeaseHandle) -> Result<(), EngineError> {
        self.ensure_repo().await?;
        h.lease.renewed_at_ms = self.lease_now();
        self.repo_ref()?.put_lease(&h.stream, &h.lease).await
    }

    /// Deletes our lease; never touches another device's.
    ///
    /// # Errors
    /// Storage errors.
    pub async fn lease_release(&mut self, h: LeaseHandle) -> Result<(), EngineError> {
        self.lease_release_forced(h.stream).await
    }

    /// Deletes this device's lease on `stream`, e.g. one a crash left behind.
    ///
    /// # Errors
    /// Storage errors.
    pub async fn lease_release_forced(&mut self, stream: StreamId) -> Result<(), EngineError> {
        self.ensure_repo().await?;
        let me = self.device_id();
        self.repo_ref()?.delete_lease(&stream, &me).await
    }

    /// Deletes this device's leases on every stream of `game` (`lease release --force`).
    /// Returns how many were deleted; other devices' leases are never touched.
    ///
    /// # Errors
    /// Storage errors; `NotSetUp` without a repo.
    pub async fn lease_release_game(&mut self, game: &str) -> Result<u32, EngineError> {
        self.ensure_repo().await?;
        let game = game.to_lowercase();
        let me = self.device_id();
        let config = repo_config::load(self.repo_ref()?, &self.state)
            .await?
            .config;
        let mut warnings = Vec::new();
        let mut released = 0;
        for pid in config.players.keys() {
            for (emu, g) in config.games_of(*pid) {
                if g != game {
                    continue;
                }
                let stream = self.repo_ref()?.stream_id(*pid, &emu, &g);
                let ours = self
                    .leases_of(&stream, &mut warnings)
                    .await?
                    .iter()
                    .any(|s| s.observed.lease.device == me);
                if ours {
                    self.repo_ref()?.delete_lease(&stream, &me).await?;
                    released += 1;
                }
            }
        }
        Ok(released)
    }

    /// Every lease on every stream of the repo config.
    ///
    /// # Errors
    /// Storage errors; `NotSetUp` without a repo.
    pub async fn lease_status(&mut self) -> Result<(Vec<LeaseInfo>, Vec<Warning>), EngineError> {
        self.ensure_repo().await?;
        let me = self.device_id();
        let config = repo_config::load(self.repo_ref()?, &self.state)
            .await?
            .config;
        let mut out = Vec::new();
        let mut warnings = Vec::new();
        for (pid, p) in &config.players {
            for (emu, game) in config.games_of(*pid) {
                let stream = self.repo_ref()?.stream_id(*pid, &emu, &game);
                for s in self.leases_of(&stream, &mut warnings).await? {
                    let l = &s.observed.lease;
                    out.push(LeaseInfo {
                        player: p.name.clone(),
                        emulator: emu.clone(),
                        game: game.clone(),
                        device_id: l.device.to_string(),
                        device: self
                            .repo_ref()?
                            .device_name(&l.device)
                            .await
                            .unwrap_or_else(|| l.device.to_string()),
                        renewed_at_ms: l.renewed_at_ms,
                        live: s.live,
                        ours: l.device == me,
                    });
                }
            }
        }
        Ok((out, warnings))
    }

    /// The device whose stale lease we took over on `stream`, consumed by the next upload.
    pub(crate) fn pending_takeover(&self, stream: &StreamId) -> Option<DeviceId> {
        self.takeovers.lock().ok()?.get(stream).copied()
    }

    pub(crate) fn clear_takeover(&self, stream: &StreamId) {
        if let Ok(mut t) = self.takeovers.lock() {
            t.remove(stream);
        }
    }
}
