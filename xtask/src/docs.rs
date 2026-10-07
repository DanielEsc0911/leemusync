//! `docs-check`: relative Markdown links resolve, every spec is listed in
//! `docs/index.md`, and every spec has the Status/Related/Code header line.

use std::fs;
use std::path::{Path, PathBuf};

const SPEC_HEADER_FIELDS: [&str; 3] = ["**Status:**", "**Related:**", "**Code:**"];
const SKIP_DIRS: [&str; 7] = [
    ".git",
    "target",
    "build",
    ".dart_tool",
    "node_modules",
    "Pods",
    ".gradle",
];

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    let mut files = Vec::new();
    collect_markdown(root, &mut files);
    files.sort();
    for file in &files {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) => {
                errors.push(format!("{}: {e}", display(root, file)));
                continue;
            }
        };
        let base = file.parent().unwrap_or(root);
        for target in relative_links(&text) {
            if !base.join(&target).exists() {
                errors.push(format!("{}: broken link `{target}`", display(root, file)));
            }
        }
    }

    let docs = root.join("docs");
    let index = fs::read_to_string(docs.join("index.md")).unwrap_or_default();
    let mut specs = Vec::new();
    collect_markdown(&docs.join("specs"), &mut specs);
    specs.sort();
    for spec in &specs {
        let rel = display(&docs, spec);
        if !index.contains(&format!("]({rel})")) {
            errors.push(format!("docs/index.md: no entry for `{rel}`"));
        }
        let text = fs::read_to_string(spec).unwrap_or_default();
        let head = text.lines().take(5).collect::<Vec<_>>().join("\n");
        for field in SPEC_HEADER_FIELDS {
            if !head.contains(field) {
                errors.push(format!("docs/{rel}: header line lacks {field}"));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Relative link targets in Markdown, without `#fragment`.
/// Skips external URLs, in-page anchors, mail links and fenced code blocks.
fn relative_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let target = after[..end]
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .split('#')
                .next()
                .unwrap_or_default();
            if !target.is_empty() && !target.contains("://") && !target.starts_with("mailto:") {
                links.push(target.to_string());
            }
            rest = &after[end + 1..];
        }
    }
    links
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name));
            if !skip {
                collect_markdown(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

/// `path` relative to `base`, with forward slashes on every OS.
fn display(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::relative_links;

    #[test]
    fn keeps_relative_links_without_fragments() {
        assert_eq!(
            relative_links("see [a](specs/a.md#x) and [b](../b.md \"title\")"),
            vec!["specs/a.md", "../b.md"]
        );
    }

    #[test]
    fn skips_external_anchor_and_mail_links() {
        assert_eq!(
            relative_links("[a](https://x.y) [b](#top) [c](mailto:a@b.c)"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn skips_fenced_code() {
        assert_eq!(
            relative_links("```\n[a](nope.md)\n```"),
            Vec::<String>::new()
        );
    }
}
