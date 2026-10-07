# Phase 0: Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task by task. Agents without those skills: do the tasks in order and tick each checkbox in the same commit as the work. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a buildable, CI-checked skeleton (Rust workspace, xtask checks, Flutter app with en/es), plus proven answers to every platform risk (spikes SP1–SP7) recorded in the specs, so Phase 1 starts on solid ground.

**Architecture:** a Rust workspace laid out per [architecture §Layers](../../specs/architecture.md#layers). `xtask` (std + `serde_json` only) enforces docs, layers and i18n. The Flutter app lives in `app/`. Spikes are timeboxed throwaway branches; their *results* go into spec Verify sections and decisions, never only into this plan.

**Tech stack:** Rust (edition 2024, pinned stable), `just`, `cargo-deny`, Flutter (pinned stable), GitHub Actions.

## Global constraints
- Rules: [AGENTS.md](../../../AGENTS.md) / [rules.md](../../rules.md). Especially B1 (layers), B2 (no `unsafe`, minimal deps), P6 (en + es), W1–W5.
- Package names `leemusync-<dir>`. Every crate: `publish = false`, `license = "MPL-2.0"`, `edition = "2024"`, `[lints] workspace = true`.
- Workspace lints: `unsafe_code = "forbid"`; clippy `all` = deny, `pedantic` = warn; `unwrap_used`, `dbg_macro`, `todo` = deny (unwrap/expect allowed in tests via `clippy.toml`). CI runs clippy with `-D warnings`.
- `xtask` dependencies: `serde_json` only.
- Commits: Conventional Commits. Author = maintainer. Trailer `Co-Authored-By: <agent> <no-reply address>` ([git-workflow](../../specs/git-workflow.md)).

## Open decisions (maintainer)
| ID | Decision | Blocks |
|---|---|---|
| OD1 | GitHub owner/org and repository name | Task 3 (push, CI run) |
| OD2 | App/bundle id (reverse-DNS, permanent on the stores). Recommended: own a domain (e.g. `leemusync.app` → `app.leemusync`); otherwise `io.github.<owner>.leemusync` | Task 4 |
| OD3 | Apple Developer membership; test devices: Samsung (One UI 8+), Xiaomi (HyperOS 3 global), iPhone with Dynamic Island, Raspberry Pi 5 | SP1, SP4, SP5 |

---

### Task 1: Toolchains and Rust workspace skeleton

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `clippy.toml`, `justfile`
- Create: `crates/{core,crypto,store,api,engine,ipc}/Cargo.toml` and `src/lib.rs`
- Create: `crates/daemon/Cargo.toml`, `crates/daemon/src/main.rs`, `crates/daemon/tests/version.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `crates/cli/tests/version.rs`

**Interfaces:**
- Produces: binaries `leemusync` (package `leemusync-cli`) and `leemusyncd` (package `leemusync-daemon`), each printing `<bin> <version>`. Library crates exist, empty, with their responsibility in the crate docs.

- [ ] **Step 1: Install toolchains**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
source "$HOME/.cargo/env"
cargo install just cargo-deny --locked
rustc --version && just --version && cargo deny --version
```
Expected: three version lines. (The fish shell uses `source "$HOME/.cargo/env.fish"`.)

- [ ] **Step 2: Pin the toolchain.** Write `rust-toolchain.toml`, using the exact version `rustc --version` printed (e.g. `1.99.0`):

```toml
[toolchain]
channel = "1.99.0" # exact stable version printed in Step 1
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 3: Write the workspace manifest and lint config**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.0.0"
edition = "2024"
license = "MPL-2.0"
rust-version = "1.99" # same as rust-toolchain.toml

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
dbg_macro = "deny"
todo = "deny"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

`clippy.toml`:
```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

- [ ] **Step 4: Write the failing CLI version test**

`crates/cli/Cargo.toml`:
```toml
[package]
name = "leemusync-cli"
description = "LeemuSync command-line interface."
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[[bin]]
name = "leemusync"
path = "src/main.rs"

[lints]
workspace = true
```

`crates/cli/src/main.rs`:
```rust
//! `leemusync`: command-line interface. Spec: docs/specs/config-and-cli.md.

fn main() {}
```

`crates/cli/tests/version.rs`:
```rust
use std::process::Command;

#[test]
fn prints_name_and_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_leemusync"))
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), format!("leemusync {}", env!("CARGO_PKG_VERSION")));
}
```

- [ ] **Step 5: Run it and watch it fail**

Run: `cargo test -p leemusync-cli`
Expected: FAIL. `assertion left == right failed`, left `""`, right `"leemusync 0.0.0"`.

- [ ] **Step 6: Implement**

`crates/cli/src/main.rs`:
```rust
//! `leemusync`: command-line interface. Spec: docs/specs/config-and-cli.md.

fn main() {
    println!("leemusync {}", env!("CARGO_PKG_VERSION"));
}
```

Run: `cargo test -p leemusync-cli` → Expected: PASS (1 test).

- [ ] **Step 7: Same for the daemon (test first, then implementation)**

`crates/daemon/Cargo.toml`: identical to the CLI manifest except `name = "leemusync-daemon"`, `description = "LeemuSync always-on sync daemon."`, and `[[bin]] name = "leemusyncd"`.

`crates/daemon/tests/version.rs`: identical to the CLI test with `CARGO_BIN_EXE_leemusyncd` and the expected prefix `leemusyncd`. Run it → FAIL with `fn main() {}`. Then:

`crates/daemon/src/main.rs`:
```rust
//! `leemusyncd`: the always-on sync daemon. Spec: docs/specs/architecture.md.

fn main() {
    println!("leemusyncd {}", env!("CARGO_PKG_VERSION"));
}
```
Run: `cargo test -p leemusync-daemon` → PASS.

- [ ] **Step 8: Library crates.** For each row, create `crates/<dir>/Cargo.toml` (same shape as the CLI manifest, without `[[bin]]`) and `crates/<dir>/src/lib.rs` with only the crate doc comment:

| dir | `description` | `lib.rs` |
|---|---|---|
| core | Domain model and pure sync logic. | `//! Domain model and pure sync logic. No I/O, no async.`<br>`//! Spec: docs/specs/architecture.md (Layers), docs/specs/sync-model.md.` |
| crypto | Encryption, key derivation and keyed hashing. | `//! Encryption, key derivation and keyed hashing. Pure, no I/O.`<br>`//! Spec: docs/specs/security.md (Cryptography).` |
| store | Storage backend interface and adapters. | `//! Storage backend interface and adapters. Moves opaque encrypted objects.`<br>`//! Spec: docs/specs/storage-backends.md.` |
| api | Engine API types and trait. | `//! Engine API: requests, responses, events and the engine trait. Versioned.`<br>`//! Spec: docs/specs/architecture.md (Engine API).` |
| engine | Sync engine orchestration. | `//! Sync engine: scanning, snapshots, transfers, leases, journal.`<br>`//! Spec: docs/specs/architecture.md (Engine internals), docs/specs/reliability.md.` |
| ipc | Same-user local IPC transport. | `//! Same-user local IPC: framing, handshake, engine client and server.`<br>`//! Spec: docs/specs/architecture.md (IPC).` |

- [ ] **Step 9: Task runner.** `justfile`:

```just
# LeemuSync task runner. `just --list` shows every recipe.
# On Windows, run from Git Bash (just uses `sh`).

default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Lints and repo checks (no tests)
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# All tests
test:
    cargo test --workspace

# Everything CI runs; must pass before every commit
check: lint test
```

- [ ] **Step 10: Verify.** Run: `just check` → Expected: fmt clean, clippy no warnings, 2 tests pass.

- [ ] **Step 11: Commit**

```bash
git add rust-toolchain.toml Cargo.toml Cargo.lock clippy.toml justfile crates/
git commit -m "build: scaffold Rust workspace with layered crates" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: xtask: `docs-check` and `layers`

**Files:**
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/docs.rs`, `xtask/src/layers.rs`, `.cargo/config.toml`
- Modify: `Cargo.toml` (members), `justfile` (lint recipe)

**Interfaces:**
- Produces: `cargo xtask docs-check`, `cargo xtask layers`. Each exits 0 with `xtask <task>: ok`, or exits 1 with one `error: …` line per problem. Internal: `docs::check(&Path) -> Result<(), Vec<String>>`, `layers::check(&Path) -> Result<(), Vec<String>>`, `docs::relative_links(&str) -> Vec<String>`, `layers::violations(&[(String, Vec<String>)]) -> Result<(), Vec<String>>`.

- [ ] **Step 1: Manifests and alias**

`Cargo.toml` → `members = ["crates/*", "xtask"]`.

`xtask/Cargo.toml`:
```toml
[package]
name = "xtask"
version = "0.0.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]

[lints]
workspace = true
```
Then run `cargo add serde_json --package xtask` (records the current version; `Cargo.lock` pins it).

`.cargo/config.toml`:
```toml
[alias]
xtask = "run --quiet --package xtask --"
```

- [ ] **Step 2: Write the failing tests.** `xtask/src/docs.rs` (tests first, with a stub that returns nothing):

```rust
//! `docs-check`: relative Markdown links resolve, every spec is listed in
//! `docs/index.md`, and every spec has the Status/Related/Code header line.

fn relative_links(_text: &str) -> Vec<String> {
    Vec::new()
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
        assert!(relative_links("[a](https://x.y) [b](#top) [c](mailto:a@b.c)").is_empty());
    }

    #[test]
    fn skips_fenced_code() {
        assert!(relative_links("```\n[a](nope.md)\n```").is_empty());
    }
}
```

`xtask/src/layers.rs` (tests first, stub always OK):
```rust
//! `layers`: internal crates may only depend on the layers below them.
//! The table mirrors docs/specs/architecture.md (Layers); change both together.

fn violations(_graph: &[(String, Vec<String>)]) -> Result<(), Vec<String>> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::violations;

    fn graph(name: &str, deps: &[&str]) -> Vec<(String, Vec<String>)> {
        vec![(name.to_string(), deps.iter().map(ToString::to_string).collect())]
    }

    #[test]
    fn allows_downward_dependency() {
        assert_eq!(violations(&graph("leemusync-engine", &["leemusync-core"])), Ok(()));
    }

    #[test]
    fn rejects_upward_dependency() {
        assert_eq!(
            violations(&graph("leemusync-core", &["leemusync-engine"])),
            Err(vec!["leemusync-core must not depend on leemusync-engine".to_string()])
        );
    }

    #[test]
    fn rejects_crate_missing_from_table() {
        assert!(violations(&graph("leemusync-extra", &[])).is_err());
    }
}
```

`xtask/src/main.rs`:
```rust
//! Repository checks, run as `cargo xtask <task>`.

mod docs;
mod layers;

fn main() {}
```

- [ ] **Step 3: Run and watch them fail**

Run: `cargo test -p xtask`
Expected: FAIL in `keeps_relative_links_without_fragments`, `rejects_upward_dependency` and `rejects_crate_missing_from_table`.

- [ ] **Step 4: Implement `docs.rs`** (replace the stub; keep the tests module):

```rust
//! `docs-check`: relative Markdown links resolve, every spec is listed in
//! `docs/index.md`, and every spec has the Status/Related/Code header line.

use std::fs;
use std::path::{Path, PathBuf};

const SPEC_HEADER_FIELDS: [&str; 3] = ["**Status:**", "**Related:**", "**Code:**"];
const SKIP_DIRS: [&str; 7] = [".git", "target", "build", ".dart_tool", "node_modules", "Pods", ".gradle"];

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

    if errors.is_empty() { Ok(()) } else { Err(errors) }
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
    let Ok(entries) = fs::read_dir(dir) else { return };
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
```

- [ ] **Step 5: Implement `layers.rs`** (replace the stub; keep the tests module):

```rust
//! `layers`: internal crates may only depend on the layers below them.
//! The table mirrors docs/specs/architecture.md (Layers); change both together.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const ALLOWED: &[(&str, &[&str])] = &[
    ("leemusync-core", &[]),
    ("leemusync-crypto", &[]),
    ("leemusync-store", &[]),
    ("leemusync-api", &[]),
    ("leemusync-engine", &["leemusync-core", "leemusync-crypto", "leemusync-store", "leemusync-api"]),
    ("leemusync-ipc", &["leemusync-api"]),
    ("leemusync-daemon", &["leemusync-api", "leemusync-engine", "leemusync-ipc"]),
    ("leemusync-cli", &["leemusync-api", "leemusync-engine", "leemusync-ipc"]),
    ("leemusync-bridge", &["leemusync-api", "leemusync-engine", "leemusync-ipc"]),
];

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| vec![format!("cannot run cargo metadata: {e}")])?;
    if !output.status.success() {
        return Err(vec![String::from_utf8_lossy(&output.stderr).into_owned()]);
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| vec![format!("invalid cargo metadata: {e}")])?;
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
                .filter(|dep| dep.starts_with("leemusync-"))
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
        if !name.starts_with("leemusync-") {
            continue;
        }
        let Some((_, allowed)) = ALLOWED.iter().find(|(crate_name, _)| *crate_name == name.as_str()) else {
            errors.push(format!("{name}: missing from the layers table (xtask/src/layers.rs)"));
            continue;
        };
        for dep in deps {
            if !allowed.contains(&dep.as_str()) {
                errors.push(format!("{name} must not depend on {dep}"));
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```

- [ ] **Step 6: Implement `main.rs`**

```rust
//! Repository checks, run as `cargo xtask <task>`.

mod docs;
mod layers;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = repo_root();
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "docs-check" => docs::check(&root),
        "layers" => layers::check(&root),
        _ => Err(vec![format!("unknown task `{task}`; available: docs-check, layers")]),
    };
    match result {
        Ok(()) => {
            println!("xtask {task}: ok");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

/// The repository root: the parent of this crate's directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
```

- [ ] **Step 7: Verify**

Run: `cargo test -p xtask` → Expected: PASS (6 tests).
Run: `cargo xtask docs-check` → Expected: `xtask docs-check: ok` (the existing docs are consistent; fix any real error it reports).
Run: `cargo xtask layers` → Expected: `xtask layers: ok`.

- [ ] **Step 8: Wire into `just lint`.** Append to the `lint` recipe:
```just
    cargo xtask layers
    cargo xtask docs-check
```
Run: `just check` → green.

- [ ] **Step 9: Commit**
```bash
git add Cargo.toml Cargo.lock .cargo/ xtask/ justfile
git commit -m "build(xtask): add docs-check and layers checks" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Supply chain and CI

**Files:**
- Create: `deny.toml`, `.github/workflows/ci.yml`, `.github/dependabot.yml`
- Modify: `justfile` (add `cargo deny check` to `lint`)

- [ ] **Step 1: `deny.toml`**

```toml
[graph]
all-features = true

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
confidence-threshold = 0.9
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
  "ISC", "MPL-2.0", "Unicode-3.0", "Zlib", "CC0-1.0",
]

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```
Run: `cargo deny check` → Expected: `advisories ok, bans ok, licenses ok, sources ok`. A license outside the list needs a written reason and maintainer approval before it's added.

- [ ] **Step 2: `.github/workflows/ci.yml`**

```yaml
name: ci

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  rust:
    name: rust (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - name: Install pinned toolchain
        run: rustup toolchain install
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: cargo xtask layers

  repo-checks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - run: rustup toolchain install
      - run: cargo xtask docs-check
      - uses: EmbarkStudios/cargo-deny-action@v2
```
`rustup toolchain install` with no arguments installs the toolchain from `rust-toolchain.toml` (rustup ≥ 1.28). If the runner's rustup is older, use `rustup show` instead.

- [ ] **Step 3: Pin actions by commit SHA** (B2). For each `uses:` line, find the SHA of the newest release tag of that major version and write `uses: owner/repo@<sha> # vX.Y.Z`:
```bash
git ls-remote --tags https://github.com/actions/checkout | grep -E 'refs/tags/v5\.[0-9]+\.[0-9]+(\^\{\})?$' | tail -2
git ls-remote --tags https://github.com/EmbarkStudios/cargo-deny-action | grep -E 'refs/tags/v2\.[0-9]+\.[0-9]+(\^\{\})?$' | tail -2
```
Use the `^{}` (peeled) SHA when it's listed.

- [ ] **Step 4: `.github/dependabot.yml`**

```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
```

- [ ] **Step 5: Add `cargo deny check` to the `just lint` recipe** (after clippy). Run `just check` → green.

- [ ] **Step 6: Commit**
```bash
git add deny.toml .github/ justfile
git commit -m "ci: add cargo-deny, CI workflow and dependabot" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

- [ ] **Step 7: Publish (maintainer approval required; needs OD1).** Create the GitHub repository, add the remote, push `main`, and confirm every CI job is green. In repository settings: protect `main` (PR + required checks), enable private vulnerability reporting, secret scanning and push protection.

---

### Task 4: Flutter app skeleton with en/es and `i18n-check`

**Files:**
- Create: `app/` (via `flutter create`), `app/l10n.yaml`, `app/lib/l10n/app_en.arb`, `app/lib/l10n/app_es.arb`, `app/test/app_test.dart`, `xtask/src/i18n.rs`
- Modify: `app/pubspec.yaml`, `app/analysis_options.yaml`, `app/lib/main.dart`, `app/.gitignore`, `xtask/src/main.rs`, `justfile`, `.github/workflows/ci.yml`, `.github/dependabot.yml`

**Interfaces:**
- Produces: `LeemuSyncApp({Key? key, Locale? locale})` in `app/lib/main.dart`; ARB keys `appTitle`, `statusSynced`, `statusUploading(count)`; `cargo xtask i18n-check`; internal `i18n::check(&Path) -> Result<(), Vec<String>>`, `i18n::diff(&str, &BTreeSet<String>, &BTreeSet<String>) -> Vec<String>`.

- [ ] **Step 1: Install Flutter (stable) and create the app** (needs OD2 for `--org`):
```bash
flutter --version
flutter create --org <OD2 reverse-domain> --project-name leemusync \
  --platforms android,ios,macos,windows,linux --empty app
```
Pin the version in `app/pubspec.yaml` under `environment:` → `flutter: <exact version from flutter --version>`.

- [ ] **Step 2: Localisation setup**
```bash
cd app && flutter pub add flutter_localizations --sdk=flutter && flutter pub add intl:any
```
In `app/pubspec.yaml`, under `flutter:` add `generate: true`.

`app/l10n.yaml`:
```yaml
arb-dir: lib/l10n
template-arb-file: app_en.arb
output-dir: lib/l10n/generated
output-localization-file: app_localizations.dart
nullable-getter: false
```
If `flutter gen-l10n` says the output dir is ignored because of a synthetic package, add `synthetic-package: false`.

Append `lib/l10n/generated/` to `app/.gitignore` (the ARB files are the source of truth).

`app/lib/l10n/app_en.arb`:
```json
{
  "@@locale": "en",
  "appTitle": "LeemuSync",
  "@appTitle": { "description": "Product name. Never translated." },
  "statusSynced": "All saves synced",
  "@statusSynced": { "description": "Home status when nothing is pending." },
  "statusUploading": "{count, plural, =1{Uploading 1 change} other{Uploading {count} changes}}",
  "@statusUploading": {
    "description": "Status while uploads are in flight.",
    "placeholders": { "count": { "type": "int" } }
  }
}
```

`app/lib/l10n/app_es.arb`:
```json
{
  "@@locale": "es",
  "appTitle": "LeemuSync",
  "statusSynced": "Todas las partidas están sincronizadas",
  "statusUploading": "{count, plural, =1{Subiendo 1 cambio} other{Subiendo {count} cambios}}"
}
```

- [ ] **Step 3: Strict analysis.** `app/analysis_options.yaml`:
```yaml
include: package:flutter_lints/flutter.yaml

analyzer:
  language:
    strict-casts: true
    strict-inference: true
    strict-raw-types: true

linter:
  rules:
    - always_declare_return_types
    - avoid_print
    - directives_ordering
    - prefer_const_constructors
    - prefer_const_declarations
    - prefer_final_locals
    - unawaited_futures
```

- [ ] **Step 4: Write the failing widget test.** `app/test/app_test.dart`:
```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:leemusync/main.dart';

void main() {
  testWidgets('shows synced status in English', (tester) async {
    await tester.pumpWidget(const LeemuSyncApp(locale: Locale('en')));
    await tester.pumpAndSettle();
    expect(find.text('All saves synced'), findsOneWidget);
  });

  testWidgets('shows synced status in Spanish', (tester) async {
    await tester.pumpWidget(const LeemuSyncApp(locale: Locale('es')));
    await tester.pumpAndSettle();
    expect(find.text('Todas las partidas están sincronizadas'), findsOneWidget);
  });
}
```
Run: `cd app && flutter gen-l10n && flutter test` → Expected: FAIL (`LeemuSyncApp` isn't defined).

- [ ] **Step 5: Implement.** `app/lib/main.dart`:
```dart
import 'package:flutter/material.dart';

import 'l10n/generated/app_localizations.dart';

void main() => runApp(const LeemuSyncApp());

/// Root widget. The design system and navigation shell replace the
/// placeholder home in Phase 3 (docs/specs/ui/).
class LeemuSyncApp extends StatelessWidget {
  const LeemuSyncApp({super.key, this.locale});

  /// Forces a locale (tests, user override). Null follows the OS.
  final Locale? locale;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      onGenerateTitle: (context) => AppLocalizations.of(context).appTitle,
      locale: locale,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: const _StatusPlaceholder(),
    );
  }
}

