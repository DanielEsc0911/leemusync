//! `i18n-check`: every locale defines exactly the keys of the English source.
//! Covers Flutter ARB files and the Rust Fluent files (`locales/<lang>/leemusync.ftl`); extend
//! when native string files arrive (docs/specs/i18n.md, Parity check).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

const ARB_DIR: &str = "app/lib/l10n";
const TEMPLATE: &str = "app_en.arb";
const FTL_DIR: &str = "locales";
const FTL_FILE: &str = "leemusync.ftl";

/// Message ids of a Fluent file: lines `id = …` at column 0 (comments and continuation lines skipped).
fn ftl_keys(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with(char::is_whitespace) && !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim()))
        .filter(|k| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_owned)
        .collect())
}

fn check_ftl(root: &Path) -> Vec<String> {
    let dir = root.join(FTL_DIR);
    let template = match ftl_keys(&dir.join("en").join(FTL_FILE)) {
        Ok(k) => k,
        Err(e) => return vec![e],
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![format!("{FTL_DIR}: missing")];
    };
    let mut errors = Vec::new();
    for entry in entries.flatten() {
        let lang = entry.file_name().to_string_lossy().into_owned();
        if lang == "en" || !entry.path().is_dir() {
            continue;
        }
        match ftl_keys(&entry.path().join(FTL_FILE)) {
            Ok(found) => errors.extend(diff(&format!("{lang}/{FTL_FILE}"), &template, &found)),
            Err(e) => errors.push(e),
        }
    }
    errors
}

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let dir = root.join(ARB_DIR);
    let template = arb_keys(&dir.join(TEMPLATE)).map_err(|e| vec![e])?;
    let entries = fs::read_dir(&dir).map_err(|e| vec![format!("{ARB_DIR}: {e}")])?;
    let mut errors = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == TEMPLATE || entry.path().extension().is_none_or(|ext| ext != "arb") {
            continue;
        }
        match arb_keys(&entry.path()) {
            Ok(found) => errors.extend(diff(&name, &template, &found)),
            Err(e) => errors.push(e),
        }
    }
    errors.extend(check_ftl(root));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Message keys of an ARB file (metadata keys start with `@`).
fn arb_keys(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: Map<String, Value> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(json
        .keys()
        .filter(|key| !key.starts_with('@'))
        .cloned()
        .collect())
}

fn diff(file: &str, template: &BTreeSet<String>, found: &BTreeSet<String>) -> Vec<String> {
    let missing = template
        .difference(found)
        .map(|key| format!("{file}: missing key `{key}`"));
    let unknown = found
        .difference(template)
        .map(|key| format!("{file}: unknown key `{key}`"));
    missing.chain(unknown).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::diff;

    fn set(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn reports_missing_and_unknown_keys() {
        assert_eq!(
            diff("app_es.arb", &set(&["a", "b"]), &set(&["b", "c"])),
            vec!["app_es.arb: missing key `a`", "app_es.arb: unknown key `c`"]
        );
    }

    #[test]
    fn accepts_identical_keys() {
        assert_eq!(
            diff("app_es.arb", &set(&["a"]), &set(&["a"])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn ftl_keys_skip_comments_and_continuations() {
        let dir = std::env::temp_dir().join(format!("xtask-ftl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.ftl");
        std::fs::write(
            &file,
            "# c\na = A\nb = { $n ->\n    [one] x\n   *[other] y\n}\n",
        )
        .unwrap();
        let keys = super::ftl_keys(&file).unwrap();
        assert_eq!(keys.into_iter().collect::<Vec<_>>(), ["a", "b"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
