//! `config.toml`, per device (config-and-cli §Config file). Never holds secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lemusync_core::DeviceId;
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
    /// What `sync` does with a conflict (sync-model §Conflicts).
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
}

/// `conflict_policy`: `ask` leaves conflicts for the player; `newest` resolves them to the
/// newest head. Either way nothing is lost: the other heads stay in history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    #[default]
    Ask,
    Newest,
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
    /// The argv prefix that starts the emulator, chosen by the user, never by a profile:
    /// `["/usr/bin/cemu"]` or `["flatpak", "run", "info.cemu.Cemu"]`. Never run through a shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exe: Vec<String>,
    /// Game file per title id (lowercase), for `{game_path}` in the profile's `launch.args`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub games: BTreeMap<String, PathBuf>,
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
            conflict_policy: ConflictPolicy::Ask,
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
        Self::parse(&text).map(Some)
    }

    /// Parses and validates `config.toml` text.
    ///
    /// # Errors
    /// `Config` for invalid TOML, an unknown `config_version` or an out-of-range value.
    pub fn parse(text: &str) -> Result<Self, EngineError> {
        let config: Self = toml::from_str(text).map_err(|e| parse_error(text, &e))?;
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
        Ok(config)
    }

    /// # Errors
    /// `Config` if it can't be serialised; `Io` if the atomic write fails.
    ///
    /// The user's comments and key order survive: the new values are merged into the file
    /// as it is on disk (config-and-cli §Config file).
    pub fn save(&self, path: &Path) -> Result<(), EngineError> {
        let fresh = toml::to_string(self).map_err(|e| EngineError::Config(e.to_string()))?;
        let text = match std::fs::read_to_string(path)
            .ok()
            .and_then(|old| old.parse::<toml_edit::DocumentMut>().ok())
        {
            Some(mut doc) => {
                let new: toml_edit::DocumentMut = fresh
                    .parse()
                    .map_err(|e: toml_edit::TomlError| EngineError::Config(e.to_string()))?;
                merge(doc.as_table_mut(), new.as_table());
                doc.to_string()
            }
            None => fresh,
        };
        fsx::write_atomic(path, text.as_bytes())?;
        Ok(())
    }
}

/// Makes `old` hold `new`'s values, keeping `old`'s comments and order where keys survive.
fn merge(old: &mut dyn toml_edit::TableLike, new: &dyn toml_edit::TableLike) {
    let gone: Vec<String> = old
        .iter()
        .map(|(k, _)| k.to_owned())
        .filter(|k| !new.contains_key(k))
        .collect();
    for k in gone {
        old.remove(&k);
    }
    for (k, n) in new.iter() {
        let same = match (old.get_mut(k), n) {
            (Some(o), n) if o.is_table_like() && n.is_table_like() => {
                if let (Some(o), Some(n)) = (o.as_table_like_mut(), n.as_table_like()) {
                    merge(o, n);
                }
                true
            }
            (Some(toml_edit::Item::Value(o)), toml_edit::Item::Value(n)) => {
                if o.to_string().trim() != n.to_string().trim() {
                    let decor = o.decor().clone();
                    *o = n.clone();
                    *o.decor_mut() = decor;
                }
                true
            }
            _ => false,
        };
        if !same {
            old.insert(k, n.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemusync_core::VersionId;

    #[test]
    fn save_keeps_the_users_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = sample();
        let text = format!("# mine\n{}", toml::to_string(&c).unwrap())
            .replace("conflict_policy", "# policy\nconflict_policy");
        std::fs::write(&path, &text).unwrap();
        c.grants.push(PathBuf::from("/home/u/other"));
        c.repo = None;
        c.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.starts_with("# mine\n"), "{saved}");
        assert!(saved.contains("# policy\nconflict_policy"), "{saved}");
        assert_eq!(LocalConfig::parse(&saved).unwrap(), c);
    }

    fn sample() -> LocalConfig {
        let mut c = LocalConfig::new(DeviceId(VersionId::from_parts(1, [3; 10])), "fedora-pc");
        c.repo = Some("r1".into());
        c.backend = Some(BackendConfig::Gdrive {
            root: "/LemuSync".into(),
            client_id: "abc.apps".into(),
        });
        c.grants = vec![PathBuf::from("/home/u/.local/share/Cemu")];
        c.emulators.insert(
            "cemu".into(),
            EmulatorOverride {
                root: Some(PathBuf::from("/opt/cemu")),
                exe: vec!["flatpak".into(), "run".into(), "info.cemu.Cemu".into()],
                games: BTreeMap::from([(
                    "101c9500".into(),
                    PathBuf::from("/games/Zelda BotW/game.wux"),
                )]),
            },
        );
        c
    }

    #[test]
    fn emulator_exe_and_games_round_trip_and_default_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let c = sample();
        c.save(&path).unwrap();
        assert_eq!(LocalConfig::load(&path).unwrap(), Some(c.clone()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("exe = [\"flatpak\", \"run\", \"info.cemu.Cemu\"]"),
            "{text}"
        );
        let mut bare = c;
        bare.emulators
            .insert("cemu".into(), EmulatorOverride::default());
        bare.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("exe") && !text.contains("games"), "{text}");
        assert_eq!(LocalConfig::load(&path).unwrap(), Some(bare));
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
    fn conflict_policy_defaults_to_ask_and_parses_newest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = toml::to_string(&sample()).unwrap();
        assert!(text.contains("conflict_policy = \"ask\""), "{text}");
        std::fs::write(&path, text.replace("conflict_policy = \"ask\"\n", "")).unwrap();
        assert_eq!(
            LocalConfig::load(&path).unwrap().unwrap().conflict_policy,
            ConflictPolicy::Ask
        );
        std::fs::write(&path, text.replace("\"ask\"", "\"newest\"")).unwrap();
        assert_eq!(
            LocalConfig::load(&path).unwrap().unwrap().conflict_policy,
            ConflictPolicy::Newest
        );
        std::fs::write(&path, text.replace("\"ask\"", "\"merge\"")).unwrap();
        assert!(LocalConfig::load(&path).is_err());
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
