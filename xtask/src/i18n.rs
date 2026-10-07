//! `i18n-check`: every locale defines exactly the keys of the English source.
//! Covers Flutter ARB files; extend when Fluent or native string files arrive
//! (docs/specs/i18n.md, Parity check).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

const ARB_DIR: &str = "app/lib/l10n";
const TEMPLATE: &str = "app_en.arb";

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
}
