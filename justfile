# LemuSync task runner. `just --list` shows every recipe.
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

# Engine budget benches (release) and the release CLI size (reliability §Budgets)
bench:
    cargo bench -p lemusync-engine --bench budgets
    cargo build --release -p lemusync-cli
    ls -l target/release/lemusync