class _StatusPlaceholder extends StatelessWidget {
  const _StatusPlaceholder();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(child: Text(AppLocalizations.of(context).statusSynced)),
    );
  }
}
```
Run: `flutter test` → Expected: PASS (2 tests). Run: `flutter analyze` → `No issues found!`

- [ ] **Step 6: `i18n-check`, test first.** `xtask/src/i18n.rs`:
```rust
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
        if name == TEMPLATE || !name.ends_with(".arb") {
            continue;
        }
        match arb_keys(&entry.path()) {
            Ok(found) => errors.extend(diff(&name, &template, &found)),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// Message keys of an ARB file (metadata keys start with `@`).
fn arb_keys(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: Map<String, Value> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(json.keys().filter(|key| !key.starts_with('@')).cloned().collect())
}

fn diff(file: &str, template: &BTreeSet<String>, found: &BTreeSet<String>) -> Vec<String> {
    let missing = template.difference(found).map(|key| format!("{file}: missing key `{key}`"));
    let unknown = found.difference(template).map(|key| format!("{file}: unknown key `{key}`"));
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
        assert!(diff("app_es.arb", &set(&["a"]), &set(&["a"])).is_empty());
    }
}
```
Write the tests module first with `fn diff(...) -> Vec<String> { Vec::new() }` as a stub → `cargo test -p xtask` FAILS in `reports_missing_and_unknown_keys` → then the full file above → PASS.

In `xtask/src/main.rs`: add `mod i18n;`, add the arm `"i18n-check" => i18n::check(&root),`, and list `i18n-check` in the unknown-task message.

Run: `cargo xtask i18n-check` → `xtask i18n-check: ok`. Then delete `statusSynced` from `app_es.arb` and run it again. Expected: exit code 1 and an error line saying `app_es.arb` is missing key `statusSynced`. Restore the key.

- [ ] **Step 7: Final `justfile`**
```just
# LeemuSync task runner. `just --list` shows every recipe.
# On Windows, run from Git Bash (just uses `sh`).

