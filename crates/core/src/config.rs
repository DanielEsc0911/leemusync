//! Repo config: players and per-device account mappings. Spec: docs/specs/sync-model.md (Repo config).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{DeviceId, PlayerId, VersionId};

pub const CONFIG_FORMAT: u32 = 1;
const MAX_NAME: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    pub players: BTreeMap<PlayerId, Player>,
    /// device → player → emulator id → account id.
    pub mappings: BTreeMap<DeviceId, BTreeMap<PlayerId, BTreeMap<String, String>>>,
    /// player → `"<emulator>/<game key>"` of every stream with a version.
    #[serde(default)]
    pub games: BTreeMap<PlayerId, BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigVersion {
    pub format: u32,
    pub id: VersionId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub created_at: u64,
    pub config: RepoConfig,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("a player name needs 1 to 32 visible characters")]
    BadName,
    #[error("the player name `{0}` is taken")]
    NameTaken(String),
    #[error("no player `{0}`")]
    UnknownPlayer(String),
    #[error("account {account} is already mapped to {player}")]
    AccountTaken { account: String, player: String },
    #[error("unsupported config format {0}")]
    Format(u32),
    #[error("config id doesn't match its object name")]
    WrongId,
    #[error("config JSON: {0}")]
    Json(String),
}

impl RepoConfig {
    /// # Errors
    /// `BadName` or `NameTaken`.
    pub fn add_player(&mut self, id: PlayerId, name: &str) -> Result<(), ConfigError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control)
        {
            return Err(ConfigError::BadName);
        }
        if self.player_named(name).is_some() {
            return Err(ConfigError::NameTaken(name.to_owned()));
        }
        self.players.insert(
            id,
            Player {
                name: name.to_owned(),
            },
        );
        Ok(())
    }

    #[must_use]
    pub fn player_named(&self, name: &str) -> Option<PlayerId> {
        let name = name.trim().to_lowercase();
        self.players
            .iter()
            .find(|(_, p)| p.name.to_lowercase() == name)
            .map(|(id, _)| *id)
    }

    /// # Errors
    /// `UnknownPlayer`, or `AccountTaken` when another player uses that account on this device.
    pub fn map_account(
        &mut self,
        device: DeviceId,
        player: PlayerId,
        emulator: &str,
        account: &str,
    ) -> Result<(), ConfigError> {
        if !self.players.contains_key(&player) {
            return Err(ConfigError::UnknownPlayer(player.to_string()));
        }
        if let Some(other) = self.player_on_account(device, emulator, account)
            && other != player
        {
            let name = self
                .players
                .get(&other)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            return Err(ConfigError::AccountTaken {
                account: account.to_owned(),
                player: name,
            });
        }
        self.mappings
            .entry(device)
            .or_default()
            .entry(player)
            .or_default()
            .insert(emulator.to_owned(), account.to_owned());
        Ok(())
    }

    #[must_use]
    pub fn account_of(&self, device: DeviceId, player: PlayerId, emulator: &str) -> Option<&str> {
        self.mappings
            .get(&device)?
            .get(&player)?
            .get(emulator)
            .map(String::as_str)
    }

    #[must_use]
    pub fn player_on_account(
        &self,
        device: DeviceId,
        emulator: &str,
        account: &str,
    ) -> Option<PlayerId> {
        self.mappings
            .get(&device)?
            .iter()
            .find_map(|(player, by_emu)| {
                (by_emu.get(emulator).map(String::as_str) == Some(account)).then_some(*player)
            })
    }

    pub fn add_game(&mut self, player: PlayerId, emulator: &str, game: &str) -> bool {
        self.games
            .entry(player)
            .or_default()
            .insert(format!("{emulator}/{game}"))
    }

    #[must_use]
    pub fn games_of(&self, player: PlayerId) -> Vec<(String, String)> {
        self.games
            .get(&player)
            .into_iter()
            .flatten()
            .filter_map(|g| g.split_once('/').map(|(e, k)| (e.to_owned(), k.to_owned())))
            .collect()
    }

    /// Union of several heads (D30). Returns the merged config and the names it had to rename.
    #[must_use]
    pub fn merge(configs: &[&RepoConfig]) -> (RepoConfig, Vec<String>) {
        let mut out = RepoConfig::default();
        for c in configs {
            for (id, player) in &c.players {
                out.players.entry(*id).or_insert_with(|| player.clone());
            }
        }
        let mut renamed = Vec::new();
        let mut used: Vec<String> = Vec::new();
        for player in out.players.values_mut() {
            let base = player.name.clone();
            let mut name = base.clone();
            let mut n = 2;
            while used.contains(&name.to_lowercase()) {
                name = format!("{base} ({n})");
                n += 1;
            }
            if name != base {
                renamed.push(name.clone());
                player.name.clone_from(&name);
            }
            used.push(name.to_lowercase());
        }
        for c in configs {
            for (device, by_player) in &c.mappings {
                for (player, by_emu) in by_player {
                    for (emu, account) in by_emu {
                        let slot = out
                            .mappings
                            .entry(*device)
                            .or_default()
                            .entry(*player)
                            .or_default();
                        match slot.get(emu) {
                            Some(existing) if existing <= account => {}
                            _ => {
                                slot.insert(emu.clone(), account.clone());
                            }
                        }
                    }
                }
            }
        }
        for c in configs {
            for (player, games) in &c.games {
                out.games
                    .entry(*player)
                    .or_default()
                    .extend(games.iter().cloned());
            }
        }
        for by_player in out.mappings.values_mut() {
            let mut taken: BTreeMap<(String, String), PlayerId> = BTreeMap::new();
            for (player, by_emu) in by_player.iter_mut() {
                by_emu.retain(|emu, account| {
                    let key = (emu.clone(), account.clone());
                    taken.insert(key, *player).is_none()
                });
            }
            by_player.retain(|_, by_emu| !by_emu.is_empty());
        }
        (out, renamed)
    }
}

