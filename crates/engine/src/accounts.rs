//! `lemusync accounts map` (choices a/b, keep or replace) and `accounts recover`
//! (players-and-accounts §Discovery).

use std::path::Path;

use lemusync_api::{API_VERSION, CreatedAccount, StreamAction, StreamReport};
use lemusync_core::PlayerId;
use lemusync_core::account::key_value;
use lemusync_core::account_template::{self, CreateTemplate, TemplateError};

use crate::discover::save_folder;
use crate::progress::Meter;
use crate::snapshot;
use crate::sync::{FIRST_SYNC, SyncOptions};
use crate::upload::{UploadMeta, upload};
use crate::{Engine, EngineError, StreamRow, fsx, grants, history_dir, now_ms, repo_config};

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

/// `key:data` usage errors (P6: the CLI maps keys to localised text).
fn map_error(e: &lemusync_core::config::ConfigError) -> EngineError {
    use lemusync_core::config::ConfigError;
    match e {
        ConfigError::UnknownPlayer(name) => EngineError::Usage(format!("unknown_player:{name}")),
        ConfigError::AccountTaken { player, .. } => {
            EngineError::Usage(format!("account_taken:{player}"))
        }
        _ => EngineError::Config("repo_config".into()),
    }
}

impl Engine {
    fn player_id(
        config: &lemusync_core::config::RepoConfig,
        name: &str,
    ) -> Result<PlayerId, EngineError> {
        config
            .player_named(name)
            .ok_or_else(|| EngineError::Usage(format!("unknown_player:{name}")))
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
            .ok_or_else(|| EngineError::Usage(format!("no_install:{emulator}")))?;
        let discovery = discovery
            .as_ref()
            .ok_or_else(|| EngineError::NotGranted(install.base.display().to_string()))?;
        if !discovery.accounts.iter().any(|a| a.id == account) {
            return Err(EngineError::Usage(format!("no_account:{account}")));
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
                .map_err(|e| map_error(&e))
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
        let (profile, install, discovery) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no_install:{emulator}")))?;
        let discovery = discovery
            .as_ref()
            .ok_or_else(|| EngineError::NotGranted(install.base.display().to_string()))?;
        let save = discovery
            .saves
            .iter()
            .find(|s| s.account == account && s.orphaned)
            .ok_or_else(|| EngineError::Usage(format!("not_orphaned:{account}")))?;
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
                "already_has_save:{}",
                save.game
            )));
        }
        let dest = history_dir(&self.env.locations.data_dir, &stream)
            .join(format!("{}-recover", now_ms()));
        let snap = snapshot::take(&save.path, &dest, profile.max_file_bytes(), &|d| {
            repo.blob_id(d)
        })?
        .ok_or_else(|| EngineError::Usage("folder_empty".into()))?;
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
            takeover_from: None,
        };
        let meter = Meter::new(self.progress.clone(), player, &profile.id, &save.game);
        let v = upload(repo, &snap, meta, &self.failpoints, &meter).await?;
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
            files: meter.transferred().0,
            bytes: meter.transferred().1,
        })
    }

    /// Writes a new emulator account from the profile's `accounts.create` data, maps it to
    /// `player`, then syncs so the player's latest saves are restored into it (backup first).
    /// Writes only `<accounts root>/<id>/<file>`: never the counter file or other accounts.
    ///
    /// # Errors
    /// `EmulatorRunning`; `Usage` for an unknown player, a profile that can't create accounts,
    /// too many accounts or a taken folder name; `NotGranted` outside the grants.
    pub async fn create_account(
        &mut self,
        player: &str,
        emulator: &str,
        name: Option<&str>,
    ) -> Result<CreatedAccount, EngineError> {
        self.ensure_repo().await?;
        let installs = self.installs()?;
        let (profile, install, _) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no_install:{emulator}")))?;
        let template = profile
            .accounts
            .as_ref()
            .and_then(|a| a.create.as_ref())
            .ok_or_else(|| EngineError::Usage(format!("cannot_create:{emulator}")))?;
        let root = install
            .accounts_root
            .clone()
            .ok_or_else(|| EngineError::Usage(format!("cannot_create:{emulator}")))?;
        let grant = grants::require(&self.config.grants, &root)?;
        fsx::ensure_no_symlinks(&grant, &root)
            .map_err(|_| EngineError::NotGranted(root.display().to_string()))?;
        let names = profile
            .platforms
            .get("linux")
            .map(|p| p.process.clone())
            .unwrap_or_default();
        if self.env.processes.running(&names) {
            return Err(EngineError::EmulatorRunning(profile.name.clone()));
        }
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        Self::player_id(&loaded.config, player)?;
        let name = account_template::fit_name(name.unwrap_or(player), template.name_max_utf16)
            .map_err(|_| EngineError::Usage("bad_account_name".into()))?;
        let id = account_template::next_id(
            template,
            read_counter(template, &root),
            &existing_ids(template, &root)?,
        )
        .map_err(|e| match e {
            TemplateError::TooManyAccounts(n) => {
                EngineError::Usage(format!("too_many_accounts:{n}"))
            }
            _ => EngineError::Usage("no_free_account_id".into()),
        })?;
        let account = format!("{id:08x}");
        let text = account_template::render(template, id, &name, &random(template.random_len())?)
            .map_err(|e| EngineError::Config(format!("accounts.create: {e}")))?;
        let folder = root.join(&account);
        // Exclusive: an existing name (folder, file or symlink) is never reused or followed.
        std::fs::create_dir(&folder).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                EngineError::Usage(format!("account_exists:{account}"))
            } else {
                e.into()
            }
        })?;
        fsx::write_atomic(&folder.join(&template.file), text.as_bytes())?;
        fsx::sync_dir(&root)?;
        let emulator = profile.id.clone();
        self.accounts_map(player, &emulator, &account, MapChoice::Keep)
            .await?;
        let sync = if self.state.meta(FIRST_SYNC)?.is_some() {
            Some(self.sync(SyncOptions { dry_run: false }).await?)
        } else {
            None
        };
        Ok(CreatedAccount {
            api_version: API_VERSION,
            emulator,
            account,
            name,
            player: player.to_owned(),
            sync,
        })
    }
}

/// The emulator's id counter, if its file has one. Read only.
fn read_counter(t: &CreateTemplate, root: &Path) -> Option<u32> {
    let path = root.join(&t.id.counter_file);
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    account_template::parse_hex8(key_value(&text, &t.id.counter_key)?.trim())
}

/// Ids of the account folders (real folders, not symlinks) under `root`.
fn existing_ids(t: &CreateTemplate, root: &Path) -> Result<Vec<u32>, EngineError> {
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(account_template::parse_hex8)
            && id >= t.min_id()
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn random(len: usize) -> Result<Vec<u8>, EngineError> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        out.extend_from_slice(&lemusync_crypto::random_bytes::<32>()?);
    }
    out.truncate(len);
    Ok(out)
}
