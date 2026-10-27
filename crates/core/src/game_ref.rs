//! Game references: find a game by alias, key or loosely typed name.
//! Spec: docs/specs/config-and-cli.md (Game references).

/// A game the user may mean: installed here, or known from the repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub emulator: String,
    pub key: String,
    pub name: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RefError<'a> {
    NotFound,
    /// Every match of the winning tier, in candidate order.
    Ambiguous(Vec<&'a Candidate>),
}

/// Folds accented Latin letters to ASCII, lowercases, drops `(…)` and `[…]`
/// groups and collapses every non-alphanumeric run into one space.
#[must_use]
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut pending_space = false;
    for c in s.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                pending_space = true;
                continue;
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                pending_space = true;
                continue;
            }
            _ if depth > 0 => continue,
            _ => {}
        }
        let folded = fold(c);
        for f in folded.chars() {
            let f = f.to_ascii_lowercase();
            if f.is_ascii_alphanumeric() {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(f);
            } else {
                pending_space = true;
            }
        }
    }
    out
}

fn fold(c: char) -> std::borrow::Cow<'static, str> {
    let s: &'static str = match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'Ā' | 'ā' | 'Ă'
        | 'ă' | 'Ą' | 'ą' => "a",
        'Æ' | 'æ' => "ae",
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Ĉ' | 'ĉ' | 'Ċ' | 'ċ' | 'Č' | 'č' => "c",
        'Ð' | 'ð' | 'Ď' | 'ď' | 'Đ' | 'đ' => "d",
        'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' | 'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę'
        | 'ę' | 'Ě' | 'ě' => "e",
        'Ĝ' | 'ĝ' | 'Ğ' | 'ğ' | 'Ġ' | 'ġ' | 'Ģ' | 'ģ' => "g",
        'Ĥ' | 'ĥ' | 'Ħ' | 'ħ' => "h",
        'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' | 'Ĩ' | 'ĩ' | 'Ī' | 'ī' | 'Ĭ' | 'ĭ' | 'Į'
        | 'į' | 'İ' | 'ı' => "i",
        'Ĳ' | 'ĳ' => "ij",
        'Ĵ' | 'ĵ' => "j",
        'Ķ' | 'ķ' | 'ĸ' => "k",
        'Ĺ' | 'ĺ' | 'Ļ' | 'ļ' | 'Ľ' | 'ľ' | 'Ŀ' | 'ŀ' | 'Ł' | 'ł' => "l",
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ņ' | 'ņ' | 'Ň' | 'ň' | 'ŉ' | 'Ŋ' | 'ŋ' => "n",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ō' | 'ō' | 'Ŏ'
        | 'ŏ' | 'Ő' | 'ő' => "o",
        'Œ' | 'œ' => "oe",
        'Ŕ' | 'ŕ' | 'Ŗ' | 'ŗ' | 'Ř' | 'ř' => "r",
        'Ś' | 'ś' | 'Ŝ' | 'ŝ' | 'Ş' | 'ş' | 'Š' | 'š' | 'ſ' => "s",
        'ß' => "ss",
        'Ţ' | 'ţ' | 'Ť' | 'ť' | 'Ŧ' | 'ŧ' => "t",
        'Þ' | 'þ' => "th",
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' | 'Ũ' | 'ũ' | 'Ū' | 'ū' | 'Ŭ' | 'ŭ' | 'Ů'
        | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => "u",
        'Ŵ' | 'ŵ' => "w",
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' | 'Ÿ' => "y",
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => "z",
        _ => return std::borrow::Cow::Owned(c.to_string()),
    };
    std::borrow::Cow::Borrowed(s)
}

