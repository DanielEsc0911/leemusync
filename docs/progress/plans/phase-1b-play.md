# Phase 1, play (M1b): leases, launcher mode, conflicts, crash safety

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Also follow the project skills: `leemusync-feature` (always), `leemusync-security` (Tasks 2–7), `leemusync-backend` (Tasks 1, 8), `leemusync-emulator-profile` (Tasks 4, 6, 9), `leemusync-docs` (every task).

**Goal:** Reach milestone M1b ([roadmap](../roadmap.md#phase-1-sync-engine--cli-desktop), [D29](../../decisions.md#d29-m1-is-split-first-both-saves-on-both-pcs-then-leases-and-play-2026-10-18)) on the Fedora PC and the CachyOS laptop through the M1a R2 repo: two players play BotW at once (SC1), one player hands off between PCs (SC2), a forced double-open becomes a conflict that's resolved with nothing lost (SC3), `kill -9` and network cuts at every journal step leave no corruption (SC5), and the budgets are measured.

**Architecture:** Builds on the M1a code and its final v1 formats. Leases are new encrypted objects under `locks/` ([sync-model §Leases](../../specs/sync-model.md#leases)); pure lease rules live in `core`, I/O in `engine/src/lease.rs`. `leemusync play` is launcher mode ([sync-model §Launcher mode](../../specs/sync-model.md#launcher-mode-primary)) in `engine/src/play.rs`. Conflict resolution writes a resolution version with the existing upload path. Cemu account creation (option c, [D32](../../decisions.md#d32-new-cemu-accounts-are-generated-like-cemu-does-never-copied-2026-10-20)) is profile data plus generic primitives in `core`. CLI only: the daemon and UI stay in Phase 2 and 3 (P1).

**Tech Stack:** as the [Phase 1 first-sync plan](phase-1-first-sync.md) (Rust 1.99.0, tokio, OpenDAL 0.59.5, SQLite, clap, Fluent). New dev-dependency only: `criterion` (Task 8), version resolved and `cargo deny`-checked in that task.

**Spec:** [sync-model](../../specs/sync-model.md) §Leases, §Play sessions, §Conflicts; [reliability](../../specs/reliability.md) §Restore, §Upload, §Budgets; [players-and-accounts](../../specs/players-and-accounts.md#creating-a-cemu-account-sp8) §Creating a Cemu account; [config-and-cli](../../specs/config-and-cli.md) §Commands, §Exit codes; [testing](../../specs/testing.md) §Layers. Read only the sections a task names.

## Global constraints

- Everything in the first-sync plan's [Global constraints](phase-1-first-sync.md#global-constraints) still holds (toolchain, clippy pedantic as errors, layers, `core` has no I/O/clock/randomness, crypto, TLS, I1–I6, secrets, no real saves in tests, P6 en + es, W1, W2).
- Formats stay v1. New objects (leases) are v1 from birth; existing manifests and config versions gain only `#[serde(default)]` fields, so `API_VERSION` stays 1 unless a task says otherwise.
- Every new user-facing string goes into `locales/{en,es}/leemusync.ftl`; `cargo xtask i18n-check` passes.
- Leases never weaken safety: any lease bug must end as a conflict, never a lost save (D6).
- The emulator is launched with argv, never a shell. The executable is chosen by the user (config), never by a profile.
- W5: Conventional Commits; the maintainer is the author; the only trailer is `Co-Authored-By: Claude <noreply@anthropic.com>`. No session links.
- Each task ends with `just check` green, then its commit.

---

### Task 1: `store` reports modified time; spike SP9 on R2 lease liveness

**Why:** lease liveness uses the backend's server-side modified time ([sync-model §Leases](../../specs/sync-model.md#leases)), but `store::Entry` has only `path` and `size`. V-SYNC-1 is open for cloud providers, R2 included.

**Files:** `crates/store/src/lib.rs` (`Entry.modified`), `crates/store/src/{s3,gdrive}.rs`, `crates/store/tests/conformance.rs`, specs `storage-backends.md` §Interface, `sync-model.md` §Verify.

**Interfaces:** Produces `pub struct Entry { pub path: String, pub size: u64, pub modified: Option<std::time::SystemTime> }`, filled by `list` and `stat` when the backend reports it (`None` on Drive `list`, V-SYNC-2).

- [x] **Step 1: failing test.** Conformance: `put` (Overwrite) `locks/a/b`, `stat` and `list` it, assert `modified.is_some()` on `fs`; overwrite after 1.1 s, assert `modified` advanced.
- [x] **Step 2:** run `cargo test -p leemusync-store --test conformance`, expect a compile failure (no field).
- [x] **Step 3: implement** from OpenDAL `Metadata::last_modified()` (convert to `SystemTime`); for `list`, request the metadata the service returns with the listing and don't add a `stat` per entry.
- [x] **Step 4:** tests pass; `cargo clippy -p leemusync-store --all-targets -- -D warnings`.
- [ ] **Step 5: spike SP9 (live, maintainer runs the command).** `cargo test -p leemusync-store --test s3_conformance -- --ignored lease_mtime` against R2: overwrite advances `LastModified` in `list` and `stat`; record the offset between R2's time and the local clock (`date -u` vs the returned time) on both PCs. Record the result under V-SYNC-1. Until it's recorded, Task 2 uses the device-clock fallback (2 min skew tolerance).
- [x] **Step 6:** specs (W1), `just check`, commit `feat(store): report object modified time`.

### Task 2: leases in `core` and `engine`

**Files:** `crates/core/src/lease.rs` (new), `crates/core/src/lib.rs`, `crates/engine/src/lease.rs` (new), `crates/engine/src/repo.rs` (AAD `lease`, path `locks/<streamId>/<deviceId>`), `crates/engine/src/lib.rs`, `crates/engine/src/failpoints.rs` (new names), spec `sync-model.md` §Leases (**Code:**).

**Interfaces:**
```rust
// core::lease — pure, no clock: callers pass `now`.
pub struct Lease { pub format: u32, pub device: DeviceId, pub player: PlayerId,
    pub session: SessionId, pub acquired_at_ms: i64, pub renewed_at_ms: i64, pub ttl_ms: i64 }
pub struct Observed { pub lease: Lease, pub server_modified_ms: Option<i64> }
pub enum Liveness { Live, Stale }
pub fn liveness(o: &Observed, now_ms: i64, skew_ms: i64) -> Liveness; // server time if Some, else renewed_at + ttl + skew
pub fn winner(live: &[Observed]) -> Option<DeviceId>;                 // earliest (acquired_at, device id)
// engine::lease
pub enum Acquire { Ours(LeaseHandle), HeldElsewhere { device: DeviceId, device_name: String, renewed_at_ms: i64, stale: bool } }
impl Engine {
    pub async fn lease_acquire(&self, stream: StreamId, player: PlayerId, takeover: bool) -> Result<Acquire, EngineError>;
    pub async fn lease_renew(&self, h: &LeaseHandle) -> Result<(), EngineError>;
    pub async fn lease_release(&self, h: LeaseHandle) -> Result<(), EngineError>;
    pub async fn lease_status(&self) -> Result<Vec<api::LeaseInfo>, EngineError>;
    pub async fn lease_release_forced(&self, stream: StreamId) -> Result<(), EngineError>; // only our own device's lease
}
```
`SessionId` is a new random id in `core::id`. Defaults: renew 60 s, TTL 5 min, skew 2 min (`config.toml` `[leases] ttl_secs`).

- [x] **Step 1: core tests (proptest where cheap).** Live within TTL; stale after TTL + skew with no server time; server time wins over `renewed_at`; `winner` picks the earliest acquire, ties by device id; encode/decode round-trip; decode rejects unknown `format`.
- [x] **Step 2:** run, expect failures; implement `core::lease`; pass.
- [x] **Step 3: engine tests** (two `Engine`s on one `Store::fs`, `crates/engine/tests/leases.rs`): A acquires → B gets `HeldElsewhere`; both write within the same instant → after the second list exactly one keeps it and the loser deletes its own file; A's lease past TTL → B gets `stale: true`, `takeover: true` acquires and the next version's `meta.takeover` names A's device; release deletes only our file; a lease object that doesn't decrypt is ignored with a warning, never trusted (B2).
- [x] **Step 4:** implement `engine::lease` with `PutMode::Overwrite` for our own lease only. Add `meta.takeover: Option<DeviceId>` to the manifest meta (`#[serde(default)]`).
- [x] **Step 5:** pass, clippy, spec **Code:** line, `just check`, commit `feat(engine): add advisory per-stream leases`.

### Task 3: conflicts listed and resolved

**Files:** `crates/core/src/reconcile.rs` (resolution parents), `crates/engine/src/resolve.rs` (new), `crates/engine/src/query.rs`, `crates/engine/src/local_config.rs` (`conflict_policy`), `crates/api/src/lib.rs` (`ConflictInfo`, `HeadInfo`), `crates/engine/tests/sync.rs`, spec `sync-model.md` §Conflicts.

**Interfaces:**
```rust
pub struct HeadInfo { pub version: VersionId, pub device_name: String, pub player: String,
    pub created_at_ms: i64, pub files: u32, pub bytes: u64, pub changed_files: u32 }
pub struct ConflictInfo { pub player: String, pub emulator: String, pub game: String, pub heads: Vec<HeadInfo> }
impl Engine {
    pub async fn conflicts(&self) -> Result<Vec<ConflictInfo>, EngineError>;
    /// Writes a resolution version: the chosen head's files, parents = all heads; then restores it with backup.
    pub async fn resolve(&mut self, player: &str, game: &str, keep: VersionId) -> Result<StreamReport, EngineError>;
}
```
Session length and emulator version from the spec's list are left out: M1a manifests don't record them (P1; added when a manifest field exists).

- [x] **Step 1: failing tests.** Two devices diverge (both dirty from one base) → `conflicts()` lists one stream with 2 heads; `resolve(keep = A's head)` on B → one head whose parents are both heads, B's local files equal A's, B's old files are in local history (I1), B's head still restorable with `restore`; `conflict_policy = "newest"` makes `sync` write the resolution for the newest head and report it; resolving with a version that isn't a head → `EngineError::Usage`.
- [x] **Step 2:** implement. The resolution manifest reuses the chosen head's blob ids (no re-upload) and goes through `upload::commit_manifest` (create-only, same failpoints).
- [x] **Step 3:** pass, clippy, spec, `just check`, commit `feat(engine): list and resolve conflicts`.

### Task 4: `launch` in the Cemu profile and per-device emulator executable

**Files:** `crates/core/src/profile.rs` (`launch.args` templates, already in the [schema](../../specs/emulator-profiles.md)), `profiles/cemu.toml`, `crates/engine/src/local_config.rs` (`[emulators.cemu] exe = […]`, `games."<title id>" = "<path>"`), spec `emulator-profiles.md`, `config-and-cli.md` §Config file.

The executable is a user-given argv prefix, so Flatpak works as `exe = ["flatpak", "run", "info.cemu.Cemu"]` and native as `exe = ["/usr/bin/cemu"]`. Cemu's args: `launch.args = ["-g", "{game_path}"]` (Verify V-PLAY-1).

- [ ] **Step 1: failing tests.** Profile parse accepts `launch.args` with `{game_path}`, rejects unknown variables and any field naming an executable; local config round-trips `exe` and `games`; building argv refuses an empty `exe` and never joins into one string.
- [ ] **Step 2:** implement `Engine::launch_argv(emulator, game) -> Result<Vec<OsString>, EngineError>`.
- [ ] **Step 3:** pass, clippy, specs, `just check`, commit `feat(profiles): describe how to launch a game`.

### Task 5: `engine` launcher mode (`play`)

**Files:** `crates/engine/src/play.rs` (new), `crates/engine/src/process.rs` (wait for exit), `crates/engine/src/lib.rs`, `crates/api/src/lib.rs` (`PlayEvent`, `PlayReport`), `crates/engine/tests/play.rs`, spec `sync-model.md` §Launcher mode (**Code:**).

**Interfaces:**
```rust
pub enum LeaseChoice { Wait, PlayAnyway, TakeOver }
pub trait PlayPrompt: Send + Sync {            // CLI answers it; tests script it
    fn lease_held(&self, by: &str, renewed_ago_s: u64, stale: bool) -> LeaseChoice;
    fn conflict(&self, c: &api::ConflictInfo) -> Option<VersionId>; // None = play this device's copy
}
pub trait Launcher: Send + Sync { fn spawn(&self, argv: &[OsString]) -> std::io::Result<Box<dyn Child>>; }
pub trait Child: Send { fn wait(&mut self) -> std::io::Result<i32>; }
impl Engine {
    pub async fn play(&mut self, player: &str, emulator: &str, game: &str,
        extra: &[OsString], prompt: &dyn PlayPrompt, launcher: &dyn Launcher) -> Result<PlayReport, EngineError>;
}
```
Steps exactly as the spec: resolve stream → sync that stream (fast-forward with backup; conflict → `prompt.conflict`) → acquire (held → prompt; `Wait` polls every 10 s) → launch → renew every 60 s on a tokio interval while waiting on the child in `spawn_blocking` → on exit wait the quiet period, snapshot, upload → release. Release also runs on error paths. A renewal failure is logged and retried, never kills the game. If the Cemu process is already running before launch, refuse (I3).

- [ ] **Step 1: failing tests** with `FakeLauncher` (writes save files then exits) and a scripted prompt: SC2 handoff (A plays, B plays, B starts from A's version); lease held → `Wait` then A releases → B launches; `PlayAnyway` on both → divergent heads → conflict reported, both versions on the remote (SC3); renewals happen (`tokio::time::pause` + advance 3 min → `renewed_at` moved); a launcher error releases the lease; exit with no changes uploads nothing.
- [ ] **Step 2:** implement; pass; clippy.
- [ ] **Step 3:** spec, `just check`, commit `feat(engine): add launcher mode`.

### Task 6: `accounts.create` profile section and account creation

**Files:** `crates/core/src/account_template.rs` (new: primitives), `crates/core/src/profile.rs` (`accounts.create`), `profiles/cemu.toml`, `crates/engine/src/accounts.rs` (`create_account`), `crates/engine/src/fsx.rs` (reuse `write_atomic`), spec `players-and-accounts.md` §Creating a Cemu account (field names settled), `emulator-profiles.md` (schema table).

**Interfaces (field names settled here, P3):**
```toml
[accounts.create]
root = "{mlc}/usr/save/system/act"
id = { format = "hex8", min = "80000001", counter_file = "persisid.dat", counter_key = "PersistentIdHead" }
max_accounts = 12
name_max_utf16 = 10
header = "AccountInstance_20120705"
file = "account.dat"
[[accounts.create.keys]]   # in file order; one table per key
key = "PersistentId"; value = "{id}"
# Uuid = "{random:16}"; TransferableIdBase = "2000004{slice:Uuid:24:32}";
# MiiData = { bytes = "<192 hex default Mii>", put_utf16le = { at = 0x1A, units = 10, from = "{name}" },
#             crc16 = { poly = 0x1021, vary = [4, 8], target = 0 } };
# MiiName = "{name}"; the rest are literals from the spec table.
```
```rust
// core: pure; caller passes random bytes and the existing ids.
pub fn next_id(rule: &IdRule, counter: Option<u32>, existing: &[AccountId]) -> Result<AccountId, CoreError>;
pub fn render(t: &CreateTemplate, id: AccountId, name: &str, random: &[u8]) -> Result<String, CoreError>;
// engine
impl Engine { pub async fn create_account(&mut self, player: &str, emulator: &str, name: Option<&str>)
    -> Result<api::CreatedAccount, EngineError>; }
```

- [ ] **Step 1: golden test.** Add the SP8 evidence as fixture `profiles/fixtures/cemu/account-create/` (the `SP8 Test` and `sp8cemu` files, non-secret: online fields empty): `render` with that file's id, name and `Uuid` bytes reproduces it byte for byte; `MiiData` CRC-16 is 0; same name → same `MiiData`.
- [ ] **Step 2: rule tests.** `next_id` = max(counter + 1, highest + 1) and ≥ `80000001`; 12 accounts → error; name longer than 10 UTF-16 units is cut, not rejected; unknown template function → profile validation error.
- [ ] **Step 3: engine tests.** Refuses while the emulator runs (`FakeProbe`) and when the folder exists; writes only `act/<id>/account.dat` (tree diff before/after: one new file); then maps the account to the player and restores the player's latest version (backup-first); the file stays inside the grant.
- [ ] **Step 4:** implement; pass; clippy; specs (W1; replace "Field names are settled in that plan"), `just check`, commit `feat(accounts): create Cemu accounts from profile data`.

### Task 7: CLI: `play`, `conflicts`, `resolve`, `lease`, `accounts create`

**Files:** `crates/cli/src/main.rs`, `crates/cli/src/play.rs` (new: `PlayPrompt` over the terminal, `ProcessLauncher` with `std::process::Command`), `crates/cli/src/output.rs`, `locales/{en,es}/leemusync.ftl`, `crates/cli/tests/`, spec `config-and-cli.md` §Commands (update the "first slice" line), §Exit codes.

Commands as the spec table: `play <emulator> <game> [--as <player>] [-- <args>]`, `conflicts`, `resolve <game> <version> [--as <player>]`, `lease status`, `lease release --force <game>`, `accounts create <player> <emulator> [--name <mii name>]` with the spec's two confirmation messages. Exit codes: 4 when a conflict needs a decision and there's no terminal; 5 when the lease is held and there's no terminal (no silent `PlayAnyway`). `--json` everywhere; prompts never shown with `--json` (fail with 4/5 instead).

- [ ] **Step 1: failing tests** (`assert_cmd`, `LEEMUSYNC_TEST_ROOT`): `conflicts --json` lists heads; `resolve` with a bad version → exit 2; `play` with a held lease and no TTY → exit 5; `accounts create` without a TTY and without `--yes` → exit 2; en and es output for each new message.
- [ ] **Step 2:** implement; pass; `cargo xtask i18n-check`; spec; `just check`; commit `feat(cli): add play, conflicts, resolve, lease and accounts create`.

### Task 8: SC5 fault matrix and budget measurements

**Files:** `crates/engine/src/failpoints.rs` (a `Kill` mode that calls `std::process::abort()`), `crates/store/src/lib.rs` (test-only `Store::faulty(inner, plan)` behind feature `faults`: fail the Nth request with a transient or network error), `crates/engine/tests/faults.rs`, `crates/engine/tests/kill.rs` (new: spawns a helper binary), `crates/engine/benches/budgets.rs` (new, criterion), `justfile` (`just bench`), specs `reliability.md` §Budgets (measured column), `testing.md` §Layers.

- [ ] **Step 1: crash matrix.** For every failpoint name (`grep -rn 'hit("' crates/engine/src`, currently `restore.after_backup`, `restore.mid_swap`, `restore.before_done`, `upload.before_manifest`, `upload.after_manifest`, `init.*`, plus Task 2's `lease.*` and Task 3's `resolve.*`): a child process arms it in `Kill` mode and dies; the parent reopens the engine and asserts I1–I6 (local files = old or new version, never mixed; no extra heads; backup present). Add a test that fails when a new failpoint name has no case.
- [ ] **Step 2: network matrix.** With `Store::faulty`, fail request N for every N in one sync (upload and fast-forward) and in one `play`; assert the sync errors or completes, a rerun converges, and no corruption or false conflict appears.
- [ ] **Step 3: budgets.** Bench: cold start to ready (`Engine::open` on a populated state DB), rescan of 10 000 files warm, idle RSS of an open engine (read `/proc/self/statm`), release binary size of `leemusync` (no daemon yet: these are the engine-side numbers the daemon will inherit). Record the numbers with date and machine in reliability §Budgets and update V-REL-3. Not enforced in CI yet.
- [ ] **Step 4:** `just check`, commit `test(engine): crash and network fault matrix, budget benches`.

### Task 9: Test day M1b on the real PCs (manual runbook)

Maintainer, Fedora + CachyOS, R2 repo from M1a, release binaries from this branch on both. Record surprises in the roadmap and the specs (W1).

- [ ] **B0: Set up.** On each PC: `leemusync` config `[emulators.cemu] exe` (Flatpak on Fedora, `/usr/bin/cemu` on CachyOS) and `games.101c9400 = "<BotW path>"`. `leemusync sync`, `leemusync doctor`: all ok.
- [ ] **B1: Account creation.** On the PC missing a player's account: `leemusync accounts create "<player>" cemu`, confirm, open Cemu, select the account, check BotW shows that player's save. Delete the hand-made M1a accounts only if the maintainer wants to.
- [ ] **B2: SC1.** Each player runs `leemusync play cemu 101c9400 --as "<player>"` on a different PC at the same time, plays 10 min, quits. Both syncs upload; `leemusync conflicts` is empty; `lease status` empty.
- [ ] **B3: SC2.** Player 1 plays on Fedora, quits, then `play` on CachyOS: the game starts with the progress just made.
- [ ] **B4: SC3.** Same player starts `play` on both PCs; on the second choose "play anyway"; save on both; quit. `leemusync conflicts` shows 2 heads; `resolve` one; the other is still in `history` and `restore` brings it back.
- [ ] **B5: SC5 spot check.** During a `play` upload, pull the network cable, then `kill -9` a sync mid-restore; rerun `leemusync sync`: no corruption, BotW loads.
- [ ] **B6: Record.** Mark M1b reached in the roadmap with date and surprises; tick this plan; commit `docs(progress): record the M1b test day`.

## Verify

- **V-SYNC-1** (existing, SP6 → SP9 for R2) R2 `LastModified` in `list`/`stat` advances on overwrite, and its skew against both PCs' clocks (Task 1 Step 5).
- **V-PLAY-1** (new, spike SP9) Cemu 2.6 starts a game with `-g <path>` on Flatpak (`flatpak run info.cemu.Cemu -g …`) and `cemu-bin`, the process LeemuSync waits for is the emulator (the Flatpak wrapper exits only when Cemu does), and the active account at launch is the one in `settings.xml` (Cemu has no documented command-line account switch). Checked at the start of Task 9 B0; if the wrapper exits early, `play` waits on the profile's process names instead.

## Self-review

- **Coverage:** SC1 (Tasks 2, 5, 9 B2), SC2 (5, 9 B3), SC3 (3, 5, 9 B4), SC5 (8, 9 B5), budgets (8), leases and `lease` CLI (2, 7), `conflict_policy` (3), option c / D32 (6, 7, 9 B1). Not planned (P1): watch mode, the daemon, sleep/resume, UI, GC, retention pruning; they're Phase 2/3.
- **Open for the maintainer:** (1) "Daemon budgets measured" while `leemusyncd` is a stub: Task 8 measures engine and CLI numbers only. (2) Cemu's active account isn't switched by LeemuSync (spec quirk), so `play --as` can't guarantee the save lands in that player's account; Task 9 assumes one player per PC is already active. (3) Session length and emulator version in conflict details aren't recorded by M1a manifests; Task 3 leaves them out.
- **Names:** `Lease`, `LeaseHandle`, `Acquire`, `LeaseChoice`, `PlayPrompt`, `Launcher`, `ConflictInfo`, `HeadInfo`, `create_account`, `Entry.modified` are used consistently across Tasks 1–8.
