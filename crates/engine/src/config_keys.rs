//! `lemusync config`: typed, dotted-key access to `config.toml` (config-and-cli §Commands).
//! Edits keep the user's comments and key order, are validated by parsing the result back
//! into `LocalConfig`, and are written atomically while the engine holds the state lock (B3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;
use toml_edit::{DocumentMut, Item, TableLike};

use crate::{ConflictPolicy, Engine, EngineError, LocalConfig, fsx, profiles};

/// A hand-edited config.toml is a few KiB; anything bigger isn't one (B2).
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Characters of a user-typed key echoed back in an error.
const KEY_ECHO_MAX: usize = 100;

/// A settable key.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    ConflictPolicy,
    LeaseTtl,
    Root(String),
    Exe(String),
    Game(String, String),
}

impl Key {
    /// The table path and the leaf key inside it.
    fn location(&self) -> (Vec<&str>, &str) {
        match self {
            Self::ConflictPolicy => (vec![], "conflict_policy"),
            Self::LeaseTtl => (vec!["leases"], "ttl_secs"),
            Self::Root(id) => (vec!["emulators", id], "root"),
            Self::Exe(id) => (vec!["emulators", id], "exe"),
            Self::Game(id, game) => (vec!["emulators", id, "games"], game),
        }
    }

    fn canonical(&self) -> String {
        let (path, leaf) = self.location();
        let mut parts = path;
        parts.push(leaf);
        parts.join(".")
    }
}

/// Control characters stripped and length capped: the key came from the command line.
fn echo(key: &str) -> String {
    key.chars()
        .filter(|c| !c.is_control())
        .take(KEY_ECHO_MAX)
        .collect()
}

fn bad_value(key: &str) -> EngineError {
    EngineError::Usage(format!("config_bad_value:{}", echo(key)))
}

fn valid_game_key(game: &str) -> bool {
    (1..=64).contains(&game.len())
        && game
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// # Errors
/// `Usage` naming why: a key managed by another command, a fixed key, an unknown emulator,
/// or an unknown key.
fn parse_key(key: &str) -> Result<Key, EngineError> {
    let head = key.split('.').next().unwrap_or_default();
    let owner = match head {
        "repo" => Some("lemusync init"),
        "backend" => Some("lemusync backend add"),
        "grants" => Some("lemusync grant add"),
        _ => None,
    };
    if let Some(owner) = owner {
        return Err(EngineError::Usage(format!("config_managed:{owner}")));
    }
    if matches!(head, "config_version" | "device") {
        return Err(EngineError::Usage(format!("config_fixed:{}", echo(key))));
    }
    let unknown = || EngineError::Usage(format!("config_unknown_key:{}", echo(key)));
    match key {
        "conflict_policy" => return Ok(Key::ConflictPolicy),
        "leases.ttl_secs" => return Ok(Key::LeaseTtl),
        _ => {}
    }
    let parts: Vec<&str> = key.splitn(4, '.').collect();
    let (["emulators", id, _] | ["emulators", id, _, _]) = parts.as_slice() else {
        return Err(unknown());
    };
    if !profiles::bundled()?.iter().any(|p| p.id == *id) {
        return Err(EngineError::Usage(format!(
            "config_unknown_emulator:{}",
            echo(id)
        )));
    }
    let id = (*id).to_owned();
    match parts.as_slice() {
        [_, _, "root"] => Ok(Key::Root(id)),
        [_, _, "exe"] => Ok(Key::Exe(id)),
        [_, _, "games", game] => {
            // Lowercased like `play` does (sync-model §Launcher mode).
            let game = game.to_lowercase();
            if valid_game_key(&game) {
                Ok(Key::Game(id, game))
            } else {
                Err(bad_value(key))
            }
        }
        _ => Err(unknown()),
    }
}

fn absolute_path(key: &str, value: &str) -> Result<Item, EngineError> {
    let path = Path::new(value);
    if value.chars().any(char::is_control)
        || !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(bad_value(key));
    }
    Ok(toml_edit::value(value))
}

