//! `version-check <tag>`: a release tag `vX.Y.Z[-pre]` must equal the workspace
//! version (Cargo.toml) and the app version (app/pubspec.yaml, build number ignored).

use std::fs;
use std::path::Path;

pub(crate) fn check(root: &Path, tag: &str) -> Result<(), Vec<String>> {
    let metadata = crate::metadata::load(root)?;
    let cargo_version = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["name"] == "leemusync-cli")
        .and_then(|package| package["version"].as_str())
        .ok_or_else(|| vec!["leemusync-cli not found in cargo metadata".to_string()])?
        .to_string();
    let pubspec = fs::read_to_string(root.join("app/pubspec.yaml"))
        .map_err(|e| vec![format!("app/pubspec.yaml: {e}")])?;
    let app_version = pubspec
        .lines()
        .find_map(|line| line.strip_prefix("version:"))
        .map(|version| version.trim().to_string())
        .ok_or_else(|| vec!["app/pubspec.yaml: no top-level `version:` line".to_string()])?;
    compare(tag, &cargo_version, &app_version)
}

fn compare(tag: &str, cargo_version: &str, app_version: &str) -> Result<(), Vec<String>> {
    let Some(wanted) = tag.strip_prefix('v') else {
        return Err(vec![format!("tag `{tag}` must start with `v`")]);
    };
    let app = app_version.split('+').next().unwrap_or_default();
    let mut errors = Vec::new();
    if cargo_version != wanted {
        errors.push(format!(
            "Cargo.toml version {cargo_version} != tag {wanted}"
        ));
    }
    if app != wanted {
        errors.push(format!("app/pubspec.yaml version {app} != tag {wanted}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::compare;

    #[test]
    fn accepts_matching_versions_ignoring_build_number() {
        assert_eq!(
            compare("v0.0.1-alpha.1", "0.0.1-alpha.1", "0.0.1-alpha.1+1"),
            Ok(())
        );
    }

    #[test]
    fn rejects_mismatch() {
        assert_eq!(
            compare("v0.2.0", "0.1.0", "0.2.0+3"),
            Err(vec!["Cargo.toml version 0.1.0 != tag 0.2.0".to_string()])
        );
    }

    #[test]
    fn rejects_tag_without_v() {
        assert!(compare("0.1.0", "0.1.0", "0.1.0").is_err());
    }
}
