//! Game references: aliases and the resolver's candidates (config-and-cli §Game references).
//! Queries come from the command line and names from disk: both are data (B2).

use std::collections::BTreeMap;

use lemusync_core::game_ref::{self, Candidate, RefError};

use crate::games::valid_key;
use crate::{Engine, EngineError, repo_config};

/// Characters of a user-typed query echoed back in an error.
const ECHO_MAX: usize = 100;
/// Candidates listed in an ambiguity error.
const LIST_MAX: usize = 20;

/// Control characters stripped and length capped.
pub(crate) fn echo(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(ECHO_MAX)
        .collect()
}

/// `^[a-z0-9][a-z0-9_-]{0,31}$`.
#[must_use]
pub fn valid_alias(alias: &str) -> bool {
    let b = alias.as_bytes();
    (1..=32).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

/// `"<emulator>/<game key>"`, both parts in the game-key alphabet.
#[must_use]
pub fn parse_target(target: &str) -> Option<(&str, &str)> {
    let (emu, key) = target.split_once('/')?;
    (valid_key(emu) && valid_key(key)).then_some((emu, key))
}

/// `id` is one of the bundled emulator profiles.
#[must_use]
pub fn is_emulator(id: &str) -> bool {
    crate::profiles::bundled().is_ok_and(|all| all.iter().any(|p| p.id == id))
}

fn label(c: &Candidate) -> String {
    let id = format!("{}/{}", c.emulator, c.key);
    match &c.name {
        Some(n) => format!("{} ({id})", echo(n)),
        None => id,
    }
}

/// The resolver's errors as usage errors: `game_unknown:<query>` or
/// `game_ambiguous:<every candidate>`.
fn usage(query: &str, e: &RefError<'_>) -> EngineError {
    match e {
        RefError::NotFound => EngineError::Usage(format!("game_unknown:{}", echo(query.trim()))),
        RefError::Ambiguous(list) => {
            let mut shown: Vec<String> = list.iter().take(LIST_MAX).map(|c| label(c)).collect();
            if list.len() > LIST_MAX {
                shown.push("…".into());
            }
            EngineError::Usage(format!("game_ambiguous:{}", shown.join("; ")))
        }
    }
}

/// Adds `(emulator, key, name)` to `out`, keeping the first name known.
fn add(
    out: &mut BTreeMap<(String, String), Option<String>>,
    emu: String,
    key: String,
    name: Option<String>,
) {
    let slot = out.entry((emu, key)).or_insert(None);
    if slot.is_none() {
        *slot = name;
    }
}

impl Engine {
    /// The candidates `games` (installed here) and the aliases give, without the repo.
    #[must_use]
    pub(crate) fn local_candidates(&self) -> Vec<Candidate> {
        let mut games = BTreeMap::new();
        for g in self.games().unwrap_or_default() {
            add(&mut games, g.emulator, g.key, g.name);
        }
        self.candidates_from(games)
    }

    fn candidates_from(
        &self,
        mut games: BTreeMap<(String, String), Option<String>>,
    ) -> Vec<Candidate> {
        for target in self.config.aliases.values() {
            if let Some((emu, key)) = parse_target(target) {
                add(&mut games, emu.to_owned(), key.to_owned(), None);
            }
        }
        games
            .into_iter()
            .map(|((emulator, key), name)| {
                let target = format!("{emulator}/{key}");
                Candidate {
                    aliases: self
                        .config
                        .aliases
                        .iter()
                        .filter(|(_, t)| **t == target)
                        .map(|(a, _)| a.clone())
                        .collect(),
                    emulator,
                    key,
                    name,
                }
            })
            .collect()
    }

    /// Every game a command can name: installed here, aliased, or synced in the repo (when this
    /// device has one; those have no name).
    ///
    /// # Errors
    /// Storage errors while reading the repo config.
    pub async fn game_candidates(&mut self) -> Result<Vec<Candidate>, EngineError> {
        let mut games = BTreeMap::new();
        for g in self.games().unwrap_or_default() {
            add(&mut games, g.emulator, g.key, g.name);
        }
        if self.config.repo.is_some() {
            self.ensure_repo().await?;
            let config = repo_config::load(self.repo_ref()?, &self.state)
                .await?
                .config;
            for pid in config.players.keys() {
                for (emu, key) in config.games_of(*pid) {
                    if valid_key(&emu) && valid_key(&key) {
                        add(&mut games, emu, key, None);
                    }
                }
            }
        }
        Ok(self.candidates_from(games))
    }

    /// Resolves a game typed by the user to `(emulator, key)`.
    ///
    /// # Errors
    /// `Usage` (`game_unknown`, `game_ambiguous`); storage errors as [`Engine::game_candidates`].
    pub async fn resolve_game(
        &mut self,
        query: &str,
        emulator: Option<&str>,
    ) -> Result<(String, String), EngineError> {
        let candidates = self.game_candidates().await?;
        game_ref::resolve(query, emulator, &candidates)
            .map(|c| (c.emulator.clone(), c.key.clone()))
            .map_err(|e| usage(query, &e))
    }

    /// Checks that `alias` may point at `target`: valid syntax, a bundled emulator and a key in
    /// the game-key alphabet (the game needn't be installed or synced yet), not the key of
    /// another game in `known`, not taken by another game.
    pub(crate) fn check_alias(
        &self,
        alias: &str,
        target: &str,
        known: &[Candidate],
    ) -> Result<(), EngineError> {
        if !valid_alias(alias) {
            return Err(EngineError::Usage(format!("alias_invalid:{}", echo(alias))));
        }
        let Some((emu, key)) = parse_target(target) else {
            return Err(EngineError::Usage(format!(
                "config_bad_value:aliases.{alias}"
            )));
        };
        if !is_emulator(emu) {
            return Err(EngineError::Usage(format!(
                "config_unknown_emulator:{}",
                echo(emu)
            )));
        }
        if known
            .iter()
            .any(|c| c.key.eq_ignore_ascii_case(alias) && (c.emulator != emu || c.key != key))
        {
            return Err(EngineError::Usage(format!("alias_is_key:{alias}")));
        }
        if self.config.aliases.get(alias).is_some_and(|t| t != target) {
            return Err(EngineError::Usage(format!("alias_taken:{alias}")));
        }
        Ok(())
    }

    /// `games alias <game> <alias>`: resolves `query` and stores the resolved
    /// `<emulator>/<key>`, never the query. Refuses an ambiguous game. A game no candidate
    /// knows may be given as `<emulator>/<key>` or `--emu <id> <key>`.
    ///
    /// # Errors
    /// As [`Engine::resolve_game`] and [`Engine::config_set`].
    pub async fn alias_add(
        &mut self,
        query: &str,
        emulator: Option<&str>,
        alias: &str,
    ) -> Result<(String, String), EngineError> {
        let candidates = self.game_candidates().await?;
        let (emu, key) = match game_ref::resolve(query, emulator, &candidates) {
            Ok(c) => (c.emulator.clone(), c.key.clone()),
            // A game nobody has installed or synced yet: `<emulator>/<key>`, or `--emu` + key,
            // the same targets `config set aliases.<alias>` takes.
            Err(RefError::NotFound) => {
                let lower = query.trim().to_ascii_lowercase();
                let target = match emulator {
                    Some(e) if !lower.contains('/') => format!("{e}/{lower}"),
                    _ => lower,
                };
                match parse_target(&target) {
                    Some((e, k)) if emulator.is_none_or(|want| want == e) => {
                        (e.to_owned(), k.to_owned())
                    }
                    _ => return Err(usage(query, &RefError::NotFound)),
                }
            }
            Err(e) => return Err(usage(query, &e)),
        };
        self.write_alias(alias, &format!("{emu}/{key}"), &candidates)?;
        Ok((emu, key))
    }

    /// `games unalias <alias>`.
    ///
    /// # Errors
    /// `Usage` (`alias_unknown`) when there's no such alias; as [`Engine::config_unset`].
    pub fn alias_remove(&mut self, alias: &str) -> Result<(), EngineError> {
        if !self.config.aliases.contains_key(alias) {
            return Err(EngineError::Usage(format!("alias_unknown:{}", echo(alias))));
        }
        self.config_unset(&format!("aliases.{alias}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_syntax() {
        for ok in ["botw", "a", "9", "mk8-dx_2", &"a".repeat(32)] {
            assert!(valid_alias(ok), "{ok}");
        }
        for bad in ["", "-a", "_a", "Botw", "a b", "a.b", "ñ", &"a".repeat(33)] {
            assert!(!valid_alias(bad), "{bad}");
        }
    }

    #[test]
    fn targets_are_emulator_slash_key() {
        assert_eq!(parse_target("cemu/101c9400"), Some(("cemu", "101c9400")));
        for bad in [
            "cemu",
            "cemu/",
            "/101c9400",
            "cemu/a/b",
            "cemu/../x",
            "Cemu/1",
        ] {
            assert_eq!(parse_target(bad), None, "{bad}");
        }
    }

    #[test]
    fn ambiguity_lists_every_candidate() {
        let c = |key: &str, name: Option<&str>| Candidate {
            emulator: "dolphin".into(),
            key: key.into(),
            name: name.map(Into::into),
            aliases: vec![],
        };
        let all = [c("gale01", Some("Melee (USA)")), c("galp01", None)];
        let EngineError::Usage(text) = usage("melee", &RefError::Ambiguous(all.iter().collect()))
        else {
            panic!()
        };
        assert_eq!(
            text,
            "game_ambiguous:Melee (USA) (dolphin/gale01); dolphin/galp01"
        );
        let EngineError::Usage(text) = usage(" x\u{1b} ", &RefError::NotFound) else {
            panic!()
        };
        assert_eq!(text, "game_unknown:x");
    }
}