default:
    @just --list

# Format Rust and Dart
fmt:
    cargo fmt --all
    cd app && dart format lib test

# Lints and repo checks (no tests)
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    cargo xtask layers
    cargo xtask docs-check
    cargo xtask i18n-check
    cd app && flutter gen-l10n
    cd app && dart format --output=none --set-exit-if-changed lib test
    cd app && flutter analyze

# Rust and Flutter tests
test:
    cargo test --workspace
    cd app && flutter test

# Everything CI runs; must pass before every commit
check: lint test
```

- [ ] **Step 8: CI and Dependabot.** In `ci.yml` add `- run: cargo xtask i18n-check` to `repo-checks`, and add the job below (pin the action SHA as in Task 3 Step 3). In `dependabot.yml` add the `pub` ecosystem with `directory: /app`.
```yaml
  flutter:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v5
      - uses: subosito/flutter-action@v2
        with:
          flutter-version-file: app/pubspec.yaml
      - run: flutter pub get
      - run: flutter gen-l10n
      - run: dart format --output=none --set-exit-if-changed lib test
      - run: flutter analyze
      - run: flutter test
```

- [ ] **Step 9: Verify and commit.** `just check` → green. Then:
```bash
git add app/ xtask/ justfile .github/
git commit -m "feat(app): add Flutter shell with English and Spanish strings" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Spikes
Each spike runs on a `spike/<id>-<slug>` branch, timeboxed (default 2 days). The code is throwaway unless a later task adopts it. **Done** means every listed Verify item is answered with evidence (versions, devices, logs, screenshots, links). The results are written into the named specs and decisions (W1, W3) and committed to `main` as `docs(spike): …`.

