//! `leemusync grant` and `leemusync scan`.

use std::path::{Path, PathBuf};

use leemusync_api::{
    API_VERSION, AccountReport, InstallReport, SaveReport, ScanReport, Warning, WarningCode,
};
use leemusync_core::profile::Profile;

use crate::discover::{Discovery, discover};
use crate::profiles::{Install, bundled, resolve};
use crate::{Engine, EngineError, fsx, grants, repo_config};

impl Engine {
    /// # Errors
    /// `Usage` for `/`, the home folder itself, or a path that isn't an existing folder.
    pub fn grant_add(&mut self, path: &Path) -> Result<PathBuf, EngineError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| EngineError::Usage("grant_path_missing".into()))?;
        let home = self
            .env
            .locations
            .home
            .canonicalize()
            .unwrap_or_else(|_| self.env.locations.home.clone());
        if !canonical.is_dir() || canonical.parent().is_none() || canonical == home {
            return Err(EngineError::Usage("grant_path_not_allowed".into()));
        }
        if !self.config.grants.contains(&canonical) {
            self.config.grants.push(canonical.clone());
            self.config.grants.sort();
            self.save_config()?;
        }
        Ok(canonical)
    }

    #[must_use]
    pub fn grant_list(&self) -> Vec<PathBuf> {
        self.config.grants.clone()
    }

    /// Every bundled profile that is installed here. The discovery is `None` until the folders it
    /// reads are granted. Paths read from the emulator's own config are checked like any other.
    pub(crate) fn installs(
        &self,
    ) -> Result<Vec<(Profile, Install, Option<Discovery>)>, EngineError> {
        let grants = self.config.grants.clone();
        let granted = move |p: &Path| grants::is_granted(&grants, p);
        let mut out = Vec::new();
        for profile in bundled()? {
            let override_root = self
                .config
                .emulators
                .get(&profile.id)
                .and_then(|o| o.root.clone());
            let Some(install) = resolve(
                &profile,
                &self.env.locations,
                override_root.as_deref(),
                &granted,
            )?
            else {
                continue;
            };
            let confined = |p: &Path| {
                grants::require(&self.config.grants, p)
                    .is_ok_and(|root| fsx::ensure_no_symlinks(&root, p).is_ok())
            };
            let readable = confined(&install.saves_root)
                && install.accounts_root.as_deref().is_none_or(confined);
            let discovery = if readable {
                Some(discover(&profile, &install)?)
            } else {
                None
            };
            out.push((profile, install, discovery));
        }
        Ok(out)
    }

    /// Lists installs, accounts (with mapped players when a repo is open), save folders and orphans.
    ///
    /// # Errors
    /// `Config` for a broken bundled profile; repo errors are ignored (players stay unknown).
    pub async fn scan(&mut self) -> Result<ScanReport, EngineError> {
        let device = self.device_id();
        let loaded = if self.ensure_repo().await.is_ok() {
            match self.repo_ref() {
                Ok(repo) => repo_config::load(repo, &self.state).await.ok(),
                Err(_) => None,
            }
        } else {
            None
        };
        let mut installs = Vec::new();
        let mut warnings = loaded
            .as_ref()
            .map(repo_config::LoadedConfig::warnings)
            .unwrap_or_default();
        let config = loaded.map(|l| l.config);
        for (profile, install, discovery) in self.installs()? {
            let mut needs_grant = Vec::new();
            if !grants::is_granted(&self.config.grants, &install.saves_root) {
                needs_grant.push(install.base.display().to_string());
            }
            if install.config_dir != install.base
                && !grants::is_granted(&self.config.grants, &install.config_dir)
                && install.config_dir.exists()
            {
                needs_grant.push(install.config_dir.display().to_string());
            }
            warnings.extend(install.warnings.iter().cloned());
            let (accounts, saves) = match &discovery {
                None => (Vec::new(), Vec::new()),
                Some(d) => {
                    for p in &d.shared_not_empty {
                        warnings.push(Warning {
                            code: WarningCode::SharedFolderNotEmpty,
                            detail: p.display().to_string(),
                        });
                    }
                    let accounts = d
                        .accounts
                        .iter()
                        .map(|a| AccountReport {
                            id: a.id.clone(),
                            name: a.name.clone(),
                            player: config.as_ref().and_then(|c| {
                                let pid = c.player_on_account(device, &profile.id, &a.id)?;
                                c.players.get(&pid).map(|p| p.name.clone())
                            }),
                        })
                        .collect();
                    let saves = d
                        .saves
                        .iter()
                        .map(|s| {
                            if s.orphaned {
                                warnings.push(Warning {
                                    code: WarningCode::OrphanedSave,
                                    detail: format!("{}/{}", s.game, s.account),
                                });
                            }
                            let files = fsx::list_files(&s.path).unwrap_or_default();
                            SaveReport {
                                game: s.game.clone(),
                                account: s.account.clone(),
                                orphaned: s.orphaned,
                                files: u32::try_from(files.len()).unwrap_or(u32::MAX),
                                bytes: files.iter().map(|(_, m)| m.len()).sum(),
                            }
                        })
                        .collect();
                    (accounts, saves)
                }
            };
            installs.push(InstallReport {
                emulator: install.emulator.clone(),
                base: install.base.display().to_string(),
                granted: discovery.is_some(),
                needs_grant,
                accounts,
                saves,
            });
        }
        Ok(ScanReport {
            api_version: API_VERSION,
            installs,
            warnings,
        })
    }
}
