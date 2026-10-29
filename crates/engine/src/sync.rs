//! `lemusync sync`: reconcile every stream of every granted install (sync-model §Reconciliation).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use lemusync_api::{API_VERSION, StreamAction, StreamReport, SyncReport, Warning, WarningCode};
use lemusync_core::config::RepoConfig;
use lemusync_core::heads::{Graph, heads};
use lemusync_core::manifest::{FileEntry, Version};
use lemusync_core::profile::Profile;
use lemusync_core::reconcile::{Action, Input, Local, newest_head, reconcile};
use lemusync_core::{DeviceId, PlayerId, StreamId, VersionId};

use crate::discover::{Discovery, save_folder};
use crate::profiles::Install;
use crate::progress::Meter;
use crate::resolve::Resolution;
use crate::restore::{self, Ctx, Source};
use crate::snapshot::{self, Quick};
use crate::upload::{UploadMeta, upload};
use crate::{
    ConflictPolicy, Engine, EngineError, StreamRow, fsx, history_dir, now_ms, repo_config,
};

/// A granted install with the `(player, game)` keys `sync` covers for it.
pub(crate) type SyncedInstall = (Profile, Install, BTreeSet<(PlayerId, String)>);

pub struct SyncOptions {
    pub dry_run: bool,
}

pub(crate) const FIRST_SYNC: &str = "first_sync_confirmed";

pub(crate) fn action_kind(a: &Action) -> StreamAction {
    match a {
        Action::Nothing => StreamAction::Nothing,
        Action::Adopt(_) => StreamAction::Adopt,
        Action::FastForward(_) => StreamAction::FastForward,
        Action::Upload { .. } => StreamAction::Upload,
        Action::UploadThenConflict { .. } => StreamAction::UploadThenConflict,
        Action::Conflict(_) => StreamAction::Conflict,
        Action::LocalMissing => StreamAction::LocalMissing,
        Action::RemoteBehind => StreamAction::RemoteBehind,
    }
}

pub(crate) fn process_names(profile: &Profile) -> Vec<String> {
    profile
        .platforms
        .get("linux")
        .map(|p| p.process.clone())
        .unwrap_or_default()
}

/// Restored files with their on-disk mtimes, so the next quick check sees "same as base".
pub(crate) fn on_disk(folder: &Path, files: &[FileEntry]) -> Vec<FileEntry> {
    files
        .iter()
        .map(|f| {
            let mtime = std::fs::symlink_metadata(folder.join(f.path.as_str()))
                .map_or(f.mtime, |m| fsx::mtime_ms(&m));
            FileEntry { mtime, ..f.clone() }
        })
        .collect()
}

/// Every `(player, game)` to sync for one install: the repo's games for this emulator, plus
/// every non-orphaned local save folder whose account is mapped to a player on this device.
fn stream_keys(
    config: &RepoConfig,
    device: DeviceId,
    emulator: &str,
    discovery: &Discovery,
) -> BTreeSet<(PlayerId, String)> {
    let mut keys = BTreeSet::new();
    for player in config.players.keys() {
        for (emu, game) in config.games_of(*player) {
            if emu == emulator {
                keys.insert((*player, game));
            }
        }
    }
    for save in discovery.saves.iter().filter(|s| !s.orphaned) {
        if let Some(p) = config.player_on_account(device, emulator, &save.account) {
            keys.insert((p, save.game.clone()));
        }
    }
    keys
}

/// The report for a stream left alone because its emulator is running (I3).
fn skipped(
    config: &RepoConfig,
    device: DeviceId,
    player: PlayerId,
    emulator: &str,
    game: &str,
) -> StreamReport {
    StreamReport {
        player: config
            .players
            .get(&player)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        emulator: emulator.to_owned(),
        game: game.to_owned(),
        account: config
            .account_of(device, player, emulator)
            .map(str::to_owned),
        action: StreamAction::Skipped,
        version: None,
        heads: vec![],
        done: false,
        files: 0,
        bytes: 0,
    }
}

impl Engine {
    /// # Errors
    /// `Io` if the state DB can't be written.
    pub fn confirm_first_sync(&self) -> Result<(), EngineError> {
        self.state.set_meta(FIRST_SYNC, &now_ms().to_string())
    }

