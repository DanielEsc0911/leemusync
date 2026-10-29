//! `status`, `history`, `restore`, `doctor`.

use lemusync_api::{
    API_VERSION, Check, DoctorReport, HistoryEntry, HistoryReport, StatusReport, StreamAction,
    StreamReport, StreamStatus,
};
use lemusync_core::heads::{Graph, heads};

use crate::discover::save_folder;
use crate::progress::Meter;
use crate::repo::REPO_JSON;
use crate::restore::{self, Ctx, Source};
use crate::{BackendConfig, Engine, EngineError, history_dir, repo_config};

/// One stream's place on this device (`Engine::target`).
pub(crate) struct Target {
    pub stream: lemusync_core::StreamId,
    pub player: lemusync_core::PlayerId,
    pub account: String,
    pub profile: lemusync_core::profile::Profile,
    pub folder: std::path::PathBuf,
    pub names: Vec<String>,
}

impl Engine {
    /// # Errors
    /// None for a device without a repo (fields stay empty); storage errors otherwise.
    pub async fn status(&mut self) -> Result<StatusReport, EngineError> {
        let device = self.device_id();
        let mut players = Vec::new();
        let mut streams = Vec::new();
        if self.config.repo.is_some() {
            self.ensure_repo().await?;
            let config = repo_config::load(self.repo_ref()?, &self.state)
                .await?
                .config;
            let names = self.game_names();
            for (pid, p) in &config.players {
                players.push(p.name.clone());
                for (emu, game) in config.games_of(*pid) {
                    let stream = self.repo_ref()?.stream_id(*pid, &emu, &game);
                    let base = self
                        .state
                        .stream(&stream)?
                        .and_then(|r| r.base)
                        .map(|b| b.to_string());
                    let account = config.account_of(device, *pid, &emu).map(str::to_owned);
                    streams.push(StreamStatus {
                        player: p.name.clone(),
                        name: names.get(&(emu.clone(), game.clone())).cloned(),
                        emulator: emu,
                        game,
                        account,
                        base,
                    });
                }
            }
        }
        Ok(StatusReport {
            api_version: API_VERSION,
            device: self.config.device.name.clone(),
            device_id: device.to_string(),
            repo: self.config.repo.clone(),
            backend: self.config.backend.as_ref().map(|b| match b {
                BackendConfig::Gdrive { .. } => "gdrive".to_owned(),
                BackendConfig::S3 {
                    endpoint, bucket, ..
                } => format!("s3 {endpoint}/{bucket}"),
                BackendConfig::Fs { path } => format!("fs {}", path.display()),
            }),
            grants: self
                .config
                .grants
                .iter()
                .map(|g| g.display().to_string())
                .collect(),
            players,
            streams,
            daemon: None,
        })
    }

    /// The stream id and player id of `player`'s save of `game`.
    ///
    /// # Errors
    /// `Usage` for an unknown player; storage errors.
    pub async fn stream_for(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
    ) -> Result<(lemusync_core::StreamId, lemusync_core::PlayerId), EngineError> {
        self.ensure_repo().await?;
        let config = repo_config::load(self.repo_ref()?, &self.state)
            .await?
            .config;
        let pid = config
            .player_named(player)
            .ok_or_else(|| EngineError::Usage(format!("unknown_player:{player}")))?;
        Ok((
            self.repo_ref()?
                .stream_id(pid, emulator, &game.to_lowercase()),
            pid,
        ))
    }

    async fn stream_of(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
    ) -> Result<lemusync_core::StreamId, EngineError> {
        Ok(self.stream_for(player, emulator, game).await?.0)
    }

