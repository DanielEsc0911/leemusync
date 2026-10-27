//! `layers`: internal crates may only depend on the layers below them.
//! The table mirrors docs/specs/architecture.md (Layers); change both together.

use std::path::Path;

const ALLOWED: &[(&str, &[&str])] = &[
    ("lemusync-core", &[]),
    ("lemusync-crypto", &[]),
    ("lemusync-store", &[]),
    ("lemusync-api", &[]),
    (
        "lemusync-engine",
        &[
            "lemusync-core",
            "lemusync-crypto",
            "lemusync-store",
            "lemusync-api",
        ],
    ),
    ("lemusync-ipc", &["lemusync-api"]),
    (
        "lemusync-daemon",
        &["lemusync-api", "lemusync-engine", "lemusync-ipc"],
    ),
    (
        "lemusync-cli",
        &["lemusync-api", "lemusync-engine", "lemusync-ipc"],
    ),
    (
        "lemusync-bridge",
        &["lemusync-api", "lemusync-engine", "lemusync-ipc"],
    ),
];

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let metadata = crate::metadata::load(root)?;
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let graph: Vec<(String, Vec<String>)> = packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().unwrap_or_default().to_string();
            let internal = package["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                // Normal dependencies only; dev/build dependencies may cross layers.
                .filter(|dep| dep["kind"].is_null())
                .filter_map(|dep| dep["name"].as_str())
                .filter(|dep| dep.starts_with("lemusync-"))
                .map(str::to_string)
                .collect();
            (name, internal)
        })
        .collect();
    violations(&graph)
}

fn violations(graph: &[(String, Vec<String>)]) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for (name, deps) in graph {
        if !name.starts_with("lemusync-") {
            continue;
        }
        let Some((_, allowed)) = ALLOWED
            .iter()
            .find(|(crate_name, _)| *crate_name == name.as_str())
        else {
            errors.push(format!(
                "{name}: missing from the layers table (xtask/src/layers.rs)"
            ));
            continue;
        };
        for dep in deps {
            if !allowed.contains(&dep.as_str()) {
                errors.push(format!("{name} must not depend on {dep}"));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::violations;

    fn graph(name: &str, deps: &[&str]) -> Vec<(String, Vec<String>)> {
        vec![(
            name.to_string(),
            deps.iter().map(ToString::to_string).collect(),
        )]
    }

    #[test]
    fn allows_downward_dependency() {
        assert_eq!(
            violations(&graph("lemusync-engine", &["lemusync-core"])),
            Ok(())
        );
    }

    #[test]
    fn rejects_upward_dependency() {
        assert_eq!(
            violations(&graph("lemusync-core", &["lemusync-engine"])),
            Err(vec![
                "lemusync-core must not depend on lemusync-engine".to_string()
            ])
        );
    }

    #[test]
    fn rejects_crate_missing_from_table() {
        assert!(violations(&graph("lemusync-extra", &[])).is_err());
    }
}
