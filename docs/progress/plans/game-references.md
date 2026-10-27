# Game references: aliases, names, region-agnostic matching

> **For agentic workers:** follow the project skills `lemusync-feature` (always) and `lemusync-security` (Task 2). Steps use checkbox (`- [ ]`) syntax.

**Goal:** `lemusync play botw --as daniel` works. Any command that takes a game accepts an alias, the game key (title id), or the game's name typed loosely. Matching ignores case, punctuation, accents and region or revision tags like `(USA)`, `(EUR)`, `(JPN)` or `[v1.2]`. Those tags are real parts of a title, but nobody should have to type them. When the emulator is unambiguous, the user doesn't have to type it either. Builds on the [game catalog](game-catalog.md) (names and keys come from `Engine::games()` plus the repo's synced streams).

**Branch:** `feat/game-references`, from `feat/game-catalog`.

**Spec:** [config-and-cli](../../specs/config-and-cli.md) §Commands, [sync-model](../../specs/sync-model.md) §Launcher mode, [players-and-accounts](../../specs/players-and-accounts.md) (the repo config).

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

### Task 1: Pure resolver in `core`

**Files:** new `crates/core/src/game_ref.rs`, spec `config-and-cli.md` (new §Game references).

**Interfaces:**
- `normalize(&str) -> String`: fold Latin-1 and Latin Extended-A letters to ASCII with a small table (no new dependency; `é→e`, `ñ→n`, `ü→u`, `ß→ss`, `œ→oe`, …), lowercase, drop bracketed groups `(…)` and `[…]`, turn every non-alphanumeric run into one space, trim.
- `resolve(query, candidates: &[Candidate { emulator, key, name: Option, aliases }]) -> Result<&Candidate, RefError::{NotFound, Ambiguous(Vec<&Candidate>)}>`, trying in order, and stopping at the first tier with any match:
  1. exact alias
  2. key, case-insensitive
  3. normalized name equality

  More than one match in the winning tier is `Ambiguous`, which lists every candidate (name, emulator, key) so region variants are visible. Optional emulator filter.

- [x] **Step 1: failing table tests:**
  - `botw` (alias) and `101c9400` (key);
  - `the legend of zelda breath of the wild` and `The Legend of Zelda: Breath of the Wild (USA)` against a name without tags; `zelda breath` is NotFound (no fuzzy tier, P1);
  - USA and EUR variants that differ only by tag → `Ambiguous`;
  - `pokemon` vs `Pokémon`, plus `é`, `ñ`, `ü`, `ß` and `œ` folds;
  - extra spaces, quotes, and an empty query.
- [x] **Step 2: implement**, then the spec, `just check`, and commit `feat(core): resolve games by alias, key or loose name`.

### Task 2: Aliases in the local config

**Why:** a short name like `botw` with no spaces or quotes. Aliases are local (P1). Sharing them through the repo config can come later if people ask; the spec records that under Open questions.

**Files:** `crates/engine/src/local_config.rs` (`aliases: BTreeMap<String, String>` alias → `"<emulator>/<game key>"`, serde default, the `[aliases]` table), the `config` allowlist (`aliases.<alias>`), `crates/cli` (`games alias <game> <alias>`, `games unalias <alias>`, aliases shown in `games`), `locales`, specs.

**Interfaces:** `config set aliases.<alias> <emulator>/<key>` is the source of truth. `games alias <game> <alias>` is a helper that resolves `<game>` with the Task 1 resolver and writes that same key. It refuses an ambiguous game rather than picking one, and it stores the resolved `<emulator>/<key>`, never the query text.

- [x] **Step 1: failing tests.**
  - Alias syntax is `^[a-z0-9][a-z0-9_-]{0,31}$`.
  - An alias is unique, and it can't equal another game's key.
  - An alias on an unknown game is refused.
  - An invalid alias in a hand-edited `config.toml` is rejected on load with a clear error naming the key.
  - `games alias` with an ambiguous `<game>` refuses and lists the candidates.
- [x] **Step 2: implement**, then specs, `just check`, commit `feat(cli): game aliases`.

### Task 3: Every game argument goes through the resolver

**Files:** `crates/cli/src/main.rs` and the commands that take a game, `crates/engine/src/{play,query,resolve,lease}.rs` where they take a game key, `crates/cli/tests/cli.rs`, `locales`, spec `config-and-cli.md` §Commands.

**Interfaces (breaking CLI change; pre-alpha, so no compatibility shim):**
- `play <game> [--emu <id>] [--as <player>] [-- args]`
- `history <game> [--emu <id>] [--as <player>]`
- `restore <game> <what> [--emu <id>] [--as <player>] [--yes]`
- `resolve <game> <version> [--as]` and `lease release --force <game>` resolve `<game>` the same way.

When `--as` is omitted, `play` keeps its current default-player behaviour. `history` and `restore` used to require a player; now they use that same default and fail with the existing "which player?" error when it's ambiguous. Add a test for that.

- [x] **Step 1: failing CLI tests:**
  - `play botw --as p1` and `play "breath of the wild" --as p1` launch the same game;
  - an ambiguous query exits with a usage error that lists the candidates and suggests `--emu` or an alias;
  - an unknown game exits with a localized not-found error that suggests `lemusync games`;
  - the old `play cemu 101c9400` form now fails with a clear usage message.
- [x] **Step 2: implement**, then update every example in the docs that uses the old shapes (`grep -rn "play cemu\|history \|restore \|resolve \|lease release" docs/ crates/`), and change the bridge or `docs/specs/ui/navigation.md` only if they mirror these arguments. CHANGELOG, board row, `just check`, commit `feat(cli)!: games by alias or name in every command`.

### Deviations (found while implementing)
- `resolve` takes the emulator filter as a parameter: `resolve(query, emulator: Option<&str>, candidates)`.
- The engine's `play`, `history`, `restore`, `resolve` and `lease_release_game` keep taking a resolved emulator and key. The CLI resolves first with `Engine::resolve_game` (`crates/engine/src/game_refs.rs`), which gathers the candidates (catalog, aliases, repo streams).
- `config set aliases.<alias>` works offline, so it checks the game against this device's catalog only; `games alias` also accepts a game known only from the repo.
- `emulators.<id>.games.<game>` accepts an alias of one of `<id>`'s games in place of the key.
- The old `play <emulator> <game>` form fails with clap's own usage error (`unexpected argument`, exit 2).