    /// # Errors
    /// `Usage` for an unknown player; storage errors.
    pub async fn history(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
    ) -> Result<HistoryReport, EngineError> {
        let stream = self.stream_of(player, emulator, game).await?;
        let mut warnings = Vec::new();
        let versions = self.fetch_graph(&stream, &mut warnings).await?;
        let graph: Graph = versions
            .iter()
            .map(|(id, v)| (*id, v.parents.clone()))
            .collect();
        let heads = heads(&graph);
        let entries = versions
            .values()
            .rev()
            .map(|v| HistoryEntry {
                id: v.id.to_string(),
                created_at: v.created_at,
                device: v.device.to_string(),
                files: u32::try_from(v.files.len()).unwrap_or(u32::MAX),
                bytes: v.files.iter().map(|f| f.size).sum(),
                parents: v.parents.iter().map(ToString::to_string).collect(),
                head: heads.contains(&v.id),
                takeover: v.meta.takeover_from.map(|d| d.to_string()),
            })
            .collect();
        let dir = history_dir(&self.env.locations.data_dir, &stream);
        let mut backups: Vec<String> = std::fs::read_dir(&dir)
            .map(|d| {
                d.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        backups.sort();
        backups.reverse();
        Ok(HistoryReport {
            api_version: API_VERSION,
            name: self
                .game_names()
                .remove(&(emulator.to_owned(), game.to_lowercase())),
            versions: entries,
            backups,
        })
    }

    /// Restores a version or a local backup into the mapped account (backup first). `base` is
    /// unchanged, so the next `sync` uploads the result as a new version.
    ///
    /// # Errors
    /// `Usage` for an unknown version, backup, player or mapping; restore errors.
    pub async fn restore(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
        what: &str,
    ) -> Result<StreamReport, EngineError> {
        self.recover_journal()?;
        let Target {
            stream,
            account,
            profile,
            folder,
            names,
            ..
        } = self.target(player, emulator, game).await?;
        let mut warnings = Vec::new();
        let versions = self.fetch_graph(&stream, &mut warnings).await?;
        let backup_dir = history_dir(&self.env.locations.data_dir, &stream).join(what);
        let meter = Meter::new(
            self.progress.clone(),
            player,
            emulator,
            &game.to_lowercase(),
        );
        let ctx = Ctx {
            repo: Some(self.repo_ref()?),
            state: &self.state,
            data_dir: &self.env.locations.data_dir,
            grants: &self.config.grants,
            probe: self.env.processes.as_ref(),
            process_names: &names,
            max_file_bytes: profile.max_file_bytes(),
            fp: &self.failpoints,
            meter: &meter,
        };
        if let Some(v) = versions
            .values()
            .find(|v| v.id.to_string().eq_ignore_ascii_case(what))
        {
            restore::restore(&ctx, &stream, &folder, Source::Remote(v), "pre-restore").await?;
        } else if !what.contains(['/', '\\']) && what != ".." && backup_dir.is_dir() {
            restore::restore(
                &ctx,
                &stream,
                &folder,
                Source::Local(&backup_dir),
                "pre-restore",
            )
            .await?;
        } else {
            return Err(EngineError::Usage(format!("unknown_restore_point:{what}")));
        }
        Ok(StreamReport {
            player: player.to_owned(),
            emulator: emulator.to_owned(),
            game: game.to_lowercase(),
            account: Some(account),
            action: StreamAction::FastForward,
            version: Some(what.to_owned()),
            heads: vec![],
            done: true,
            files: meter.transferred().0,
            bytes: meter.transferred().1,
        })
    }

    /// Where `player`'s save of `game` lives on this device.
    ///
    /// # Errors
    /// `Usage` for an unknown player, mapping or install; storage errors.
    pub(crate) async fn target(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
    ) -> Result<Target, EngineError> {
        let (stream, pid) = self.stream_for(player, emulator, game).await?;
        let config = repo_config::load(self.repo_ref()?, &self.state)
            .await?
            .config;
        let account = config
            .account_of(self.device_id(), pid, emulator)
            .ok_or_else(|| EngineError::Usage(format!("not_mapped:{emulator}")))?
            .to_owned();
        let (profile, install, _) = self
            .installs()?
            .into_iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no_install:{emulator}")))?;
        let folder = save_folder(&profile, &install, &game.to_lowercase(), &account)?;
        let names = profile
            .platforms
            .get("linux")
            .map(|p| p.process.clone())
            .unwrap_or_default();
        Ok(Target {
            stream,
            player: pid,
            account,
            profile,
            folder,
            names,
        })
    }

    /// # Errors
    /// Never for failed checks (they're reported); only for a broken bundled profile.
    pub async fn doctor(&mut self) -> Result<DoctorReport, EngineError> {
        let mut checks = Vec::new();
        let probe = "lemusync-doctor-probe";
        let keystore = self
            .env
            .secrets
            .set(probe, "ok")
            .and_then(|()| self.env.secrets.get(probe))
            .and_then(|v| {
                self.env.secrets.delete(probe)?;
                Ok(v)
            });
        checks.push(Check {
            name: "keystore".into(),
            ok: keystore.is_ok(),
            detail: keystore.err().map(|e| e.to_string()).unwrap_or_default(),
        });
        let backend = match self.store() {
            Ok(s) => s
                .stat(REPO_JSON)
                .await
                .map(|e| {
                    if e.is_some() {
                        "repo found"
                    } else {
                        "no repo yet"
                    }
                })
                .map_err(EngineError::from),
            Err(e) => Err(e),
        };
        checks.push(Check {
            name: "backend".into(),
            ok: backend.is_ok(),
            detail: backend.map_or_else(|e| e.to_string(), str::to_owned),
        });
        if self.config.repo.is_some() {
            let unlocked = self.ensure_repo().await;
            checks.push(Check {
                name: "repo".into(),
                ok: unlocked.is_ok(),
                detail: unlocked.err().map(|e| e.to_string()).unwrap_or_default(),
            });
        }
        for (profile, install, discovery) in self.installs()? {
            checks.push(Check {
                name: format!("{} folder", profile.id),
                ok: discovery.is_some(),
                detail: install.base.display().to_string(),
            });
            let names = profile
                .platforms
                .get("linux")
                .map(|p| p.process.clone())
                .unwrap_or_default();
            let running = self.env.processes.running(&names);
            checks.push(Check {
                name: format!("{} running", profile.id),
                ok: true,
                detail: if running {
                    "running".into()
                } else {
                    "closed".into()
                },
            });
        }
        Ok(DoctorReport {
            api_version: API_VERSION,
            checks,
        })
    }
}