/// Resolves `query` against `candidates`, optionally only those of `emulator`.
/// Tiers, first with any match wins: exact alias, key (case-insensitive),
/// normalized name. No fuzzy tier (P1). An alias equal to another candidate's key (a
/// hand-edited file) is ignored, so a key always means its own game.
///
/// # Errors
/// `NotFound` when no tier matches; `Ambiguous` when the winning tier has
/// more than one match.
pub fn resolve<'a>(
    query: &str,
    emulator: Option<&str>,
    candidates: &'a [Candidate],
) -> Result<&'a Candidate, RefError<'a>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(RefError::NotFound);
    }
    let pool: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| emulator.is_none_or(|e| c.emulator.eq_ignore_ascii_case(e)))
        .collect();
    let normalized = normalize(query);
    let tiers: [&dyn Fn(&Candidate) -> bool; 3] = [
        &|c| {
            c.aliases.iter().any(|a| a == query)
                && !candidates
                    .iter()
                    .any(|o| o.key.eq_ignore_ascii_case(query) && o != c)
        },
        &|c| c.key.eq_ignore_ascii_case(query),
        &|c| {
            !normalized.is_empty()
                && c.name
                    .as_deref()
                    .is_some_and(|n| normalize(n) == normalized)
        },
    ];
    for tier in tiers {
        let hits: Vec<&Candidate> = pool.iter().copied().filter(|c| tier(c)).collect();
        match hits.len() {
            0 => {}
            1 => return Ok(hits[0]),
            _ => return Err(RefError::Ambiguous(hits)),
        }
    }
    Err(RefError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(emu: &str, key: &str, name: Option<&str>, aliases: &[&str]) -> Candidate {
        Candidate {
            emulator: emu.into(),
            key: key.into(),
            name: name.map(Into::into),
            aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
        }
    }

    fn catalog() -> Vec<Candidate> {
        vec![
            cand(
                "cemu",
                "101c9400",
                Some("The Legend of Zelda: Breath of the Wild"),
                &["botw"],
            ),
            cand(
                "dolphin",
                "GALE01",
                Some("Super Smash Bros. Melee (USA)"),
                &[],
            ),
            cand(
                "dolphin",
                "GALP01",
                Some("Super Smash Bros. Melee (EUR)"),
                &[],
            ),
            cand(
                "dolphin",
                "GPXE01",
                Some("Pokémon Box: Ruby & Sapphire"),
                &[],
            ),
            cand("cemu", "deadbeef", None, &[]),
        ]
    }

    fn key(q: &str) -> Result<String, String> {
        let c = catalog();
        resolve(q, None, &c)
            .map(|c| c.key.clone())
            .map_err(|e| match e {
                RefError::NotFound => "notfound".into(),
                RefError::Ambiguous(v) => v
                    .iter()
                    .map(|c| c.key.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            })
    }

    #[test]
    fn resolves_by_tier() {
        let ok = |q: &str, k: &str| assert_eq!(key(q), Ok(k.to_string()), "{q}");
        ok("botw", "101c9400");
        ok("101c9400", "101c9400");
        ok("101C9400", "101c9400");
        ok("gale01", "GALE01");
        ok("the legend of zelda breath of the wild", "101c9400");
        ok("The Legend of Zelda: Breath of the Wild (USA)", "101c9400");
        ok("  the  legend of zelda   breath of the wild ", "101c9400");
        ok("\"The Legend of Zelda - Breath of the Wild\"", "101c9400");
        ok("pokemon box ruby sapphire", "GPXE01");
        ok("deadbeef", "deadbeef");
    }

    #[test]
    fn misses_and_ambiguity() {
        assert_eq!(key("zelda breath"), Err("notfound".into()));
        assert_eq!(key(""), Err("notfound".into()));
        assert_eq!(key("   "), Err("notfound".into()));
        assert_eq!(key("(USA)"), Err("notfound".into()));
        assert_eq!(key("super smash bros melee"), Err("GALE01,GALP01".into()));
        assert_eq!(key("BOTW"), Err("notfound".into()), "aliases are exact");
    }

    #[test]
    fn emulator_filter() {
        let c = catalog();
        assert_eq!(
            resolve("super smash bros melee", Some("cemu"), &c),
            Err(RefError::NotFound)
        );
        assert_eq!(resolve("botw", Some("cemu"), &c).unwrap().key, "101c9400");
    }

    #[test]
    fn alias_beats_name_but_never_another_game_s_key() {
        let c = vec![
            cand("a", "x1", Some("Other"), &[]),
            cand("b", "x2", Some("x1"), &["x1"]),
            cand("b", "x3", Some("Melee"), &[]),
            cand("b", "x4", None, &["melee"]),
        ];
        assert_eq!(resolve("x1", None, &c).unwrap().key, "x1", "the key wins");
        assert_eq!(
            resolve("melee", None, &c).unwrap().key,
            "x4",
            "the alias shadows a name"
        );
        assert_eq!(
            resolve("Melee", None, &c).unwrap().key,
            "x3",
            "aliases are exact"
        );
    }

    #[test]
    fn edge_cases() {
        // A name made only of tags normalizes to nothing and matches nothing.
        let c = vec![
            cand("a", "k1", Some("(USA)"), &[]),
            cand("a", "k2", Some("[v1]"), &[]),
            cand("b", "k1", Some("Same Key"), &[]),
        ];
        assert_eq!(normalize("(USA)"), "");
        for q in ["(USA)", "usa", "()", "[]", "-"] {
            assert_eq!(resolve(q, None, &c), Err(RefError::NotFound), "{q}");
        }
        // The same key under two emulators is ambiguous until `--emu` picks one.
        assert!(matches!(resolve("K1", None, &c), Err(RefError::Ambiguous(v)) if v.len() == 2));
        assert_eq!(resolve("k1", Some("b"), &c).unwrap().emulator, "b");
        // Letters outside the fold table are dropped like punctuation, never a panic.
        assert_eq!(normalize("ポケモン Ω x ٣"), "x");
        assert_eq!(normalize("Ωmega 🎮 Ünïcode"), "mega unicode");
        let jp = vec![cand("a", "k9", Some("ゼルダの伝説"), &[])];
        assert_eq!(resolve("ゼルダの伝説", None, &jp), Err(RefError::NotFound));
        // Huge or deeply bracketed queries finish and match nothing.
        let long = "a".repeat(1 << 20);
        assert_eq!(resolve(&long, None, &c), Err(RefError::NotFound));
        let deep = "(".repeat(1 << 16);
        assert_eq!(normalize(&deep), "");
        assert_eq!(normalize(&")".repeat(1 << 16)), "");
    }

    #[test]
    fn folds_and_normalizes() {
        assert_eq!(normalize("Pokémon"), "pokemon");
        assert_eq!(normalize("Ñandú"), "nandu");
        assert_eq!(normalize("Über"), "uber");
        assert_eq!(normalize("Straße"), "strasse");
        assert_eq!(normalize("Œuvre"), "oeuvre");
        assert_eq!(normalize("Mario Kart [v1.2] (JPN)"), "mario kart");
        assert_eq!(normalize("  A--B  "), "a b");
        assert_eq!(normalize("Zelda (USA) Remix"), "zelda remix");
    }
}
