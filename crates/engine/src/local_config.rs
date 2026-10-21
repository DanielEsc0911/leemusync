//! `config.toml`, per device (config-and-cli §Config file). Never holds secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use leemusync_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{EngineError, fsx};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalConfig {
    pub config_version: u32,
    pub device: DeviceSection,
    pub repo: Option<String>,
    pub backend: Option<BackendConfig>,
    #[serde(default)]
    pub grants: Vec<PathBuf>,
    #[serde(default)]
    pub emulators: BTreeMap<String, EmulatorOverride>,
    #[serde(default)]
    pub leases: LeaseSettings,
}

/// `[leases]` (sync-model §Leases).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseSettings {
    /// Lease lifetime; at least two renewals (120 s), at most a day.
    pub ttl_secs: u64,
}

impl Default for LeaseSettings {
    fn default() -> Self {
        Self { ttl_secs: 300 }
    }
}

impl LeaseSettings {
    pub const MIN_TTL_SECS: u64 = 120;
    pub const MAX_TTL_SECS: u64 = 86_400;

    #[must_use]
    pub fn ttl_ms(&self) -> i64 {
        i64::try_from(self.ttl_secs.saturating_mul(1000)).unwrap_or(i64::MAX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSection {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendConfig {
    Gdrive {
        root: String,
        client_id: String,
    },
    /// S3-compatible (R2 for the first test, D31). The secret key is in the keystore.
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        root: String,
        access_key_id: String,
    },
    Fs {
        path: PathBuf,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmulatorOverride {
    pub root: Option<PathBuf>,
}

/// `toml`'s `Display` quotes the offending source line, which can hold a secret pasted by
/// mistake. Keep only its message and the position.
fn parse_error(text: &str, e: &toml::de::Error) -> EngineError {
    let Some(span) = e.span() else {
        return EngineError::Config(e.message().to_owned());
    };
    let before = text.get(..span.start).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    EngineError::Config(format!("line {line}, column {column}: {}", e.message()))
}

impl LocalConfig {
    #[must_use]
    pub fn new(id: DeviceId, name: &str) -> Self {
        Self {
            config_version: CONFIG_VERSION,
            device: DeviceSection {
                id,
                name: name.to_owned(),
            },
            repo: None,
            backend: None,
            grants: Vec::new(),
            emulators: BTreeMap::new(),
            leases: LeaseSettings::default(),
        }
    }

    /// # Errors
    /// `Config` for invalid TOML or an unknown `config_version`; `Io` for read errors.
    pub fn load(path: &Path) -> Result<Option<Self>, EngineError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let config: Self = toml::from_str(&text).map_err(|e| parse_error(&text, &e))?;
        if config.config_version != CONFIG_VERSION {
            return Err(EngineError::Config(format!(
                "unsupported config_version {}",
                config.config_version
            )));
        }
        let ttl = config.leases.ttl_secs;
        if !(LeaseSettings::MIN_TTL_SECS..=LeaseSettings::MAX_TTL_SECS).contains(&ttl) {
            return Err(EngineError::Config(format!(
                "leases.ttl_secs must be {}..={}",
                LeaseSettings::MIN_TTL_SECS,
                LeaseSettings::MAX_TTL_SECS
            )));
        }
        Ok(Some(config))
    }

    /// # Errors
    /// `Config` if it can't be serialised; `Io` if the atomic write fails.
    pub fn save(&self, path: &Path) -> Result<(), EngineError> {
        let text = toml::to_string(self).map_err(|e| EngineError::Config(e.to_string()))?;
        fsx::write_atomic(path, text.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::VersionId;

    fn sample() -> LocalConfig {
        let mut c = LocalConfig::new(DeviceId(VersionId::from_parts(1, [3; 10])), "fedora-pc");
        c.repo = Some("r1".into());
        c.backend = Some(BackendConfig::Gdrive {
            root: "/LeemuSync".into(),
            client_id: "abc.apps".into(),
        });
        c.grants = vec![PathBuf::from("/home/u/.local/share/Cemu")];
        c.emulators.insert(
            "cemu".into(),
            EmulatorOverride {
                root: Some(PathBuf::from("/opt/cemu")),
            },
        );
        c
    }

    #[test]
    fn s3_backend_round_trips_without_its_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = sample();
        c.backend = Some(BackendConfig::S3 {
            endpoint: "https://acct.r2.cloudflarestorage.com".into(),
            bucket: "saves".into(),
            region: "auto".into(),
            root: "/".into(),
            access_key_id: "key-id".into(),
        });
        c.save(&path).unwrap();
        assert_eq!(LocalConfig::load(&path).unwrap(), Some(c));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("kind = \"s3\""), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(LocalConfig::load(&path).unwrap(), None);
        sample().save(&path).unwrap();
        assert_eq!(LocalConfig::load(&path).unwrap(), Some(sample()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("kind = \"gdrive\""), "{text}");
    }

    #[test]
    fn rejects_unknown_versions_and_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let good = toml::to_string(&sample()).unwrap();
        std::fs::write(
            &path,
            good.replace("config_version = 1", "config_version = 2"),
        )
        .unwrap();
        assert!(matches!(
            LocalConfig::load(&path),
            Err(EngineError::Config(_))
        ));
        std::fs::write(&path, format!("extra = 1\n{good}")).unwrap();
        assert!(matches!(
            LocalConfig::load(&path),
            Err(EngineError::Config(_))
        ));
        std::fs::write(
            &path,
            good.replace("client_id = ", "client_secret = \"s\"\nclient_id = "),
        )
        .unwrap();
        assert!(
            matches!(LocalConfig::load(&path), Err(EngineError::Config(_))),
            "no secrets in config.toml"
        );
    }

    #[test]
    fn lease_ttl_defaults_and_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let good = toml::to_string(&sample()).unwrap();
        assert!(good.contains("[leases]\nttl_secs = 300"), "{good}");
        let without = good.replace("[leases]\nttl_secs = 300\n", "");
        std::fs::write(&path, &without).unwrap();
        assert_eq!(
            LocalConfig::load(&path).unwrap().unwrap().leases.ttl_secs,
            300
        );
        for bad in ["119", "86401"] {
            std::fs::write(
                &path,
                good.replace("ttl_secs = 300", &format!("ttl_secs = {bad}")),
            )
            .unwrap();
            assert!(matches!(
                LocalConfig::load(&path),
                Err(EngineError::Config(_))
            ));
        }
    }

    #[test]
    fn errors_never_echo_the_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let good = toml::to_string(&sample()).unwrap();
        let cases = [
            good.replace(
                "client_id = ",
                "client_secret = \"SUPERSECRET\"\nclient_id = ",
            ),
            good.replace(
                "client_id = ",
                "client_secret = \"SUPERSECRET\nclient_id = ",
            ),
            good.replace("client_id = ", "client_secret = SUPERSECRET\nclient_id = "),
        ];
        for text in cases {
            std::fs::write(&path, &text).unwrap();
            let err = LocalConfig::load(&path).unwrap_err();
            assert!(matches!(err, EngineError::Config(_)));
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains("SUPERSECRET"), "{shown}");
            assert!(shown.contains("line "), "{shown}");
        }
    }
}
