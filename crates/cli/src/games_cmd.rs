//! `lemusync games`, and game names for every other output (config-and-cli §Commands).
//! Local and offline: the engine's game catalog, never the store.

use lemusync_api::{GameSource, GamesReport};
use lemusync_engine::Engine;

use crate::i18n::{I18n, s};
use crate::output;

/// Characters of a name or path shown at most.
const SHOWN_MAX: usize = 300;

/// Control characters stripped and length capped: names and paths come from disk (B2).
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(SHOWN_MAX)
        .collect()
}

/// "Name (key)" when a name is known, else the key.
pub fn label(name: Option<&str>, key: &str) -> String {
    match name {
        Some(name) => format!("{} ({})", clean(name), clean(key)),
        None => clean(key),
    }
}

/// [`label`] with the name from the engine's catalog on this device.
pub fn label_of(engine: &Engine, emulator: &str, key: &str) -> String {
    let key = key.to_lowercase();
    let names = engine.game_names();
    label(
        names
            .get(&(emulator.to_owned(), key.clone()))
            .map(String::as_str),
        &key,
    )
}

fn source_key(source: GameSource) -> &'static str {
    match source {
        GameSource::Config => "games-source-config",
        GameSource::Emulator => "games-source-emulator",
        GameSource::Scan => "games-source-scan",
    }
}

pub fn print(t: &I18n, r: &GamesReport) {
    if r.games.is_empty() {
        output::line(t, "games-none", &[]);
    }
    let no_path = t.t("games-no-path", &[]);
    for g in &r.games {
        output::line(
            t,
            "games-line",
            &[
                ("emulator", s(g.emulator.clone())),
                ("name", s(clean(g.name.as_deref().unwrap_or(&g.key)))),
                ("key", s(clean(&g.key))),
                (
                    "path",
                    s(g.path.as_deref().map_or_else(|| no_path.clone(), clean)),
                ),
                ("source", s(t.t(source_key(g.source), &[]))),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_show_the_name_and_key_or_just_the_key() {
        assert_eq!(
            label(Some("Some\u{1b}[2J Game"), "101c9400"),
            "Some [2J Game (101c9400)"
        );
        assert_eq!(label(None, "101c9500"), "101c9500");
    }
}