/// A table that holds settable keys: `leases`, `emulators`, `emulators.<id>`,
/// `emulators.<id>.games` (`get` shows it whole).
fn is_table(key: &str) -> Result<bool, EngineError> {
    let parts: Vec<&str> = key.split('.').collect();
    Ok(match parts.as_slice() {
        ["leases" | "emulators"] => true,
        ["emulators", id] | ["emulators", id, "games"] => {
            profiles::bundled()?.iter().any(|p| p.id == *id)
        }
        _ => false,
    })
}

/// The TOML value for `key`, from the command-line words.
fn typed_value(key: &Key, raw_key: &str, values: &[String]) -> Result<Item, EngineError> {
    let one = || match values {
        [v] => Ok(v.as_str()),
        _ => Err(EngineError::Usage(format!(
            "config_one_value:{}",
            echo(raw_key)
        ))),
    };
    match key {
        Key::ConflictPolicy => {
            let v = one()?;
            serde_json::from_value::<ConflictPolicy>(Json::String(v.to_owned()))
                .map_err(|_| bad_value(raw_key))?;
            Ok(toml_edit::value(v))
        }
        Key::LeaseTtl => {
            let ttl: i64 = one()?.parse().map_err(|_| bad_value(raw_key))?;
            Ok(toml_edit::value(ttl))
        }
        Key::Root(_) | Key::Game(..) => absolute_path(raw_key, one()?),
        Key::Exe(_) => {
            if values.is_empty() || values.iter().any(String::is_empty) {
                return Err(bad_value(raw_key));
            }
            Ok(toml_edit::value(
                values
                    .iter()
                    .map(String::as_str)
                    .collect::<toml_edit::Array>(),
            ))
        }
    }
}

fn set_in(table: &mut dyn TableLike, path: &[&str], leaf: &str, item: Item) -> Option<()> {
    let Some((head, rest)) = path.split_first() else {
        table.insert(leaf, item);
        return Some(());
    };
    if table.get(head).is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        table.insert(head, Item::Table(t));
    }
    set_in(table.get_mut(head)?.as_table_like_mut()?, rest, leaf, item)
}

/// Removes `leaf`, then every table on the path that the removal left empty.
fn unset_in(table: &mut dyn TableLike, path: &[&str], leaf: &str) {
    let Some((head, rest)) = path.split_first() else {
        table.remove(leaf);
        return;
    };
    let Some(child) = table.get_mut(head).and_then(Item::as_table_like_mut) else {
        return;
    };
    unset_in(child, rest, leaf);
    if child.is_empty() {
        table.remove(head);
    }
}

/// `config.toml` as plain values: nested tables become dotted keys, arrays stay whole.
fn flatten(prefix: &str, value: Json, out: &mut BTreeMap<String, Json>) {
    match value {
        Json::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        other => {
            out.insert(prefix.to_owned(), other);
        }
    }
}