impl ConfigVersion {
    /// # Errors
    /// `ConfigError::Json` if serialisation fails.
    pub fn to_json(&self) -> Result<Vec<u8>, ConfigError> {
        serde_json::to_vec(self).map_err(|e| ConfigError::Json(e.to_string()))
    }

    /// # Errors
    /// `Json`, `Format` or `WrongId`.
    pub fn from_json(bytes: &[u8], id: &VersionId) -> Result<Self, ConfigError> {
        let v: Self =
            serde_json::from_slice(bytes).map_err(|e| ConfigError::Json(e.to_string()))?;
        if v.format != CONFIG_FORMAT {
            return Err(ConfigError::Format(v.format));
        }
        if v.id != *id || v.parents.contains(id) {
            return Err(ConfigError::WrongId);
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u64) -> PlayerId {
        PlayerId(VersionId::from_parts(n, [1; 10]))
    }
    fn d(n: u64) -> DeviceId {
        DeviceId(VersionId::from_parts(n, [2; 10]))
    }

    #[test]
    fn players_need_unique_valid_names() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "Daniel").unwrap();
        assert_eq!(
            c.add_player(p(2), "daniel"),
            Err(ConfigError::NameTaken("daniel".into()))
        );
        assert_eq!(c.add_player(p(3), "  "), Err(ConfigError::BadName));
        assert_eq!(
            c.add_player(p(4), &"x".repeat(33)),
            Err(ConfigError::BadName)
        );
        assert_eq!(c.add_player(p(5), "a\nb"), Err(ConfigError::BadName));
        assert_eq!(c.player_named("DANIEL"), Some(p(1)));
    }

    #[test]
    fn the_same_id_trap_is_per_device() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "A").unwrap();
        c.add_player(p(2), "B").unwrap();
        c.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        c.map_account(d(2), p(2), "cemu", "80000001").unwrap();
        assert_eq!(c.account_of(d(1), p(1), "cemu"), Some("80000001"));
        assert_eq!(c.player_on_account(d(2), "cemu", "80000001"), Some(p(2)));
        assert_eq!(
            c.map_account(d(1), p(2), "cemu", "80000001"),
            Err(ConfigError::AccountTaken {
                account: "80000001".into(),
                player: "A".into()
            })
        );
        assert!(
            c.map_account(d(1), p(9), "cemu", "80000002").is_err(),
            "unknown player"
        );
    }

    #[test]
    fn merge_is_a_union_and_renames_name_clashes() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        a.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        let mut b = a.clone();
        b.map_account(d(2), p(1), "cemu", "80000002").unwrap();
        let mut c = RepoConfig::default();
        c.add_player(p(3), "a").unwrap();
        c.map_account(d(3), p(3), "cemu", "80000001").unwrap();
        let (m, renamed) = RepoConfig::merge(&[&a, &b, &c]);
        assert_eq!(m.players.len(), 2, "p1 is in two configs but counts once");
        assert_eq!(m.players[&p(1)].name, "A", "a shared id keeps its name");
        assert_eq!(m.players[&p(3)].name, "a (2)");
        assert_eq!(renamed, vec!["a (2)".to_string()]);
        assert_eq!(m.account_of(d(1), p(1), "cemu"), Some("80000001"));
        assert_eq!(m.account_of(d(2), p(1), "cemu"), Some("80000002"));
        assert_eq!(m.account_of(d(3), p(3), "cemu"), Some("80000001"));
        assert_eq!(
            RepoConfig::merge(&[&c, &b, &a]).0,
            m,
            "order doesn't matter"
        );
        assert_eq!(
            RepoConfig::merge(&[&b, &c, &a]).0,
            m,
            "order doesn't matter"
        );
    }

    #[test]
    fn games_are_listed_per_player_and_merged_by_union() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        let mut b = a.clone();
        assert!(a.add_game(p(1), "cemu", "101c9400"));
        assert!(!a.add_game(p(1), "cemu", "101c9400"), "already known");
        b.add_game(p(1), "cemu", "101c9500");
        let (m, _) = RepoConfig::merge(&[&a, &b]);
        assert_eq!(
            m.games_of(p(1)),
            vec![
                ("cemu".to_string(), "101c9400".to_string()),
                ("cemu".to_string(), "101c9500".to_string())
            ]
        );
    }

    #[test]
    fn merge_drops_a_second_player_on_one_account() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        a.add_player(p(2), "B").unwrap();
        let mut b = a.clone();
        a.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        b.map_account(d(1), p(2), "cemu", "80000001").unwrap();
        let (m, _) = RepoConfig::merge(&[&a, &b]);
        assert_eq!(m.player_on_account(d(1), "cemu", "80000001"), Some(p(1)));
        assert_eq!(m.account_of(d(1), p(2), "cemu"), None);
    }

    #[test]
    fn config_version_round_trips() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "A").unwrap();
        let v = ConfigVersion {
            format: CONFIG_FORMAT,
            id: VersionId::from_parts(10, [3; 10]),
            parents: vec![],
            device: d(1),
            created_at: 10,
            config: c,
        };
        let bytes = v.to_json().unwrap();
        assert_eq!(ConfigVersion::from_json(&bytes, &v.id).unwrap(), v);
        assert!(ConfigVersion::from_json(&bytes, &VersionId::from_parts(11, [3; 10])).is_err());
    }
}
