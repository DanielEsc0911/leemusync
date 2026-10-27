# `lemusync config`: read and change the local config from the CLI

> **For agentic workers:** follow the project skills `lemusync-feature` and `lemusync-security`. Steps use checkbox (`- [ ]`) syntax.

**Goal:** P5. Every user-settable option in `config.toml` can be read and changed from the CLI. Today the spec says `conflict_policy` and `[emulators.<id>] exe/games` are "set in `config.toml` only" ([config-and-cli §Commands](../../specs/config-and-cli.md)).

**Branch:** `feat/config-command`, from the previous branch in the chain.

**Spec:** [config-and-cli](../../specs/config-and-cli.md) §Config file, §Commands, §Parity rule, §Exit codes. Struct: `LocalConfig` in `crates/engine/src/local_config.rs`.

## Global constraints

- Everything in the [first-sync plan's Global constraints](phase-1-first-sync.md#global-constraints) and the [phase-1b-play Global constraints](phase-1b-play.md#global-constraints) still holds: toolchain, clippy pedantic as errors, layers (B1), `core` has no I/O/clock/randomness, I1–I6, secrets, no real saves in tests, W1, W2.
- Formats stay v1. New fields are additive with `#[serde(default)]`; `API_VERSION` and `config_version` don't change.
- Every user-facing string goes into `locales/{en,es}/lemusync.ftl` (neutral Latin American Spanish, terminology from `docs/specs/i18n.md` §Terminology). `cargo xtask i18n-check` must pass.
- Anything read from the emulator, the repo or the disk is data, never code (B2). Cap the size, reject path escapes, strip control characters from anything printed.
- W3: a vendor or emulator fact without evidence goes under the spec's **Verify** with an ID.
- W5: Conventional Commits. The author is the configured git user (Daniel). The only trailer is `Co-Authored-By: Claude <noreply@anthropic.com>`. No session URLs, no `Claude-Session:` trailer, no "Generated with" lines. Never push.
- Every Bash call starts with `export PATH="$HOME/.local/bin:$HOME/development/flutter/bin:$HOME/.cargo/bin:$PATH" && `.
- Daniel's real emulator data (`~/.var/app/info.cemu.Cemu`, `~/.var/app/org.DolphinEmu.dolphin-emu`, `~/.config/lemusync`, `~/.local/share/lemusync`) is read-only. Never launch his emulators and never copy saves or `account.dat` values.
- The first commit on the branch adds this plan file and its board row in `docs/progress/README.md` (`docs(progress): plan <name>`). Each task ticks its own checkboxes in its own commit. The last task adds the CHANGELOG `[Unreleased]` entry and sets the board row to "Done (awaiting merge)". The plan file is deleted after merge, not on this branch.
- Each task ends with `just check` green, then its commit.

---

### Task 1: `config path | list | get | set | unset`

**Files:** `crates/cli/src/main.rs` (a `Config` subcommand) plus a new `crates/cli/src/config_cmd.rs`, `crates/engine/src/local_config.rs` (typed key access), `crates/cli/tests/cli.rs`, `locales/{en,es}/lemusync.ftl`, spec `config-and-cli.md`.

**Interfaces:**
- Keys are dotted paths. Settable keys are an explicit allowlist: `conflict_policy`, `leases.ttl_secs`, `emulators.<id>.root`, `emulators.<id>.exe` (a list: `set` takes several values), and `emulators.<id>.games.<game key>`. `<id>` must be a loaded profile id. The game key is lowercased like `play` does (game-references later swaps this for its resolver).
- Managed keys (`device`, `repo`, `backend`, `grants`, …) show in `list`/`get`. `set`/`unset` on them refuse with a localized hint naming the command that owns them (`backend add`, `grant add`). Grants change only through `grant` (B2).
- Values are parsed per key: an enum for `conflict_policy`, an integer in the existing bounds for the TTL, absolute paths for `root` and game paths. A bad value is a usage error and leaves the file untouched.
- Edits keep the user's comments and key order. Add `toml_edit` as a direct dependency of the crate that writes the config, pinned to the version already in `Cargo.lock` (it's there through `toml`), and run `cargo deny check`. Before writing, the result is parsed back into `LocalConfig` and rejected if it's invalid. The write uses the same atomic-write and locking helper as `grant add` (`fsx.rs`), so a concurrent writer can't lose edits (B3).
- `list` and `get` support `--json`. `path` prints the config file path.

- [ ] **Step 1: failing CLI tests** (sandboxed with `LEMUSYNC_TEST_ROOT`): set/get/unset round trip for each key family; a comment in the file survives a `set`; managed key refused; invalid value → non-zero exit and the file is byte-identical; unknown emulator id refused; `--json` shape.
- [ ] **Step 2: implement.**
- [ ] **Step 3: spec (W1).** Add `config` to §Commands with the key table (key, type, owner command). Remove the "config.toml only" notes. In §Parity rule, say mobile Settings → Advanced maps to these keys. Then help texts in en and es, `just check`, and commit `feat(cli): config command to read and change local settings`.