impl Engine {
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.env.locations.config_dir.join("config.toml")
    }

    /// Every value in `config.toml`, by dotted key, defaults included.
    ///
    /// # Errors
    /// `Config` if the config can't be turned into values (a bug).
    pub fn config_values(&self) -> Result<BTreeMap<String, Json>, EngineError> {
        let value =
            serde_json::to_value(&self.config).map_err(|e| EngineError::Config(e.to_string()))?;
        let mut out = BTreeMap::new();
        flatten("", value, &mut out);
        Ok(out)
    }

    /// One value, or a whole table as one object; `null` for a known key that isn't set.
    ///
    /// # Errors
    /// `Usage` for an unknown key or emulator.
    pub fn config_get(&self, key: &str) -> Result<Json, EngineError> {
        let key = match parse_key(key) {
            Ok(k) => k.canonical(),
            Err(EngineError::Usage(why))
                if why.starts_with("config_managed:") || why.starts_with("config_fixed:") =>
            {
                key.to_owned()
            }
            Err(_) if is_table(key)? => key.to_owned(),
            Err(e) => return Err(e),
        };
        let values = self.config_values()?;
        if let Some(v) = values.get(&key) {
            return Ok(v.clone());
        }
        let prefix = format!("{key}.");
        let sub: serde_json::Map<String, Json> = values
            .into_iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|r| (r.to_owned(), v)))
            .collect();
        Ok(if sub.is_empty() {
            Json::Null
        } else {
            Json::Object(sub)
        })
    }

    /// Sets a settable key. `values` has one word, or several for `emulators.<id>.exe`.
    ///
    /// # Errors
    /// `Usage` for a managed, fixed or unknown key or a bad value (the file is untouched);
    /// `Config`/`Io` if the file can't be read or written.
    pub fn config_set(&mut self, key: &str, values: &[String]) -> Result<(), EngineError> {
        let parsed = parse_key(key)?;
        let item = typed_value(&parsed, key, values)?;
        let (path, leaf) = parsed.location();
        self.edit_config(key, |doc| {
            set_in(doc.as_table_mut(), &path, leaf, item).is_some()
        })
    }

    /// Removes a settable key, back to its default.
    ///
    /// # Errors
    /// As [`Engine::config_set`].
    pub fn config_unset(&mut self, key: &str) -> Result<(), EngineError> {
        let parsed = parse_key(key)?;
        let (path, leaf) = parsed.location();
        self.edit_config(key, |doc| {
            unset_in(doc.as_table_mut(), &path, leaf);
            true
        })
    }

    /// Reads the file, applies `edit`, checks the result parses as a valid `LocalConfig`,
    /// then writes it atomically. The state lock (taken in `Engine::open`) keeps other
    /// writers out.
    fn edit_config(
        &mut self,
        key: &str,
        edit: impl FnOnce(&mut DocumentMut) -> bool,
    ) -> Result<(), EngineError> {
        let path = self.config_path();
        if std::fs::metadata(&path)?.len() > MAX_CONFIG_BYTES {
            return Err(EngineError::Config("config.toml is too large".into()));
        }
        let text = std::fs::read_to_string(&path)?;
        let mut doc: DocumentMut = text
            .parse()
            .map_err(|_| EngineError::Config("config.toml isn't valid TOML".into()))?;
        if !edit(&mut doc) {
            return Err(bad_value(key));
        }
        let new_text = doc.to_string();
        let config = LocalConfig::parse(&new_text).map_err(|_| bad_value(key))?;
        if new_text != text {
            fsx::write_atomic(&path, new_text.as_bytes())?;
        }
        self.config = config;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_to_their_family_or_say_who_owns_them() {
        assert_eq!(
            parse_key("emulators.cemu.games.101C9500").unwrap(),
            Key::Game("cemu".into(), "101c9500".into())
        );
        assert_eq!(
            parse_key("emulators.cemu.exe").unwrap(),
            Key::Exe("cemu".into())
        );
        let usage = |k: &str| match parse_key(k) {
            Err(EngineError::Usage(s)) => s,
            other => panic!("{k}: {other:?}"),
        };
        assert_eq!(usage("grants"), "config_managed:lemusync grant add");
        assert_eq!(usage("backend.root"), "config_managed:lemusync backend add");
        assert_eq!(usage("device.name"), "config_fixed:device.name");
        assert_eq!(usage("emulators.x.root"), "config_unknown_emulator:x");
        assert_eq!(
            usage("emulators.cemu.other"),
            "config_unknown_key:emulators.cemu.other"
        );
        assert_eq!(
            usage("emulators.cemu.games.a/b"),
            "config_bad_value:emulators.cemu.games.a/b"
        );
        assert_eq!(usage("x\u{1b}[31m"), "config_unknown_key:x[31m");
    }

    #[test]
    fn paths_must_be_absolute_without_parent_steps() {
        assert!(absolute_path("k", "/opt/cemu").is_ok());
        assert!(absolute_path("k", "/opt/../etc").is_err());
        assert!(absolute_path("k", "opt/cemu").is_err());
    }

    #[test]
    fn tables_with_settable_keys_are_known() {
        assert!(is_table("emulators.cemu").unwrap());
        assert!(is_table("emulators.cemu.games").unwrap());
        assert!(is_table("leases").unwrap());
        assert!(!is_table("emulators.nope").unwrap());
        assert!(!is_table("emulators.cemu.other").unwrap());
    }

    #[test]
    fn unset_prunes_tables_it_empties() {
        let mut doc: DocumentMut = "[leases]\nttl_secs = 600\n".parse().unwrap();
        unset_in(doc.as_table_mut(), &["leases"], "ttl_secs");
        assert_eq!(doc.to_string(), "");
    }
}
