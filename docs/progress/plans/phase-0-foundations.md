# Phase 0: Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task by task. Agents without those skills: do the tasks in order and tick each checkbox in the same commit as the work. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a buildable, CI-checked skeleton (Rust workspace, xtask checks, Flutter app with en/es) with a release pipeline that produces `.apk` `.msi` `.exe` `.dmg` `.deb` `.rpm` `.AppImage` `.tar.gz`, plus proven answers to every platform risk (spikes SP1–SP7) recorded in the specs, so Phase 1 starts on solid ground.

**Architecture:** a Rust workspace laid out per [architecture §Layers](../../specs/architecture.md#layers). `xtask` (std + `serde_json` only) enforces docs, layers and i18n. The Flutter app lives in `app/`. Spikes are timeboxed throwaway branches; their *results* go into spec Verify sections and decisions, never only into this plan.

**Tech stack:** Rust (edition 2024, pinned stable), `just`, `cargo-deny`, Flutter (pinned stable), GitHub Actions.

## Global constraints
- Rules: [AGENTS.md](../../../AGENTS.md) / [rules.md](../../rules.md). Especially B1 (layers), B2 (no `unsafe`, minimal deps), P6 (en + es), W1–W5.
- Package names `leemusync-<dir>`. Every crate: `publish = false`, `license = "MPL-2.0"`, `edition = "2024"`, `[lints] workspace = true`.
- Workspace lints: `unsafe_code = "forbid"`; clippy `all` = deny, `pedantic` = warn; `unwrap_used`, `dbg_macro`, `todo` = deny (unwrap/expect allowed in tests via `clippy.toml`). CI runs clippy with `-D warnings`.
- `xtask` dependencies: `serde_json` only.
- Code snippets here are correct but not always in rustfmt layout. Run `just fmt` before `just check` (found in Task 1).
- Clippy's `assert_is_empty` lint (denied via `clippy::all`) rejects `assert!(x.is_empty())`. Use `assert_eq!(x, Vec::<String>::new())` (found in Task 2).
- Clippy's `case_sensitive_file_extension_comparisons` (pedantic) rejects `name.ends_with(".ext")`. Compare `path.extension()` instead (found in Task 4).
- WiX: a Component with more than one file can't use an auto-generated GUID unless its keypath file is versioned. The Rust binaries have no version resource, so give each one its own Component (found in Task 5 Step 9).
- Commits: Conventional Commits. Author = maintainer. Trailer `Co-Authored-By: <agent> <no-reply address>` ([git-workflow](../../specs/git-workflow.md)).

## Open decisions (maintainer)
| ID | Decision | Outcome |
|---|---|---|
| OD1 | Repository | Resolved: `github.com/DanielEsc0911/leemusync` (remote `origin` = `git@github.com:DanielEsc0911/leemusync.git`) |
| OD2 | App/bundle id | Resolved: `io.github.danielesc0911.leemusync` ([D22](../../decisions.md#d22-app-and-bundle-id-iogithubdanielesc0911leemusync-2026-10-07)) |
| OD3 | Store accounts and test devices | No Apple Developer or Play account yet ([D23](../../decisions.md#d23-distribution-before-store-accounts-2026-10-07)). Devices: [testing §Device lab](../../specs/testing.md#device-lab) |

## Deferred device checks (maintainer, 2026-10-08)
Known gaps, not dropped. Task 6 must answer or re-scope each one.
- Windows `.exe` and `.msi`, CachyOS `.AppImage` and `.tar.gz`: V-PKG-6 ([release](../../specs/release.md#verify)).
- SP7 emulator pass on Windows, macOS and CachyOS: V-PROF-1, V-ACC-1.
- Android: SP3 pass on the Galaxy A25, including DraStic and melonDS (V-AND-2).

---

### Task 1: Toolchains and Rust workspace skeleton

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `clippy.toml`, `justfile`
- Create: `crates/{core,crypto,store,api,engine,ipc}/Cargo.toml` and `src/lib.rs`
- Create: `crates/daemon/Cargo.toml`, `crates/daemon/src/main.rs`, `crates/daemon/tests/version.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `crates/cli/tests/version.rs`

**Interfaces:**
- Produces: binaries `leemusync` (package `leemusync-cli`) and `leemusyncd` (package `leemusync-daemon`), each printing `<bin> <version>`. Library crates exist, empty, with their responsibility in the crate docs.

- [x] **Step 1: Install toolchains**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
source "$HOME/.cargo/env"
cargo install just cargo-deny --locked
rustc --version && just --version && cargo deny --version
```
Expected: three version lines. (The fish shell uses `source "$HOME/.cargo/env.fish"`.)

- [x] **Step 2: Pin the toolchain.** Write `rust-toolchain.toml`, using the exact version `rustc --version` printed (e.g. `1.99.0`):

```toml
[toolchain]
channel = "1.99.0" # exact stable version printed in Step 1
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [x] **Step 3: Write the workspace manifest and lint config**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.0.0"
edition = "2024"
license = "MPL-2.0"
repository = "https://github.com/DanielEsc0911/leemusync"
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

- [x] **Step 4: Write the failing CLI version test**

`crates/cli/Cargo.toml`:
```toml
[package]
name = "leemusync-cli"
description = "LeemuSync command-line interface."
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
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
    assert_eq!(
        stdout.trim(),
        format!("leemusync {}", env!("CARGO_PKG_VERSION"))
    );
}
```

- [x] **Step 5: Run it and watch it fail**

Run: `cargo test -p leemusync-cli`
Expected: FAIL. `assertion left == right failed`, left `""`, right `"leemusync 0.0.0"`.

- [x] **Step 6: Implement**

`crates/cli/src/main.rs`:
```rust
//! `leemusync`: command-line interface. Spec: docs/specs/config-and-cli.md.

fn main() {
    println!("leemusync {}", env!("CARGO_PKG_VERSION"));
}
```

Run: `cargo test -p leemusync-cli` → Expected: PASS (1 test).

- [x] **Step 7: Same for the daemon (test first, then implementation)**

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

- [x] **Step 8: Library crates.** For each row, create `crates/<dir>/Cargo.toml` (same shape as the CLI manifest, without `[[bin]]`) and `crates/<dir>/src/lib.rs` with only the crate doc comment:

| dir | `description` | `lib.rs` |
|---|---|---|
| core | Domain model and pure sync logic. | `//! Domain model and pure sync logic. No I/O, no async.`<br>`//! Spec: docs/specs/architecture.md (Layers), docs/specs/sync-model.md.` |
| crypto | Encryption, key derivation and keyed hashing. | `//! Encryption, key derivation and keyed hashing. Pure, no I/O.`<br>`//! Spec: docs/specs/security.md (Cryptography).` |
| store | Storage backend interface and adapters. | `//! Storage backend interface and adapters. Moves opaque encrypted objects.`<br>`//! Spec: docs/specs/storage-backends.md.` |
| api | Engine API types and trait. | `//! Engine API: requests, responses, events and the engine trait. Versioned.`<br>`//! Spec: docs/specs/architecture.md (Engine API).` |
| engine | Sync engine orchestration. | `//! Sync engine: scanning, snapshots, transfers, leases, journal.`<br>`//! Spec: docs/specs/architecture.md (Engine internals), docs/specs/reliability.md.` |
| ipc | Same-user local IPC transport. | `//! Same-user local IPC: framing, handshake, engine client and server.`<br>`//! Spec: docs/specs/architecture.md (IPC).` |

- [x] **Step 9: Task runner.** `justfile`:

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

- [x] **Step 10: Verify.** Run: `just check` → Expected: fmt clean, clippy no warnings, 2 tests pass.

- [x] **Step 11: Commit**

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

- [x] **Step 1: Manifests and alias**

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

- [x] **Step 2: Write the failing tests.** `xtask/src/docs.rs` (tests first, with a stub that returns nothing):

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
        assert_eq!(
            relative_links("[a](https://x.y) [b](#top) [c](mailto:a@b.c)"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn skips_fenced_code() {
        assert_eq!(relative_links("```\n[a](nope.md)\n```"), Vec::<String>::new());
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

- [x] **Step 3: Run and watch them fail**

Run: `cargo test -p xtask`
Expected: FAIL in `keeps_relative_links_without_fragments`, `rejects_upward_dependency` and `rejects_crate_missing_from_table`.

- [x] **Step 4: Implement `docs.rs`** (replace the stub; keep the tests module):

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

- [x] **Step 5: Implement `layers.rs`** (replace the stub; keep the tests module):

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

- [x] **Step 6: Implement `main.rs`**

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

- [x] **Step 7: Verify**

Run: `cargo test -p xtask` → Expected: PASS (6 tests).
Run: `cargo xtask docs-check` → Expected: `xtask docs-check: ok` (the existing docs are consistent; fix any real error it reports).
Run: `cargo xtask layers` → Expected: `xtask layers: ok`.

- [x] **Step 8: Wire into `just lint`.** Append to the `lint` recipe:
```just
    cargo xtask layers
    cargo xtask docs-check
```
Run: `just check` → green.

- [x] **Step 9: Commit**
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

- [x] **Step 1: `deny.toml`**

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

- [x] **Step 2: `.github/workflows/ci.yml`**

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

- [x] **Step 3: Pin actions by commit SHA** (B2). For each `uses:` line, find the SHA of the newest release tag of that major version and write `uses: owner/repo@<sha> # vX.Y.Z`:
```bash
git ls-remote --tags https://github.com/actions/checkout | grep -E 'refs/tags/v5\.[0-9]+\.[0-9]+(\^\{\})?$' | tail -2
git ls-remote --tags https://github.com/EmbarkStudios/cargo-deny-action | grep -E 'refs/tags/v2\.[0-9]+\.[0-9]+(\^\{\})?$' | tail -2
```
Use the `^{}` (peeled) SHA when it's listed.

- [x] **Step 4: `.github/dependabot.yml`**

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

- [x] **Step 5: Add `cargo deny check` to the `just lint` recipe** (after clippy). Run `just check` → green.

- [x] **Step 6: Commit**
```bash
git add deny.toml .github/ justfile
git commit -m "ci: add cargo-deny, CI workflow and dependabot" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

- [x] **Step 7: Publish (maintainer approval required).** The remote `origin` is `git@github.com:DanielEsc0911/leemusync.git`, and `main` and the work branch are pushed. Push the latest commits, open a PR `chore/phase-0-foundations` → `main`, confirm every CI job is green, and merge. In repository settings: protect `main` (PR + required checks), enable private vulnerability reporting, secret scanning and push protection.

---

### Task 4: Flutter app skeleton with en/es and `i18n-check`

**Files:**
- Create: `app/` (via `flutter create`), `app/l10n.yaml`, `app/lib/l10n/app_en.arb`, `app/lib/l10n/app_es.arb`, `app/test/app_test.dart`, `xtask/src/i18n.rs`
- Modify: `app/pubspec.yaml`, `app/analysis_options.yaml`, `app/lib/main.dart`, `app/.gitignore`, `xtask/src/main.rs`, `justfile`, `.github/workflows/ci.yml`, `.github/dependabot.yml`

**Interfaces:**
- Produces: `LeemuSyncApp({Key? key, Locale? locale})` in `app/lib/main.dart`; ARB keys `appTitle`, `statusSynced`, `statusUploading(count)`; `cargo xtask i18n-check`; internal `i18n::check(&Path) -> Result<(), Vec<String>>`, `i18n::diff(&str, &BTreeSet<String>, &BTreeSet<String>) -> Vec<String>`.

- [x] **Step 1: Install Flutter (stable) and create the app** (`--org` from D22):
```bash
flutter --version
flutter create --org io.github.danielesc0911 --project-name leemusync \
  --platforms android,ios,macos,windows,linux --empty app
```
Pin the version in `app/pubspec.yaml` under `environment:` → `flutter: <exact version from flutter --version>`. Set the top-level `version: 0.0.0+1` so it matches the workspace version ([release §Versioning](../../specs/release.md#versioning)).

- [x] **Step 2: Localisation setup**
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

- [x] **Step 3: Strict analysis.** `app/analysis_options.yaml`:
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

- [x] **Step 4: Write the failing widget test.** `app/test/app_test.dart`:
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

- [x] **Step 5: Implement.** `app/lib/main.dart`:
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

- [x] **Step 6: `i18n-check`, test first.** `xtask/src/i18n.rs`:
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
        if name == TEMPLATE || entry.path().extension().is_none_or(|ext| ext != "arb") {
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
        assert_eq!(diff("app_es.arb", &set(&["a"]), &set(&["a"])), Vec::<String>::new());
    }
}
```
Write the tests module first with `fn diff(...) -> Vec<String> { Vec::new() }` as a stub → `cargo test -p xtask` FAILS in `reports_missing_and_unknown_keys` → then the full file above → PASS.

In `xtask/src/main.rs`: add `mod i18n;`, add the arm `"i18n-check" => i18n::check(&root),`, and list `i18n-check` in the unknown-task message.

Run: `cargo xtask i18n-check` → `xtask i18n-check: ok`. Then delete `statusSynced` from `app_es.arb` and run it again. Expected: exit code 1 and an error line saying `app_es.arb` is missing key `statusSynced`. Restore the key.

- [x] **Step 7: Final `justfile`**
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

- [x] **Step 8: CI and Dependabot.** In `ci.yml` add `- run: cargo xtask i18n-check` to `repo-checks`, and add the job below (pin the action SHA as in Task 3 Step 3). In `dependabot.yml` add the `pub` ecosystem with `directory: /app`.
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

- [x] **Step 9: Verify and commit.** `just check` → green. Then:
```bash
git add app/ xtask/ justfile .github/
git commit -m "feat(app): add Flutter shell with English and Spanish strings" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Release pipeline (`.apk` `.msi` `.exe` `.dmg` `.deb` `.rpm` `.AppImage` `.tar.gz`)

Spec: [release](../../specs/release.md) (Artifacts, Install layout, Signing). Decisions: D22, D23, D24.

**Files:**
- Create: `xtask/src/metadata.rs`, `xtask/src/version.rs`, `.github/workflows/release.yml`, `packaging/README.md`, `packaging/icons/leemusync.png`, `packaging/linux/nfpm.yaml`, `packaging/linux/io.github.danielesc0911.leemusync.desktop`, `packaging/windows/leemusync.iss`, `packaging/windows/leemusync.wxs`, `packaging/windows/en-us.wxl`, `packaging/windows/es-es.wxl`, `packaging/macos/make-dmg.sh`
- Modify: `xtask/src/layers.rs`, `xtask/src/main.rs`, `app/windows/CMakeLists.txt`, `app/windows/runner/Runner.rc`, `app/linux/CMakeLists.txt`, `app/macos/Runner/Configs/AppInfo.xcconfig`, `app/android/app/build.gradle.kts`, `.gitignore`, `docs/specs/release.md`

**Interfaces:**
- Consumes: binaries `leemusync` and `leemusyncd` (Task 1), the Flutter app (Task 4), `xtask` (Task 2).
- Produces: `cargo xtask version-check <tag>`; internal `metadata::load(&Path) -> Result<serde_json::Value, Vec<String>>`, `version::compare(&str, &str, &str) -> Result<(), Vec<String>>`; `release.yml` jobs `version`, `linux`, `headless`, `windows`, `macos`, `android`, `publish`.

**Permanent identifiers (never change after the first release):**
- Inno Setup `AppId`: `{350A21EC-E42B-4C95-8E97-B1433D66294D}`
- WiX `UpgradeCode`: `95ABB515-7BAD-4E28-AC9F-BF5865B70A15`
- App/bundle id and Windows AppUserModelID: `io.github.danielesc0911.leemusync`

- [x] **Step 1: Shared `cargo metadata` loader (refactor; tests must stay green).** Move the command call out of `layers.rs` into `xtask/src/metadata.rs`:

```rust
//! Shared `cargo metadata` loader for xtask checks.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

pub(crate) fn load(root: &Path) -> Result<Value, Vec<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| vec![format!("cannot run cargo metadata: {e}")])?;
    if !output.status.success() {
        return Err(vec![String::from_utf8_lossy(&output.stderr).into_owned()]);
    }
    serde_json::from_slice(&output.stdout).map_err(|e| vec![format!("invalid cargo metadata: {e}")])
}
```
In `layers.rs`, `check` starts with `let metadata = crate::metadata::load(root)?;`. Remove the inline call and the now-unused `Command` and `Value` imports (clippy fails on unused imports). Add `mod metadata;` to `main.rs`. Run `cargo test -p xtask` and `cargo xtask layers` → both still pass.

- [x] **Step 2: `version-check`, test first.** `xtask/src/version.rs` with the tests below and a stub `fn compare(..) -> Result<(), Vec<String>> { Ok(()) }` → `cargo test -p xtask` FAILS in `rejects_mismatch` and `rejects_tag_without_v`. Then the full file:

```rust
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
        errors.push(format!("Cargo.toml version {cargo_version} != tag {wanted}"));
    }
    if app != wanted {
        errors.push(format!("app/pubspec.yaml version {app} != tag {wanted}"));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

#[cfg(test)]
mod tests {
    use super::compare;

    #[test]
    fn accepts_matching_versions_ignoring_build_number() {
        assert_eq!(compare("v0.0.1-alpha.1", "0.0.1-alpha.1", "0.0.1-alpha.1+1"), Ok(()));
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
```
In `main.rs`: `mod version;`, the arm `"version-check" => version::check(&root, &std::env::args().nth(2).unwrap_or_default()),`, and `version-check <tag>` in the unknown-task message. Run: `cargo test -p xtask` → PASS. `cargo xtask version-check v0.0.0` → ok only if `pubspec.yaml` says `version: 0.0.0+1`; set it so.

- [x] **Step 3: GUI executable names and placeholder icon** (release §Install layout)
  - `app/windows/CMakeLists.txt`: `set(BINARY_NAME "leemusync")` → `set(BINARY_NAME "LeemuSync")`. In `app/windows/runner/Runner.rc`, set `FileDescription` and `ProductName` to `LeemuSync`.
  - `app/linux/CMakeLists.txt`: `set(BINARY_NAME "leemusync")` → `set(BINARY_NAME "leemusync-gui")`. Keep `APPLICATION_ID` = `io.github.danielesc0911.leemusync`.
  - `app/macos/Runner/Configs/AppInfo.xcconfig`: `PRODUCT_NAME = LeemuSync`.
  - `cp app/macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_256.png packaging/icons/leemusync.png` (Flutter's default icon; placeholder until the Phase 3 brand).
  - Run `cd app && flutter build linux --release` → `build/linux/x64/release/bundle/leemusync-gui` exists, and `flutter test` still passes.

- [x] **Step 4: Linux packaging.** `packaging/linux/io.github.danielesc0911.leemusync.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=LeemuSync
Comment=Keep your emulator saves in sync on every device
Comment[es]=Mantén tus partidas guardadas sincronizadas en todos tus dispositivos
Exec=leemusync-gui
Icon=io.github.danielesc0911.leemusync
Terminal=false
Categories=Game;Utility;
```
`packaging/linux/nfpm.yaml` (the workflow exports `LEEMUSYNC_VERSION` and `NFPM_ARCH` = `amd64`|`arm64`, and stages files under `dist/stage/`):
```yaml
name: leemusync
arch: ${NFPM_ARCH}
platform: linux
version: ${LEEMUSYNC_VERSION}
version_schema: semver
maintainer: Daniel <daescalona@proton.me>
description: Keep your emulator saves in sync on every device, using storage you own.
homepage: https://github.com/DanielEsc0911/leemusync
license: MPL-2.0
contents:
  - src: dist/stage/gui/
    dst: /opt/leemusync/
    type: tree
  - src: /opt/leemusync/leemusync-gui
    dst: /usr/bin/leemusync-gui
    type: symlink
  - src: dist/stage/bin/leemusync
    dst: /usr/bin/leemusync
  - src: dist/stage/bin/leemusyncd
    dst: /usr/bin/leemusyncd
  - src: packaging/linux/io.github.danielesc0911.leemusync.desktop
    dst: /usr/share/applications/io.github.danielesc0911.leemusync.desktop
  - src: packaging/icons/leemusync.png
    dst: /usr/share/icons/hicolor/256x256/apps/io.github.danielesc0911.leemusync.png
overrides:
  deb:
    depends: [libgtk-3-0]
  rpm:
    depends: [gtk3]
```
Commands (pin the nfpm and appimagetool versions in the workflow; check each flag with `--help` at that version, V-PKG-3):
```bash
nfpm package --config packaging/linux/nfpm.yaml --packager deb --target dist/out/
nfpm package --config packaging/linux/nfpm.yaml --packager rpm --target dist/out/
# APPIMAGE_ARCH = x86_64 | aarch64
# AppImage: AppDir = GUI bundle + usr/bin/{leemusync,leemusyncd} + .desktop + icon + AppRun (exec "$APPDIR/leemusync-gui" "$@")
ARCH="$APPIMAGE_ARCH" appimagetool dist/AppDir "dist/out/leemusync-${LEEMUSYNC_VERSION}-linux-${APPIMAGE_ARCH}.AppImage"
# tar.gz layout: leemusync/leemusync-gui (+ bundle files) and leemusync/bin/ (release §Install layout)
mkdir -p dist/tar/leemusync && cp -r dist/stage/gui/. dist/tar/leemusync/ && cp -r dist/stage/bin dist/tar/leemusync/bin
tar -C dist/tar -czf "dist/out/leemusync-${LEEMUSYNC_VERSION}-linux-${APPIMAGE_ARCH}.tar.gz" leemusync
```
Check: `dpkg-deb -c` and `rpm -qlp` list exactly the install layout; nfpm turns `-alpha.1` into a version that sorts before the final release (deb `~`). Record the result.

- [x] **Step 5: Windows packaging.** Both installers install the layout from release §Install layout and remove it cleanly on uninstall.
  - `packaging/windows/leemusync.iss` (Inno Setup 6.3+): `AppId={{350A21EC-E42B-4C95-8E97-B1433D66294D}`, `AppName=LeemuSync`, `AppVersion` from env `LEEMUSYNC_VERSION`, `VersionInfoVersion` = numeric `X.Y.Z.0`, `DefaultDirName={autopf}\LeemuSync`, `ArchitecturesAllowed=x64compatible`, `ArchitecturesInstallIn64BitMode=x64compatible`, `LicenseFile=..\..\LICENSE`, `ChangesEnvironment=yes`, `WizardStyle=modern`, `OutputBaseFilename=leemusync-<version>-windows-x64-setup`. `[Languages]`: `en` = `compiler:Default.isl`, `es` = `compiler:Languages\Spanish.isl` (P6). `[Files]`: GUI bundle → `{app}`, CLI + daemon → `{app}\bin`. `[Icons]`: Start Menu shortcut to `{app}\LeemuSync.exe` with `AppUserModelID: "io.github.danielesc0911.leemusync"`. Add `{app}\bin` to the machine `PATH` on install and remove it on uninstall, without duplicates.
  - `packaging/windows/leemusync.wxs` (WiX v5+, V-PKG-1): `Package` with `UpgradeCode="95ABB515-7BAD-4E28-AC9F-BF5865B70A15"`, `Version` = **numeric** `X.Y.Z` (MSI rejects `-alpha`; the pre-release only appears in the file name), `MajorUpgrade`, `MediaTemplate EmbedCab="yes"`. GUI files under `ProgramFiles64Folder\LeemuSync` (harvested from the Flutter output folder), `bin\` with CLI + daemon (one Component per file), a Start Menu shortcut with `ShortcutProperty Key="System.AppUserModel.ID" Value="io.github.danielesc0911.leemusync"`, and an `Environment` element appending `[INSTALLFOLDER]bin` to the system `PATH`. User-visible strings in `en-us.wxl` / `es-es.wxl`. Build one MSI per culture: `wix build -culture en-US …` and `-culture es-ES …` → `…-windows-x64-en.msi`, `…-windows-x64-es.msi`.
  - Test both on the Windows machine: install → `LeemuSync` in the Start Menu, `leemusync` works in a new terminal → uninstall → nothing left behind.

- [x] **Step 6: macOS DMG.** `packaging/macos/make-dmg.sh`:
```bash
#!/usr/bin/env bash
# Usage: make-dmg.sh <path/to/LeemuSync.app> <dir with leemusync + leemusyncd> <output.dmg>
set -euo pipefail
app="$1"; helpers="$2"; out="$3"
mkdir -p "$app/Contents/Helpers"
cp "$helpers/leemusync" "$helpers/leemusyncd" "$app/Contents/Helpers/"
codesign --force --deep --sign - "$app"   # ad-hoc signature (D23); Developer ID later
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname LeemuSync -srcfolder "$stage" -ov -format UDZO "$out"
```
The CLI and daemon are universal binaries: build `aarch64-apple-darwin` and `x86_64-apple-darwin`, then `lipo -create -output <out> <arm64> <x86_64>`. Test on the MacBook: open the DMG → drag to Applications → Open Anyway → the app starts; `LeemuSync.app/Contents/Helpers/leemusync` prints its version.

- [x] **Step 7: Android release signing.** In `app/android/app/build.gradle.kts`, sign `release` with the maintainer key when CI provides it; otherwise fall back to the debug key:
```kotlin
val releaseKeystore = System.getenv("ANDROID_KEYSTORE_PATH")

android {
    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            signingConfig = signingConfigs.getByName(if (releaseKeystore != null) "release" else "debug")
        }
    }
}
```
Merge this into the generated `android { … }` block; don't create a second one. Add `*.jks` and `*.keystore` to `.gitignore`.
**Maintainer, once:** `keytool -genkeypair -v -keystore leemusync-release.jks -keyalg RSA -keysize 4096 -validity 10000 -alias leemusync`. Store `base64` of the file and the passwords as repository secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Keep two offline backups: losing the key means installed apps can never update.

- [x] **Step 8: `.github/workflows/release.yml`.** Triggers: `push: tags: ['v*']`, `workflow_dispatch`, and `pull_request` with `paths: ['.github/workflows/release.yml', 'packaging/**']`. GitHub only lets you start a `workflow_dispatch` workflow once it's on the default branch, so PRs that touch packaging are the dry run. Top-level `permissions: contents: read`. Every action pinned by SHA (Task 3 Step 3). Version = tag without `v`, or for manual and PR runs `0.0.0-dev.<run_number>` for file names only.

| Job | Runner(s) | Does |
|---|---|---|
| `version` | ubuntu-latest | Tag runs only: `cargo xtask version-check "$GITHUB_REF_NAME"` |
| `linux` | `ubuntu-22.04` (x86_64), `ubuntu-22.04-arm` (arm64) | apt: `clang cmake ninja-build pkg-config libgtk-3-dev liblzma-dev libstdc++-12-dev`; `cargo build --release -p leemusync-cli -p leemusync-daemon`; `flutter build linux --release`; stage; nfpm deb + rpm; AppImage; tar.gz. If the Flutter action lacks linux-arm64 (V-PKG-4), clone Flutter at the version pinned in `pubspec.yaml` |
| `headless` | ubuntu-22.04 | CLI + daemon `.tar.gz` for x86_64, aarch64, armv7 (`rustup target add`; apt `gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf`; `CARGO_TARGET_<TRIPLE>_LINKER`) |
| `windows` | windows-latest | Rust release build; `flutter build windows --release`; Inno Setup → `.exe`; WiX → two `.msi` (V-PKG-2: install with `choco`/`dotnet tool` if absent) |
| `macos` | macos-latest | Rust for both macOS targets + `lipo`; `flutter build macos --release`; `make-dmg.sh` |
| `android` | ubuntu-latest | `actions/setup-java` (Temurin 17); decode `ANDROID_KEYSTORE_BASE64` to a temp file and export `ANDROID_KEYSTORE_PATH` when present; **tag runs fail if the secrets are missing**; `flutter build apk --release --split-per-abi`; rename to `leemusync-<v>-android-<abi>.apk` (`-debug` suffix when unsigned) |
| `publish` | ubuntu-latest, needs all | Download artifacts; `sha256sum * > SHA256SUMS`; `actions/attest-build-provenance` over every file (job permissions: `id-token: write`, `attestations: write`, `contents: write`); tag → `gh release create "$GITHUB_REF_NAME" --draft --verify-tag --title "LeemuSync $VERSION" dist/*`; manual or PR → upload one combined workflow artifact. Attest only on tag and manual runs (PRs from forks get no OIDC token) |

- [x] **Step 9: Dry run.** Push the branch and open a PR. The `pull_request` trigger builds every artifact (download them from the run's Artifacts section). Download the artifacts and test each on the [Device lab](../../specs/testing.md#device-lab): `.exe` + `.msi` on Windows; `.rpm`, `.AppImage`, `.tar.gz` on Fedora; `.AppImage` + `.tar.gz` on CachyOS; `.deb` in an Ubuntu container (`dpkg -i` + `leemusync`); `.apk` on the Galaxy A25; `.dmg` on the MacBook. Write the results and the answers to V-PKG-1…5 into release.md. Done 2026-10-08 (run 37789852214): see [release §Dry run results](../../specs/release.md#dry-run-results-2026-10-08). Windows installers and CachyOS deferred → V-PKG-6.

- [ ] **Step 10: Tag test (maintainer approval required).** Set the version to `0.0.1-alpha.1` in `Cargo.toml` and `app/pubspec.yaml` (`0.0.1-alpha.1+1`). Tag `v0.0.1-alpha.1` and push the tag. Confirm the draft release lists every format + `SHA256SUMS`, and that `gh attestation verify <file> --repo DanielEsc0911/leemusync` passes. Leave it as a draft (don't publish) or delete the draft and tag, as the maintainer decides.

- [x] **Step 11: Commit.** `just check` → green, then:
```bash
git add xtask/ packaging/ .github/workflows/release.yml app/ .gitignore docs/specs/release.md
git commit -m "ci(release): build apk, msi, exe, dmg, deb, rpm, AppImage and tar.gz" \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Spikes
Each spike runs on a `spike/<id>-<slug>` branch, timeboxed (default 2 days). The code is throwaway unless a later task adopts it. **Done** means every listed Verify item is answered with evidence (versions, devices, logs, screenshots, links). The results are written into the named specs and decisions (W1, W3) and committed to `main` as `docs(spike): …`.

### SP1: Flutter + Rust bridge on every target
- **Answers:** D4 (FRB + UniFFI in one cdylib, one engine instance), V-DESK-4, V-SEC-2.
- [ ] Create `crates/bridge` (cdylib + staticlib) with flutter_rust_bridge exposing `fn core_version() -> String` and UniFFI exposing `fn engine_ping() -> String`. Both read and increment one global counter, which proves there's a single instance.
- [ ] Call FRB from Dart and UniFFI from Kotlin (Android) and Swift (iOS) in the same app. Confirm the counter is shared.
- [ ] Build and run on Windows x64, macOS (MacBook Air M4), Linux x86_64 (Fedora XFCE, CachyOS), Android arm64 (Galaxy A25), iOS (iPhone 15 Pro, free Apple ID provisioning). Build and test without a device on Windows arm64 and Linux arm64 (GitHub ARM runners). See [testing §Device lab](../../specs/testing.md#device-lab).
- [ ] Make one HTTPS request with `rustls` + platform verifier on each target.
- [ ] Measure per target: cold start, app size, idle RSS, frame times for a 120 Hz animation test screen (profile mode).
- [ ] **Record:** architecture (bridge details), decisions D2/D4 (confirm or supersede), desktop V-DESK-4, security V-SEC-2, design-system budgets (adjust with data).

### SP2: Desktop daemon presence
- **Answers:** V-DESK-1/2/3, V-REL-1/2, V-SEC-3, V-STAT-4.
- [ ] Prototype daemon: tray icon with state swap + menu; actionable notification; on Windows, macOS, Linux XFCE (Fedora), CachyOS's desktop, and GNOME in a VM (with and without the AppIndicator extension).
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
- [ ] Verify the status-bar chip on the Android 16 emulator (Pixel image), the Now Bar on the Galaxy A25 (record its One UI version first), and Super Island on the Redmi Note 15 Pro (record its HyperOS version and region). Screenshots.
- [ ] Fallback on Android 13–15 devices.
- [ ] **Record:** status-and-notifications, android.

### SP5: iOS access and integrations
- **Answers:** V-IOS-1/2/3/4/5, V-PROF-3, V-STAT-3.
- [ ] With free Apple ID provisioning on the iPhone 15 Pro: which capabilities work (Live Activities, App Groups, App Intents, background tasks)? Note the 7-day re-signing limit.
- [ ] For RetroArch, PPSSPP, Delta, Gamma, Provenance, Folium, DolphiniOS, MeloNX: are saves exposed in Files? Where? Same format as desktop?
- [ ] Security-scoped bookmark to another app's Files folder: survives restart and update?
- [ ] App Intent triggered by a Shortcuts "app is closed" automation runs the Rust bridge to completion.
- [ ] Live Activity + Dynamic Island updated from a `BGContinuedProcessingTask`; record duration and update limits.
- [ ] **Record:** ios, emulator-profiles (iOS roots), status-and-notifications.

### SP6: Storage backends
- **Answers:** V-STORE-1/2/3/4, V-SYNC-1/2.
- [ ] Capability matrix with OpenDAL for s3 (AWS, MinIO, R2, B2), webdav (Nextcloud), sftp, fs, gdrive, onedrive, dropbox: `if_not_exists`, server mtime, list-after-write delay, rate limits.
  - Local part done 2026-10-08 (OpenDAL 0.59.4; s3 on SeaweedFS and RustFS since MinIO images weren't pullable, webdav on Nextcloud, sftp, fs; see [storage-backends §Tested capabilities](../../specs/storage-backends.md#tested-capabilities-sp6)); cloud services pending accounts.
- [ ] Google Drive with `drive.file`: OAuth flow on desktop and mobile; scope classification and verification needs.
- [ ] MEGA: test S4 via s3; review the existing Rust MEGA crates (maintenance, security); MEGAcmd WebDAV.
- [ ] SFTP on Android and iOS builds.
- [ ] **Record:** storage-backends, sync-model (lease liveness), decision for MEGA.

### SP7: Desktop emulator layouts (initial five)
- **Answers:** V-PROF-1, V-ACC-1.
- [ ] On Linux and Windows (+ macOS where available): install Cemu, RetroArch, Dolphin, DuckStation, PCSX2. Record base paths (native, Flatpak, portable), config keys for custom paths, process names, save layouts, and per-game/folder card settings.
  - Linux done 2026-10-08 (Flatpak Cemu 2.6, RetroArch 1.22.2, Dolphin 2606a, DuckStation 0.1-9482, PCSX2 v2.8.2 + upstream source for native/portable rules; V-ACC-1 format, BotW ids and `user/common` answered; drafts in `profiles/`); Windows and macOS pending.
- [ ] Cemu: account folder layout, how to read the display name from `account.dat`, whether BotW uses `user/common`, and BotW title IDs per region (expected: US `101c9400`, EU `101c9500`, JP `101c9300`, high part `00050000`).
- [ ] Turn each into a draft profile + fixture tree (adopted in Phase 1).
- [ ] **Record:** emulator-profiles, players-and-accounts.

---

### Task 6: Close Phase 0
- [ ] Every Verify item targeted by SP1–SP7 is answered: moved into its spec's body with evidence, or re-scoped with a reason.
- [ ] Decisions confirmed or superseded (D2, D4, D10 at least). New decisions added (Flatpak, MEGA, Android all-files access).
- [ ] Spec statuses updated. `cargo xtask docs-check` green.
- [ ] Write `docs/progress/plans/phase-1-engine.md` with the writing-plans format and add it to the board.
- [ ] `CHANGELOG.md` `[Unreleased]` updated; this plan removed from the board and deleted ([progress](../README.md)).
- [ ] Commit: `docs(progress): close phase 0 and plan phase 1`.
