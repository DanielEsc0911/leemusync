//! Where `LemuSync` keeps its files (config-and-cli §Locations).

use std::path::{Path, PathBuf};

use crate::EngineError;

#[derive(Clone, Debug)]
pub struct Locations {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub xdg_data: PathBuf,
    pub xdg_config: PathBuf,
}

impl Locations {
    /// # Errors
    /// `Config` when the OS reports no home directory.
    pub fn from_os() -> Result<Self, EngineError> {
        let project = directories::ProjectDirs::from("", "", "lemusync")
            .ok_or_else(|| EngineError::Config("no_home_dir".into()))?;
        let base = directories::BaseDirs::new()
            .ok_or_else(|| EngineError::Config("no_home_dir".into()))?;
        Ok(Self {
            config_dir: project.config_dir().to_path_buf(),
            data_dir: project.data_dir().to_path_buf(),
            home: base.home_dir().to_path_buf(),
            xdg_data: base.data_dir().to_path_buf(),
            xdg_config: base.config_dir().to_path_buf(),
        })
    }

    /// Everything under one folder, for tests and the two-device simulation.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        let home = root.join("home");
        Self {
            config_dir: root.join("config/lemusync"),
            data_dir: root.join("data/lemusync"),
            xdg_data: home.join(".local/share"),
            xdg_config: home.join(".config"),
            home,
        }
    }
}