### SP1: Flutter + Rust bridge on every target
- **Answers:** D4 (FRB + UniFFI in one cdylib, one engine instance), V-DESK-4, V-SEC-2.
- [ ] Create `crates/bridge` (cdylib + staticlib) with flutter_rust_bridge exposing `fn core_version() -> String` and UniFFI exposing `fn engine_ping() -> String`. Both read and increment one global counter, which proves there's a single instance.
- [ ] Call FRB from Dart and UniFFI from Kotlin (Android) and Swift (iOS) in the same app. Confirm the counter is shared.
- [ ] Build and run on Windows x64, Windows arm64, macOS, Linux x64, Linux arm64 (Pi 5), Android arm64, iOS device.
- [ ] Make one HTTPS request with `rustls` + platform verifier on each target.
- [ ] Measure per target: cold start, app size, idle RSS, frame times for a 120 Hz animation test screen (profile mode).
- [ ] **Record:** architecture (bridge details), decisions D2/D4 (confirm or supersede), desktop V-DESK-4, security V-SEC-2, design-system budgets (adjust with data).

### SP2: Desktop daemon presence
- **Answers:** V-DESK-1/2/3, V-REL-1/2, V-SEC-3, V-STAT-4.
- [ ] Prototype daemon: tray icon with state swap + menu; actionable notification; on Windows, macOS, Linux KDE and GNOME (with and without the AppIndicator extension).
- [ ] Service registration: systemd user unit (+ linger), `SMAppService` LaunchAgent from a signed bundle, Windows logon task with restart. Confirm the tray works when started by each.
- [ ] Sleep inhibition during a fake transfer, then release. Resume event delivery.
- [ ] Keystore read/write from the daemon on each OS, including headless Linux.
- [ ] Flatpak: test static permissions vs. the portal for reading `~/.var/app/<emulator>/`. Recommend a packaging strategy.
- [ ] **Record:** desktop, reliability, status-and-notifications, security; decision entry for Flatpak strategy.

