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
    cargo deny check
    cargo xtask layers
    cargo xtask docs-check

# All tests
test:
    cargo test --workspace

# Everything CI runs; must pass before every commit
check: lint test
