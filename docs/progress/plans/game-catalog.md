# Game catalog: find installed games, their folders and their names

> **For agentic workers:** follow the project skills `lemusync-feature` (always), `lemusync-emulator-profile` (Tasks 1–3) and `lemusync-security` (Tasks 2–3). Steps use checkbox (`- [ ]`) syntax.

**Goal:** stop asking users to hand-write `[emulators.<id>] games."<title id>" = "<path>"`. LemuSync reads the game folders the emulator already knows (its own game-path list or title cache), plus any folders the user adds. From those it learns each installed game's key, path and real name. `play` then works for any installed game with no manual config, and every output shows names instead of title ids.

**Branch:** `feat/game-catalog`, from the previous branch in the chain.

**Spec:** [emulator-profiles](../../specs/emulator-profiles.md), [sync-model](../../specs/sync-model.md) §Launcher mode, [config-and-cli](../../specs/config-and-cli.md) §Commands, [product](../../specs/product.md) (non-goal: syncing ROMs; this plan only reads metadata and never copies ROMs). P3: the emulator-specific knowledge lives in profile data (TOML). Core has no Cemu code.

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

### Task 1: Spike SP10: how emulators list games and their names (W3)

**Why:** there's no evidence yet for where Cemu keeps its game folders and names. Today `GamePaths` appears nowhere in the repo.

**Files:** `docs/specs/emulator-profiles.md` (new §Game discovery), `docs/progress/roadmap.md` (spike list).

- [x] **Step 1: Cemu, from source.** In `cemu-project/Cemu` at the v2.6 tag, find:
  - how settings.xml stores game folders (element path),
  - whether a title list cache file exists (name, location per platform, fields: title id, name, path, format),
  - where a game's display name comes from (`meta/meta.xml` `longname_*` for unpacked games; what happens for `.wua`/`.wud`/`.wux`),
  - which language field Cemu picks.

  Cite file and line at the tag.
- [x] **Step 2: Cemu, on this machine, read-only.** In Daniel's Flatpak (`~/.var/app/info.cemu.Cemu`), confirm the element and file names from Step 1. Record only the structure (element and attribute names). Never record his paths, game names or ids in the repo.
- [x] **Step 3:** write §Game discovery with the evidence. Anything unproven (Windows/macOS locations) goes under Verify `V-GAME-n` (SP10). Add SP10 to the roadmap's spike list (it still says "SP1–SP7"; fix it to list through SP10). Commit `docs(spike): SP10 how emulators list games`.

### Task 2: Profile `[games]` section and its reader

**Files:** `crates/core/src/profile.rs` (pure schema), new `crates/engine/src/games.rs` (I/O: reading emulator files, scanning folders), `profiles/cemu.toml`, synthetic fixtures under `profiles/fixtures/cemu/games/`, spec `emulator-profiles.md`.

**Interfaces (the shape SP10 proved; the earlier sketch also had a `cache` part, dropped because no title list cache exists on the Linux Flatpak, V-GAME-2):**
```toml
[games]
dirs = { file = "settings.xml", key = "content/GamePaths/Entry" }   # folders the emulator knows
scan = { marker = "meta/meta.xml", key = "menu/title_id", key_last = 8, name = "menu/longname_en", launch_dir = "code", launch_ext = "rpx" }
```
Reuse the XML reader the profile `config_keys` already use for settings.xml. The section is additive and optional, so profiles without it still load.

- [x] **Step 1: failing tests** on synthetic fixtures. First check which XML crate the existing reader uses and whether it can expand DTDs. Then:
  - dirs listed by the emulator are found, and each scanned game maps to (key, name, path);
  - a scan goes exactly one folder deep, stops at 5,000 entries, and never follows a symlink out of the dir;
  - hostile input is refused or handled: an oversized file, a `DOCTYPE` (rejected; plus entity expansion if the crate could do it), a `..` path, control characters, and a 1 MB name;
  - names are trimmed and capped at 200 characters.
- [x] **Step 2: implement**, then specs, `just check`, and commit `feat(profiles): describe how an emulator lists its games`.

### Task 3: Catalog, user game folders, and `play` without manual paths

**Files:** `crates/engine/src/games.rs`, `crates/engine/src/launch.rs`, `crates/engine/src/local_config.rs` (`[emulators.<id>] game_dirs: Vec<PathBuf>`, serde default), the `config` command allowlist (add `emulators.<id>.game_dirs`), specs `sync-model.md` §Launcher mode and `config-and-cli.md`.

**Interfaces:** `Engine::games() -> Vec<GameEntry { emulator, key, name: Option<String>, path: Option<PathBuf>, source: Config | Emulator | Scan }>`. A game found by several sources merges into one entry. Path precedence: the user's `games.<key>` override, then the emulator's own game folders (`Emulator`), then the user's `game_dirs` (`Scan`); SP10 found no cache to read. Names are computed on each device at runtime and are not stored in the repo (P1; no new remote format). Log that as a new decision in `decisions.md`. The spec says, as expected behaviour, that a device without the game shows bare keys. Sync itself is driven by save folders and doesn't need the catalog. LemuSync never creates ROM or game folders. A user "adds" a folder only by listing it in `game_dirs`.

- [x] **Step 1: failing tests.**
  - With no `games` override, `launch` uses the detected path.
  - The override still wins.
  - A user `game_dirs` folder is scanned with the profile's rule.
  - A game missing from every source gives a localized error. It suggests `lemusync games` and `lemusync config set emulators.<id>.games.<key> <path>`.
- [x] **Step 2: implement**, then specs, `just check`, and commit `feat(engine): find installed games without manual paths`.

### Task 4: `lemusync games` and names everywhere

**Files:** `crates/cli/src/main.rs` plus a new `crates/cli/src/games_cmd.rs`, the outputs of `status`, `history`, `conflicts` and `play`, `locales/{en,es}/lemusync.ftl`, `crates/cli/tests/cli.rs`, spec `config-and-cli.md` §Commands. Also add a "Games" mention in `docs/specs/ui/navigation.md` only if a games screen is already planned there; otherwise note the future UI parity under the spec's open questions.

- [x] **Step 1: failing CLI tests.** `lemusync games [--json]` lists emulator, key, name (or the key when no name is known), path and source. It's local and offline: no store access. `status`, `history` and `conflicts` show "Name (key)" when a name is known. `play` prints "Starting <name>…". A game found only by detection (no config entry) shows its name in `status`.
- [x] **Step 2: implement**, then specs. Last task: CHANGELOG, set the board row, `just check`, commit `feat(cli): games command and game names in every output`.