    pub(crate) async fn fetch_graph(
        &self,
        stream: &StreamId,
        warnings: &mut Vec<Warning>,
    ) -> Result<BTreeMap<VersionId, Version>, EngineError> {
        let repo = self.repo_ref()?;
        let mut out = BTreeMap::new();
        for id in repo.list_versions(stream).await? {
            let json = match self.state.cached_manifest(stream, &id)? {
                Some(j) => j,
                None => match repo.get_version(stream, &id).await {
                    Ok(j) => j,
                    Err(EngineError::Data(d)) => {
                        warnings.push(Warning {
                            code: WarningCode::InvalidVersion,
                            detail: d,
                        });
                        continue;
                    }
                    Err(e) => return Err(e),
                },
            };
            match Version::from_json(&json, stream, &id) {
                Ok(v) => {
                    self.state.cache_manifest(stream, &id, &json)?;
                    out.insert(id, v);
                }
                Err(e) => warnings.push(Warning {
                    code: WarningCode::InvalidVersion,
                    detail: e.to_string(),
                }),
            }
        }
        Ok(out)
    }

    /// The installs `sync` covers (granted, not provisional) with their `(player, game)` keys.
    /// The others become `NotGranted` warnings. Local only. Also used by `watch_targets`.
    ///
    /// # Errors
    /// `Config` for a broken bundled profile; `Io` while discovering saves.
    pub(crate) fn synced_installs(
        &self,
        config: &RepoConfig,
        warnings: &mut Vec<Warning>,
    ) -> Result<Vec<SyncedInstall>, EngineError> {
        let device = self.device_id();
        let mut out = Vec::new();
        for (profile, install, discovery) in self.installs()? {
            let Some(discovery) = discovery else {
                warnings.push(Warning {
                    code: WarningCode::NotGranted,
                    detail: install.base.display().to_string(),
                });
                continue;
            };
            if install.provisional {
                // settings.xml exists but isn't granted: the paths are defaults and may be stale.
                warnings.push(Warning {
                    code: WarningCode::NotGranted,
                    detail: install.config_dir.display().to_string(),
                });
                continue;
            }
            let keys = stream_keys(config, device, &profile.id, &discovery);
            out.push((profile, install, keys));
        }
        Ok(out)
    }

    /// # Errors
    /// `NeedsConfirmation` on the first real sync; storage, crypto and file errors abort the run.
    pub async fn sync(&mut self, opts: SyncOptions) -> Result<SyncReport, EngineError> {
        self.ensure_repo().await?;
        if !opts.dry_run && self.state.meta(FIRST_SYNC)?.is_none() {
            return Err(EngineError::NeedsConfirmation);
        }
        let mut warnings = self.recover_journal()?;
        let device = self.device_id();
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        // Invalid config versions, renamed players and merged heads (D30): reported once, here.
        warnings.extend(loaded.warnings());
        let merged = loaded.heads.len() > 1;
        let config = loaded.config;
        let mut streams = Vec::new();
        let mut new_games: Vec<(PlayerId, String, String)> = Vec::new();
        for (profile, install, keys) in self.synced_installs(&config, &mut warnings)? {
            let names = process_names(&profile);
            let running = self.env.processes.running(&names);
            if running {
                warnings.push(Warning {
                    code: WarningCode::EmulatorRunning,
                    detail: profile.name.clone(),
                });
            }
            for (player, game) in keys {
                let report = if running {
                    skipped(&config, device, player, &profile.id, &game)
                } else {
                    self.sync_stream(
                        &profile,
                        &install,
                        &config,
                        player,
                        &game,
                        opts.dry_run,
                        &mut warnings,
                    )
                    .await?
                };
                // Adopt counts too: after a crash between manifest and base, the version exists but the
                // game may not be listed yet.
                let has_version = report.done
                    && matches!(
                        report.action,
                        StreamAction::Upload
                            | StreamAction::UploadThenConflict
                            | StreamAction::Adopt
                    );
                if has_version
                    && !config
                        .games_of(player)
                        .contains(&(profile.id.clone(), game.clone()))
                {
                    new_games.push((player, profile.id.clone(), game.clone()));
                }
                streams.push(report);
            }
        }
        if !opts.dry_run && (merged || !new_games.is_empty()) {
            // The merge warnings were already reported from the load above.
            let _ = repo_config::update(self.repo_ref()?, &self.state, device, |c| {
                for (p, e, g) in &new_games {
                    c.add_game(*p, e, g);
                }
                Ok(())
            })
            .await?;
        }
        Ok(SyncReport {
            api_version: API_VERSION,
            dry_run: opts.dry_run,
            streams,
            warnings,
        })
    }