### SP3: Android storage and emulator inventory
- **Answers:** V-AND-1/2/3, V-PROF-2.
- [ ] For each emulator in [android §Emulators to inventory](../../specs/platforms/android.md#emulators-to-inventory-sp3): save path, user-movable?, readable via SAF?, launch intent + extras for a game path, tier.
- [ ] Measure SAF read speed for a typical save tree.
- [ ] Check Google Play's current All-files-access policy for a save-sync app.
- [ ] **Record:** android, emulator-profiles (Android roots), decision if All-files access is used.

### SP4: Android live surfaces
- **Answers:** V-STAT-1/2, V-AND-4.
- [ ] Post a `ProgressStyle` notification requesting promotion on Android 16. Record the requirements (permission, flags, style limits, user toggles).
- [ ] Verify the chip on Pixel, Now Bar on Samsung One UI 8+, Super Island on Xiaomi HyperOS 3 (global). Screenshots.
- [ ] Fallback on Android 13–15 devices.
- [ ] **Record:** status-and-notifications, android.

### SP5: iOS access and integrations
- **Answers:** V-IOS-1/2/3/4, V-PROF-3, V-STAT-3.
- [ ] For RetroArch, PPSSPP, Delta, Gamma, Provenance, Folium, DolphiniOS, MeloNX: are saves exposed in Files? Where? Same format as desktop?
- [ ] Security-scoped bookmark to another app's Files folder: survives restart and update?
- [ ] App Intent triggered by a Shortcuts "app is closed" automation runs the Rust bridge to completion.
- [ ] Live Activity + Dynamic Island updated from a `BGContinuedProcessingTask`; record duration and update limits.
- [ ] **Record:** ios, emulator-profiles (iOS roots), status-and-notifications.

### SP6: Storage backends
- **Answers:** V-STORE-1/2/3/4, V-SYNC-1/2.
- [ ] Capability matrix with OpenDAL for s3 (AWS, MinIO, R2, B2), webdav (Nextcloud), sftp, fs, gdrive, onedrive, dropbox: `if_not_exists`, server mtime, list-after-write delay, rate limits.
- [ ] Google Drive with `drive.file`: OAuth flow on desktop and mobile; scope classification and verification needs.
- [ ] MEGA: test S4 via s3; review the existing Rust MEGA crates (maintenance, security); MEGAcmd WebDAV.
- [ ] SFTP on Android and iOS builds.
- [ ] **Record:** storage-backends, sync-model (lease liveness), decision for MEGA.

### SP7: Desktop emulator layouts (initial five)
- **Answers:** V-PROF-1, V-ACC-1.
- [ ] On Linux and Windows (+ macOS where available): install Cemu, RetroArch, Dolphin, DuckStation, PCSX2. Record base paths (native, Flatpak, portable), config keys for custom paths, process names, save layouts, and per-game/folder card settings.
- [ ] Cemu: account folder layout, how to read the display name from `account.dat`, whether BotW uses `user/common`, and BotW title IDs per region (expected: US `101c9400`, EU `101c9500`, JP `101c9300`, high part `00050000`).
- [ ] Turn each into a draft profile + fixture tree (adopted in Phase 1).
- [ ] **Record:** emulator-profiles, players-and-accounts.

---

### Task 5: Close Phase 0
- [ ] Every Verify item targeted by SP1–SP7 is answered: moved into its spec's body with evidence, or re-scoped with a reason.
- [ ] Decisions confirmed or superseded (D2, D4, D10 at least). New decisions added (Flatpak, MEGA, Android all-files access).
- [ ] Spec statuses updated. `cargo xtask docs-check` green.
- [ ] Write `docs/progress/plans/phase-1-engine.md` with the writing-plans format and add it to the board.
- [ ] `CHANGELOG.md` `[Unreleased]` updated; this plan removed from the board and deleted ([progress](../README.md)).
- [ ] Commit: `docs(progress): close phase 0 and plan phase 1`.
