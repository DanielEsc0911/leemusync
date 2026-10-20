//! `leemusync accounts map` (choices a/b, keep or replace) and `accounts recover`
//! (players-and-accounts §Discovery).

use leemusync_api::{StreamAction, StreamReport};
use leemusync_core::PlayerId;

use crate::discover::save_folder;
use crate::snapshot;
use crate::upload::{UploadMeta, upload};
use crate::{Engine, EngineError, StreamRow, fsx, history_dir, now_ms, repo_config};

pub enum MapChoice {
    Ask,
    Keep,
    Replace,
}

pub enum MapOutcome {
    Mapped,
    /// The account has files and the player has versions of these games: keep or replace?
    NeedsChoice {
        games: Vec<String>,
    },
}

impl Engine {
    fn player_id(
        config: &leemusync_core::config::RepoConfig,
        name: &str,
    ) -> Result<PlayerId, EngineError> {
        config.player_named(name).ok_or_else(|| {
            EngineError::Usage(format!("no player `{name}`; run `leemusync players add`"))
        })
    }

    /// # Errors
    /// `Usage` for an unknown player, an account this device doesn't have, or one mapped to another player.
    pub async fn accounts_map(
        &mut self,
        player: &str,
        emulator: &str,
        account: &str,
        choice: MapChoice,
    ) -> Result<MapOutcome, EngineError> {
        self.ensure_repo().await?;
        let device = self.device_id();
        let installs = self.installs()?;
        let (profile, install, discovery) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| {
                EngineError::Usage(format!("no {emulator} install found; run `leemusync scan`"))
            })?;
        let discovery = discovery
            .as_ref()
            .ok_or_else(|| EngineError::NotGranted(install.base.display().to_string()))?;
        if !discovery.accounts.iter().any(|a| a.id == account) {
            return Err(EngineError::Usage(format!(
                "{} has no account {account} on this device; create it in {}, then run `leemusync scan`",
                profile.name, profile.name
            )));
        }
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        let pid = Self::player_id(&loaded.config, player)?;
        let mut relevant = Vec::new();
        for (emu, game) in loaded.config.games_of(pid) {
            if emu != profile.id {
                continue;
            }
            let folder = save_folder(profile, install, &game, account)?;
            let has_files = folder.is_dir() && !fsx::list_files(&folder)?.is_empty();
            let stream = self.repo_ref()?.stream_id(pid, &profile.id, &game);
            if has_files && !self.repo_ref()?.list_versions(&stream).await?.is_empty() {
                relevant.push(game);
            }
        }
        if !relevant.is_empty() && matches!(choice, MapChoice::Ask) {
            return Ok(MapOutcome::NeedsChoice { games: relevant });
        }
        repo_config::update(self.repo_ref()?, &self.state, device, |c| {
            c.map_account(device, pid, &profile.id, account)
                .map_err(|e| EngineError::Usage(e.to_string()))
        })
        .await?;
        if matches!(choice, MapChoice::Replace) {
            for game in relevant {
                let stream = self.repo_ref()?.stream_id(pid, &profile.id, &game);
                let mut row = self.state.stream(&stream)?.unwrap_or(StreamRow {
                    stream,
                    player: pid,
                    emulator: profile.id.clone(),
                    game: game.clone(),
                    base: None,
                    base_files: vec![],
                    highest_seen: None,
                    replace_local: false,
                });
                row.base = None;
                row.base_files = vec![];
                row.replace_local = true;
                self.state.put_stream(&row)?;
            }
        }
        Ok(MapOutcome::Mapped)
    }

    /// Uploads an orphaned save folder as the first version of `player`'s stream for that game.
    ///
    /// # Errors
    /// `Usage` if the folder isn't orphaned or the player already has a version of the game.
    pub async fn accounts_recover(
        &mut self,
        emulator: &str,
        account: &str,
        player: &str,
    ) -> Result<StreamReport, EngineError> {
        self.ensure_repo().await?;
        let device = self.device_id();
        let installs = self.installs()?;
        let (profile, _, discovery) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no {emulator} install found")))?;
        let discovery = discovery
            .as_ref()
            .ok_or(EngineError::NotSetUp("grant the emulator folder first"))?;
        let save = discovery
            .saves
            .iter()
            .find(|s| s.account == account && s.orphaned)
            .ok_or_else(|| {
                EngineError::Usage(format!("{account} isn't an orphaned save folder here"))
            })?;
        let names = profile
            .platforms
            .get("linux")
            .map(|p| p.process.clone())
            .unwrap_or_default();
        if self.env.processes.running(&names) {
            return Err(EngineError::EmulatorRunning(profile.name.clone()));
        }
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        let pid = Self::player_id(&loaded.config, player)?;
        let repo = self.repo_ref()?;
        let stream = repo.stream_id(pid, &profile.id, &save.game);
        if !repo.list_versions(&stream).await?.is_empty() {
            return Err(EngineError::Usage(format!(
                "{player} already has a save for {}; recover it to a new player instead",
                save.game
            )));
        }
        let dest = history_dir(&self.env.locations.data_dir, &stream)
            .join(format!("{}-recover", now_ms()));
        let snap = snapshot::take(&save.path, &dest, profile.max_file_bytes(), &|d| {
            repo.blob_id(d)
        })?
        .ok_or_else(|| EngineError::Usage("that folder is empty".into()))?;
        let meta = UploadMeta {
            stream,
            parents: vec![],
            device,
            player: pid,
            emulator: &profile.id,
            base_files: &[],
            note: Some(format!(
                "recovered from orphaned folder {account} on {}",
                self.config.device.name
            )),
        };
        let v = upload(repo, &snap, meta, &self.failpoints).await?;
        let game = save.game.clone();
        repo_config::update(self.repo_ref()?, &self.state, device, |c| {
            c.add_game(pid, &profile.id, &game);
            Ok(())
        })
        .await?;
        Ok(StreamReport {
            player: player.to_owned(),
            emulator: profile.id.clone(),
            game,
            account: None,
            action: StreamAction::Upload,
            version: Some(v.id.to_string()),
            heads: vec![v.id.to_string()],
            done: true,
        })
    }
}