    /// Syncs one stream, as `sync` would (launcher mode steps 2 and 6). Refuses while the
    /// emulator runs (I3) instead of skipping.
    ///
    /// # Errors
    /// As `sync`; `EmulatorRunning`; `Usage` without a granted install of `emulator`.
    pub(crate) async fn sync_one(
        &mut self,
        player: PlayerId,
        emulator: &str,
        game: &str,
        warnings: &mut Vec<Warning>,
    ) -> Result<StreamReport, EngineError> {
        self.ensure_repo().await?;
        if self.state.meta(FIRST_SYNC)?.is_none() {
            return Err(EngineError::NeedsConfirmation);
        }
        warnings.extend(self.recover_journal()?);
        let device = self.device_id();
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        warnings.extend(loaded.warnings());
        let config = loaded.config;
        let (profile, install) = self
            .installs()?
            .into_iter()
            .find(|(p, i, d)| p.id == emulator && d.is_some() && !i.provisional)
            .map(|(p, i, _)| (p, i))
            .ok_or_else(|| EngineError::Usage(format!("no_install:{emulator}")))?;
        if self.env.processes.running(&process_names(&profile)) {
            return Err(EngineError::EmulatorRunning(profile.name.clone()));
        }
        let report = self
            .sync_stream(&profile, &install, &config, player, game, false, warnings)
            .await?;
        let has_version = report.done
            && matches!(
                report.action,
                StreamAction::Upload | StreamAction::UploadThenConflict | StreamAction::Adopt
            );
        if has_version
            && !config
                .games_of(player)
                .contains(&(emulator.to_owned(), game.to_owned()))
        {
            let _ = repo_config::update(self.repo_ref()?, &self.state, device, |c| {
                c.add_game(player, emulator, game);
                Ok(())
            })
            .await?;
        }
        Ok(report)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one stream's whole decision, kept in one place
    async fn sync_stream(
        &self,
        profile: &Profile,
        install: &Install,
        config: &RepoConfig,
        player: PlayerId,
        game: &str,
        dry_run: bool,
        warnings: &mut Vec<Warning>,
    ) -> Result<StreamReport, EngineError> {
        let device = self.device_id();
        let repo = self.repo_ref()?;
        let stream = repo.stream_id(player, &profile.id, game);
        let account = config
            .account_of(device, player, &profile.id)
            .map(str::to_owned);
        let mut report = StreamReport {
            player: config
                .players
                .get(&player)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            emulator: profile.id.clone(),
            game: game.to_owned(),
            account: account.clone(),
            action: StreamAction::Nothing,
            version: None,
            heads: vec![],
            done: false,
            files: 0,
            bytes: 0,
        };
        let meter = Meter::new(self.progress.clone(), &report.player, &profile.id, game);
        let mut versions = self.fetch_graph(&stream, warnings).await?;
        let graph: Graph = versions
            .iter()
            .map(|(id, v)| (*id, v.parents.clone()))
            .collect();
        let heads = heads(&graph);
        report.heads = heads.iter().map(ToString::to_string).collect();
        let Some(account) = account else {
            if !heads.is_empty() {
                report.action = StreamAction::NeedsAccount;
            }
            return Ok(report);
        };
        let folder = save_folder(profile, install, game, &account)?;
        let mut row = self.state.stream(&stream)?.unwrap_or(StreamRow {
            stream,
            player,
            emulator: profile.id.clone(),
            game: game.to_owned(),
            base: None,
            base_files: vec![],
            highest_seen: None,
            replace_local: false,
        });
        if let Some(seen) = row.highest_seen
            && !versions.contains_key(&seen)
        {
            warnings.push(Warning {
                code: WarningCode::RemoteBehind,
                detail: format!("{} {game}", report.player),
            });
        }
        row.highest_seen = versions.keys().max().copied().max(row.highest_seen);

        let history = history_dir(&self.env.locations.data_dir, &stream);
        let mut snap = None;
        let mut local = match snapshot::quick(&folder, &row.base_files)? {
            Quick::Missing => Local::Missing,
            Quick::SameAsBase if row.base.is_some() => Local::Clean,
            _ => Local::Dirty,
        };
        if local == Local::Dirty {
            let dest = history.join(format!("{}-snapshot", now_ms()));
            snap = snapshot::take(&folder, &dest, profile.max_file_bytes(), &|d| {
                repo.blob_id(d)
            })?;
            match &snap {
                None => local = Local::Missing,
                Some(s)
                    if row.base.is_some() && snapshot::same_files(&s.files, &row.base_files) =>
                {
                    local = Local::Clean;
                    row.base_files.clone_from(&s.files);
                    let _ = std::fs::remove_dir_all(&dest);
                    snap = None;
                }
                Some(_) => {}
            }
        }
        let matches = snap.as_ref().and_then(|s| {
            heads
                .iter()
                .find(|h| versions.get(h).is_some_and(|v| v.same_content(&s.files)))
                .copied()
        });
        let mut adopted = false;
        if let (Some(h), Some(_)) = (matches, row.base) {
            if row.base != Some(h) {
                adopted = true;
            }
            row.base = Some(h);
            row.base_files = snap.as_ref().map(|s| s.files.clone()).unwrap_or_default();
            local = Local::Clean;
        }
        let action = reconcile(&Input {
            base: row.base,
            local,
            graph: &graph,
            local_matches: matches,
            replace_local: row.replace_local,
        });
        report.action = if adopted && action == Action::Nothing {
            StreamAction::Adopt
        } else {
            action_kind(&action)
        };
        if dry_run {
            if let Some(s) = &snap {
                let _ = std::fs::remove_dir_all(&s.dir);
            }
            return Ok(report);
        }
        match &action {
            Action::Nothing | Action::Conflict(_) | Action::LocalMissing | Action::RemoteBehind => {
            }
            Action::Adopt(h) => {
                row.base = Some(*h);
                row.base_files = snap.as_ref().map(|s| s.files.clone()).unwrap_or_default();
            }
            Action::Upload { parents } | Action::UploadThenConflict { parents } => {
                let s = snap
                    .as_ref()
                    .ok_or_else(|| EngineError::Data("upload without a snapshot".into()))?;
                let meta = UploadMeta {
                    stream,
                    parents: parents.clone(),
                    device,
                    player,
                    emulator: &profile.id,
                    base_files: &row.base_files,
                    note: None,
                    takeover_from: self.pending_takeover(&stream),
                };
                let v = upload(repo, s, meta, &self.failpoints, &meter).await?;
                self.clear_takeover(&stream);
                report.version = Some(v.id.to_string());
                row.base = Some(v.id);
                row.base_files.clone_from(&v.files);
                row.highest_seen = row.highest_seen.max(Some(v.id));
                row.replace_local = false;
                versions.insert(v.id, v);
            }
            Action::FastForward(h) => {
                let v = versions
                    .get(h)
                    .ok_or_else(|| EngineError::Data("head vanished".into()))?;
                let names = process_names(profile);
                let ctx = Ctx {
                    repo: Some(repo),
                    state: &self.state,
                    data_dir: &self.env.locations.data_dir,
                    grants: &self.config.grants,
                    probe: self.env.processes.as_ref(),
                    process_names: &names,
                    max_file_bytes: profile.max_file_bytes(),
                    fp: &self.failpoints,
                    meter: &meter,
                };
                restore::restore(&ctx, &stream, &folder, Source::Remote(v), "pre-restore").await?;
                report.version = Some(h.to_string());
                row.base = Some(*h);
                row.base_files = on_disk(&folder, &v.files);
                row.replace_local = false;
            }
        }
        report.done = true;
        self.state.put_stream(&row)?;
        if self.config.conflict_policy == ConflictPolicy::Newest
            && matches!(
                action,
                Action::Conflict(_) | Action::UploadThenConflict { .. }
            )
        {
            let all: Graph = versions
                .iter()
                .map(|(id, v)| (*id, v.parents.clone()))
                .collect();
            let timed: Vec<_> = lemusync_core::heads::heads(&all)
                .into_iter()
                .filter_map(|h| versions.get(&h).map(|v| (h, v.created_at)))
                .collect();
            if let Some(keep) = newest_head(&timed) {
                let names = process_names(profile);
                let v = self
                    .write_resolution(&Resolution {
                        stream,
                        player,
                        emulator: &profile.id,
                        game,
                        folder: &folder,
                        names: &names,
                        max_file_bytes: profile.max_file_bytes(),
                        versions: &versions,
                        keep,
                        meter: &meter,
                    })
                    .await?;
                report.action = StreamAction::Resolved;
                report.version = Some(v.id.to_string());
                report.heads = vec![v.id.to_string()];
            }
        }
        (report.files, report.bytes) = meter.transferred();
        Ok(report)
    }
}
