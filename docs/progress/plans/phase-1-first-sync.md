# Phase 1, first sync (M1a): both BotW saves on both PCs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Also follow the project skills: `leemusync-feature` (always), `leemusync-security` (Tasks 6–8, 10–16), `leemusync-backend` (Tasks 7–8), `leemusync-emulator-profile` (Tasks 5, 12, 18), `leemusync-docs` (every task).

**Goal:** Two Linux PCs (Fedora 44 + Cemu Flatpak, CachyOS + native `cemu-bin`), each with one Cemu account holding a different player's BotW save, end up with both saves on both PCs through an end-to-end encrypted repo on the maintainer's Google Drive, driven by the `leemusync` CLI.

**Architecture:** Thin but final slices of the layered crates (B1): pure logic in `core` (ids, paths, manifest, heads, reconciliation, repo config, profile schema), primitives in `crypto`, raw encrypted objects in `store` (OpenDAL `gdrive` + `fs`), orchestration in `engine` (discovery, snapshot, upload, journaled restore, SQLite state, sync), report types in `api`, and the `leemusync` binary in `cli`. Formats are v1 and final, so the next slice (M1b: leases, `play`, conflict resolution) builds on the data this test creates.

**Tech Stack:** Rust 1.99.0 (edition 2024), tokio, OpenDAL 0.59.5, reqwest 0.13 + rustls (ring) + rustls-platform-verifier, XChaCha20-Poly1305, Argon2id, BLAKE3, SQLite (rusqlite, bundled), keyring (Secret Service via vendored libdbus, Keychain, Credential Manager), clap, Fluent.

**Spec:** this plan implements the design approved on 2026-10-18 (brainstorm with the maintainer). The design lives in the specs it touches, updated by Task 1 before any code: [sync-model](../../specs/sync-model.md), [players-and-accounts](../../specs/players-and-accounts.md), [reliability](../../specs/reliability.md), [security](../../specs/security.md), [storage-backends](../../specs/storage-backends.md), [config-and-cli](../../specs/config-and-cli.md), [emulator-profiles](../../specs/emulator-profiles.md), [decisions](../../decisions.md) D26–D30. Read the spec sections a task names before starting it. This plan is part of Phase 0 Task 6's "write the Phase 1 plan" item; the rest of Phase 1 gets its own plan after this test.

## Global constraints

- Toolchain `1.99.0` from `rust-toolchain.toml`; `edition = "2024"`; every crate keeps `[lints] workspace = true`.
- `just lint` runs `cargo clippy --workspace --all-targets -- -D warnings`, so clippy **pedantic warnings are errors**. Follow the repo's style: `#[must_use]` on public functions that return a value and have no side effects, a `/// # Errors` doc section on public functions that return `Result`, no `unwrap()`/`expect()` outside tests (`clippy.toml` allows them in tests). After every implementation step run `cargo clippy -p <crate> --all-targets -- -D warnings` and fix what it reports before moving on.
- Layers (`cargo xtask layers`, `xtask/src/layers.rs`): `core`, `crypto`, `store`, `api` depend on no internal crate. `engine` → `core`, `crypto`, `store`, `api`. `cli` → `api`, `engine`, `ipc`. Dev-dependencies may cross layers.
- `core` and `crypto`: no I/O, no async, no clock, no randomness inside `core` (callers pass time and random bytes in).
- Crypto exactly as [security §Cryptography](../../specs/security.md#cryptography): XChaCha20-Poly1305 with a random 192-bit nonce per object, Argon2id `m = 65536 KiB, t = 3, p = 1` by default, BLAKE3 `derive_key` contexts `"leemusync v1 object"`, `"leemusync v1 blob-id"`, `"leemusync v1 stream-id"`, OS CSPRNG, `zeroize` on drop. AAD binds kind and id.
- TLS: `rustls` 0.23.46 with the `ring` provider and `rustls-platform-verifier` 0.7.1. Both `cargo tree -i aws-lc-sys -e normal` and `cargo tree -i openssl-sys -e normal` must print `did not match any packages`.
- Reliability invariants I1–I6 ([reliability §Invariants](../../specs/reliability.md#invariants)) hold for every code path that writes save files.
- Secrets (passphrase, master key, recovery key, OAuth refresh/access tokens, client secret) never appear in `config.toml`, logs, stdout/stderr, `--json`, or error text. Every `store` error message passes through `redact()`.
- Real user saves are never used in tests or committed. Tests build fake trees in temp dirs or use `profiles/fixtures/`.
- P6: every human-readable CLI string exists in `locales/en/leemusync.ftl` and `locales/es/leemusync.ftl` (repo root, as [i18n §Sources per layer](../../specs/i18n.md#sources-per-layer) says) (neutral Latin American Spanish, [i18n §Terminology](../../specs/i18n.md#terminology)). `--json` keys are English and never translated.
- W1: each task updates the spec lines it makes true in the same commit (Status, **Code:** fields, Verify lines).
- W2: test first. `just check` must be green before every commit.
- W5: Conventional Commits; the maintainer is the author; the trailer is `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Exact dependency versions (resolved and license-checked with `cargo deny` against `deny.toml` on 2026-10-18; add them to `[workspace.dependencies]` in the root `Cargo.toml` in the task that first needs them, and reference them with `{ workspace = true }`):

| Crate | Version / features | First used in |
|---|---|---|
| `serde` | `1.0.229`, `derive` | Task 2 |
| `serde_json` | `1.0.154` | Task 3 |
| `thiserror` | `2.0.21` | Task 2 |
| `toml` | `1.1.8` | Task 5 |
| `chacha20poly1305` | `0.10.1` | Task 6 |
| `argon2` | `0.5.3`, `default-features = false`, `alloc`, `zeroize` | Task 6 |
| `blake3` | `1.8.7` | Task 6 |
| `zeroize` | `1.9.1`, `derive` | Task 6 |
| `getrandom` | `0.4.3` | Task 6 |
| `tokio` | `1.53.3` | Task 7 (features per crate) |
| `opendal` | `0.59.5`, `default-features = false`, `services-gdrive`, `services-fs`, `executors-tokio` | Task 7 |
| `opendal-http-transport-reqwest` | `0.59.5`, `default-features = false`, `rustls-no-provider` | Task 8 |
| `reqwest` | `0.13.5`, `default-features = false`, `rustls-no-provider`, `form`, `json` | Task 8 |
| `rustls` | `0.23.46`, `default-features = false`, `ring`, `std`, `tls12` | Task 8 |
| `rustls-platform-verifier` | `0.7.1` | Task 8 |
| `url` | `2.5.8` | Task 8 |
| `base64` | `0.22.1` | Task 8 |
| `sha2` | `0.10.9` | Task 8 |
| `keyring` | `3.6.3`, `default-features = false`; per OS in the crate's target tables: Linux `sync-secret-service`, `crypto-rust`, `vendored` (libdbus built from source, no system package); macOS `apple-native`; Windows `windows-native` | Task 8 (dev), Task 10 |
| `rusqlite` | `0.37.0`, `bundled` | Task 10 |
| `directories` | `6.0.0` | Task 10 |
| `regex` | `1.13.1`, `default-features = false`, `std`, `unicode-perl` | Task 12 |
| `roxmltree` | `0.21.1` | Task 12 |
| `sysinfo` | `0.37.2`, `default-features = false`, `system` | Task 12 |
| `clap` | `4.6.7`, `derive` | Task 16 |
| `rpassword` | `7.5.4` | Task 16 |
| `fluent-bundle` | `0.16.0` | Task 16 |
| `unic-langid` | `0.9.6` | Task 16 |
| `tempfile` | `3.27.0` (dev) | Task 7 |

Do **not** enable OpenDAL's `reqwest-rustls-no-provider-tls` feature: in 0.59.5 it pulls in `http-transport-reqwest-rustls`, which enables `aws-lc-rs`. Depend on `opendal-http-transport-reqwest` directly instead (Task 8).

## File structure

```
Cargo.toml                                   + [workspace.dependencies]
crates/core/src/
  lib.rs                                     module list + re-exports
  id.rs                                      VersionId (ULID-style), Hash32, BlobId, StreamId, PlayerId, DeviceId
  path.rs                                    RelPath: validated relative path
  manifest.rs                                Version manifest v1 + validation
  heads.rs                                   Graph, heads, descends_from
  reconcile.rs                               Local, Action, reconcile()
  config.rs                                  RepoConfig, ConfigVersion, merge
  profile.rs                                 Profile (subset), template expansion, pattern matching
  account.rs                                 key=value account files, MiiName decoding
crates/crypto/src/
  lib.rs                                     Key, seal/open, derive, keyed_hash, KDF, wrap, RecoveryKey
crates/store/src/
  lib.rs                                     Store, Entry, PutMode, Capabilities
  error.rs                                   StoreError, ErrorClass, classify()
  redact.rs                                  redact()
  http.rs                                    http_client(): reqwest + rustls ring + platform verifier
  oauth.rs                                   Google OAuth: login (loopback + PKCE), refresh
  gdrive.rs                                  GdriveSession: access-token operator, rebuild before expiry
crates/store/tests/conformance.rs            one suite, fs always, gdrive #[ignore]
crates/store/examples/gdrive_login.rs        stores a test refresh token in the keystore (never prints it)
crates/api/src/lib.rs                        report types for --json (versioned)
crates/engine/src/
  lib.rs                                     Engine, EngineEnv, module list
  error.rs                                   EngineError + exit code kind
  locations.rs                               config/data dirs, home, xdg dirs
  local_config.rs                            config.toml v1
  secrets.rs                                 SecretStore trait, KeyringSecrets, MemorySecrets
  state.rs                                   SQLite state DB + exclusive lock
  failpoints.rs                              test-only fault injection
  fsx.rs                                     atomic write, no-follow checks, copy_tree
  grants.rs                                  grant checks
  repo.rs                                    repo.json, encrypted objects, versions, devices
  repo_config.rs                             repo-config stream: load (merge), write, update
  setup.rs                                   backend login, init, join, players
  scan.rs                                    grants, scan
  profiles.rs                                bundled profiles + Cemu install resolution
  discover.rs                                accounts, save folders, orphans
  process.rs                                 ProcessProbe trait, SysinfoProbe
  snapshot.rs                                consistent snapshot into local history
  upload.rs                                  upload procedure
  restore.rs                                 journaled restore + crash recovery
  sync.rs                                    per-stream sync
  accounts.rs                                accounts map (keep/replace), recover orphaned saves
  query.rs                                   status, history, restore, doctor
crates/engine/tests/
  common/mod.rs                              shared test helpers (env, fixture install, devices)
  discovery.rs                               fixture-driven Cemu discovery
  faults.rs                                  crash at every journal step
  two_devices.rs                             the test day, simulated
crates/cli/src/
  main.rs                                    clap, runtime, exit codes
  i18n.rs                                    Fluent loader
  prompt.rs                                  interactive questions
  output.rs                                  human vs --json output
locales/{en,es}/leemusync.ftl                Rust strings (Fluent), used by the CLI
xtask/src/i18n.rs                            + Fluent parity
profiles/cemu.toml                           + config_roots, max_file_mb
```

---
### Task 1: Specs and decisions first

The design must be in the specs before code (skill `leemusync-feature`, "Before code" step 3). No code in this task.

**Files:**
- Modify: `docs/decisions.md` (append D26–D30)
- Modify: `docs/specs/sync-model.md` (§Versions, §Repo config, Verify)
- Modify: `docs/specs/players-and-accounts.md` (§Discovery)
- Modify: `docs/specs/storage-backends.md` (§Interface, §Adapters, Verify)
- Modify: `docs/specs/security.md` (§Cryptography, §Network)
- Modify: `docs/specs/config-and-cli.md` (§Config file, §Commands)
- Modify: `docs/specs/emulator-profiles.md` (§Schema)
- Modify: `docs/progress/roadmap.md` (Phase 1 exit)
- Modify: `docs/progress/README.md` (board row)

- [x] **Step 1: Append the decisions.** Add to the end of `docs/decisions.md`, word for word:

```markdown
### D26 Google Drive is the backend for the first real test (2026-10-18)
The first end-to-end test (M1a, [roadmap](progress/roadmap.md#phase-1-sync-engine--cli-desktop)) syncs through Google Drive, pulled forward from Phase 6. Both machines reach it from anywhere, it was live-tested in SP6, and the maintainer has an account. `fs` is built at the same time, for tests. S3, WebDAV and SFTP stay in Phase 1 and come after this test. The test uses the maintainer's own OAuth client (type "Desktop app", consent screen in Testing), entered at setup. Its secret is kept in the OS keystore, never in config. A public client for official builds stays open (V-STORE-2). Why: the maintainer's choice, and the least setup for two machines on different networks. Rejected for the first test: SFTP to the Fedora PC (needs `sshd` and LAN reachability), WebDAV, S3.

### D27 `store` refreshes Google tokens itself; OpenDAL gets only an access token (2026-10-18)
Answers V-STORE-7. `crates/store` runs the OAuth refresh itself (form body, its own HTTP client) and builds the OpenDAL `gdrive` operator with `access_token` only. OpenDAL then never refreshes: `opendal-service-gdrive` 0.59.5 `backend.rs` sets a given access token's expiry to `Timestamp::MAX`. The store rebuilds the operator 60 s before the token expires. So the refresh token and client secret never enter a URL. Every store error is still redacted (V-SEC-4). Rejected: patching OpenDAL first (slower; an upstream issue is still worth filing).

### D28 OAuth loopback listener during login only (2026-10-18)
The desktop OAuth flow needs a redirect target. Only during `leemusync backend add gdrive` (and `backend login`), the CLI binds `127.0.0.1` on a random port, accepts exactly one request whose `state` matches, and closes after it or after 5 minutes. This is the one exception to "no inbound listeners" ([security §Network](specs/security.md#network)). It never runs in the daemon. Rejected: pasting the code by hand (worse UX; Google removed the out-of-band flow).

### D29 M1 is split: first "both saves on both PCs", then leases and play (2026-10-18)
M1a: encrypted repo, players, account mappings, scan, `sync` with versions, heads, fast-forward and backup-first restore. Conflicts are detected and kept, not resolved. M1b: the rest of the M1 exit (leases, launcher mode, SC1–SC3, SC5). Why: the maintainer wants the first real test soon, and M1a's formats are final, so M1b builds on the same data (P1).

### D30 Repo-config heads merge automatically (2026-10-18)
Several heads of the repo-config stream are merged by union instead of being shown as a conflict. Players are kept by id. Each device writes only its own account mappings. The merge version lists all heads as parents. Why: config edits are additive and per device, so a union never loses data, and asking the player would be noise. If two devices add the same player name at once, both players are kept, the later id gets a ` (2)` suffix, and the CLI says so.
```

Each snippet below is written for the spec it goes into, so its links are relative to that spec's folder.

- [x] **Step 2: sync-model.md.**
  - §Versions, after the field list in the first bullet, add:

```markdown
`created_at` and `files[].mtime` are Unix milliseconds. `blob` is the lowercase hex of the keyed BLAKE3 hash. Serialised as JSON with unknown fields rejected; `format` is `1`.
```

  - §Repo config, append:

```markdown
Several heads are merged automatically by union ([D30](../decisions.md#d30-repo-config-heads-merge-automatically-2026-10-18)). A config version is `{format, id, parents[], device, created_at, config}` stored at `config/versions/<versionId>`. The config holds players, per-device account mappings, and each player's games (`"<emulator>/<game key>"`), so a device can find a player's streams although stream ids are opaque.
```

  - §Reconciliation, under the table, add:

```markdown
With no `base` yet: no local files and one head → fast-forward. Local files and one head whose files are identical → adopt that head as `base` (clean). Local files and one different head → upload with no parents, which makes two heads (conflict): nothing is overwritten. `replace_local` (set when the player chooses to replace an account's current files, [players-and-accounts §Discovery](players-and-accounts.md#discovery)) turns that last case into a fast-forward with backup. With a `base`: if the local files equal a head, that head becomes `base` (this also makes a crash between writing a manifest and saving `base` harmless). A `base` the remote no longer has → warn that the remote went backwards ([security §Rollback and withholding](security.md#rollback-and-withholding)), no action.
```

- [x] **Step 3: players-and-accounts.md.** Append to §Discovery:

```markdown
**A player with no account here.** When a player's stream exists in the repo but the player has no account mapped on this device, `sync` reports it and does nothing for that stream. The player chooses:
- **a. Use an existing account:** `leemusync accounts map <player> cemu <id>`. If that account already has files and the player has a version in the repo, the CLI asks whether to keep them (they're uploaded and become a second head, a conflict) or replace them (`--replace`): the current files go to local history first (I1), then the player's latest version is restored.
- **b. Create the account in the emulator first,** then `leemusync scan` and `accounts map`.
- **c. Let LeemuSync create it:** only after spike SP8 proves Cemu accepts it (V-ACC-3).

**Recovering an orphaned save:** `leemusync accounts recover cemu <id> --player <name>` uploads the orphaned folder as the first version of that player's stream. It's refused if the player already has a version of that game, so a recovered save can never replace one. The folder itself is only read.
```

  Add to §Verify:

```markdown
- **V-ACC-3** Cemu 2.6 accepts an `account.dat` written by LeemuSync from an existing account as the template (new `PersistentId` from `persisid.dat`, new `Uuid`, new `MiiName`): it lists the account and BotW saves under it (spike SP8, [Phase 1 first-sync plan](../progress/plans/phase-1-first-sync.md)).
```

- [x] **Step 4: storage-backends.md.**
  - §Interface: replace "exposes one trait, sketched here:" with "exposes one `Store` type with these operations (one adapter per OpenDAL service; a trait comes when a non-OpenDAL adapter does):". Under the table add: "`list` returns names and sizes only in Phase 1; server modified time is read with `stat` when leases need it."
  - §Adapters, Google Drive row: replace "Phase 6." with:

```markdown
Phase 1 for the first test ([D26](../decisions.md#d26-google-drive-is-the-backend-for-the-first-real-test-2026-10-18)); the public client in Phase 6.
```

  - Verify: replace V-STORE-7 with:

```markdown
- **V-STORE-7** Answered by [D27](../decisions.md#d27-store-refreshes-google-tokens-itself-opendal-gets-only-an-access-token-2026-10-18) in Phase 1. Still to do: an upstream OpenDAL issue.
```

    and end V-STORE-8 with: "Phase 1 handling: `init` refuses when `repo.json` already exists, and after writing it reads it back and compares the repo id. A mismatch means another device won the race, and `init` stops and tells the user to `join`."
- [x] **Step 5: security.md.**
  - §Cryptography, after the table, add: "`repo.json` KDF parameters are bounded on read (`m` 19 456–1 048 576 KiB, `t` 1–10, `p` 1–4), so a tampered file can neither weaken a rewrap below OWASP's minimum nor exhaust memory. The recovery key is 32 random bytes shown as Crockford base32 in groups of 4; its key-encryption key is BLAKE3 `derive_key("leemusync v1 recovery-kek", recovery key)` (it's already full-entropy, so no Argon2id)."
  - §Network, replace the first bullet with:

```markdown
- **No inbound listeners**, ever, except the OAuth loopback redirect during login ([D28](../decisions.md#d28-oauth-loopback-listener-during-login-only-2026-10-18)).
```

- [x] **Step 6: config-and-cli.md.**
  - §Config file, replace the first bullet with: "`config.toml`, versioned (`config_version = 1`), per device: device id and name, repo id, backend reference (`kind = "gdrive"` with `root` and `client_id`, or `kind = "fs"` with `path`), grants, emulator root overrides. Later phases add schedule, UI language and notification preferences."
  - §Commands: add rows "`leemusync backend add gdrive --client-id <id>` / `backend add fs <path>` / `backend login`", "`leemusync scan`" (accounts, save folders, orphaned saves, grants needed) and "`leemusync accounts recover <emulator> <id> --player <name>`". Change the `accounts map` row to "`leemusync players add/list` / `accounts map <player> <emulator> <id> [--keep\|--replace]`". After the table add:

```markdown
The Phase 1 first slice ships `init`, `join`, `backend add/login`, `grant add/list`, `players add/list`, `scan`, `accounts map/recover`, `sync [--dry-run] [--yes]`, `status`, `history`, `restore` and `doctor` ([plan](../progress/plans/phase-1-first-sync.md)).
```

- [x] **Step 7: emulator-profiles.md §Schema.** Add two rows to the schema table: "`platforms.<os>.config_roots` | [string] | Config folder for each entry of `roots`, same order (Cemu keeps `settings.xml` outside its data folder). Optional; defaults to the matching root" and "`max_file_mb` | int | Per-file size limit for snapshots (disk-fill guard). Default 64". Remove the "Schema gap (SP7)" wording wherever it appears in that spec, and point to `config_roots`.
- [x] **Step 8: roadmap.md.** In Phase 1 **Exit**, before "Two players play BotW", add:

```markdown
It's reached in two steps ([D29](../decisions.md#d29-m1-is-split-first-both-saves-on-both-pcs-then-leases-and-play-2026-10-18)): **M1a**, both players' saves on both PCs ([plan](plans/phase-1-first-sync.md)), then **M1b**, the rest below.
```

- [x] **Step 9: Board.** In `docs/progress/README.md`, set this plan's row to `In progress (Task 1 done)` with today's date.
- [x] **Step 10: Check and commit.**

Run: `cargo xtask docs-check`
Expected: `xtask docs-check: ok`

```bash
git add docs/
git commit -m "docs(sync): design the first sync slice (M1a)

Decisions D26-D30: Google Drive for the first test, store-side token
refresh, the OAuth loopback exception, the M1a/M1b split and automatic
repo-config merges. Specs gain the no-account choices, orphan recovery,
reconciliation without a base, KDF bounds and the slice's CLI.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `core` ids and validated paths

**Files:**
- Modify: `Cargo.toml` (add `[workspace.dependencies]` with `serde` and `thiserror`)
- Modify: `crates/core/Cargo.toml`
- Modify: `crates/core/src/lib.rs`
- Create: `crates/core/src/id.rs`, `crates/core/src/path.rs`

**Interfaces:**
- Produces: `VersionId::from_parts(unix_ms: u64, random: [u8; 10]) -> VersionId`, `VersionId::unix_ms(self) -> u64`, `Display`/`FromStr` (26-char Crockford base32), serde as string; `Hash32([u8; 32])` with `to_hex()`, `from_hex(&str)`, serde as hex; `BlobId(pub Hash32)`, `StreamId(pub Hash32)`; `PlayerId(pub VersionId)`, `DeviceId(pub VersionId)` (serde as the inner string); `RelPath::parse(&str) -> Result<RelPath, PathError>`, `RelPath::as_str()`, `RelPath::components()`; `IdError`, `PathError`.

- [x] **Step 1: Add the dependencies.** Root `Cargo.toml`, after `[workspace.package]`:

```toml
[workspace.dependencies]
serde = { version = "1.0.229", features = ["derive"] }
thiserror = "2.0.21"
```

`crates/core/Cargo.toml`:

```toml
[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
serde_json = "1.0.154"
```

- [x] **Step 2: Write the failing tests** in `crates/core/src/id.rs`:

```rust
//! Identifiers. Spec: docs/specs/sync-model.md (Versions, Streams).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_id_round_trips_and_sorts_by_time() {
        let a = VersionId::from_parts(1_000, [0xff; 10]);
        let b = VersionId::from_parts(1_001, [0x00; 10]);
        assert!(a < b);
        assert_eq!(a.unix_ms(), 1_000);
        let text = a.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text.parse::<VersionId>().unwrap(), a);
        assert!(b.to_string() > a.to_string(), "text order matches time order");
    }

    #[test]
    fn version_id_keeps_only_48_bits_of_time() {
        let id = VersionId::from_parts(u64::MAX, [0; 10]);
        assert_eq!(id.unix_ms(), 0xFFFF_FFFF_FFFF);
    }

    #[test]
    fn version_id_rejects_bad_text() {
        assert!("".parse::<VersionId>().is_err());
        assert!("0123456789ABCDEFGHJKMNPQRSTV".parse::<VersionId>().is_err(), "too long");
        assert!("8ZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<VersionId>().is_err(), "overflows 128 bits");
        assert!("0000000000000000000000000U".parse::<VersionId>().is_err(), "U is not Crockford");
    }

    #[test]
    fn version_id_parses_lowercase() {
        let id = VersionId::from_parts(42, [7; 10]);
        assert_eq!(id.to_string().to_lowercase().parse::<VersionId>().unwrap(), id);
    }

    #[test]
    fn hash_hex_round_trips() {
        let h = Hash32([0xab; 32]);
        assert_eq!(h.to_hex(), "ab".repeat(32));
        assert_eq!(Hash32::from_hex(&h.to_hex()).unwrap(), h);
        assert!(Hash32::from_hex("AB").is_err());
        assert!(Hash32::from_hex(&"AB".repeat(32)).is_err(), "uppercase is not canonical");
    }

    #[test]
    fn ids_serialise_as_strings() {
        let blob = BlobId(Hash32([1; 32]));
        let json = serde_json::to_string(&blob).unwrap();
        assert_eq!(json, format!("\"{}\"", "01".repeat(32)));
        assert_eq!(serde_json::from_str::<BlobId>(&json).unwrap(), blob);
        let player = PlayerId(VersionId::from_parts(5, [9; 10]));
        let json = serde_json::to_string(&player).unwrap();
        assert_eq!(serde_json::from_str::<PlayerId>(&json).unwrap(), player);
    }
}
```

- [x] **Step 3: Run them to see them fail.**

Run: `cargo test -p leemusync-core`
Expected: compile errors, `cannot find type VersionId`.

- [x] **Step 4: Implement `id.rs`** (above the tests):

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("an id must be 26 Crockford base32 characters")]
    BadVersionId,
    #[error("a hash must be 64 lowercase hex characters")]
    BadHash,
}

/// Time-ordered unique id: 48-bit Unix milliseconds + 80 random bits (ULID layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionId(u128);

impl VersionId {
    /// Builds an id from a clock reading and 10 random bytes (callers supply both: `core` has no I/O).
    #[must_use]
    pub fn from_parts(unix_ms: u64, random: [u8; 10]) -> Self {
        let time = u128::from(unix_ms & 0xFFFF_FFFF_FFFF) << 80;
        let rand = random.iter().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        Self(time | rand)
    }

    #[must_use]
    pub fn unix_ms(self) -> u64 {
        u64::try_from(self.0 >> 80).unwrap_or(u64::MAX)
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [0u8; 26];
        for (i, slot) in out.iter_mut().enumerate() {
            let shift = 125 - 5 * i;
            let index = usize::try_from((self.0 >> shift) & 31).unwrap_or(0);
            *slot = CROCKFORD[index];
        }
        f.write_str(std::str::from_utf8(&out).map_err(|_| fmt::Error)?)
    }
}

impl FromStr for VersionId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 26 {
            return Err(IdError::BadVersionId);
        }
        let mut value: u128 = 0;
        for (i, c) in s.bytes().enumerate() {
            let upper = c.to_ascii_uppercase();
            let digit = CROCKFORD
                .iter()
                .position(|d| *d == upper)
                .ok_or(IdError::BadVersionId)?;
            if i == 0 && digit > 7 {
                return Err(IdError::BadVersionId);
            }
            value = (value << 5) | u128::from(u8::try_from(digit).map_err(|_| IdError::BadVersionId)?);
        }
        Ok(Self(value))
    }
}

/// A 32-byte keyed hash, written as 64 lowercase hex characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    #[must_use]
    pub fn to_hex(&self) -> String {
        use fmt::Write as _;
        self.0.iter().fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    }

    /// # Errors
    /// `IdError::BadHash` unless `s` is exactly 64 lowercase hex characters.
    pub fn from_hex(s: &str) -> Result<Self, IdError> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return Err(IdError::BadHash);
        }
        let mut out = [0u8; 32];
        for (i, pair) in bytes.chunks(2).enumerate() {
            let hi = hex_digit(pair[0])?;
            let lo = hex_digit(pair[1])?;
            out[i] = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

fn hex_digit(c: u8) -> Result<u8, IdError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(IdError::BadHash),
    }
}

macro_rules! string_serde {
    ($ty:ty, $to:expr, $from:expr) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&$to(self))
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                $from(&text).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// Content id of a blob: keyed hash of its plaintext.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobId(pub Hash32);
/// Opaque stream id: keyed hash of the canonical stream tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(pub Hash32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub VersionId);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub VersionId);

string_serde!(VersionId, |v: &VersionId| v.to_string(), |t: &str| t.parse::<VersionId>());
string_serde!(Hash32, Hash32::to_hex, Hash32::from_hex);
string_serde!(BlobId, |v: &BlobId| v.0.to_hex(), |t: &str| Hash32::from_hex(t).map(BlobId));
string_serde!(StreamId, |v: &StreamId| v.0.to_hex(), |t: &str| Hash32::from_hex(t).map(StreamId));
string_serde!(PlayerId, |v: &PlayerId| v.0.to_string(), |t: &str| t.parse().map(PlayerId));
string_serde!(DeviceId, |v: &DeviceId| v.0.to_string(), |t: &str| t.parse().map(DeviceId));

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_hex())
    }
}
impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_hex())
    }
}
impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
```

- [x] **Step 5: Write the failing path tests** in `crates/core/src/path.rs`:

```rust
//! Validated relative paths. Spec: docs/specs/security.md (File-system confinement).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_save_paths() {
        for ok in ["option.sav", "0/game_data.sav", "album/a b.jpg", "pict_book/x.sav"] {
            assert_eq!(RelPath::parse(ok).unwrap().as_str(), ok);
        }
    }

    #[test]
    fn rejects_escapes_and_absolute_paths() {
        for bad in ["", "/etc/passwd", "../x", "a/../b", "a/./b", "./a", "a//b", "a/", "C:/x", "a\\b", "a:b"] {
            assert!(RelPath::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn rejects_control_characters_and_nul() {
        assert!(RelPath::parse("a\0b").is_err());
        assert!(RelPath::parse("a\nb").is_err());
    }

    #[test]
    fn rejects_windows_reserved_names_and_trailing_dots() {
        for bad in ["CON", "con.txt", "a/NUL", "Com1.sav", "lpt9", "a/b.", "a/b "] {
            assert!(RelPath::parse(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(RelPath::parse("console.sav").is_ok(), "only exact reserved stems");
    }

    #[test]
    fn enforces_length_limits() {
        assert!(RelPath::parse(&"a".repeat(256)).is_err());
        let deep = vec!["d"; 600].join("/");
        assert!(RelPath::parse(&deep).is_err(), "over 1024 bytes");
    }

    #[test]
    fn splits_components() {
        let p = RelPath::parse("0/game_data.sav").unwrap();
        assert_eq!(p.components().collect::<Vec<_>>(), ["0", "game_data.sav"]);
    }
}
```

- [x] **Step 6: Run them to see them fail.** Run: `cargo test -p leemusync-core path` → compile errors.

- [x] **Step 7: Implement `path.rs`** (above the tests):

```rust
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_PATH: usize = 1024;
const MAX_COMPONENT: usize = 255;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path is longer than 1024 bytes")]
    TooLong,
    #[error("path component is empty, `.` or `..`")]
    BadComponent,
    #[error("path component is longer than 255 bytes")]
    ComponentTooLong,
    #[error("path contains a forbidden character")]
    BadChar,
    #[error("path uses a name Windows reserves")]
    Reserved,
}

/// A relative, `/`-separated path that can't escape its root on any OS.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// # Errors
    /// A `PathError` naming the first rule `s` breaks.
    pub fn parse(s: &str) -> Result<Self, PathError> {
        if s.is_empty() {
            return Err(PathError::Empty);
        }
        if s.len() > MAX_PATH {
            return Err(PathError::TooLong);
        }
        for component in s.split('/') {
            check_component(component)?;
        }
        Ok(Self(s.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

fn check_component(c: &str) -> Result<(), PathError> {
    if c.is_empty() || c == "." || c == ".." {
        return Err(PathError::BadComponent);
    }
    if c.len() > MAX_COMPONENT {
        return Err(PathError::ComponentTooLong);
    }
    if c.chars().any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err(PathError::BadChar);
    }
    if c.ends_with('.') || c.ends_with(' ') {
        return Err(PathError::BadChar);
    }
    let stem = c.split('.').next().unwrap_or(c);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(PathError::Reserved);
    }
    Ok(())
}

impl Serialize for RelPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}
```

- [x] **Step 8: Wire the modules.** `crates/core/src/lib.rs`:

```rust
//! Domain model and pure sync logic. No I/O, no async.
//! Spec: docs/specs/architecture.md (Layers), docs/specs/sync-model.md.

pub mod id;
pub mod path;

pub use id::{BlobId, DeviceId, Hash32, IdError, PlayerId, StreamId, VersionId};
pub use path::{PathError, RelPath};
```

- [x] **Step 9: Run the tests.** Run: `cargo test -p leemusync-core && cargo clippy -p leemusync-core --all-targets -- -D warnings` → all pass, no warnings.
- [x] **Step 10: Commit.** Update `docs/specs/security.md` §File-system confinement: add "Implemented in `crates/core/src/path.rs`." Then `just check`, and:

```bash
git add Cargo.toml Cargo.lock crates/core docs/specs/security.md
git commit -m "feat(core): add time-ordered ids and validated relative paths

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 3: `core` manifest, heads and reconciliation

Spec: [sync-model §Versions, §Heads and divergence, §Reconciliation](../../specs/sync-model.md#versions) (as updated in Task 1).

**Files:**
- Modify: `Cargo.toml` (`serde_json = "1.0.154"` in `[workspace.dependencies]`), `crates/core/Cargo.toml` (`serde_json = { workspace = true }` under `[dependencies]`; drop it from `[dev-dependencies]`), `crates/core/src/lib.rs`
- Create: `crates/core/src/manifest.rs`, `crates/core/src/heads.rs`, `crates/core/src/reconcile.rs`, `crates/core/src/golden/manifest-v1.json`

**Interfaces:**
- Consumes: Task 2 ids and `RelPath`.
- Produces:
  - `manifest::{Version, EmulatorRef, FileEntry, Meta, ManifestError, FORMAT}`; `Version::to_json(&self) -> Result<Vec<u8>, ManifestError>`; `Version::from_json(bytes: &[u8], stream: &StreamId, id: &VersionId) -> Result<Version, ManifestError>`; `Version::validate(&self, stream: &StreamId, id: &VersionId) -> Result<(), ManifestError>`; `Version::same_content(&self, files: &[FileEntry]) -> bool` (same paths and blobs, mtimes ignored).
  - `heads::{Graph, heads, descends_from}`: `type Graph = BTreeMap<VersionId, Vec<VersionId>>` (version → parents); `heads(&Graph) -> Vec<VersionId>` (sorted); `descends_from(&Graph, child: VersionId, ancestor: VersionId) -> bool` (true when equal).
  - `reconcile::{Local, Action, Input, reconcile}`: `reconcile(&Input) -> Action`.

- [x] **Step 1: Failing manifest tests** in `crates/core/src/manifest.rs`:

```rust
//! Version manifest, format 1. Spec: docs/specs/sync-model.md (Versions).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlobId, DeviceId, Hash32, PlayerId, RelPath, StreamId, VersionId};

    fn sample() -> Version {
        Version {
            format: FORMAT,
            stream: StreamId(Hash32([1; 32])),
            id: VersionId::from_parts(1_760_000_000_000, [2; 10]),
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [3; 10])),
            player: PlayerId(VersionId::from_parts(2, [4; 10])),
            created_at: 1_760_000_000_000,
            emulator: EmulatorRef { id: "cemu".into(), version: None },
            files: vec![FileEntry {
                path: RelPath::parse("option.sav").unwrap(),
                size: 3,
                mtime: 1_760_000_000_000,
                blob: BlobId(Hash32([5; 32])),
            }],
            meta: Meta { session_secs: None, changed_files: 1, takeover: false, note: None },
        }
    }

    #[test]
    fn round_trips_through_json() {
        let v = sample();
        let bytes = v.to_json().unwrap();
        assert_eq!(Version::from_json(&bytes, &v.stream, &v.id).unwrap(), v);
    }

    #[test]
    fn matches_the_golden_file() {
        let json = String::from_utf8(sample().to_json().unwrap()).unwrap();
        assert_eq!(json, include_str!("golden/manifest-v1.json").trim_end());
    }

    #[test]
    #[ignore = "prints the golden file; run once with --nocapture and review"]
    fn print_golden() {
        println!("{}", String::from_utf8(sample().to_json().unwrap()).unwrap());
    }

    #[test]
    fn rejects_unknown_fields_and_other_formats() {
        let v = sample();
        let mut json: serde_json::Value = serde_json::from_slice(&v.to_json().unwrap()).unwrap();
        json["extra"] = serde_json::json!(1);
        assert!(Version::from_json(json.to_string().as_bytes(), &v.stream, &v.id).is_err());
        let mut v2 = sample();
        v2.format = 2;
        assert_eq!(v2.validate(&v.stream, &v.id), Err(ManifestError::Format(2)));
    }

    #[test]
    fn rejects_a_manifest_stored_under_another_name() {
        let v = sample();
        let other = VersionId::from_parts(9, [9; 10]);
        assert_eq!(v.validate(&v.stream, &other), Err(ManifestError::WrongId));
        assert_eq!(v.validate(&StreamId(Hash32([7; 32])), &v.id), Err(ManifestError::WrongStream));
    }

    #[test]
    fn rejects_duplicate_or_unsorted_paths_and_bad_parents() {
        let mut v = sample();
        v.files.push(v.files[0].clone());
        assert_eq!(v.validate(&v.stream, &v.id), Err(ManifestError::Files));
        let mut v = sample();
        v.parents = vec![v.id];
        assert_eq!(v.validate(&v.stream, &v.id), Err(ManifestError::Parents));
    }

    #[test]
    fn same_content_ignores_mtime() {
        let v = sample();
        let mut files = v.files.clone();
        files[0].mtime = 1;
        assert!(v.same_content(&files));
        files[0].blob = BlobId(Hash32([6; 32]));
        assert!(!v.same_content(&files));
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-core manifest` → compile errors.
- [x] **Step 3: Implement** (above the tests):

```rust
use serde::{Deserialize, Serialize};

use crate::{BlobId, DeviceId, PlayerId, RelPath, StreamId, VersionId};

pub const FORMAT: u32 = 1;
const MAX_FILES: usize = 100_000;
const MAX_TEXT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub format: u32,
    pub stream: StreamId,
    pub id: VersionId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub player: PlayerId,
    /// Unix milliseconds.
    pub created_at: u64,
    pub emulator: EmulatorRef,
    /// Sorted by path, no duplicates.
    pub files: Vec<FileEntry>,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmulatorRef {
    pub id: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub path: RelPath,
    pub size: u64,
    /// Unix milliseconds.
    pub mtime: i64,
    pub blob: BlobId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub session_secs: Option<u64>,
    pub changed_files: u32,
    pub takeover: bool,
    pub note: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("unsupported manifest format {0}")]
    Format(u32),
    #[error("manifest belongs to another stream")]
    WrongStream,
    #[error("manifest id doesn't match its object name")]
    WrongId,
    #[error("manifest files are unsorted, duplicated or too many")]
    Files,
    #[error("manifest parents are duplicated or include itself")]
    Parents,
    #[error("manifest text field is too long")]
    Text,
    #[error("manifest JSON: {0}")]
    Json(String),
}

impl Version {
    /// # Errors
    /// `ManifestError::Json` if serialisation fails.
    pub fn to_json(&self) -> Result<Vec<u8>, ManifestError> {
        serde_json::to_vec(self).map_err(|e| ManifestError::Json(e.to_string()))
    }

    /// Parses and validates a manifest read from `streams/<stream>/versions/<id>`.
    ///
    /// # Errors
    /// Any `ManifestError`; callers then ignore the version (sync-model, Commit point).
    pub fn from_json(bytes: &[u8], stream: &StreamId, id: &VersionId) -> Result<Self, ManifestError> {
        let v: Self = serde_json::from_slice(bytes).map_err(|e| ManifestError::Json(e.to_string()))?;
        v.validate(stream, id)?;
        Ok(v)
    }

    /// # Errors
    /// The first rule the manifest breaks.
    pub fn validate(&self, stream: &StreamId, id: &VersionId) -> Result<(), ManifestError> {
        if self.format != FORMAT {
            return Err(ManifestError::Format(self.format));
        }
        if self.stream != *stream {
            return Err(ManifestError::WrongStream);
        }
        if self.id != *id {
            return Err(ManifestError::WrongId);
        }
        if self.files.len() > MAX_FILES || self.files.windows(2).any(|w| w[0].path >= w[1].path) {
            return Err(ManifestError::Files);
        }
        let mut parents = self.parents.clone();
        parents.sort_unstable();
        parents.dedup();
        if parents.len() != self.parents.len() || parents.contains(&self.id) {
            return Err(ManifestError::Parents);
        }
        let texts = [Some(&self.emulator.id), self.emulator.version.as_ref(), self.meta.note.as_ref()];
        if texts.iter().flatten().any(|t| t.len() > MAX_TEXT) {
            return Err(ManifestError::Text);
        }
        Ok(())
    }

    #[must_use]
    pub fn same_content(&self, files: &[FileEntry]) -> bool {
        self.files.len() == files.len()
            && self.files.iter().zip(files).all(|(a, b)| a.path == b.path && a.blob == b.blob)
    }
}
```

- [x] **Step 4: Create the golden file.** Create `crates/core/src/golden/manifest-v1.json` empty, run `cargo test -p leemusync-core print_golden -- --ignored --nocapture`, copy the printed line into the file, and check it by eye: fields in struct order, `"files":[{"path":"option.sav",...}]`, ids as strings. This file freezes format 1. Changing it later means a new format number.
- [x] **Step 5: Run** `cargo test -p leemusync-core manifest` → PASS.
- [x] **Step 6: Failing heads and reconcile tests.** `crates/core/src/heads.rs`:

```rust
//! Version graph. Spec: docs/specs/sync-model.md (Heads and divergence).

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn v(n: u64) -> VersionId {
        VersionId::from_parts(n, [0; 10])
    }

    #[test]
    fn a_chain_has_one_head() {
        let g: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(2)])].into();
        assert_eq!(heads(&g), vec![v(3)]);
        assert!(descends_from(&g, v(3), v(1)));
        assert!(!descends_from(&g, v(1), v(3)));
        assert!(descends_from(&g, v(2), v(2)));
    }

    #[test]
    fn a_fork_has_two_heads_and_a_merge_has_one() {
        let mut g: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(1)])].into();
        assert_eq!(heads(&g), vec![v(2), v(3)]);
        g.insert(v(4), vec![v(2), v(3)]);
        assert_eq!(heads(&g), vec![v(4)]);
    }

    #[test]
    fn missing_parents_are_tolerated() {
        let g: Graph = [(v(5), vec![v(1)])].into();
        assert_eq!(heads(&g), vec![v(5)]);
        assert!(descends_from(&g, v(5), v(1)));
    }
}
```

`crates/core/src/reconcile.rs`:

```rust
//! What to do with one stream. Spec: docs/specs/sync-model.md (Reconciliation).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heads::Graph;

    fn v(n: u64) -> VersionId {
        VersionId::from_parts(n, [0; 10])
    }

    fn run(base: Option<u64>, local: Local, graph: &Graph, matches: Option<u64>, replace: bool) -> Action {
        reconcile(&Input {
            base: base.map(v),
            local,
            graph,
            local_matches: matches.map(v),
            replace_local: replace,
        })
    }

    #[test]
    fn the_spec_table() {
        let one: Graph = [(v(1), vec![])].into();
        let two: Graph = [(v(1), vec![]), (v(2), vec![v(1)])].into();
        assert_eq!(run(Some(1), Local::Clean, &one, None, false), Action::Nothing);
        assert_eq!(run(Some(1), Local::Clean, &two, None, false), Action::FastForward(v(2)));
        assert_eq!(run(Some(1), Local::Dirty, &one, None, false), Action::Upload { parents: vec![v(1)] });
        assert_eq!(
            run(Some(1), Local::Dirty, &two, None, false),
            Action::UploadThenConflict { parents: vec![v(1)] }
        );
        let fork: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(1)])].into();
        assert_eq!(run(Some(1), Local::Clean, &fork, None, false), Action::Conflict(vec![v(2), v(3)]));
        assert_eq!(
            run(Some(1), Local::Dirty, &fork, None, false),
            Action::UploadThenConflict { parents: vec![v(1)] }
        );
    }

    #[test]
    fn first_sync_without_a_base() {
        let empty = Graph::new();
        let one: Graph = [(v(1), vec![])].into();
        assert_eq!(run(None, Local::Dirty, &empty, None, false), Action::Upload { parents: vec![] });
        assert_eq!(run(None, Local::Missing, &empty, None, false), Action::Nothing);
        assert_eq!(run(None, Local::Missing, &one, None, false), Action::FastForward(v(1)));
        assert_eq!(run(None, Local::Dirty, &one, Some(1), false), Action::Adopt(v(1)));
        assert_eq!(
            run(None, Local::Dirty, &one, None, false),
            Action::UploadThenConflict { parents: vec![] },
            "different local files never get overwritten silently"
        );
        assert_eq!(run(None, Local::Dirty, &one, None, true), Action::FastForward(v(1)));
    }

    #[test]
    fn local_folder_gone_and_remote_behind() {
        let one: Graph = [(v(1), vec![])].into();
        let empty = Graph::new();
        assert_eq!(run(Some(1), Local::Missing, &one, None, false), Action::LocalMissing);
        assert_eq!(run(Some(7), Local::Clean, &one, None, false), Action::RemoteBehind);
        assert_eq!(run(Some(1), Local::Clean, &empty, None, false), Action::RemoteBehind);
        assert_eq!(
            run(Some(7), Local::Dirty, &one, None, false),
            Action::RemoteBehind,
            "a base the remote no longer has is a rollback, even with local changes"
        );
        assert_eq!(run(Some(1), Local::Dirty, &empty, None, false), Action::RemoteBehind);
        let fork: Graph = [(v(2), vec![]), (v(3), vec![])].into();
        assert_eq!(run(Some(1), Local::Dirty, &fork, None, false), Action::RemoteBehind);
    }
}
```

- [x] **Step 7: Run** `cargo test -p leemusync-core` → compile errors for `heads` and `reconcile`.
- [x] **Step 8: Implement `heads.rs`** (above its tests):

```rust
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::VersionId;

/// Version → its parents.
pub type Graph = BTreeMap<VersionId, Vec<VersionId>>;

/// Versions that no other version lists as a parent, sorted.
#[must_use]
pub fn heads(graph: &Graph) -> Vec<VersionId> {
    let parents: BTreeSet<VersionId> = graph.values().flatten().copied().collect();
    graph.keys().filter(|id| !parents.contains(id)).copied().collect()
}

/// True when `ancestor` is `child` or reachable through parents.
#[must_use]
pub fn descends_from(graph: &Graph, child: VersionId, ancestor: VersionId) -> bool {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([child]);
    while let Some(id) = queue.pop_front() {
        if id == ancestor {
            return true;
        }
        if seen.insert(id) {
            queue.extend(graph.get(&id).into_iter().flatten().copied());
        }
    }
    false
}
```

- [x] **Step 9: Implement `reconcile.rs`** (above its tests):

```rust
use crate::VersionId;
use crate::heads::{Graph, descends_from, heads};

/// The stream's local files compared with `base`. With no `base`, existing files count as `Dirty`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Local {
    Missing,
    Clean,
    Dirty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Local files already equal this head: record it as `base`, touch nothing.
    Adopt(VersionId),
    /// Restore this head (backup first).
    FastForward(VersionId),
    Upload { parents: Vec<VersionId> },
    /// Upload local files first so they're safe, then report the conflict.
    UploadThenConflict { parents: Vec<VersionId> },
    Conflict(Vec<VersionId>),
    /// A mapped account's folder disappeared: ask the player.
    LocalMissing,
    /// The remote lost the base this device saw (rollback): warn, change nothing, even if local files changed.
    RemoteBehind,
}

pub struct Input<'a> {
    pub base: Option<VersionId>,
    pub local: Local,
    pub graph: &'a Graph,
    /// A head whose files equal the local files (computed by the engine).
    pub local_matches: Option<VersionId>,
    /// The player chose to replace this account's files with the remote head.
    pub replace_local: bool,
}

#[must_use]
pub fn reconcile(input: &Input<'_>) -> Action {
    if let Some(base) = input.base
        && input.local != Local::Missing
        && !input.graph.contains_key(&base)
    {
        return Action::RemoteBehind;
    }
    let heads = heads(input.graph);
    match heads.as_slice() {
        [] => match (input.base, input.local) {
            (_, Local::Missing) => Action::Nothing,
            (None, _) => Action::Upload { parents: vec![] },
            (Some(_), Local::Clean) => Action::RemoteBehind,
            (Some(base), Local::Dirty) => Action::Upload { parents: vec![base] },
        },
        [head] => one_head(input, *head),
        _ => match input.local {
            Local::Dirty => Action::UploadThenConflict { parents: input.base.into_iter().collect() },
            _ => Action::Conflict(heads),
        },
    }
}

fn one_head(input: &Input<'_>, head: VersionId) -> Action {
    let Some(base) = input.base else {
        return match input.local {
            Local::Missing => Action::FastForward(head),
            _ if input.local_matches == Some(head) => Action::Adopt(head),
            _ if input.replace_local => Action::FastForward(head),
            _ => Action::UploadThenConflict { parents: vec![] },
        };
    };
    if input.local == Local::Missing {
        return Action::LocalMissing;
    }
    if !descends_from(input.graph, head, base) {
        return match input.local {
            Local::Dirty => Action::UploadThenConflict { parents: vec![base] },
            _ => Action::RemoteBehind,
        };
    }
    match (head == base, input.local) {
        (true, Local::Dirty) => Action::Upload { parents: vec![base] },
        (false, Local::Clean) => Action::FastForward(head),
        (false, Local::Dirty) => Action::UploadThenConflict { parents: vec![base] },
        _ => Action::Nothing,
    }
}
```

- [x] **Step 10: Wire and run.** Add to `lib.rs`: `pub mod heads; pub mod manifest; pub mod reconcile;`. Run `cargo test -p leemusync-core && cargo clippy -p leemusync-core --all-targets -- -D warnings` → PASS.
- [x] **Step 11: Commit.** Set `docs/specs/sync-model.md` **Code:** to `` `crates/core/src/{manifest,heads,reconcile}.rs` ``, then `just check` and:

```bash
git add Cargo.toml Cargo.lock crates/core docs/specs/sync-model.md
git commit -m "feat(core): add the v1 manifest, heads and reconciliation

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `core` repo config and automatic merge

Spec: [sync-model §Repo config](../../specs/sync-model.md#repo-config), [players-and-accounts §The same-ID trap](../../specs/players-and-accounts.md#the-same-id-trap), D30.

**Files:**
- Create: `crates/core/src/config.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod config;`)

**Interfaces:**
- Produces: `config::{RepoConfig, Player, ConfigVersion, ConfigError, CONFIG_FORMAT}`.
  - `RepoConfig { players: BTreeMap<PlayerId, Player>, mappings: BTreeMap<DeviceId, BTreeMap<PlayerId, BTreeMap<String, String>>>, games: BTreeMap<PlayerId, BTreeSet<String>> }` (mappings: device → player → emulator id → account id; games: player → `"<emulator>/<game key>"` for every stream that has a version, so a device can find a player's streams without listing opaque stream ids), `Player { name: String }`.
  - `RepoConfig::add_player(&mut self, id: PlayerId, name: &str) -> Result<(), ConfigError>`, `player_named(&self, name: &str) -> Option<PlayerId>`, `map_account(&mut self, device: DeviceId, player: PlayerId, emulator: &str, account: &str) -> Result<(), ConfigError>`, `account_of(&self, device: DeviceId, player: PlayerId, emulator: &str) -> Option<&str>`, `player_on_account(&self, device: DeviceId, emulator: &str, account: &str) -> Option<PlayerId>`, `add_game(&mut self, player: PlayerId, emulator: &str, game: &str) -> bool` (true if it was new), `games_of(&self, player: PlayerId) -> Vec<(String, String)>` (emulator, game), `merge(configs: &[&RepoConfig]) -> (RepoConfig, Vec<String>)` (the strings are renamed player names).
  - `ConfigVersion { format, id, parents, device, created_at, config }` with `to_json(&self) -> Result<Vec<u8>, ConfigError>` and `from_json(bytes: &[u8], id: &VersionId) -> Result<ConfigVersion, ConfigError>`.

- [x] **Step 1: Failing tests** in `crates/core/src/config.rs`:

```rust
//! Repo config: players and per-device account mappings. Spec: docs/specs/sync-model.md (Repo config).

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u64) -> PlayerId {
        PlayerId(VersionId::from_parts(n, [1; 10]))
    }
    fn d(n: u64) -> DeviceId {
        DeviceId(VersionId::from_parts(n, [2; 10]))
    }

    #[test]
    fn players_need_unique_valid_names() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "Daniel").unwrap();
        assert_eq!(c.add_player(p(2), "daniel"), Err(ConfigError::NameTaken("daniel".into())));
        assert_eq!(c.add_player(p(3), "  "), Err(ConfigError::BadName));
        assert_eq!(c.add_player(p(4), &"x".repeat(33)), Err(ConfigError::BadName));
        assert_eq!(c.add_player(p(5), "a\nb"), Err(ConfigError::BadName));
        assert_eq!(c.player_named("DANIEL"), Some(p(1)));
    }

    #[test]
    fn the_same_id_trap_is_per_device() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "A").unwrap();
        c.add_player(p(2), "B").unwrap();
        c.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        c.map_account(d(2), p(2), "cemu", "80000001").unwrap();
        assert_eq!(c.account_of(d(1), p(1), "cemu"), Some("80000001"));
        assert_eq!(c.player_on_account(d(2), "cemu", "80000001"), Some(p(2)));
        assert_eq!(
            c.map_account(d(1), p(2), "cemu", "80000001"),
            Err(ConfigError::AccountTaken { account: "80000001".into(), player: "A".into() })
        );
        assert!(c.map_account(d(1), p(9), "cemu", "80000002").is_err(), "unknown player");
    }

    #[test]
    fn merge_is_a_union_and_renames_name_clashes() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        a.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        let mut b = a.clone();
        b.map_account(d(2), p(1), "cemu", "80000002").unwrap();
        let mut c = RepoConfig::default();
        c.add_player(p(3), "a").unwrap();
        c.map_account(d(3), p(3), "cemu", "80000001").unwrap();
        let (m, renamed) = RepoConfig::merge(&[&a, &b, &c]);
        assert_eq!(m.players.len(), 2, "p1 is in two configs but counts once");
        assert_eq!(m.players[&p(1)].name, "A", "a shared id keeps its name");
        assert_eq!(m.players[&p(3)].name, "a (2)");
        assert_eq!(renamed, vec!["a (2)".to_string()]);
        assert_eq!(m.account_of(d(1), p(1), "cemu"), Some("80000001"));
        assert_eq!(m.account_of(d(2), p(1), "cemu"), Some("80000002"));
        assert_eq!(m.account_of(d(3), p(3), "cemu"), Some("80000001"));
        assert_eq!(RepoConfig::merge(&[&c, &b, &a]).0, m, "order doesn't matter");
        assert_eq!(RepoConfig::merge(&[&b, &c, &a]).0, m, "order doesn't matter");
    }

    #[test]
    fn games_are_listed_per_player_and_merged_by_union() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        let mut b = a.clone();
        assert!(a.add_game(p(1), "cemu", "101c9400"));
        assert!(!a.add_game(p(1), "cemu", "101c9400"), "already known");
        b.add_game(p(1), "cemu", "101c9500");
        let (m, _) = RepoConfig::merge(&[&a, &b]);
        assert_eq!(
            m.games_of(p(1)),
            vec![("cemu".to_string(), "101c9400".to_string()), ("cemu".to_string(), "101c9500".to_string())]
        );
    }

    #[test]
    fn merge_drops_a_second_player_on_one_account() {
        let mut a = RepoConfig::default();
        a.add_player(p(1), "A").unwrap();
        a.add_player(p(2), "B").unwrap();
        let mut b = a.clone();
        a.map_account(d(1), p(1), "cemu", "80000001").unwrap();
        b.map_account(d(1), p(2), "cemu", "80000001").unwrap();
        let (m, _) = RepoConfig::merge(&[&a, &b]);
        assert_eq!(m.player_on_account(d(1), "cemu", "80000001"), Some(p(1)));
        assert_eq!(m.account_of(d(1), p(2), "cemu"), None);
    }

    #[test]
    fn config_version_round_trips() {
        let mut c = RepoConfig::default();
        c.add_player(p(1), "A").unwrap();
        let v = ConfigVersion {
            format: CONFIG_FORMAT,
            id: VersionId::from_parts(10, [3; 10]),
            parents: vec![],
            device: d(1),
            created_at: 10,
            config: c,
        };
        let bytes = v.to_json().unwrap();
        assert_eq!(ConfigVersion::from_json(&bytes, &v.id).unwrap(), v);
        assert!(ConfigVersion::from_json(&bytes, &VersionId::from_parts(11, [3; 10])).is_err());
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-core config` → compile errors.
- [x] **Step 3: Implement** (above the tests):

```rust
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{DeviceId, PlayerId, VersionId};

pub const CONFIG_FORMAT: u32 = 1;
const MAX_NAME: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    pub players: BTreeMap<PlayerId, Player>,
    /// device → player → emulator id → account id.
    pub mappings: BTreeMap<DeviceId, BTreeMap<PlayerId, BTreeMap<String, String>>>,
    /// player → `"<emulator>/<game key>"` of every stream with a version.
    #[serde(default)]
    pub games: BTreeMap<PlayerId, BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigVersion {
    pub format: u32,
    pub id: VersionId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub created_at: u64,
    pub config: RepoConfig,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("a player name needs 1 to 32 visible characters")]
    BadName,
    #[error("the player name `{0}` is taken")]
    NameTaken(String),
    #[error("no player `{0}`")]
    UnknownPlayer(String),
    #[error("account {account} is already mapped to {player}")]
    AccountTaken { account: String, player: String },
    #[error("unsupported config format {0}")]
    Format(u32),
    #[error("config id doesn't match its object name")]
    WrongId,
    #[error("config JSON: {0}")]
    Json(String),
}

impl RepoConfig {
    /// # Errors
    /// `BadName` or `NameTaken`.
    pub fn add_player(&mut self, id: PlayerId, name: &str) -> Result<(), ConfigError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
            return Err(ConfigError::BadName);
        }
        if self.player_named(name).is_some() {
            return Err(ConfigError::NameTaken(name.to_owned()));
        }
        self.players.insert(id, Player { name: name.to_owned() });
        Ok(())
    }

    #[must_use]
    pub fn player_named(&self, name: &str) -> Option<PlayerId> {
        let name = name.trim().to_lowercase();
        self.players.iter().find(|(_, p)| p.name.to_lowercase() == name).map(|(id, _)| *id)
    }

    /// # Errors
    /// `UnknownPlayer`, or `AccountTaken` when another player uses that account on this device.
    pub fn map_account(
        &mut self,
        device: DeviceId,
        player: PlayerId,
        emulator: &str,
        account: &str,
    ) -> Result<(), ConfigError> {
        if !self.players.contains_key(&player) {
            return Err(ConfigError::UnknownPlayer(player.to_string()));
        }
        if let Some(other) = self.player_on_account(device, emulator, account) {
            if other != player {
                let name = self.players.get(&other).map(|p| p.name.clone()).unwrap_or_default();
                return Err(ConfigError::AccountTaken { account: account.to_owned(), player: name });
            }
        }
        self.mappings
            .entry(device)
            .or_default()
            .entry(player)
            .or_default()
            .insert(emulator.to_owned(), account.to_owned());
        Ok(())
    }

    #[must_use]
    pub fn account_of(&self, device: DeviceId, player: PlayerId, emulator: &str) -> Option<&str> {
        self.mappings.get(&device)?.get(&player)?.get(emulator).map(String::as_str)
    }

    #[must_use]
    pub fn player_on_account(&self, device: DeviceId, emulator: &str, account: &str) -> Option<PlayerId> {
        self.mappings.get(&device)?.iter().find_map(|(player, by_emu)| {
            (by_emu.get(emulator).map(String::as_str) == Some(account)).then_some(*player)
        })
    }

    pub fn add_game(&mut self, player: PlayerId, emulator: &str, game: &str) -> bool {
        self.games.entry(player).or_default().insert(format!("{emulator}/{game}"))
    }

    #[must_use]
    pub fn games_of(&self, player: PlayerId) -> Vec<(String, String)> {
        self.games
            .get(&player)
            .into_iter()
            .flatten()
            .filter_map(|g| g.split_once('/').map(|(e, k)| (e.to_owned(), k.to_owned())))
            .collect()
    }

    /// Union of several heads (D30). Returns the merged config and the names it had to rename.
    #[must_use]
    pub fn merge(configs: &[&RepoConfig]) -> (RepoConfig, Vec<String>) {
        let mut out = RepoConfig::default();
        for c in configs {
            for (id, player) in &c.players {
                out.players.entry(*id).or_insert_with(|| player.clone());
            }
        }
        let mut renamed = Vec::new();
        let mut used: Vec<String> = Vec::new();
        for player in out.players.values_mut() {
            let base = player.name.clone();
            let mut name = base.clone();
            let mut n = 2;
            while used.contains(&name.to_lowercase()) {
                name = format!("{base} ({n})");
                n += 1;
            }
            if name != base {
                renamed.push(name.clone());
                player.name.clone_from(&name);
            }
            used.push(name.to_lowercase());
        }
        for c in configs {
            for (device, by_player) in &c.mappings {
                for (player, by_emu) in by_player {
                    for (emu, account) in by_emu {
                        let slot = out.mappings.entry(*device).or_default().entry(*player).or_default();
                        match slot.get(emu) {
                            Some(existing) if existing <= account => {}
                            _ => {
                                slot.insert(emu.clone(), account.clone());
                            }
                        }
                    }
                }
            }
        }
        for c in configs {
            for (player, games) in &c.games {
                out.games.entry(*player).or_default().extend(games.iter().cloned());
            }
        }
        for by_player in out.mappings.values_mut() {
            let mut taken: BTreeMap<(String, String), PlayerId> = BTreeMap::new();
            for (player, by_emu) in by_player.iter_mut() {
                by_emu.retain(|emu, account| {
                    let key = (emu.clone(), account.clone());
                    match taken.get(&key) {
                        Some(_) => false,
                        None => {
                            taken.insert(key, *player);
                            true
                        }
                    }
                });
            }
            by_player.retain(|_, by_emu| !by_emu.is_empty());
        }
        (out, renamed)
    }
}

impl ConfigVersion {
    /// # Errors
    /// `ConfigError::Json` if serialisation fails.
    pub fn to_json(&self) -> Result<Vec<u8>, ConfigError> {
        serde_json::to_vec(self).map_err(|e| ConfigError::Json(e.to_string()))
    }

    /// # Errors
    /// `Json`, `Format` or `WrongId`.
    pub fn from_json(bytes: &[u8], id: &VersionId) -> Result<Self, ConfigError> {
        let v: Self = serde_json::from_slice(bytes).map_err(|e| ConfigError::Json(e.to_string()))?;
        if v.format != CONFIG_FORMAT {
            return Err(ConfigError::Format(v.format));
        }
        if v.id != *id || v.parents.contains(id) {
            return Err(ConfigError::WrongId);
        }
        Ok(v)
    }
}
```

Note on `merge`: players iterate in id order (`BTreeMap`), so the earlier id keeps the plain name and the result doesn't depend on the order of `configs`. Mapping clashes keep the smaller player id for the same reason.

- [x] **Step 4: Run** `cargo test -p leemusync-core config && cargo clippy -p leemusync-core --all-targets -- -D warnings` → PASS.
- [x] **Step 5: Commit** (`just check` first):

```bash
git add crates/core
git commit -m "feat(core): add the repo config with per-device account mappings

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: `core` profile schema and Cemu account files

Spec: [emulator-profiles §Schema, §Validation](../../specs/emulator-profiles.md#schema-v1), [players-and-accounts §Cemu accounts](../../specs/players-and-accounts.md#cemu-accounts-sp7). Skill: `leemusync-emulator-profile`.

**Files:**
- Modify: `Cargo.toml` (`toml = "1.1.8"` in `[workspace.dependencies]`), `crates/core/Cargo.toml` (`toml = { workspace = true }`), `crates/core/src/lib.rs`
- Create: `crates/core/src/profile.rs`, `crates/core/src/account.rs`
- Modify: `profiles/cemu.toml`

**Interfaces:**
- Produces:
  - `profile::{Profile, Platform, ConfigKey, Saves, Accounts, GameKey, ProfileError, PatternPart, ALLOWED_VARS}`; `Profile::from_toml(text: &str) -> Result<Profile, ProfileError>` (parses and validates); `profile::expand(template: &str, vars: &BTreeMap<String, String>) -> Result<String, ProfileError>`; `profile::parse_pattern(pattern: &str) -> Result<Vec<PatternPart>, ProfileError>` with `PatternPart::{Literal(String), Var(String)}`; `Profile::max_file_bytes(&self) -> u64`.
  - `account::key_value(text: &str, key: &str) -> Option<&str>`; `account::decode_mii_name(hex: &str) -> Option<String>`.

- [x] **Step 1: Update `profiles/cemu.toml`** (data change, evidence in [emulator-profiles §Linux paths](../../specs/emulator-profiles.md#linux-paths-sp7)). Replace the "Schema gap (SP7)" comment and `[[config_keys]]` block, and add the two new fields, so the file reads:

```toml
# DRAFT from spike SP7. Verified: Cemu 2.6, Linux (Flatpak info.cemu.Cemu + source tag v2.6), 2026-10-08;
# native cemu-bin 2.6 on CachyOS, 2026-10-18. Loaded by crates/engine/src/profiles.rs from Phase 1.
# Evidence: docs/specs/emulator-profiles.md#linux-paths-sp7
# windows/macos: pending SP7 device pass
schema = 1
id = "cemu"
name = "Cemu"
systems = ["wiiu"]
layout = "per_game_user_dir"
requires_closed = true

[platforms.linux]
# Portable: a folder named "portable" next to the executable (or next to the AppImage) holds data and config.
roots = ["{exe_dir}/portable", "{flatpak:info.cemu.Cemu}/data/Cemu", "{xdg_data}/Cemu"]
# settings.xml lives in the config folder, not under the data root: one entry per root, same order.
config_roots = ["{exe_dir}/portable", "{flatpak:info.cemu.Cemu}/config/Cemu", "{xdg_config}/Cemu"]
# Flatpak runs Cemu_relwithdebinfo via Cemu-wrapper; comm is truncated to 15 chars ("Cemu_relwithdeb").
# Arch package cemu-bin installs the binary as lowercase /usr/bin/cemu (CachyOS, 2026-10-18).
process = ["Cemu_relwithdebinfo", "Cemu", "cemu"]

[[config_keys]]
file = "settings.xml"              # relative to the matching config root
format = "xml"
key = "content/mlc_path"
var = "mlc"
default = "{base}/mlc01"           # used when the key is missing or empty

[saves]
root = "{mlc}/usr/save"
max_file_mb = 64
pattern = "00050000/{title_id}/user/{account}"
shared = ["00050000/{title_id}/user/common"]

[accounts]
root = "{mlc}/usr/save/system/act"
id_regex = "^8[0-9a-f]{7}$"
name_file = "account.dat"          # line-based key=value
name_key = "MiiName"               # 44 hex chars = 11 UTF-16 code units, 4 hex digits each, NUL padded

[game_key]
from = "title_id"
normalize = ["lowercase"]
```

Mirror the `config_roots`, `max_file_mb` and `default` lines in the Cemu example of `docs/specs/emulator-profiles.md` §Example.

- [x] **Step 2: Failing tests.** `crates/core/src/account.rs`:

```rust
//! Line-based `key=value` account files and Cemu's `MiiName` encoding.
//! Spec: docs/specs/players-and-accounts.md (Cemu accounts).

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(
        "../../../profiles/fixtures/cemu/flatpak-botw-two-accounts/data/Cemu/mlc01/usr/save/system/act/80000001/account.dat"
    );

    #[test]
    fn reads_the_fixture_name() {
        assert!(FIXTURE.starts_with("AccountInstance_20120705"));
        let hex = key_value(FIXTURE, "MiiName").unwrap();
        assert_eq!(decode_mii_name(hex).as_deref(), Some("PlayerA"));
    }

    #[test]
    fn key_value_matches_whole_keys_only() {
        let text = "AccountInstance_20120705\nMiiNameX=1\nMiiName=2\r\n";
        assert_eq!(key_value(text, "MiiName"), Some("2"));
        assert_eq!(key_value(text, "Missing"), None);
    }

    #[test]
    fn rejects_bad_mii_names() {
        assert_eq!(decode_mii_name("0041"), None, "wrong length");
        assert_eq!(decode_mii_name(&"zz".repeat(22)), None, "not hex");
        assert_eq!(decode_mii_name(&"0".repeat(44)), Some(String::new()));
        assert_eq!(decode_mii_name(&format!("D800{}", "0".repeat(40))), None, "lone surrogate");
    }
}
```

`crates/core/src/profile.rs`:

```rust
//! Emulator profile schema (subset used in Phase 1), templates and save patterns.
//! Spec: docs/specs/emulator-profiles.md (Schema, Templates, Validation).

#[cfg(test)]
mod tests {
    use super::*;

    const CEMU: &str = include_str!("../../../profiles/cemu.toml");

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn the_bundled_cemu_profile_is_valid() {
        let p = Profile::from_toml(CEMU).unwrap();
        assert_eq!(p.id, "cemu");
        let linux = &p.platforms["linux"];
        assert_eq!(linux.roots.len(), linux.config_roots.as_ref().unwrap().len());
        assert!(linux.process.contains(&"cemu".to_owned()));
        assert_eq!(p.max_file_bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn expands_known_variables_only() {
        let v = vars(&[("xdg_data", "/home/u/.local/share"), ("flatpak:info.cemu.Cemu", "/home/u/.var/app/info.cemu.Cemu")]);
        assert_eq!(expand("{xdg_data}/Cemu", &v).unwrap(), "/home/u/.local/share/Cemu");
        assert_eq!(
            expand("{flatpak:info.cemu.Cemu}/data/Cemu", &v).unwrap(),
            "/home/u/.var/app/info.cemu.Cemu/data/Cemu"
        );
        assert_eq!(expand("{exe_dir}/portable", &v), Err(ProfileError::MissingVar("exe_dir".into())));
        assert_eq!(expand("{nope}/x", &v), Err(ProfileError::UnknownVar("nope".into())));
        assert!(expand("{xdg_data", &v).is_err());
    }

    #[test]
    fn expands_home_when_supplied() {
        let v = vars(&[("home", "/home/u")]);
        assert_eq!(expand("{home}/x", &v).unwrap(), "/home/u/x");
    }

    #[test]
    fn rejects_a_bad_size_limit() {
        for bad in ["0", "4097"] {
            let text = CEMU.replace("max_file_mb = 64", &format!("max_file_mb = {bad}"));
            assert_eq!(Profile::from_toml(&text).unwrap_err(), ProfileError::Field("saves.max_file_mb"));
        }
        let none = CEMU.replace("max_file_mb = 64\n", "");
        assert_eq!(Profile::from_toml(&none).unwrap().max_file_bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn patterns_only_allow_captures() {
        assert_eq!(parse_pattern("{home}/x"), Err(ProfileError::UnknownVar("home".into())));
        assert_eq!(
            parse_pattern("00050000/{mlc}/user/{account}"),
            Err(ProfileError::UnknownVar("mlc".into()))
        );
        assert!(parse_pattern("{game}/{account}").is_ok());
        let bad_shared = CEMU.replace("00050000/{title_id}/user/common", "{exe_dir}");
        assert_eq!(Profile::from_toml(&bad_shared).unwrap_err(), ProfileError::UnknownVar("exe_dir".into()));
        let bad_pattern = CEMU.replace("00050000/{title_id}/user/{account}", "{home}/{title_id}/{account}");
        assert!(matches!(Profile::from_toml(&bad_pattern), Err(ProfileError::UnknownVar(_))));
    }

    #[test]
    fn parses_save_patterns() {
        assert_eq!(
            parse_pattern("00050000/{title_id}/user/{account}").unwrap(),
            vec![
                PatternPart::Literal("00050000".into()),
                PatternPart::Var("title_id".into()),
                PatternPart::Literal("user".into()),
                PatternPart::Var("account".into()),
            ]
        );
        assert!(parse_pattern("../{account}").is_err());
        assert!(parse_pattern("/abs/{account}").is_err());
        assert!(parse_pattern("a{account}").is_err(), "a variable must be a whole component");
    }

    #[test]
    fn rejects_profiles_that_break_the_rules() {
        let bad_layout = CEMU.replace("per_game_user_dir", "container");
        assert!(matches!(Profile::from_toml(&bad_layout), Err(ProfileError::Unsupported(_))));
        let bad_var = CEMU.replace("{xdg_data}/Cemu", "{nope}/Cemu");
        assert!(matches!(Profile::from_toml(&bad_var), Err(ProfileError::UnknownVar(_))));
        let extra = format!("{CEMU}\nrun = \"/bin/sh\"\n");
        assert!(matches!(Profile::from_toml(&extra), Err(ProfileError::Toml(_))), "no unknown fields");
        let bad_process = CEMU.replace("\"cemu\"]", "\"/usr/bin/cemu\"]");
        assert!(matches!(Profile::from_toml(&bad_process), Err(ProfileError::Field(_))));
    }
}
```

- [x] **Step 3: Run** `cargo test -p leemusync-core` → compile errors.
- [x] **Step 4: Implement `account.rs`** (above its tests):

```rust
/// Value of the first `key=value` line whose key is exactly `key`.
#[must_use]
pub fn key_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k == key).then(|| v.trim_end_matches('\r'))
    })
}

/// Cemu `MiiName`: exactly 44 hex digits = 11 UTF-16 code units (big-endian), cut at the first NUL.
#[must_use]
pub fn decode_mii_name(hex: &str) -> Option<String> {
    if hex.len() != 44 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let units: Vec<u16> = (0..11)
        .map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    String::from_utf16(&units[..end]).ok()
}
```

- [x] **Step 5: Implement `profile.rs`** (above its tests):

```rust
use std::collections::BTreeMap;

use serde::Deserialize;

const CAPTURES: [&str; 3] = ["title_id", "account", "game"];
pub const ALLOWED_VARS: [&str; 8] = ["base", "mlc", "title_id", "account", "home", "xdg_data", "xdg_config", "exe_dir"];
const DEFAULT_MAX_FILE_MB: u32 = 64;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile TOML: {0}")]
    Toml(String),
    #[error("profile field is invalid: {0}")]
    Field(&'static str),
    #[error("profile uses an unsupported value: {0}")]
    Unsupported(String),
    #[error("unknown template variable `{0}`")]
    UnknownVar(String),
    #[error("template variable `{0}` has no value here")]
    MissingVar(String),
    #[error("malformed template or pattern")]
    Malformed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub systems: Vec<String>,
    pub layout: String,
    pub requires_closed: bool,
    pub platforms: BTreeMap<String, Platform>,
    #[serde(default)]
    pub config_keys: Vec<ConfigKey>,
    pub saves: Saves,
    pub accounts: Option<Accounts>,
    pub game_key: GameKey,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub roots: Vec<String>,
    pub config_roots: Option<Vec<String>>,
    pub process: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigKey {
    pub file: String,
    pub format: String,
    pub key: String,
    pub var: String,
    pub default: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saves {
    pub root: String,
    pub pattern: String,
    #[serde(default)]
    pub shared: Vec<String>,
    pub max_file_mb: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    pub root: String,
    pub id_regex: String,
    pub name_file: String,
    pub name_key: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameKey {
    pub from: String,
    pub normalize: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternPart {
    Literal(String),
    Var(String),
}

impl Profile {
    /// # Errors
    /// `Toml` for syntax or unknown fields, else the first validation rule broken.
    pub fn from_toml(text: &str) -> Result<Self, ProfileError> {
        let p: Self = toml::from_str(text).map_err(|e| ProfileError::Toml(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    #[must_use]
    pub fn max_file_bytes(&self) -> u64 {
        u64::from(self.saves.max_file_mb.unwrap_or(DEFAULT_MAX_FILE_MB)) * 1024 * 1024
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.schema != 1 {
            return Err(ProfileError::Unsupported(format!("schema {}", self.schema)));
        }
        if self.id.is_empty() || !self.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(ProfileError::Field("id"));
        }
        if self.layout != "per_game_user_dir" {
            return Err(ProfileError::Unsupported(self.layout.clone()));
        }
        if self.saves.max_file_mb.is_some_and(|mb| mb == 0 || mb > 4096) {
            return Err(ProfileError::Field("saves.max_file_mb"));
        }
        for platform in self.platforms.values() {
            for t in platform.roots.iter().chain(platform.config_roots.iter().flatten()) {
                check_template(t)?;
            }
            if platform.config_roots.as_ref().is_some_and(|c| c.len() != platform.roots.len()) {
                return Err(ProfileError::Field("config_roots"));
            }
            if platform.process.iter().any(|p| p.is_empty() || p.contains(['/', '\\'])) {
                return Err(ProfileError::Field("process"));
            }
        }
        for key in &self.config_keys {
            check_template(&key.default)?;
            if key.format != "xml" || key.file.contains(['/', '\\']) || key.file.starts_with('.') {
                return Err(ProfileError::Field("config_keys"));
            }
        }
        check_template(&self.saves.root)?;
        let parts = parse_pattern(&self.saves.pattern)?;
        for needed in ["title_id", "account"] {
            if !parts.contains(&PatternPart::Var(needed.into())) {
                return Err(ProfileError::Field("saves.pattern"));
            }
        }
        for shared in &self.saves.shared {
            parse_pattern(shared)?;
        }
        if let Some(acc) = &self.accounts {
            check_template(&acc.root)?;
            if acc.id_regex.len() > 200 || acc.name_file.contains(['/', '\\']) {
                return Err(ProfileError::Field("accounts"));
            }
        }
        if self.game_key.from != "title_id" || self.game_key.normalize.iter().any(|n| n != "lowercase") {
            return Err(ProfileError::Field("game_key"));
        }
        Ok(())
    }
}

fn is_allowed(name: &str) -> bool {
    ALLOWED_VARS.contains(&name)
        || name.strip_prefix("flatpak:").is_some_and(|app| {
            !app.is_empty() && app.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

fn check_template(t: &str) -> Result<(), ProfileError> {
    expand_with(t, |_| Ok(String::new())).map(|_| ())
}

/// Replaces every `{name}` in `template` with its value from `vars`.
///
/// # Errors
/// `UnknownVar` for a name outside `ALLOWED_VARS` and `flatpak:<app id>`,
/// `MissingVar` when `vars` has no value, `Malformed` for unbalanced braces.
pub fn expand(template: &str, vars: &BTreeMap<String, String>) -> Result<String, ProfileError> {
    expand_with(template, |name| vars.get(name).cloned().ok_or_else(|| ProfileError::MissingVar(name.to_owned())))
}

fn expand_with(
    template: &str,
    mut value: impl FnMut(&str) -> Result<String, ProfileError>,
) -> Result<String, ProfileError> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or(ProfileError::Malformed)?;
        let name = &after[..end];
        if name.contains('{') {
            return Err(ProfileError::Malformed);
        }
        if !is_allowed(name) {
            return Err(ProfileError::UnknownVar(name.to_owned()));
        }
        out.push_str(&value(name)?);
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(ProfileError::Malformed);
    }
    out.push_str(rest);
    Ok(out)
}

/// Splits a relative save pattern into literal and `{var}` components.
///
/// # Errors
/// `Malformed` for absolute paths, `..`, empty components or a variable that isn't a whole component,
/// `UnknownVar` for a variable that is not a capture (`title_id`, `account`, `game`): path variables
/// such as `{home}` expand to absolute paths and would escape the save root.
pub fn parse_pattern(pattern: &str) -> Result<Vec<PatternPart>, ProfileError> {
    pattern
        .split('/')
        .map(|c| {
            if let Some(name) = c.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                if CAPTURES.contains(&name) {
                    Ok(PatternPart::Var(name.to_owned()))
                } else {
                    Err(ProfileError::UnknownVar(name.to_owned()))
                }
            } else if c.is_empty() || c == "." || c == ".." || c.contains(['{', '}', '\\', ':']) {
                Err(ProfileError::Malformed)
            } else {
                Ok(PatternPart::Literal(c.to_owned()))
            }
        })
        .collect()
}
```

- [x] **Step 6: Wire and run.** `lib.rs`: `pub mod account; pub mod profile;`. Run `cargo test -p leemusync-core && cargo clippy -p leemusync-core --all-targets -- -D warnings` → PASS.
- [x] **Step 7: Commit.** Update `docs/specs/emulator-profiles.md`: the Cemu row in §Linux paths now says the profile is loaded (no longer "not tested"); **Code:** `` `crates/core/src/profile.rs` ``. Remove "No loader exists yet" from `profiles/fixtures/cemu/flatpak-botw-two-accounts/expected.toml` (it gets loaded in Task 12). `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/core profiles docs/specs/emulator-profiles.md
git commit -m "feat(core): load and validate emulator profiles and Cemu account names

Profiles gain config_roots (settings.xml lives outside the data folder),
a default for config keys, and max_file_mb.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 6: `crypto` primitives, key wrapping and the recovery key

Spec: [security §Cryptography, §Keys](../../specs/security.md#cryptography) (with Task 1's KDF bounds and recovery-key text). Skill: `leemusync-security`.

**Files:**
- Modify: `Cargo.toml` (`chacha20poly1305 = "0.10.1"`, `argon2 = { version = "0.5.3", default-features = false, features = ["alloc", "zeroize"] }`, `blake3 = "1.8.7"`, `zeroize = { version = "1.9.1", features = ["derive"] }`, `getrandom = "0.4.3"` in `[workspace.dependencies]`), `crates/crypto/Cargo.toml` (all five as `{ workspace = true }`, plus `thiserror`)
- Modify: `crates/crypto/src/lib.rs`

**Interfaces:**
- Produces (all in `leemusync_crypto`):
  - `Key` (32 bytes, zeroized on drop, `Debug` prints `Key(..)`): `Key::from_bytes([u8; 32])`, `Key::as_bytes(&self) -> &[u8; 32]`, `Key::random() -> Result<Key, CryptoError>`.
  - `random_bytes<const N: usize>() -> Result<[u8; N], CryptoError>`.
  - `derive(master: &Key, context: &str) -> Key`; `keyed_hash(key: &Key, data: &[u8]) -> [u8; 32]`.
  - `seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>`; `open(key: &Key, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError>`. Object layout: `b"LSE1"` ‖ 24-byte nonce ‖ ciphertext with tag.
  - `KdfParams { m_kib: u32, t: u32, p: u32 }`, `KdfParams::DEFAULT`, `KdfParams::check(&self) -> Result<(), CryptoError>`; `passphrase_kek(passphrase: &[u8], salt: &[u8; 16], params: &KdfParams) -> Result<Key, CryptoError>`.
  - `RepoKeys { object, blob_id, stream_id }` with `RepoKeys::from_master(&Key) -> RepoKeys`.
  - `wrap(kek: &Key, aad: &[u8], master: &Key) -> Result<Vec<u8>, CryptoError>`, `unwrap_key(kek: &Key, aad: &[u8], wrapped: &[u8]) -> Result<Key, CryptoError>`.
  - `RecoveryKey`: `RecoveryKey::generate() -> Result<RecoveryKey, CryptoError>`, `to_display(&self) -> Zeroizing<String>` (re-exported `zeroize::Zeroizing`; 13 groups of 4 Crockford base32 characters, `-` separated), `RecoveryKey::parse(&str) -> Result<RecoveryKey, CryptoError>`, `kek(&self) -> Key`.
  - `CryptoError::{Random, Encrypt, Decrypt, Malformed, KdfParams, Kdf, RecoveryKey}`.
  - Constants `OBJECT_CONTEXT = "leemusync v1 object"`, `BLOB_ID_CONTEXT = "leemusync v1 blob-id"`, `STREAM_ID_CONTEXT = "leemusync v1 stream-id"`, `RECOVERY_CONTEXT = "leemusync v1 recovery-kek"`.

- [x] **Step 1: Failing tests** at the bottom of `crates/crypto/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    #[test]
    fn seal_open_round_trip_and_tamper_detection() {
        let key = Key::from_bytes([7; 32]);
        let sealed = seal(&key, b"blob\0abc", b"save data").unwrap();
        assert_eq!(&sealed[..4], b"LSE1");
        assert_eq!(open(&key, b"blob\0abc", &sealed).unwrap(), b"save data");
        assert_eq!(
            open(&key, b"blob\0abd", &sealed),
            Err(CryptoError::Decrypt),
            "AAD binds the id"
        );
        assert_eq!(
            open(&Key::from_bytes([8; 32]), b"blob\0abc", &sealed),
            Err(CryptoError::Decrypt)
        );
        let mut flipped = sealed.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert_eq!(
            open(&key, b"blob\0abc", &flipped),
            Err(CryptoError::Decrypt)
        );
        assert_eq!(
            open(&key, b"blob\0abc", b"LSE1short"),
            Err(CryptoError::Malformed)
        );
        assert_eq!(
            open(&key, b"blob\0abc", &[b'X'; 64]),
            Err(CryptoError::Malformed)
        );
    }

    #[test]
    fn every_seal_uses_a_fresh_nonce() {
        let key = Key::from_bytes([7; 32]);
        assert_ne!(
            seal(&key, b"", b"x").unwrap(),
            seal(&key, b"", b"x").unwrap()
        );
    }

    #[test]
    fn subkeys_differ_and_are_stable() {
        let master = Key::from_bytes([1; 32]);
        let keys = RepoKeys::from_master(&master);
        assert_ne!(keys.object.as_bytes(), keys.blob_id.as_bytes());
        assert_ne!(keys.blob_id.as_bytes(), keys.stream_id.as_bytes());
        assert_eq!(
            RepoKeys::from_master(&master).object.as_bytes(),
            keys.object.as_bytes()
        );
        assert_eq!(
            keyed_hash(&keys.blob_id, b"a"),
            keyed_hash(&keys.blob_id, b"a")
        );
        assert_ne!(
            keyed_hash(&keys.blob_id, b"a"),
            keyed_hash(&keys.stream_id, b"a")
        );
    }

    #[test]
    fn argon2id_matches_rfc_9106() {
        // RFC 9106 section 5.3 (Argon2id): checks the dependency is wired as Argon2id v0x13.
        let params = argon2::ParamsBuilder::new()
            .m_cost(32)
            .t_cost(3)
            .p_cost(4)
            .data(argon2::AssociatedData::new(&[4; 12]).unwrap())
            .output_len(32)
            .build()
            .unwrap();
        let a = argon2::Argon2::new_with_secret(
            &[3; 8],
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        )
        .unwrap();
        let mut out = [0u8; 32];
        a.hash_password_into(&[1; 32], &[2; 16], &mut out).unwrap();
        let hex = out.iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });
        assert_eq!(
            hex,
            "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
        );
    }

    #[test]
    fn passphrase_kek_is_deterministic_and_bounded() {
        let salt = [9; 16];
        let small = KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let a = passphrase_kek(b"correct horse", &salt, &small).unwrap();
        let b = passphrase_kek(b"correct horse", &salt, &small).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        assert_ne!(
            passphrase_kek(b"wrong", &salt, &small).unwrap().as_bytes(),
            a.as_bytes()
        );
        for bad in [
            KdfParams {
                m_kib: 1024,
                t: 3,
                p: 1,
            },
            KdfParams {
                m_kib: 2_000_000,
                t: 3,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 0,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 11,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 3,
                p: 5,
            },
        ] {
            assert_eq!(
                passphrase_kek(b"x", &salt, &bad).err(),
                Some(CryptoError::KdfParams)
            );
        }
        assert!(KdfParams::DEFAULT.check().is_ok());
    }

    #[test]
    fn wrap_unwrap_with_both_keks() {
        let master = Key::random().unwrap();
        let kek = Key::random().unwrap();
        let wrapped = wrap(&kek, b"wrap\0passphrase", &master).unwrap();
        assert_eq!(
            unwrap_key(&kek, b"wrap\0passphrase", &wrapped)
                .unwrap()
                .as_bytes(),
            master.as_bytes()
        );
        assert_eq!(
            unwrap_key(&kek, b"wrap\0recovery", &wrapped).err(),
            Some(CryptoError::Decrypt)
        );
        let recovery = RecoveryKey::generate().unwrap();
        let wrapped = wrap(&recovery.kek(), b"wrap\0recovery", &master).unwrap();
        let typed =
            RecoveryKey::parse(&recovery.to_display().to_lowercase().replace('-', " ")).unwrap();
        assert_eq!(
            unwrap_key(&typed.kek(), b"wrap\0recovery", &wrapped)
                .unwrap()
                .as_bytes(),
            master.as_bytes()
        );
    }

    #[test]
    fn recovery_key_display_and_parse() {
        let r = RecoveryKey::generate().unwrap();
        let text = r.to_display();
        assert_eq!(text.len(), 13 * 4 + 12);
        assert_eq!(text.split('-').count(), 13);
        assert_eq!(
            RecoveryKey::parse(&text).unwrap().kek().as_bytes(),
            r.kek().as_bytes()
        );
        let ambiguous = text.replace('0', "O").replace('1', "l");
        assert_eq!(
            RecoveryKey::parse(&ambiguous).unwrap().kek().as_bytes(),
            r.kek().as_bytes()
        );
        assert_eq!(
            RecoveryKey::parse("ABCD").err(),
            Some(CryptoError::RecoveryKey)
        );
        assert_eq!(
            RecoveryKey::parse(&text.replacen(|c: char| c.is_ascii_alphanumeric(), "U", 1)).err(),
            Some(CryptoError::RecoveryKey)
        );
    }

    #[test]
    fn recovery_key_rejects_padding_bits_and_accepts_aliases() {
        let r = RecoveryKey::generate().unwrap();
        let text = r.to_display();
        // The last character carries 1 key bit and 4 padding bits: '1' sets a padding bit.
        let mut padded = text.clone();
        padded.pop();
        padded.push('1');
        assert_eq!(
            RecoveryKey::parse(&padded).err(),
            Some(CryptoError::RecoveryKey)
        );
        let canonical = text.replace('0', "O").replace('1', "I").to_lowercase();
        assert_eq!(
            RecoveryKey::parse(&canonical).unwrap().kek().as_bytes(),
            r.kek().as_bytes()
        );
        let mixed = text.replace('0', "o").replace('1', "i");
        assert_eq!(
            RecoveryKey::parse(&mixed).unwrap().kek().as_bytes(),
            r.kek().as_bytes()
        );
    }

    #[test]
    fn minimum_length_object_is_an_authentication_failure() {
        let key = Key::from_bytes([7; 32]);
        let mut exact = b"LSE1".to_vec();
        exact.extend_from_slice(&[0; 40]);
        assert_eq!(exact.len(), 44);
        assert_eq!(open(&key, b"", &exact), Err(CryptoError::Decrypt));
    }

    #[test]
    fn keys_never_print_their_bytes() {
        let k = Key::from_bytes([0xab; 32]);
        assert_eq!(format!("{k:?}"), "Key(..)");
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-crypto` → compile errors.
- [x] **Step 3: Implement** (top of `lib.rs`, replacing the stub body but keeping the module doc comment):

```rust
//! Encryption, key derivation and keyed hashing. Pure, no I/O except the OS CSPRNG.
//! Spec: docs/specs/security.md (Cryptography, Keys).

use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use zeroize::Zeroizing;

pub const OBJECT_CONTEXT: &str = "leemusync v1 object";
pub const BLOB_ID_CONTEXT: &str = "leemusync v1 blob-id";
pub const STREAM_ID_CONTEXT: &str = "leemusync v1 stream-id";
pub const RECOVERY_CONTEXT: &str = "leemusync v1 recovery-kek";

const MAGIC: &[u8; 4] = b"LSE1";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const RECOVERY_CHARS: usize = 52; // 256 bits in 5-bit groups, last group padded

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the OS random number generator failed")]
    Random,
    #[error("encryption failed")]
    Encrypt,
    #[error("authentication failed: wrong key or tampered data")]
    Decrypt,
    #[error("not a LeemuSync encrypted object")]
    Malformed,
    #[error("key derivation parameters are out of the allowed range")]
    KdfParams,
    #[error("key derivation failed")]
    Kdf,
    #[error("that recovery key isn't valid")]
    RecoveryKey,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);

impl Key {
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// # Errors
    /// `CryptoError::Random` if the OS CSPRNG fails.
    pub fn random() -> Result<Self, CryptoError> {
        random_bytes().map(Self)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(..)")
    }
}

/// # Errors
/// `CryptoError::Random` if the OS CSPRNG fails.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| CryptoError::Random)?;
    Ok(out)
}

#[must_use]
pub fn derive(master: &Key, context: &str) -> Key {
    Key(blake3::derive_key(context, master.as_bytes()))
}

#[must_use]
pub fn keyed_hash(key: &Key, data: &[u8]) -> [u8; 32] {
    blake3::keyed_hash(key.as_bytes(), data).into()
}

/// Encrypts with a fresh random nonce. Layout: `LSE1` ‖ nonce ‖ ciphertext+tag.
///
/// # Errors
/// `Random` or `Encrypt`.
pub fn seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let nonce: [u8; NONCE_LEN] = random_bytes()?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| CryptoError::Encrypt)?;
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// # Errors
/// `Malformed` for a wrong header or length, `Decrypt` when authentication fails.
pub fn open(key: &Key, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() < MAGIC.len() + NONCE_LEN + TAG_LEN || &sealed[..MAGIC.len()] != MAGIC {
        return Err(CryptoError::Malformed);
    }
    let (nonce, ct) = sealed[MAGIC.len()..].split_at(NONCE_LEN);
    let cipher =
        XChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| CryptoError::Decrypt)?;
    cipher
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| CryptoError::Decrypt)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    pub const DEFAULT: Self = Self {
        m_kib: 65_536,
        t: 3,
        p: 1,
    };

    /// Bounds from security §Cryptography: a tampered `repo.json` can't weaken or exhaust.
    ///
    /// # Errors
    /// `CryptoError::KdfParams` when out of range.
    pub fn check(&self) -> Result<(), CryptoError> {
        let ok = (19_456..=1_048_576).contains(&self.m_kib)
            && (1..=10).contains(&self.t)
            && (1..=4).contains(&self.p);
        if ok {
            Ok(())
        } else {
            Err(CryptoError::KdfParams)
        }
    }
}

/// Argon2id passphrase → key-encryption key.
///
/// # Errors
/// `KdfParams` for out-of-range parameters, `Kdf` if hashing fails.
pub fn passphrase_kek(
    passphrase: &[u8],
    salt: &[u8; 16],
    params: &KdfParams,
) -> Result<Key, CryptoError> {
    params.check()?;
    let p = argon2::Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|_| CryptoError::KdfParams)?;
    let block_count = p.block_count();
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    // Caller-owned memory, wiped on drop (with argon2's `zeroize` feature, H0 and the final block are wiped too).
    let mut blocks = Zeroizing::new(vec![argon2::Block::default(); block_count]);
    argon
        .hash_password_into_with_memory(passphrase, salt, out.as_mut(), blocks.as_mut_slice())
        .map_err(|_| CryptoError::Kdf)?;
    Ok(Key(*out))
}

/// The three subkeys every repo object uses (security §Keys).
pub struct RepoKeys {
    pub object: Key,
    pub blob_id: Key,
    pub stream_id: Key,
}

impl RepoKeys {
    #[must_use]
    pub fn from_master(master: &Key) -> Self {
        Self {
            object: derive(master, OBJECT_CONTEXT),
            blob_id: derive(master, BLOB_ID_CONTEXT),
            stream_id: derive(master, STREAM_ID_CONTEXT),
        }
    }
}

/// # Errors
/// Same as `seal`.
pub fn wrap(kek: &Key, aad: &[u8], master: &Key) -> Result<Vec<u8>, CryptoError> {
    seal(kek, aad, master.as_bytes())
}

/// # Errors
/// Same as `open`, plus `Malformed` if the payload isn't 32 bytes.
pub fn unwrap_key(kek: &Key, aad: &[u8], wrapped: &[u8]) -> Result<Key, CryptoError> {
    let plain = Zeroizing::new(open(kek, aad, wrapped)?);
    let bytes: Zeroizing<[u8; 32]> = Zeroizing::new(
        plain
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::Malformed)?,
    );
    Ok(Key(*bytes))
}

/// 32 random bytes the user writes down at setup.
pub struct RecoveryKey(Key);

impl RecoveryKey {
    /// # Errors
    /// `CryptoError::Random`.
    pub fn generate() -> Result<Self, CryptoError> {
        Key::random().map(Self)
    }

    #[must_use]
    pub fn kek(&self) -> Key {
        derive(&self.0, RECOVERY_CONTEXT)
    }

    #[must_use]
    pub fn to_display(&self) -> Zeroizing<String> {
        let bits = self.0.as_bytes();
        let mut text = Zeroizing::new(String::with_capacity(RECOVERY_CHARS + RECOVERY_CHARS / 4));
        for i in 0..RECOVERY_CHARS {
            if i > 0 && i % 4 == 0 {
                text.push('-');
            }
            let mut v = 0usize;
            for b in 0..5 {
                let bit = i * 5 + b;
                let set = bit < 256 && bits[bit / 8] & (0x80 >> (bit % 8)) != 0;
                v = (v << 1) | usize::from(set);
            }
            text.push(char::from(CROCKFORD[v]));
        }
        text
    }

    /// Accepts any case, spaces and dashes, and Crockford's `O`→`0`, `I`/`L`→`1`.
    ///
    /// # Errors
    /// `CryptoError::RecoveryKey`.
    pub fn parse(text: &str) -> Result<Self, CryptoError> {
        let digits: Zeroizing<Vec<usize>> = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| {
                let c = match c.to_ascii_uppercase() {
                    'O' => '0',
                    'I' | 'L' => '1',
                    other => other,
                };
                CROCKFORD
                    .iter()
                    .position(|d| char::from(*d) == c)
                    .ok_or(CryptoError::RecoveryKey)
            })
            .collect::<Result<Vec<usize>, _>>()
            .map(Zeroizing::new)?;
        if digits.len() != RECOVERY_CHARS {
            return Err(CryptoError::RecoveryKey);
        }
        let mut out = Zeroizing::new([0u8; 32]);
        for (i, d) in digits.iter().enumerate() {
            for b in 0..5 {
                let bit = i * 5 + b;
                let set = d & (0x10 >> b) != 0;
                if bit >= 256 {
                    if set {
                        return Err(CryptoError::RecoveryKey);
                    }
                } else if set {
                    out[bit / 8] |= 0x80 >> (bit % 8);
                }
            }
        }
        Ok(Self(Key(*out)))
    }
}
```

Note: the last recovery test replaces one character with `U`, which isn't in the Crockford alphabet, so `parse` fails. That's how a mistyped key is caught.

- [x] **Step 4: Run** `cargo test -p leemusync-crypto && cargo clippy -p leemusync-crypto --all-targets -- -D warnings` → PASS. (The `argon2id_matches_rfc_9106` vector was checked against `argon2` 0.5.3 on 2026-10-18.)
- [x] **Step 5: Commit.** `docs/specs/security.md` **Code:** `` `crates/crypto/src/lib.rs` ``; `just check`; then:

```bash
git add Cargo.toml Cargo.lock crates/crypto docs/specs/security.md
git commit -m "feat(crypto): add sealing, key wrapping and the recovery key

XChaCha20-Poly1305 with AAD, Argon2id with bounded parameters, BLAKE3
subkeys and a Crockford base32 recovery key.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 7: `store` core: `Store`, error classes, redaction, `fs`, conformance suite

Spec: [storage-backends §Interface](../../specs/storage-backends.md#interface) (as updated in Task 1), [security §Credentials](../../specs/security.md#credentials). Skills: `leemusync-backend`, `leemusync-security`.

**Files:**
- Modify: `Cargo.toml` (`tokio = "1.53.3"`, `opendal = { version = "0.59.5", default-features = false, features = ["services-gdrive", "services-fs", "executors-tokio"] }`, `tempfile = "3.27.0"` in `[workspace.dependencies]`)
- Modify: `crates/store/Cargo.toml`:

```toml
[dependencies]
opendal = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true, features = ["sync", "time", "net", "io-util"] }

[dev-dependencies]
tempfile = { workspace = true }
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

- Modify: `crates/store/src/lib.rs`
- Create: `crates/store/src/error.rs`, `crates/store/src/redact.rs`, `crates/store/tests/conformance.rs`

**Interfaces:**
- Produces:
  - `Store` (`Clone`, `Send + Sync`): `Store::fs(root: &Path) -> Result<Store, StoreError>`; `async get(&self, path: &str) -> Result<Vec<u8>, StoreError>`; `async put(&self, path: &str, bytes: Vec<u8>, mode: PutMode) -> Result<(), StoreError>`; `async list(&self, prefix: &str) -> Result<Vec<Entry>, StoreError>` (files directly under `prefix`, which must end with `/`, else `Permanent`; the root `""` can't be listed; a missing prefix gives an empty list); `async stat(&self, path: &str) -> Result<Option<Entry>, StoreError>`; `async delete(&self, path: &str) -> Result<(), StoreError>` (idempotent; a path ending in `/` is `Permanent`); `capabilities(&self) -> Capabilities`.
  - `PutMode::{CreateOnly, Overwrite}`; `Entry { path: String, size: u64 }` (`path` relative to the store root, `size` may be 0 on Drive); `Capabilities { native_create_only: bool }`.
  - `StoreError` with `class(&self) -> ErrorClass` and `message(&self) -> &str` (always redacted); `ErrorClass::{NotFound, AlreadyExists, Transient, Auth, Quota, Permanent}`.
  - `redact(text: &str) -> String`.
- Object paths are validated: ASCII `[a-z0-9._/-]`, no leading `/`, no `..`, ≤ 512 bytes. Anything else is `Permanent` before any I/O.

- [x] **Step 1: Failing unit tests.** `crates/store/src/redact.rs`:

```rust
//! Scrubs OAuth secrets out of any text before it can reach output or logs (V-SEC-4).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_the_opendal_token_url_from_the_spec() {
        let text = "Unexpected (temporary) at stat, context: { url: https://oauth2.googleapis.com/token?refresh_token=FAKE-REFRESH-TOKEN&client_id=FAKE-CLIENT-ID&client_secret=FAKE-CLIENT-SECRET&grant_type=refresh_token, called: reqwest::send, service: gdrive, path: a } => send http request, source: error sending request";
        let out = redact(text);
        assert!(!out.contains("FAKE-REFRESH-TOKEN"));
        assert!(!out.contains("FAKE-CLIENT-SECRET"));
        assert!(
            out.contains("client_id=FAKE-CLIENT-ID"),
            "client ids aren't secret"
        );
        assert!(out.contains("grant_type=refresh_token"));
    }

    #[test]
    fn scrubs_json_bodies_and_bearer_headers() {
        let out = redact(
            r#"{"access_token": "ya29.SECRET", "expires_in": 3599, "refresh_token":"1//SECRET2"}"#,
        );
        assert!(!out.contains("SECRET"));
        assert!(out.contains("3599"));
        assert_eq!(
            redact("authorization: Bearer ya29.SECRET rest"),
            "authorization: Bearer REDACTED rest"
        );
    }

    #[test]
    fn leaves_lookalikes_alone() {
        assert_eq!(redact("barcode=1 zipcode=2"), "barcode=1 zipcode=2");
        assert_eq!(redact("code=abc&state=xyz"), "code=REDACTED&state=xyz");
    }

    #[test]
    fn names_and_schemes_match_any_case() {
        assert_eq!(
            redact("Refresh_Token=abc&x=1"),
            "Refresh_Token=REDACTED&x=1"
        );
        assert_eq!(
            redact("Authorization: BEARER abc rest"),
            "Authorization: BEARER REDACTED rest"
        );
        assert_eq!(
            redact("authorization: bearer abc"),
            "authorization: bearer REDACTED"
        );
    }

    #[test]
    fn tolerates_whitespace_around_separators() {
        assert_eq!(
            redact("client_secret: abc, ok"),
            "client_secret: REDACTED, ok"
        );
        assert_eq!(
            redact("client_secret = abc ok"),
            "client_secret = REDACTED ok"
        );
        assert_eq!(
            redact(r#"{"access_token" : "abc", "n": 1}"#),
            r#"{"access_token" : "REDACTED", "n": 1}"#
        );
    }

    #[test]
    fn handles_backslash_escaped_json() {
        let out = redact(r#"body: {\"access_token\":\"ya29.SECRET\",\"expires_in\":3599}"#);
        assert!(!out.contains("SECRET"), "{out}");
        assert!(out.contains("3599"));
    }

    #[test]
    fn scrubs_the_extra_names() {
        for text in [
            "password=SECRET",
            "password: SECRET",
            "token=SECRET",
            r#"{"token":"SECRET"}"#,
            "api_key=SECRET",
            "apikey=SECRET",
            "X-Amz-Signature=SECRET",
            "x-amz-credential=SECRET",
            "x-amz-security-token=SECRET",
            "Authorization: Basic SECRET",
        ] {
            assert!(
                !redact(text).contains("SECRET"),
                "{text} -> {}",
                redact(text)
            );
        }
    }

    #[test]
    fn token_type_and_prose_are_not_secrets() {
        assert_eq!(
            redact(r#"{"token_type":"Bearer"}"#),
            r#"{"token_type":"Bearer"}"#
        );
        assert_eq!(redact("token_type=Bearer&a=1"), "token_type=Bearer&a=1");
        assert_eq!(
            redact("Basic usage: error code: 404"),
            "Basic usage: error code: 404"
        );
        assert_eq!(redact("mytoken=1 x-token=2"), "mytoken=1 x-token=2");
    }
}
```

`crates/store/src/error.rs`:

```rust
//! Store errors, classified for retry and user prompts (storage-backends §Interface).

#[cfg(test)]
mod tests {
    use super::*;
    use opendal::ErrorKind;

    #[test]
    fn classifies_by_kind_then_by_http_status() {
        assert_eq!(
            classify(ErrorKind::NotFound, false, ""),
            ErrorClass::NotFound
        );
        assert_eq!(
            classify(ErrorKind::ConditionNotMatch, false, ""),
            ErrorClass::AlreadyExists
        );
        assert_eq!(
            classify(ErrorKind::PermissionDenied, false, ""),
            ErrorClass::Auth
        );
        assert_eq!(
            classify(ErrorKind::RateLimited, false, ""),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, true, ""),
            ErrorClass::Transient
        );
        // OpenDAL's gdrive service reports a 401 as Unexpected (seen 2026-10-18 with a fake token).
        assert_eq!(
            classify(
                ErrorKind::Unexpected,
                false,
                "response: Parts { status: 401, ..."
            ),
            ErrorClass::Auth
        );
        assert_eq!(
            classify(
                ErrorKind::Unexpected,
                false,
                "status: 403 ... storageQuotaExceeded"
            ),
            ErrorClass::Quota
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, false, "status: 503"),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(ErrorKind::Unexpected, false, "status: 400"),
            ErrorClass::Permanent
        );
    }

    #[test]
    fn messages_are_always_redacted() {
        let e = StoreError::new(ErrorClass::Permanent, "refresh_token=SECRET");
        assert_eq!(e.message(), "refresh_token=REDACTED");
        assert_eq!(e.to_string(), "refresh_token=REDACTED");
    }

    #[test]
    fn provider_errors_are_redacted_on_conversion() {
        let raw = opendal::Error::new(
            ErrorKind::Unexpected,
            "send failed: refresh_token=SECRET&client_secret=SECRET2",
        );
        let e = StoreError::from(raw);
        assert!(!e.message().contains("SECRET"), "{}", e.message());
        assert!(!e.to_string().contains("SECRET"));
    }

    #[test]
    fn drive_403s_arrive_as_permission_denied_but_keep_their_meaning() {
        let denied = ErrorKind::PermissionDenied;
        assert_eq!(
            classify(denied, false, "reason: storageQuotaExceeded"),
            ErrorClass::Quota
        );
        assert_eq!(
            classify(denied, false, "reason: rateLimitExceeded"),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(denied, false, "reason: userRateLimitExceeded"),
            ErrorClass::Transient
        );
        assert_eq!(
            classify(denied, false, "reason: insufficientPermissions"),
            ErrorClass::Auth
        );
        assert_eq!(classify(denied, false, ""), ErrorClass::Auth);
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-store` → compile errors.
- [x] **Step 3: Implement `redact.rs`** (above its tests):

```rust
/// Secret names, lowercase. The flag says whether `name: value` (no quotes) counts; it is off for
/// words that also appear in prose ("error code: 404").
const SECRET_NAMES: [(&str, bool); 14] = [
    ("access_token", true),
    ("refresh_token", true),
    ("client_secret", true),
    ("code", false),
    ("code_verifier", true),
    ("id_token", true),
    ("password", true),
    ("token", false),
    ("api_key", true),
    ("apikey", true),
    ("x-amz-signature", true),
    ("x-amz-credential", true),
    ("x-amz-security-token", true),
    ("signature", false),
];
const VALUE_END: [char; 9] = ['&', ' ', '"', '\'', ',', '}', ')', '\n', '\r'];
const MARK: &str = "REDACTED";

/// Replaces OAuth secret values: `name=value`, `name: value`, JSON `"name": "value"` (also
/// backslash-escaped), `Bearer <token>` and `Authorization: Basic <token>`. Names and schemes are
/// matched without regard to case. `token_type` and words that merely end in a secret name are kept.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for (name, colon_ok) in SECRET_NAMES {
        out = redact_named(&out, name, colon_ok);
    }
    out = redact_scheme(&out, "bearer ", false);
    redact_scheme(&out, "basic ", true)
}

/// True when `at` is not in the middle of a longer identifier.
fn starts_param(text: &str, at: usize) -> bool {
    !text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn skip_blanks(text: &str, from: usize) -> usize {
    from + (text[from..].len() - text[from..].trim_start_matches([' ', '\t']).len())
}

/// Where the secret value starting at `at` begins and ends.
fn value_span(text: &str, at: usize) -> (usize, usize) {
    let rest = &text[at..];
    if let Some(inner) = rest.strip_prefix("\\\"") {
        let start = at + 2;
        let end = [inner.find("\\\""), inner.find('"')]
            .into_iter()
            .flatten()
            .min()
            .map_or(text.len(), |n| start + n);
        (start, end)
    } else if rest.starts_with(['"', '\'']) {
        let quote = &rest[..1];
        let start = at + 1;
        (
            start,
            text[start..].find(quote).map_or(text.len(), |n| start + n),
        )
    } else {
        (at, rest.find(VALUE_END).map_or(text.len(), |n| at + n))
    }
}

fn redact_named(text: &str, name: &str, colon_ok: bool) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(name) {
        let at = search + rel;
        let mut i = at + name.len();
        search = i;
        if !starts_param(text, at) {
            continue;
        }
        let quoted_key = text[i..].starts_with("\\\"") || text[i..].starts_with('"');
        if text[i..].starts_with("\\\"") {
            i += 2;
        } else if quoted_key {
            i += 1;
        }
        i = skip_blanks(text, i);
        let sep_ok = match text[i..].chars().next() {
            Some('=') => true,
            Some(':') => colon_ok || quoted_key,
            _ => false,
        };
        if !sep_ok {
            continue;
        }
        let (start, end) = value_span(text, skip_blanks(text, i + 1));
        if start == end {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(MARK);
        cursor = end;
        search = end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// `scheme` is lowercase with a trailing space. `needs_header` limits the match to the value of an
/// `Authorization:` header, for words that are also plain English ("Basic usage").
fn redact_scheme(text: &str, scheme: &str, needs_header: bool) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(scheme) {
        let at = search + rel;
        let start = at + scheme.len();
        search = start;
        let header_ok = !needs_header || lower[..at].trim_end().ends_with("authorization:");
        if !starts_param(text, at) || !header_ok {
            continue;
        }
        let end = text[start..]
            .find(VALUE_END)
            .map_or(text.len(), |n| start + n);
        if start == end {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(MARK);
        cursor = end;
        search = end;
    }
    out.push_str(&text[cursor..]);
    out
}
```

- [x] **Step 4: Implement `error.rs`** (above its tests):

```rust
use std::fmt;

use opendal::ErrorKind;

use crate::redact::redact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorClass {
    NotFound,
    AlreadyExists,
    /// Retry with backoff.
    Transient,
    /// Ask the user to log in again.
    Auth,
    Quota,
    Permanent,
}

#[derive(Debug, Clone)]
pub struct StoreError {
    class: ErrorClass,
    message: String,
}

impl StoreError {
    #[must_use]
    pub fn new(class: ErrorClass, message: &str) -> Self {
        Self {
            class,
            message: redact(message),
        }
    }

    #[must_use]
    pub fn class(&self) -> ErrorClass {
        self.class
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

impl From<opendal::Error> for StoreError {
    fn from(e: opendal::Error) -> Self {
        let text = e.to_string();
        Self::new(classify(e.kind(), e.is_temporary(), &text), &text)
    }
}

pub(crate) fn classify(kind: ErrorKind, temporary: bool, text: &str) -> ErrorClass {
    match kind {
        ErrorKind::NotFound => ErrorClass::NotFound,
        ErrorKind::ConditionNotMatch | ErrorKind::AlreadyExists => ErrorClass::AlreadyExists,
        // Drive reports its 403s (quota, rate limit, real denials) as PermissionDenied.
        ErrorKind::PermissionDenied if text.contains("storageQuotaExceeded") => ErrorClass::Quota,
        ErrorKind::PermissionDenied if text.contains("ateLimitExceeded") => ErrorClass::Transient,
        ErrorKind::PermissionDenied => ErrorClass::Auth,
        ErrorKind::RateLimited => ErrorClass::Transient,
        _ if temporary => ErrorClass::Transient,
        _ if text.contains("status: 401") => ErrorClass::Auth,
        _ if text.contains("storageQuotaExceeded") || text.contains("status: 507") => {
            ErrorClass::Quota
        }
        _ if text.contains("status: 429") || text.contains("status: 5") => ErrorClass::Transient,
        _ => ErrorClass::Permanent,
    }
}
```

- [x] **Step 5: Failing conformance suite** `crates/store/tests/conformance.rs`:

```rust
//! One suite for every adapter (storage-backends §Conformance suite).
#![allow(clippy::unwrap_used)] // test helper; clippy.toml only exempts `#[test]` fns

use leemusync_store::{Entry, ErrorClass, PutMode, Store};

async fn suite(store: &Store, root: &str) {
    let a = format!("{root}a");
    let b = format!("{root}b");
    store
        .put(&a, b"one".to_vec(), PutMode::CreateOnly)
        .await
        .unwrap();
    let again = store
        .put(&a, b"two".to_vec(), PutMode::CreateOnly)
        .await
        .unwrap_err();
    assert_eq!(
        again.class(),
        ErrorClass::AlreadyExists,
        "create-only never overwrites"
    );
    assert_eq!(store.get(&a).await.unwrap(), b"one");
    assert!(store.stat(&a).await.unwrap().is_some());
    assert!(
        store
            .stat(&format!("{root}missing"))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get(&format!("{root}missing"))
            .await
            .unwrap_err()
            .class(),
        ErrorClass::NotFound
    );
    store
        .put(&b, b"v1".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    store
        .put(&b, b"v2".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    assert_eq!(store.get(&b).await.unwrap(), b"v2");
    let mut names: Vec<String> = store
        .list(root)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    names.sort();
    assert_eq!(names, vec![a.clone(), b.clone()]);
    assert_eq!(
        store.list(&format!("{root}nothing/")).await.unwrap(),
        Vec::<Entry>::new()
    );
    let empty = format!("{root}empty");
    store
        .put(&empty, Vec::new(), PutMode::CreateOnly)
        .await
        .unwrap();
    assert_eq!(store.get(&empty).await.unwrap(), Vec::<u8>::new());
    assert_eq!(store.stat(&empty).await.unwrap().map(|e| e.size), Some(0));
    store.delete(&empty).await.unwrap();
    store.delete(&a).await.unwrap();
    store.delete(&a).await.unwrap();
    assert!(store.stat(&a).await.unwrap().is_none());
    assert!(
        store.get("../escape").await.is_err(),
        "paths are validated before I/O"
    );
    store.delete(&b).await.unwrap();
}

#[tokio::test]
async fn fs_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::fs(dir.path()).unwrap();
    assert!(store.capabilities().native_create_only);
    suite(&store, "conformance/").await;
}
```

- [x] **Step 6: Run** `cargo test -p leemusync-store --test conformance` → compile errors.
- [x] **Step 7: Implement `lib.rs`:**

```rust
//! Storage backend interface and adapters. Moves opaque encrypted objects.
//! Spec: docs/specs/storage-backends.md.

mod error;
mod redact;

use std::path::Path;

use opendal::Operator;
use opendal::services::Fs;

pub use error::{ErrorClass, StoreError};
pub use redact::redact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutMode {
    CreateOnly,
    /// Only leases and `repo.json` (storage-backends §Interface).
    Overwrite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub native_create_only: bool,
}

#[derive(Clone)]
pub struct Store {
    inner: Inner,
    caps: Capabilities,
}

#[derive(Clone)]
enum Inner {
    Plain(Operator),
}

impl Store {
    /// A local folder. Test-grade in Phase 1: V-STORE-5 (crash-safe atomic create) is still open.
    ///
    /// # Errors
    /// `Permanent` if the folder can't be created or opened.
    pub fn fs(root: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(root)
            .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))?;
        let root = root
            .to_str()
            .ok_or_else(|| StoreError::new(ErrorClass::Permanent, "store path is not UTF-8"))?;
        let op = Operator::new(Fs::default().root(root))?;
        let native = op.info().capability().write_with_if_not_exists;
        Ok(Self {
            inner: Inner::Plain(op),
            caps: Capabilities {
                native_create_only: native,
            },
        })
    }

    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        self.caps
    }

    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)] // used by Task 8: the Drive operator is rebuilt when its token expires
    async fn op(&self) -> Result<Operator, StoreError> {
        match &self.inner {
            Inner::Plain(op) => Ok(op.clone()),
        }
    }

    /// # Errors
    /// `NotFound`, or any classified backend error.
    pub async fn get(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        check_path(path)?;
        Ok(self.op().await?.read(path).await?.to_vec())
    }

    /// `CreateOnly` uses the backend's conditional write, or stat-then-write where there is none
    /// (safe because names are unique, D5).
    ///
    /// # Errors
    /// `AlreadyExists` for `CreateOnly` on an existing name, or any classified backend error.
    pub async fn put(&self, path: &str, bytes: Vec<u8>, mode: PutMode) -> Result<(), StoreError> {
        check_path(path)?;
        let op = self.op().await?;
        match mode {
            PutMode::Overwrite => {
                op.write(path, bytes).await?;
            }
            PutMode::CreateOnly if self.caps.native_create_only => {
                op.write_with(path, bytes).if_not_exists(true).await?;
            }
            PutMode::CreateOnly => {
                if op.exists(path).await? {
                    return Err(StoreError::new(ErrorClass::AlreadyExists, path));
                }
                op.write(path, bytes).await?;
            }
        }
        Ok(())
    }

    /// Files directly under `prefix`. The prefix is a valid object path that ends with `/`; the
    /// store root itself (`""`) can't be listed, because the engine only lists named folders.
    ///
    /// # Errors
    /// `Permanent` for a prefix without a trailing `/`. Any classified backend error except `NotFound`, which gives an empty list.
    pub async fn list(&self, prefix: &str) -> Result<Vec<Entry>, StoreError> {
        check_path(prefix)?;
        if !prefix.ends_with('/') {
            return Err(StoreError::new(
                ErrorClass::Permanent,
                "list prefix must end with /",
            ));
        }
        let op = self.op().await?;
        match op.list(prefix).await {
            Ok(entries) => Ok(entries
                .into_iter()
                .filter(|e| e.metadata().is_file())
                .map(|e| Entry {
                    path: e.path().to_owned(),
                    size: e.metadata().content_length(),
                })
                .collect()),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// # Errors
    /// Any classified backend error except `NotFound`, which gives `None`.
    pub async fn stat(&self, path: &str) -> Result<Option<Entry>, StoreError> {
        check_path(path)?;
        match self.op().await?.stat(path).await {
            Ok(meta) => Ok(Some(Entry {
                path: path.to_owned(),
                size: meta.content_length(),
            })),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Idempotent. On Google Drive this moves the file to the trash (V-STORE-9).
    ///
    /// # Errors
    /// `Permanent` for a path ending in `/` (folders aren't objects). Any classified backend error.
    pub async fn delete(&self, path: &str) -> Result<(), StoreError> {
        check_path(path)?;
        if path.ends_with('/') {
            return Err(StoreError::new(
                ErrorClass::Permanent,
                "delete takes an object, not a folder",
            ));
        }
        Ok(self.op().await?.delete(path).await?)
    }
}

fn check_path(path: &str) -> Result<(), StoreError> {
    let ok = !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && path.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'/' | b'-')
        })
        && path
            .strip_suffix('/')
            .unwrap_or(path)
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..");
    if ok {
        Ok(())
    } else {
        Err(StoreError::new(
            ErrorClass::Permanent,
            "invalid object path",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_path_accepts_object_names() {
        for ok in [
            "a",
            "leemusync/streams/ab-cd_1.2/versions/",
            "x".repeat(512).as_str(),
        ] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn check_path_rejects_everything_else() {
        let long = "x".repeat(513);
        for bad in [
            "",
            "/abs",
            "Upper",
            "a/../b",
            "..",
            "a//b",
            "a b",
            "caf\u{e9}",
            long.as_str(),
        ] {
            let e = check_path(bad).unwrap_err();
            assert_eq!(e.class(), ErrorClass::Permanent, "{bad}");
        }
    }

    #[tokio::test]
    async fn list_needs_a_trailing_slash_and_delete_forbids_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::fs(dir.path()).unwrap();
        for bad in ["a", "", "/"] {
            assert_eq!(
                store.list(bad).await.unwrap_err().class(),
                ErrorClass::Permanent,
                "{bad:?}"
            );
        }
        assert_eq!(
            store.delete("a/").await.unwrap_err().class(),
            ErrorClass::Permanent
        );
    }
}
```

Version ids are uppercase Crockford. Object paths must stay lowercase-only, so the engine writes version ids lowercased in paths and parses them case-insensitively (`VersionId::from_str` accepts lowercase, Task 2).

- [x] **Step 8: Run** `cargo test -p leemusync-store && cargo clippy -p leemusync-store --all-targets -- -D warnings` → PASS. If OpenDAL's `delete` on a missing path returns `NotFound` instead of `Ok`, map `NotFound` to `Ok(())` in `delete` and re-run.
- [x] **Step 9: Commit.** `docs/specs/storage-backends.md`: **Code:** `` `crates/store/src/` ``, and §Conformance suite: "`crates/store/tests/conformance.rs`: `fs` runs in CI." `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/store docs/specs/storage-backends.md
git commit -m "feat(store): add the Store type, error classes and redaction

One conformance suite, run on fs in CI.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `store` Google Drive: TLS client, OAuth login and refresh, adapter

Spec: [storage-backends §Google Drive (live)](../../specs/storage-backends.md#google-drive-live), D26, D27, D28, [security §Network](../../specs/security.md#network). Skills: `leemusync-backend`, `leemusync-security`.

**Files:**
- Modify: `Cargo.toml` (`opendal-http-transport-reqwest`, `reqwest`, `rustls`, `rustls-platform-verifier`, `url`, `base64`, `sha2`, `getrandom`, `zeroize`, `serde`, `serde_json` per the table; `keyring` for the dev-dependency)
- Modify: `crates/store/Cargo.toml`:

```toml
[dependencies]
# ...Task 7 entries...
opendal-http-transport-reqwest = { workspace = true }
reqwest = { workspace = true }
rustls = { workspace = true }
rustls-platform-verifier = { workspace = true }
url = { workspace = true }
base64 = { workspace = true }
sha2 = { workspace = true }
getrandom = { workspace = true }
zeroize = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
# ...Task 7 entries...

[target.'cfg(target_os = "linux")'.dev-dependencies]
keyring = { workspace = true, features = ["sync-secret-service", "crypto-rust", "vendored"] }

[target.'cfg(target_os = "macos")'.dev-dependencies]
keyring = { workspace = true, features = ["apple-native"] }

[target.'cfg(windows)'.dev-dependencies]
keyring = { workspace = true, features = ["windows-native"] }
```

- Create: `crates/store/src/http.rs`, `crates/store/src/oauth.rs`, `crates/store/src/gdrive.rs`, `crates/store/examples/gdrive_login.rs`
- Modify: `crates/store/src/lib.rs`, `crates/store/tests/conformance.rs`

**Interfaces:**
- Consumes: Task 7's `Store`, `StoreError`, `ErrorClass`, `redact`.
- Produces:
  - `http::http_client() -> Result<reqwest::Client, StoreError>`: rustls with the ring provider and the platform verifier, HTTPS only, 60 s timeout.
  - `oauth::OAuthClient { client_id: String, client_secret: Option<Zeroizing<String>> }` (its `Debug` hides the secret); `oauth::AccessToken { token: Zeroizing<String>, expires_at_ms: u64 }`; `oauth::Tokens { refresh_token: Zeroizing<String>, access: AccessToken }`; `oauth::start_login(client: &OAuthClient) -> Result<PendingLogin, StoreError>` (async); `PendingLogin::url(&self) -> &str`; `PendingLogin::finish(self, client: &OAuthClient, http: &reqwest::Client) -> Result<Tokens, StoreError>` (async, waits at most 5 minutes); `oauth::refresh(client: &OAuthClient, http: &reqwest::Client, refresh_token: &str) -> Result<AccessToken, StoreError>` (async).
  - `gdrive::GdriveConfig { root: String, client: OAuthClient, refresh_token: Zeroizing<String> }`; `Store::gdrive(config: GdriveConfig) -> Result<Store, StoreError>` (capability `native_create_only: false`).

- [x] **Step 1: Failing tests.** `crates/store/src/oauth.rs`:

```rust
//! Google OAuth for Drive (`drive.file`): authorization code + PKCE with a loopback redirect (D28),
//! and token refresh with a form body so secrets never enter a URL (D27).

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> OAuthClient {
        OAuthClient { client_id: "id.apps.googleusercontent.com".into(), client_secret: Some(Zeroizing::new("s3cret".into())) }
    }

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        // RFC 7636 appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[tokio::test]
    async fn login_url_asks_for_drive_file_only_and_offline_access() {
        let pending = start_login(&client()).await.unwrap();
        let url = url::Url::parse(pending.url()).unwrap();
        let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], "https://www.googleapis.com/auth/drive.file");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["access_type"], "offline");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(!pending.url().contains("s3cret"), "the secret never goes in the browser URL");
    }

    #[tokio::test]
    async fn the_listener_rejects_a_wrong_state() {
        let pending = start_login(&client()).await.unwrap();
        let redirect = pending.redirect.clone();
        let hit = tokio::spawn(async move {
            let url = url::Url::parse(&redirect).unwrap();
            let mut s = tokio::net::TcpStream::connect((url.host_str().unwrap(), url.port().unwrap())).await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            s.write_all(b"GET /?code=abc&state=WRONG HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf).await;
        });
        let http = crate::http::http_client().unwrap();
        let err = pending.finish(&client(), &http).await.unwrap_err();
        assert_eq!(err.class(), ErrorClass::Auth);
        hit.await.unwrap();
    }

    #[test]
    fn debug_never_shows_secrets() {
        let text = format!("{:?}", client());
        assert!(!text.contains("s3cret"));
    }
}
```

`crates/store/src/http.rs`:

```rust
//! The one HTTP client: rustls (ring) + the OS certificate verifier (security §Network).

#[cfg(test)]
mod tests {
    #[test]
    fn builds() {
        assert!(super::http_client().is_ok());
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-store` → compile errors.
- [x] **Step 3: Implement `http.rs`** (above its tests):

```rust
use std::sync::Arc;
use std::time::Duration;

use rustls_platform_verifier::BuilderVerifierExt;

use crate::{ErrorClass, StoreError};

/// # Errors
/// `Permanent` if TLS or the client can't be set up.
pub fn http_client() -> Result<reqwest::Client, StoreError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(BuilderVerifierExt::with_platform_verifier)
        .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))?
        .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("leemusync/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))
}
```

- [x] **Step 4: Implement `oauth.rs`** (above its tests):

```rust
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zeroize::Zeroizing;

use crate::{ErrorClass, StoreError};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_REQUEST: usize = 8 * 1024;
const DONE_PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><meta charset=utf-8><title>LeemuSync</title><p>LeemuSync: you can close this tab.</p><p>LeemuSync: ya puedes cerrar esta pestaña.</p>";

pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<Zeroizing<String>>,
}

impl fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClient").field("client_id", &self.client_id).finish_non_exhaustive()
    }
}

pub struct AccessToken {
    pub token: Zeroizing<String>,
    pub expires_at_ms: u64,
}

pub struct Tokens {
    pub refresh_token: Zeroizing<String>,
    pub access: AccessToken,
}

pub struct PendingLogin {
    listener: TcpListener,
    state: String,
    verifier: Zeroizing<String>,
    pub(crate) redirect: String,
    url: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

fn store_err(class: ErrorClass, text: &str) -> StoreError {
    StoreError::new(class, text)
}

fn random_b64(n: usize) -> Result<String, StoreError> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|_| store_err(ErrorClass::Permanent, "random number generator failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Binds `127.0.0.1:<random>` and builds the consent URL. Open the URL, then call `finish`.
///
/// # Errors
/// `Permanent` if the loopback port can't be bound.
pub async fn start_login(client: &OAuthClient) -> Result<PendingLogin, StoreError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
    let port = listener.local_addr().map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let state = random_b64(16)?;
    let verifier = Zeroizing::new(random_b64(32)?);
    let url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", challenge(&verifier).as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?
    .to_string();
    Ok(PendingLogin { listener, state, verifier, redirect, url })
}

impl PendingLogin {
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Accepts exactly one redirect (5 minutes at most), then exchanges the code.
    ///
    /// # Errors
    /// `Auth` for a timeout, a wrong `state`, a refusal or a missing refresh token; else classified HTTP errors.
    pub async fn finish(self, client: &OAuthClient, http: &reqwest::Client) -> Result<Tokens, StoreError> {
        let accepted = tokio::time::timeout(LOGIN_TIMEOUT, self.listener.accept())
            .await
            .map_err(|_| store_err(ErrorClass::Auth, "login timed out"))?
            .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
        drop(self.listener);
        let (mut socket, _) = accepted;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST {
            let n = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .map_err(|_| store_err(ErrorClass::Auth, "login redirect timed out"))?
                .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let _ = socket.write_all(DONE_PAGE.as_bytes()).await;
        let _ = socket.shutdown().await;
        let request = String::from_utf8_lossy(&buf);
        let target = request
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("GET "))
            .and_then(|l| l.split(' ').next())
            .ok_or_else(|| store_err(ErrorClass::Auth, "unexpected login redirect"))?;
        let url = url::Url::parse(&format!("{}{target}", self.redirect))
            .map_err(|_| store_err(ErrorClass::Auth, "unexpected login redirect"))?;
        let mut code = None;
        let mut state = None;
        for (k, v) in url.query_pairs() {
            match k.as_ref() {
                "code" => code = Some(Zeroizing::new(v.into_owned())),
                "state" => state = Some(v.into_owned()),
                "error" => return Err(store_err(ErrorClass::Auth, &format!("Google refused the login: {v}"))),
                _ => {}
            }
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return Err(store_err(ErrorClass::Auth, "login state didn't match"));
        }
        let code = code.ok_or_else(|| store_err(ErrorClass::Auth, "login redirect had no code"))?;
        let mut form = vec![
            ("code", code.as_str()),
            ("client_id", client.client_id.as_str()),
            ("code_verifier", self.verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", self.redirect.as_str()),
        ];
        if let Some(secret) = &client.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        let response = post_token(http, &form).await?;
        let refresh = response
            .refresh_token
            .ok_or_else(|| store_err(ErrorClass::Auth, "Google returned no refresh token"))?;
        Ok(Tokens {
            refresh_token: Zeroizing::new(refresh),
            access: AccessToken {
                token: Zeroizing::new(response.access_token),
                expires_at_ms: now_ms() + response.expires_in * 1000,
            },
        })
    }
}

/// # Errors
/// `Auth` when Google rejects the refresh token (`invalid_grant`, e.g. the 7-day Testing expiry).
pub async fn refresh(client: &OAuthClient, http: &reqwest::Client, refresh_token: &str) -> Result<AccessToken, StoreError> {
    let mut form = vec![
        ("refresh_token", refresh_token),
        ("client_id", client.client_id.as_str()),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    let response = post_token(http, &form).await?;
    Ok(AccessToken {
        token: Zeroizing::new(response.access_token),
        expires_at_ms: now_ms() + response.expires_in * 1000,
    })
}

async fn post_token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse, StoreError> {
    let response = http
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| store_err(if e.is_timeout() || e.is_connect() { ErrorClass::Transient } else { ErrorClass::Permanent }, &e.to_string()))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| store_err(ErrorClass::Transient, &e.to_string()))?;
    if !status.is_success() {
        let class = match status.as_u16() {
            400 | 401 => ErrorClass::Auth,
            429 | 500..=599 => ErrorClass::Transient,
            _ => ErrorClass::Permanent,
        };
        return Err(store_err(class, &format!("token endpoint returned {status}: {body}")));
    }
    serde_json::from_str(&body).map_err(|_| store_err(ErrorClass::Permanent, "token endpoint returned unexpected JSON"))
}
```

- [x] **Step 5: Implement `gdrive.rs`:**

```rust
//! Google Drive through OpenDAL with an access token only; the store refreshes it (D27).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opendal::services::Gdrive;
use opendal::{HttpTransporter, OperationContext, Operator};
use opendal_http_transport_reqwest::ReqwestTransport;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::oauth::{self, OAuthClient};
use crate::StoreError;

const REFRESH_MARGIN_MS: u64 = 60_000;

pub struct GdriveConfig {
    /// Folder inside "My Drive", e.g. `/`. LeemuSync's own objects live under `leemusync/`.
    pub root: String,
    pub client: OAuthClient,
    pub refresh_token: Zeroizing<String>,
}

pub(crate) struct GdriveSession {
    config: GdriveConfig,
    http: reqwest::Client,
    current: Mutex<Option<(Operator, u64)>>,
}

impl GdriveSession {
    pub(crate) fn new(config: GdriveConfig, http: reqwest::Client) -> Arc<Self> {
        Arc::new(Self { config, http, current: Mutex::new(None) })
    }

    pub(crate) async fn operator(&self) -> Result<Operator, StoreError> {
        let mut current = self.current.lock().await;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        if let Some((op, expires)) = current.as_ref() {
            if *expires > now + REFRESH_MARGIN_MS {
                return Ok(op.clone());
            }
        }
        let access = oauth::refresh(&self.config.client, &self.http, &self.config.refresh_token).await?;
        let op = Operator::new(Gdrive::default().root(&self.config.root).access_token(&access.token))?;
        let transport = HttpTransporter::new(ReqwestTransport::new(self.http.clone()));
        let op = op.with_context(OperationContext::new().with_http_transport(transport));
        *current = Some((op.clone(), access.expires_at_ms));
        Ok(op)
    }
}
```

In `lib.rs`: add `pub mod http; pub mod oauth; mod gdrive; pub use gdrive::GdriveConfig;`, a variant `Gdrive(Arc<gdrive::GdriveSession>)` in `Inner`, the `op()` arm `Inner::Gdrive(s) => s.operator().await`, and:

```rust
impl Store {
    /// Google Drive with the user's OAuth client and refresh token (D26, D27).
    ///
    /// # Errors
    /// `Permanent` if the HTTP client can't be built.
    pub fn gdrive(config: GdriveConfig) -> Result<Self, StoreError> {
        let http = http::http_client()?;
        Ok(Self {
            inner: Inner::Gdrive(gdrive::GdriveSession::new(config, http)),
            caps: Capabilities { native_create_only: false },
        })
    }
}
```

- [x] **Step 6: The live test and its login helper.** `crates/store/examples/gdrive_login.rs` logs in once and keeps the tokens in the keystore. It never prints them:

```rust
//! One-time login for the live Drive test. Stores the refresh token and client secret in the OS
//! keystore (service `leemusync-test`). Usage:
//!   LEEMUSYNC_TEST_GDRIVE_CLIENT_ID=<id> cargo run -p leemusync-store --example gdrive_login

use std::io::BufRead as _;

use leemusync_store::oauth::{self, OAuthClient};
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = std::env::var("LEEMUSYNC_TEST_GDRIVE_CLIENT_ID")?;
    eprintln!("Paste the OAuth client secret and press Enter (it is not echoed back):");
    let mut secret = Zeroizing::new(String::new());
    std::io::stdin().lock().read_line(&mut secret)?;
    let secret = Zeroizing::new(secret.trim().to_owned());
    let client = OAuthClient { client_id, client_secret: Some(secret.clone()) };
    let http = leemusync_store::http::http_client()?;
    let pending = oauth::start_login(&client).await?;
    eprintln!("Open this URL in your browser:\n{}", pending.url());
    let tokens = pending.finish(&client, &http).await?;
    keyring::Entry::new("leemusync-test", "gdrive-refresh")?.set_password(&tokens.refresh_token)?;
    keyring::Entry::new("leemusync-test", "gdrive-client-secret")?.set_password(&secret)?;
    eprintln!("Saved to the keystore. Run: cargo test -p leemusync-store --test conformance -- --ignored");
    Ok(())
}
```

Add `tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }` and `zeroize` to store `[dev-dependencies]` if they aren't there. Append to `tests/conformance.rs`:

```rust
#[tokio::test]
#[ignore = "live Google Drive: run `cargo run -p leemusync-store --example gdrive_login` first"]
async fn gdrive_conformance() {
    use leemusync_store::{GdriveConfig, oauth::OAuthClient};
    use zeroize::Zeroizing;
    let client_id = std::env::var("LEEMUSYNC_TEST_GDRIVE_CLIENT_ID").unwrap();
    let secret = keyring::Entry::new("leemusync-test", "gdrive-client-secret").unwrap().get_password().unwrap();
    let refresh = keyring::Entry::new("leemusync-test", "gdrive-refresh").unwrap().get_password().unwrap();
    let store = Store::gdrive(GdriveConfig {
        root: "/".into(),
        client: OAuthClient { client_id, client_secret: Some(Zeroizing::new(secret)) },
        refresh_token: Zeroizing::new(refresh),
    })
    .unwrap();
    let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    suite(&store, &format!("leemusync-conformance/{run}/")).await;
}
```

- [x] **Step 7: Run** `cargo test -p leemusync-store && cargo clippy -p leemusync-store --all-targets -- -D warnings` → PASS (the live test stays ignored). Then check TLS: `cargo tree -i aws-lc-sys -e normal` and `cargo tree -i openssl-sys -e normal` must both print `did not match any packages`.
- [ ] **Step 8: Live run (maintainer, once).** With the maintainer's Desktop OAuth client (SP6 setup, [storage-backends §Google Drive (live)](../../specs/storage-backends.md#google-drive-live)): run the example, log in, then `LEEMUSYNC_TEST_GDRIVE_CLIENT_ID=<id> cargo test -p leemusync-store --test conformance -- --ignored`. Expected: PASS. Record the date, OpenDAL 0.59.5 and the result in storage-backends §Google Drive (live), and delete the `leemusync-conformance` folder in Drive by hand. If Drive's `list` returns sizes of 0, the suite still passes (it compares names only).
- [x] **Step 9: Commit.** Update `docs/specs/storage-backends.md` (V-STORE-7 code pointer `crates/store/src/{oauth,gdrive}.rs`) and `docs/specs/security.md` V-SEC-4 ("store errors pass through `crates/store/src/redact.rs`; tests cover the token URL, JSON bodies and Bearer headers"). `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/store docs/specs
git commit -m "feat(store): add Google Drive with store-side token refresh

OAuth code + PKCE over a one-shot loopback listener; OpenDAL gets only an
access token and our rustls (ring) client with the platform verifier.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 9: `api` report types

Spec: [architecture §Engine API](../../specs/architecture.md#engine-api), [config-and-cli §Commands](../../specs/config-and-cli.md#commands) (every command supports `--json` with stable English keys).

**Files:**
- Modify: `crates/api/Cargo.toml` (`serde = { workspace = true }`; dev: `serde_json = { workspace = true }`), `crates/api/src/lib.rs`

**Interfaces:**
- Produces (all `#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]`, enums `#[serde(rename_all = "snake_case")]`; `StreamAction` and `WarningCode` also derive `Copy, Hash`, so callers can copy them out of borrowed reports): `API_VERSION: u32 = 1`; `SyncReport { api_version, dry_run, streams: Vec<StreamReport>, warnings: Vec<Warning> }`; `StreamReport { player, emulator, game, account: Option<String>, action: StreamAction, version: Option<String>, heads: Vec<String>, done: bool }`; `StreamAction::{Nothing, Adopt, FastForward, Upload, UploadThenConflict, Conflict, LocalMissing, RemoteBehind, NeedsAccount, Skipped}`; `Warning { code: WarningCode, detail: String }`; `WarningCode::{EmulatorRunning, NotGranted, SharedFolderNotEmpty, OrphanedSave, ConfigMerged, PlayerRenamed, RemoteBehind, RestoreRolledBack, InvalidVersion}`; `ScanReport { api_version, installs: Vec<InstallReport>, warnings: Vec<Warning> }`; `InstallReport { emulator, base, granted: bool, needs_grant: Vec<String>, accounts: Vec<AccountReport>, saves: Vec<SaveReport> }`; `AccountReport { id, name: Option<String>, player: Option<String> }`; `SaveReport { game, account, orphaned: bool, files: u32, bytes: u64 }`; `StatusReport { api_version, device, device_id, repo: Option<String>, backend: Option<String>, grants: Vec<String>, players: Vec<String>, streams: Vec<StreamStatus> }`; `StreamStatus { player, emulator, game, account: Option<String>, base: Option<String> }`; `HistoryReport { api_version, versions: Vec<HistoryEntry>, backups: Vec<String> }`; `HistoryEntry { id, created_at: u64, device: String, files: u32, bytes: u64, parents: Vec<String>, head: bool }`; `InitReport { api_version, repo, device }`; `DoctorReport { api_version, checks: Vec<Check> }`; `Check { name, ok: bool, detail: String }`; `ErrorReport { api_version, error: ErrorBody }`; `ErrorBody { code, message }`. Every `String` field holds display text that is never translated (player names, ids, paths).
- The recovery key is **not** in any report. The CLI prints it once, in human form only (Task 16).

- [x] **Step 1: Failing test** at the bottom of `crates/api/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_report_json_is_stable() {
        let r = SyncReport {
            api_version: API_VERSION,
            dry_run: true,
            streams: vec![StreamReport {
                player: "A".into(),
                emulator: "cemu".into(),
                game: "101c9400".into(),
                account: Some("80000001".into()),
                action: StreamAction::UploadThenConflict,
                version: None,
                heads: vec![],
                done: false,
            }],
            warnings: vec![Warning { code: WarningCode::OrphanedSave, detail: "80000002".into() }],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"api_version":1,"dry_run":true,"streams":[{"player":"A","emulator":"cemu","game":"101c9400","account":"80000001","action":"upload_then_conflict","version":null,"heads":[],"done":false}],"warnings":[{"code":"orphaned_save","detail":"80000002"}]}"#
        );
        assert_eq!(serde_json::from_str::<SyncReport>(&json).unwrap(), r);
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-api` → compile errors.
- [x] **Step 3: Implement** every type in the Interfaces list, in that order, in `crates/api/src/lib.rs` under the existing doc comment, with `pub` fields, the derives above and a one-line `///` doc per type. Use `pub const API_VERSION: u32 = 1;`.
- [x] **Step 4: Run** `cargo test -p leemusync-api && cargo clippy -p leemusync-api --all-targets -- -D warnings` → PASS.
- [x] **Step 5: Commit** (`just check` first): `feat(api): add versioned report types for the CLI's --json output` with the trailer.

---

### Task 10: `engine` foundation: environment, local config, keystore, state DB, files, processes

Spec: [config-and-cli §Locations, §Config file](../../specs/config-and-cli.md#locations), [security §Credentials](../../specs/security.md#credentials), [reliability §Invariants](../../specs/reliability.md#invariants) (I4), [architecture §Process model](../../specs/architecture.md#process-model) (exclusive state lock), D12.

**Files:**
- Modify: `Cargo.toml` (`keyring`, `rusqlite`, `directories`, `sysinfo` per the table), `crates/engine/Cargo.toml`:

```toml
[features]
# Test-only fault injection (crates/engine/src/failpoints.rs).
failpoints = []

[dependencies]
leemusync-api = { path = "../api" }
leemusync-core = { path = "../core" }
leemusync-crypto = { path = "../crypto" }
leemusync-store = { path = "../store" }
directories = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
sysinfo = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true, features = ["rt"] }
toml = { workspace = true }
zeroize = { workspace = true }

[dev-dependencies]
leemusync-engine = { path = ".", features = ["failpoints"] }
tempfile = { workspace = true }
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }

[target.'cfg(target_os = "linux")'.dependencies]
keyring = { workspace = true, features = ["sync-secret-service", "crypto-rust", "vendored"] }

[target.'cfg(target_os = "macos")'.dependencies]
keyring = { workspace = true, features = ["apple-native"] }

[target.'cfg(windows)'.dependencies]
keyring = { workspace = true, features = ["windows-native"] }
```

- Create: `crates/engine/src/{error,locations,local_config,secrets,state,fsx,failpoints,process}.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `leemusync_core::{DeviceId, VersionId, StreamId, PlayerId, manifest::FileEntry}`, `leemusync_crypto::{random_bytes, KdfParams}`, `leemusync_store::{StoreError, ErrorClass}`.
- Produces:
  - `EngineError` (variants `Usage(String)`, `Config(String)`, `Store(StoreError)`, `Io(String)`, `Crypto(CryptoError)`, `Unlock`, `Locked(String)`, `EmulatorRunning(String)`, `NotGranted(String)`, `NotSetUp(&'static str)`, `Secrets(String)`, `Data(String)`, `NeedsConfirmation`, `Busy(String)`, `Injected(&'static str)`), with `code(&self) -> &'static str` and `exit_code(&self) -> u8` ([config-and-cli §Exit codes](../../specs/config-and-cli.md#exit-codes): usage/not set up/needs confirmation → 2, config → 3, store `Auth`/`Transient` → 6, everything else → 1).
  - `Locations { config_dir, data_dir, home, xdg_data, xdg_config }` (`PathBuf`s) with `Locations::from_os() -> Result<Locations, EngineError>` and `Locations::under(root: &Path) -> Locations` (tests: `root/config/leemusync`, `root/data/leemusync`, `root/home`, `root/home/.local/share`, `root/home/.config`).
  - `LocalConfig { config_version, device: DeviceSection { id: DeviceId, name: String }, repo: Option<String>, backend: Option<BackendConfig>, grants: Vec<PathBuf>, emulators: BTreeMap<String, EmulatorOverride> }`, `BackendConfig::{Gdrive { root: String, client_id: String }, Fs { path: PathBuf }}`, `EmulatorOverride { root: Option<PathBuf> }`, `LocalConfig::{new, load, save}`.
  - `trait SecretStore: Send + Sync { fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError>; fn set(&self, name: &str, value: &str) -> Result<(), EngineError>; fn delete(&self, name: &str) -> Result<(), EngineError>; }`, `KeyringSecrets` (service `leemusync`), `MemorySecrets::default()`; name helpers `secret_master(repo: &str)`, `secret_gdrive_refresh(device: DeviceId)`, `secret_gdrive_client(device: DeviceId)`.
  - `State` with `open(data_dir: &Path) -> Result<State, EngineError>` (takes the exclusive lock), `stream(&self, id: &StreamId) -> Result<Option<StreamRow>, EngineError>`, `put_stream(&self, row: &StreamRow) -> Result<(), EngineError>`, `cached_manifest(&self, stream: &StreamId, id: &VersionId) -> Result<Option<Vec<u8>>, EngineError>`, `cache_manifest(&self, stream: &StreamId, id: &VersionId, json: &[u8]) -> Result<(), EngineError>`, `cached_config(&self, id: &VersionId) -> Result<Option<Vec<u8>>, EngineError>`, `cache_config(&self, id: &VersionId, json: &[u8]) -> Result<(), EngineError>`, `journal_put(&self, e: &JournalEntry) -> Result<(), EngineError>`, `journal_all(&self) -> Result<Vec<JournalEntry>, EngineError>`, `journal_delete(&self, op: &str) -> Result<(), EngineError>`, `meta(&self, key: &str) -> Result<Option<String>, EngineError>`, `set_meta(&self, key: &str, value: &str) -> Result<(), EngineError>`.
  - `StreamRow { stream: StreamId, player: PlayerId, emulator: String, game: String, base: Option<VersionId>, base_files: Vec<FileEntry>, highest_seen: Option<VersionId>, replace_local: bool }`; `JournalEntry { op: String, stream: StreamId, kind: String, step: String, data: String }`.
  - `fsx::{write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()>, ensure_no_symlinks(root: &Path, path: &Path) -> io::Result<()>, list_files(dir: &Path) -> io::Result<Vec<(String, std::fs::Metadata)>>, copy_tree(src: &Path, dst: &Path) -> io::Result<()>, mtime_ms(meta: &Metadata) -> i64}`.
  - `Failpoints` with `hit(&self, name: &'static str) -> Result<(), EngineError>` and, in unit tests or with feature `failpoints`, `arm(&self, name: &'static str)`.
  - `trait ProcessProbe: Send + Sync { fn running(&self, names: &[String]) -> bool; }`, `SysinfoProbe`, `FakeProbe` (`FakeProbe::default()`, `set(&self, running: bool)`).
  - `EngineEnv { locations: Locations, secrets: Arc<dyn SecretStore>, processes: Arc<dyn ProcessProbe>, kdf: KdfParams }` with `EngineEnv::system() -> Result<EngineEnv, EngineError>`; `Engine::open(env: EngineEnv) -> Result<Engine, EngineError>`, `Engine::device_id(&self) -> DeviceId`, `Engine::local_config(&self) -> &LocalConfig`, `Engine::arm_failpoint(&self, name: &'static str)` (feature `failpoints`); crate-private `now_ms() -> u64` and `new_version_id() -> Result<VersionId, EngineError>`.

- [x] **Step 1: Failing tests.** `crates/engine/src/fsx.rs` (tests at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sav");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(ensure_no_symlinks(dir.path(), &dir.path().join("link/x.sav")).is_err());
        assert!(ensure_no_symlinks(dir.path(), &dir.path().join("real/x.sav")).is_ok());
        assert!(ensure_no_symlinks(dir.path(), outside.path()).is_err(), "outside the root");
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("f"), dir.path().join("g")).unwrap();
        assert!(list_files(dir.path()).is_err(), "listing refuses symlinks too");
    }

    #[test]
    fn copy_tree_and_list_files_are_recursive_and_sorted() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("0")).unwrap();
        std::fs::write(src.path().join("0/game_data.sav"), b"g").unwrap();
        std::fs::write(src.path().join("option.sav"), b"o").unwrap();
        let dst = tempfile::tempdir().unwrap();
        copy_tree(src.path(), &dst.path().join("copy")).unwrap();
        let names: Vec<String> = list_files(&dst.path().join("copy")).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["0/game_data.sav", "option.sav"]);
    }
}
```

`crates/engine/src/state.rs` (tests at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::{Hash32, VersionId};

    fn row() -> StreamRow {
        StreamRow {
            stream: StreamId(Hash32([1; 32])),
            player: PlayerId(VersionId::from_parts(1, [1; 10])),
            emulator: "cemu".into(),
            game: "101c9400".into(),
            base: Some(VersionId::from_parts(2, [2; 10])),
            base_files: vec![],
            highest_seen: None,
            replace_local: true,
        }
    }

    #[test]
    fn streams_journal_and_meta_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::open(dir.path()).unwrap();
        state.put_stream(&row()).unwrap();
        assert_eq!(state.stream(&row().stream).unwrap(), Some(row()));
        let e = JournalEntry { op: "op1".into(), stream: row().stream, kind: "restore".into(), step: "swap".into(), data: "{}".into() };
        state.journal_put(&e).unwrap();
        assert_eq!(state.journal_all().unwrap(), vec![e]);
        state.journal_delete("op1").unwrap();
        assert!(state.journal_all().unwrap().is_empty());
        state.set_meta("k", "v").unwrap();
        assert_eq!(state.meta("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn a_second_engine_on_the_same_data_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _first = State::open(dir.path()).unwrap();
        assert!(matches!(State::open(dir.path()), Err(EngineError::Locked(_))));
    }
}
```

`crates/engine/src/lib.rs` (tests at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn test_env(root: &std::path::Path) -> EngineEnv {
        EngineEnv {
            locations: Locations::under(root),
            secrets: Arc::new(MemorySecrets::default()),
            processes: Arc::new(FakeProbe::default()),
            kdf: leemusync_crypto::KdfParams { m_kib: 19_456, t: 1, p: 1 },
        }
    }

    #[test]
    fn the_device_id_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let first = Engine::open(test_env(dir.path())).unwrap().device_id();
        let second = Engine::open(test_env(dir.path())).unwrap().device_id();
        assert_eq!(first, second);
        let text = std::fs::read_to_string(Locations::under(dir.path()).config_dir.join("config.toml")).unwrap();
        assert!(text.contains("config_version = 1"));
    }

    #[test]
    fn exit_codes_follow_the_cli_spec() {
        assert_eq!(EngineError::Usage("x".into()).exit_code(), 2);
        assert_eq!(EngineError::Config("x".into()).exit_code(), 3);
        let auth = leemusync_store::StoreError::new(leemusync_store::ErrorClass::Auth, "x");
        assert_eq!(EngineError::Store(auth).exit_code(), 6);
        assert_eq!(EngineError::Unlock.exit_code(), 1);
    }
}
```

`crates/engine/src/secrets.rs` (tests at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_secrets_round_trip() {
        let s = MemorySecrets::default();
        assert!(s.get("a").unwrap().is_none());
        s.set("a", "b").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref().map(String::as_str), Some("b"));
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());
    }

    #[test]
    #[ignore = "uses the real OS keystore"]
    fn keyring_round_trip() {
        let s = KeyringSecrets;
        s.set("leemusync-selftest", "x").unwrap();
        assert_eq!(s.get("leemusync-selftest").unwrap().as_deref().map(String::as_str), Some("x"));
        s.delete("leemusync-selftest").unwrap();
    }
}
```

- [x] **Step 2: Run** `cargo test -p leemusync-engine` → compile errors.
- [x] **Step 3: Implement `error.rs`:**

```rust
//! Engine errors. Applications map `code()` to localised text (architecture §Engine internals).

use leemusync_store::{ErrorClass, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Usage(String),
    #[error("invalid config: {0}")]
    Config(String),
    #[error("storage: {0}")]
    Store(#[from] StoreError),
    #[error("file system: {0}")]
    Io(String),
    #[error("crypto: {0}")]
    Crypto(#[from] leemusync_crypto::CryptoError),
    #[error("wrong passphrase or recovery key")]
    Unlock,
    #[error("another LeemuSync process is using {0}")]
    Locked(String),
    #[error("{0} is running; close it first")]
    EmulatorRunning(String),
    #[error("{0} is outside every granted folder")]
    NotGranted(String),
    #[error("not set up yet: {0}")]
    NotSetUp(&'static str),
    #[error("keystore: {0}")]
    Secrets(String),
    #[error("invalid data in the repo: {0}")]
    Data(String),
    #[error("the first sync on this device needs confirmation")]
    NeedsConfirmation,
    #[error("{0} changed while it was being read; try again when the emulator is idle")]
    Busy(String),
    #[error("fault injected at {0}")]
    Injected(&'static str),
}

impl EngineError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Config(_) => "config",
            Self::Store(e) if e.class() == ErrorClass::Auth => "backend_auth",
            Self::Store(_) => "backend",
            Self::Io(_) => "io",
            Self::Crypto(_) => "crypto",
            Self::Unlock => "unlock",
            Self::Locked(_) => "locked",
            Self::EmulatorRunning(_) => "emulator_running",
            Self::NotGranted(_) => "not_granted",
            Self::NotSetUp(_) => "not_set_up",
            Self::Secrets(_) => "secrets",
            Self::Data(_) => "data",
            Self::NeedsConfirmation => "needs_confirmation",
            Self::Busy(_) => "busy",
            Self::Injected(_) => "injected",
        }
    }

    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::NotSetUp(_) | Self::NeedsConfirmation => 2,
            Self::Config(_) => 3,
            Self::Store(e) if matches!(e.class(), ErrorClass::Auth | ErrorClass::Transient) => 6,
            _ => 1,
        }
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Io(format!("state database: {e}"))
    }
}
```

- [x] **Step 4: Implement `locations.rs`:**

```rust
//! Where `LeemuSync` keeps its files (config-and-cli §Locations).

use std::path::{Path, PathBuf};

use crate::EngineError;

#[derive(Clone, Debug)]
pub struct Locations {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub xdg_data: PathBuf,
    pub xdg_config: PathBuf,
}

impl Locations {
    /// # Errors
    /// `Config` when the OS reports no home directory.
    pub fn from_os() -> Result<Self, EngineError> {
        let project = directories::ProjectDirs::from("", "", "leemusync")
            .ok_or_else(|| EngineError::Config("no_home_dir".into()))?;
        let base = directories::BaseDirs::new()
            .ok_or_else(|| EngineError::Config("no_home_dir".into()))?;
        Ok(Self {
            config_dir: project.config_dir().to_path_buf(),
            data_dir: project.data_dir().to_path_buf(),
            home: base.home_dir().to_path_buf(),
            xdg_data: base.data_dir().to_path_buf(),
            xdg_config: base.config_dir().to_path_buf(),
        })
    }

    /// Everything under one folder, for tests and the two-device simulation.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        let home = root.join("home");
        Self {
            config_dir: root.join("config/leemusync"),
            data_dir: root.join("data/leemusync"),
            xdg_data: home.join(".local/share"),
            xdg_config: home.join(".config"),
            home,
        }
    }
}
```

- [x] **Step 5: Implement `fsx.rs`** (above its tests):

```rust
//! File helpers that keep invariant I4 (atomic writes) and never follow symlinks.

use std::fmt::Write as _;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path};
use std::time::UNIX_EPOCH;

const TEMP_PREFIX: &str = ".leemusync-";
const TEMP_SUFFIX: &str = ".tmp";

/// `write_atomic`'s temp names. A crash can orphan one; it is never snapshotted or restored.
fn is_temp_name(name: &str) -> bool {
    name.len() > TEMP_PREFIX.len() + TEMP_SUFFIX.len()
        && name.starts_with(TEMP_PREFIX)
        && name.ends_with(TEMP_SUFFIX)
}

/// Temp file in the same folder (`create_new`, so an existing name or symlink is never opened),
/// write, fsync, rename over the target (replaces the name, never follows it), fsync the folder (unix).
///
/// # Errors
/// Any I/O error; the target is then unchanged.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let random = leemusync_crypto::random_bytes::<8>().map_err(io::Error::other)?;
    let name = random.iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    });
    let tmp = parent.join(format!("{TEMP_PREFIX}{name}{TEMP_SUFFIX}"));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Makes the rename durable. Not done on Windows; see reliability V-REL-6.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // same signature as the unix version
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Fails if `path` is outside `root` or any existing component between them is a symlink.
///
/// The root is matched against `path`'s ancestors (shortest first) lexically or by canonical
/// form, so `/var` → `/private/var` (macOS) and `\\?\` prefixes (Windows) aren't false
/// rejections. Below the matched ancestor every component is checked with `symlink_metadata`,
/// never followed.
///
/// # Errors
/// `PermissionDenied` naming the problem.
pub fn ensure_no_symlinks(root: &Path, path: &Path) -> io::Result<()> {
    let base = matching_ancestor(root, path).ok_or_else(|| {
        io::Error::new(io::ErrorKind::PermissionDenied, "path is outside its root")
    })?;
    let rel = path
        .strip_prefix(base)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "path is outside its root"))?;
    let mut current = base.to_path_buf();
    for c in rel.components() {
        let Component::Normal(part) = c else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path has `..` or a prefix",
            ));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink inside a save folder",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The shortest ancestor of `path` (itself included) that is `root`, lexically or canonically.
fn matching_ancestor<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    if let Some(found) = ancestors.iter().find(|a| **a == root) {
        return Some(found);
    }
    let canonical_root = fs::canonicalize(root).ok()?;
    ancestors
        .into_iter()
        .find(|a| fs::canonicalize(a).is_ok_and(|c| c == canonical_root))
}

/// Every regular file under `dir`, as `/`-separated relative paths, sorted. Refuses symlinks.
/// Skips (never deletes) `write_atomic` temp files.
///
/// # Errors
/// I/O errors, or `PermissionDenied` for a symlink or a non-UTF-8 name.
pub fn list_files(dir: &Path) -> io::Result<Vec<(String, Metadata)>> {
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Metadata)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "non-UTF-8 file name"))?;
        if is_temp_name(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("symlink at {rel}"),
            ));
        } else if meta.is_dir() {
            walk(&entry.path(), &rel, out)?;
        } else if meta.is_file() {
            out.push((rel, meta));
        }
    }
    Ok(())
}

/// Copies every file under `src` into `dst` (created), each written atomically.
///
/// # Errors
/// Any I/O error from listing, reading or writing.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for (rel, _) in list_files(src)? {
        write_atomic(&dst.join(&rel), &fs::read(src.join(&rel))?)?;
    }
    Ok(())
}

#[must_use]
pub fn mtime_ms(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
```

- [x] **Step 6: Implement `local_config.rs`:**

```rust
//! `config.toml`, per device (config-and-cli §Config file). Never holds secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use leemusync_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{EngineError, fsx};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalConfig {
    pub config_version: u32,
    pub device: DeviceSection,
    pub repo: Option<String>,
    pub backend: Option<BackendConfig>,
    #[serde(default)]
    pub grants: Vec<PathBuf>,
    #[serde(default)]
    pub emulators: BTreeMap<String, EmulatorOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSection {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendConfig {
    Gdrive { root: String, client_id: String },
    Fs { path: PathBuf },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmulatorOverride {
    pub root: Option<PathBuf>,
}

/// `toml`'s `Display` quotes the offending source line, which can hold a secret pasted by
/// mistake. Keep only its message and the position.
fn parse_error(text: &str, e: &toml::de::Error) -> EngineError {
    let Some(span) = e.span() else {
        return EngineError::Config(e.message().to_owned());
    };
    let before = text.get(..span.start).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    EngineError::Config(format!("line {line}, column {column}: {}", e.message()))
}

impl LocalConfig {
    #[must_use]
    pub fn new(id: DeviceId, name: &str) -> Self {
        Self {
            config_version: CONFIG_VERSION,
            device: DeviceSection {
                id,
                name: name.to_owned(),
            },
            repo: None,
            backend: None,
            grants: Vec::new(),
            emulators: BTreeMap::new(),
        }
    }

    /// # Errors
    /// `Config` for invalid TOML or an unknown `config_version`; `Io` for read errors.
    pub fn load(path: &Path) -> Result<Option<Self>, EngineError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let config: Self = toml::from_str(&text).map_err(|e| parse_error(&text, &e))?;
        if config.config_version != CONFIG_VERSION {
            return Err(EngineError::Config(format!(
                "unsupported config_version {}",
                config.config_version
            )));
        }
        Ok(Some(config))
    }

    /// # Errors
    /// `Config` if it can't be serialised; `Io` if the atomic write fails.
    pub fn save(&self, path: &Path) -> Result<(), EngineError> {
        let text = toml::to_string(self).map_err(|e| EngineError::Config(e.to_string()))?;
        fsx::write_atomic(path, text.as_bytes())?;
        Ok(())
    }
}
```

- [x] **Step 7: Implement `secrets.rs`** (above its tests):

```rust
//! Secrets live in the OS keystore only (security §Credentials).

use std::collections::BTreeMap;
use std::sync::Mutex;

use leemusync_core::DeviceId;
use zeroize::Zeroizing;

use crate::EngineError;

const SERVICE: &str = "leemusync";

pub trait SecretStore: Send + Sync {
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError>;
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn set(&self, name: &str, value: &str) -> Result<(), EngineError>;
    /// Idempotent.
    ///
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn delete(&self, name: &str) -> Result<(), EngineError>;
}

#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_master(repo: &str) -> String {
    format!("repo-{repo}-master-key")
}
#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_gdrive_refresh(device: DeviceId) -> String {
    format!("device-{device}-gdrive-refresh-token")
}
#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_gdrive_client(device: DeviceId) -> String {
    format!("device-{device}-gdrive-client-secret")
}

/// Secret Service on Linux (`KWallet` or GNOME Keyring), Keychain, Credential Manager.
pub struct KeyringSecrets;

fn keyring_err(e: &keyring::Error) -> EngineError {
    EngineError::Secrets(leemusync_store::redact(&e.to_string()))
}

impl SecretStore for KeyringSecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keyring_err(&e))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(&e)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), EngineError> {
        keyring::Entry::new(SERVICE, name)
            .and_then(|e| e.set_password(value))
            .map_err(|e| keyring_err(&e))
    }

    fn delete(&self, name: &str) -> Result<(), EngineError> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keyring_err(&e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(&e)),
        }
    }
}

/// In-memory keystore for tests.
#[derive(Default)]
pub struct MemorySecrets(Mutex<BTreeMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError> {
        let map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        Ok(map.get(name).map(|v| Zeroizing::new(v.clone())))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), EngineError> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        map.insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), EngineError> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        map.remove(name);
        Ok(())
    }
}
```

- [x] **Step 8: Implement `state.rs`** (above its tests):

```rust
//! Local SQLite state DB and journal (D12, reliability §Restore procedure).

use std::fs::File;
use std::path::Path;

use leemusync_core::manifest::FileEntry;
use leemusync_core::{PlayerId, StreamId, VersionId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::EngineError;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE streams (
  stream_id TEXT PRIMARY KEY,
  player_id TEXT NOT NULL,
  emulator TEXT NOT NULL,
  game TEXT NOT NULL,
  base_version TEXT,
  base_files TEXT NOT NULL,
  highest_seen TEXT,
  replace_local INTEGER NOT NULL
);
CREATE TABLE manifests (stream_id TEXT NOT NULL, version_id TEXT NOT NULL, body BLOB NOT NULL,
  PRIMARY KEY (stream_id, version_id));
CREATE TABLE config_versions (version_id TEXT PRIMARY KEY, body BLOB NOT NULL);
CREATE TABLE journal (op_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL, kind TEXT NOT NULL,
  step TEXT NOT NULL, data TEXT NOT NULL);
PRAGMA user_version = 1;
";

/// The `PRAGMA user_version` this build writes and reads.
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamRow {
    pub stream: StreamId,
    pub player: PlayerId,
    pub emulator: String,
    pub game: String,
    pub base: Option<VersionId>,
    pub base_files: Vec<FileEntry>,
    pub highest_seen: Option<VersionId>,
    pub replace_local: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub op: String,
    pub stream: StreamId,
    pub kind: String,
    pub step: String,
    pub data: String,
}

pub struct State {
    conn: Connection,
    _lock: File,
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, EngineError> {
    text.parse()
        .map_err(|_| EngineError::Io("state database holds an invalid id".into()))
}

/// 0 → create the schema and stamp 1 in one transaction; 1 → as is; newer → refuse.
fn migrate(conn: &mut Connection) -> Result<(), EngineError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    match version {
        0 => {
            tx.execute_batch(SCHEMA)?;
            tx.commit()?;
            Ok(())
        }
        SCHEMA_VERSION => Ok(()),
        newer => Err(EngineError::Data(format!("state.db user_version {newer}"))),
    }
}

impl State {
    /// Opens (and creates) `<data_dir>/state.db` and holds `<data_dir>/lock` until dropped.
    ///
    /// # Errors
    /// `Locked` when another engine holds the lock; `Io` otherwise.
    pub fn open(data_dir: &Path) -> Result<Self, EngineError> {
        std::fs::create_dir_all(data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(data_dir.join("lock"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(EngineError::Locked(data_dir.display().to_string()));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let mut conn = Connection::open(data_dir.join("state.db"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")?;
        migrate(&mut conn)?;
        Ok(Self { conn, _lock: lock })
    }

    /// # Errors
    /// `Io` for database errors or corrupt rows.
    pub fn stream(&self, id: &StreamId) -> Result<Option<StreamRow>, EngineError> {
        let row = self
            .conn
            .query_row(
                "SELECT player_id, emulator, game, base_version, base_files, highest_seen, replace_local
                 FROM streams WHERE stream_id = ?1",
                params![id.to_string()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, bool>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((player, emulator, game, base, files, highest, replace)) = row else {
            return Ok(None);
        };
        Ok(Some(StreamRow {
            stream: *id,
            player: PlayerId(parse(&player)?),
            emulator,
            game,
            base: base.as_deref().map(parse).transpose()?,
            base_files: serde_json::from_str(&files).map_err(|e| EngineError::Io(e.to_string()))?,
            highest_seen: highest.as_deref().map(parse).transpose()?,
            replace_local: replace,
        }))
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn put_stream(&self, row: &StreamRow) -> Result<(), EngineError> {
        let files =
            serde_json::to_string(&row.base_files).map_err(|e| EngineError::Io(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO streams (stream_id, player_id, emulator, game, base_version, base_files, highest_seen, replace_local)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(stream_id) DO UPDATE SET player_id = ?2, emulator = ?3, game = ?4, base_version = ?5,
               base_files = ?6, highest_seen = ?7, replace_local = ?8",
            params![
                row.stream.to_string(),
                row.player.to_string(),
                row.emulator,
                row.game,
                row.base.map(|v| v.to_string()),
                files,
                row.highest_seen.map(|v| v.to_string()),
                row.replace_local,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cached_manifest(
        &self,
        stream: &StreamId,
        id: &VersionId,
    ) -> Result<Option<Vec<u8>>, EngineError> {
        Ok(self
            .conn
            .query_row(
                "SELECT body FROM manifests WHERE stream_id = ?1 AND version_id = ?2",
                params![stream.to_string(), id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cache_manifest(
        &self,
        stream: &StreamId,
        id: &VersionId,
        json: &[u8],
    ) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO manifests (stream_id, version_id, body) VALUES (?1, ?2, ?3)",
            params![stream.to_string(), id.to_string(), json],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cached_config(&self, id: &VersionId) -> Result<Option<Vec<u8>>, EngineError> {
        Ok(self
            .conn
            .query_row(
                "SELECT body FROM config_versions WHERE version_id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cache_config(&self, id: &VersionId, json: &[u8]) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO config_versions (version_id, body) VALUES (?1, ?2)",
            params![id.to_string(), json],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn journal_put(&self, e: &JournalEntry) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO journal (op_id, stream_id, kind, step, data) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(op_id) DO UPDATE SET step = ?4, data = ?5",
            params![e.op, e.stream.to_string(), e.kind, e.step, e.data],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors or corrupt rows.
    pub fn journal_all(&self) -> Result<Vec<JournalEntry>, EngineError> {
        let mut stmt = self
            .conn
            .prepare("SELECT op_id, stream_id, kind, step, data FROM journal ORDER BY op_id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?;
        rows.map(|row| {
            let (op, stream, kind, step, data) = row?;
            let hash = leemusync_core::Hash32::from_hex(&stream)
                .map_err(|e| EngineError::Io(e.to_string()))?;
            Ok(JournalEntry {
                op,
                stream: StreamId(hash),
                kind,
                step,
                data,
            })
        })
        .collect()
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn journal_delete(&self, op: &str) -> Result<(), EngineError> {
        self.conn
            .execute("DELETE FROM journal WHERE op_id = ?1", params![op])?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn meta(&self, key: &str) -> Result<Option<String>, EngineError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}
```

- [x] **Step 9: Implement `failpoints.rs` and `process.rs`:**

```rust
//! Test-only fault injection: `hit` fails once at an armed point (testing §Fault injection).

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::EngineError;

#[derive(Default)]
pub struct Failpoints {
    armed: Mutex<BTreeSet<&'static str>>,
}

impl Failpoints {
    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm(&self, name: &'static str) {
        if let Ok(mut armed) = self.armed.lock() {
            armed.insert(name);
        }
    }

    /// # Errors
    /// `Injected(name)` once, if `name` was armed.
    pub fn hit(&self, name: &'static str) -> Result<(), EngineError> {
        let fired = self.armed.lock().is_ok_and(|mut armed| armed.remove(name));
        if fired {
            Err(EngineError::Injected(name))
        } else {
            Ok(())
        }
    }
}
```

```rust
//! Is an emulator running? (I3; emulator-profiles §Linux paths: match the executable name, not `comm`.)

use std::sync::atomic::{AtomicBool, Ordering};

pub trait ProcessProbe: Send + Sync {
    fn running(&self, names: &[String]) -> bool;
}

pub struct SysinfoProbe;

impl ProcessProbe for SysinfoProbe {
    fn running(&self, names: &[String]) -> bool {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes().values().any(|p| {
            let exe = p
                .exe()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            let comm = p.name().to_string_lossy().into_owned();
            names
                .iter()
                .any(|n| exe.as_deref() == Some(n.as_str()) || comm == *n)
        })
    }
}

/// For tests: reports "running" when set.
#[derive(Default)]
pub struct FakeProbe(AtomicBool);

impl FakeProbe {
    pub fn set(&self, running: bool) {
        self.0.store(running, Ordering::SeqCst);
    }
}

impl ProcessProbe for FakeProbe {
    fn running(&self, _names: &[String]) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
```

- [x] **Step 10: Implement `lib.rs`** (above its tests):

```rust
//! Sync engine: scanning, snapshots, transfers, journal, state.
//! Spec: docs/specs/architecture.md (Engine internals), docs/specs/reliability.md.

mod error;
mod failpoints;
pub mod fsx;
mod local_config;
mod locations;
mod process;
mod secrets;
mod state;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use leemusync_core::{DeviceId, VersionId};
use leemusync_crypto::KdfParams;

pub use error::EngineError;
pub use failpoints::Failpoints;
pub use local_config::{BackendConfig, DeviceSection, EmulatorOverride, LocalConfig};
pub use locations::Locations;
pub use process::{FakeProbe, ProcessProbe, SysinfoProbe};
pub use secrets::{KeyringSecrets, MemorySecrets, SecretStore};
pub use state::{JournalEntry, State, StreamRow};

pub struct EngineEnv {
    pub locations: Locations,
    pub secrets: Arc<dyn SecretStore>,
    pub processes: Arc<dyn ProcessProbe>,
    /// Argon2id parameters for new repos (tests use the minimum).
    pub kdf: KdfParams,
}

impl EngineEnv {
    /// The real OS: XDG locations, the OS keystore, `sysinfo`, default Argon2id.
    ///
    /// # Errors
    /// `Config` when there is no home directory.
    pub fn system() -> Result<Self, EngineError> {
        Ok(Self {
            locations: Locations::from_os()?,
            secrets: Arc::new(KeyringSecrets),
            processes: Arc::new(SysinfoProbe),
            kdf: KdfParams::DEFAULT,
        })
    }
}

#[allow(dead_code)] // read from Task 11
pub struct Engine {
    env: EngineEnv,
    config: LocalConfig,
    state: State,
    failpoints: Failpoints,
}

impl Engine {
    /// Takes the state lock, then loads or creates `config.toml` (a new device id on first run).
    ///
    /// # Errors
    /// `Locked` if another engine runs; `Config`/`Io` for unreadable files.
    pub fn open(env: EngineEnv) -> Result<Self, EngineError> {
        // The lock comes first: config.toml is only read or written while holding it.
        let state = State::open(&env.locations.data_dir)?;
        std::fs::create_dir_all(&env.locations.config_dir)?;
        let path = env.locations.config_dir.join("config.toml");
        let config = if let Some(c) = LocalConfig::load(&path)? {
            c
        } else {
            let name = sysinfo::System::host_name().unwrap_or_else(|| "device".into());
            let c = LocalConfig::new(DeviceId(new_version_id()?), &name);
            c.save(&path)?;
            c
        };
        Ok(Self {
            env,
            config,
            state,
            failpoints: Failpoints::default(),
        })
    }

    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        self.config.device.id
    }

    #[must_use]
    pub fn local_config(&self) -> &LocalConfig {
        &self.config
    }

    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm_failpoint(&self, name: &'static str) {
        self.failpoints.arm(name);
    }

    #[allow(dead_code)] // read from Task 11
    fn save_config(&self) -> Result<(), EngineError> {
        self.config
            .save(&self.env.locations.config_dir.join("config.toml"))
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub(crate) fn new_version_id() -> Result<VersionId, EngineError> {
    Ok(VersionId::from_parts(
        now_ms(),
        leemusync_crypto::random_bytes::<10>()?,
    ))
}
```

`save_config`, `Engine::state`, `Engine::env` and `Engine::failpoints` are first read in Tasks 11–14. Until then put `#[allow(dead_code)] // read from Task 11` on `save_config` and on the `Engine` struct, and remove both attributes in Task 14 once everything is used (`-D warnings` turns dead-code warnings into errors).

- [x] **Step 11: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS. Run `cargo xtask layers` → ok.
- [x] **Step 12: Commit.** `docs/specs/config-and-cli.md` **Code:** `` `crates/engine/src/local_config.rs`, `crates/cli/` ``; `docs/specs/reliability.md` I4 gains "(`crates/engine/src/fsx.rs`)". `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/engine docs/specs
git commit -m "feat(engine): add the engine environment, state DB and atomic file helpers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 11: `engine` repo: `repo.json`, encrypted objects, repo-config stream, `init`/`join`/backend/players

Spec: [sync-model §Remote layout, §Repo config](../../specs/sync-model.md#remote-layout), [security §Keys](../../specs/security.md#keys), D30, V-STORE-8 handling (Task 1). Skill: `leemusync-security`.

**Files:**
- Create: `crates/engine/src/repo.rs`, `crates/engine/src/repo_config.rs`, `crates/engine/src/setup.rs` (the `Engine` methods for backend, init, join, players)
- Modify: `crates/engine/src/lib.rs` (modules, `repo: Option<Repo>` field on `Engine`)

**Interfaces:**
- Consumes: Tasks 2–4 (`core`), Task 6 (`crypto`), Tasks 7–8 (`Store`, `GdriveConfig`, `oauth`), Task 9 (`InitReport`), Task 10 (engine foundation).
- Produces:
  - `repo::Repo` (crate-private) with `create(store: Store, passphrase: &str, kdf: KdfParams) -> Result<(Repo, Key, RecoveryKey), EngineError>`, `open(store: Store, unlock: Unlock<'_>) -> Result<(Repo, Key), EngineError>`, `id() -> &str`, `blob_id(&[u8]) -> BlobId`, `stream_id(PlayerId, emulator: &str, game: &str) -> StreamId`, `put_blob(&[u8]) -> Result<BlobId, _>`, `get_blob(&BlobId) -> Result<Vec<u8>, _>` (verifies the keyed hash), `put_version(&Version)`, `list_versions(&StreamId) -> Result<Vec<VersionId>, _>`, `get_version(&StreamId, &VersionId) -> Result<Vec<u8>, _>` (decrypted JSON), `put_config(&ConfigVersion)`, `list_configs() -> Result<Vec<VersionId>, _>`, `get_config(&VersionId) -> Result<Vec<u8>, _>`, `put_device(&DeviceRecord)`. `Unlock::{Passphrase(&str), Recovery(&RecoveryKey), Master(Key)}`.
  - Remote names (all lowercase): `leemusync/repo.json`, `leemusync/blobs/<2 hex>/<blob id>`, `leemusync/streams/<stream id>/versions/<version id>`, `leemusync/config/versions/<version id>`, `leemusync/devices/<device id>`. AAD strings: `"blob\0<blob id>"`, `"version\0<stream id>\0<version id>"`, `"config\0<version id>"`, `"device\0<device id>"`, `"wrap\0passphrase\0<repo id>"`, `"wrap\0recovery\0<repo id>"`, `"check\0<repo id>"`.
  - `repo_config::{load, update, write, LoadedConfig}` (crate-private): `load(&Repo, &State) -> Result<LoadedConfig, EngineError>` where `LoadedConfig { config: RepoConfig, heads: Vec<VersionId>, renamed: Vec<String>, invalid: usize }`; `write(&Repo, &State, DeviceId, parents: Vec<VersionId>, config: RepoConfig) -> Result<VersionId, EngineError>`; `update(&Repo, &State, DeviceId, f: impl FnOnce(&mut RepoConfig) -> Result<(), EngineError>) -> Result<RepoConfig, EngineError>`.
  - Public `Engine` methods: `backend_add_fs(&mut self, path: &Path) -> Result<(), EngineError>`; `gdrive_login_start(&mut self, client_id: &str, client_secret: &str) -> Result<GdriveLogin, EngineError>` (async) with `GdriveLogin::url(&self) -> &str`; `gdrive_login_finish(&mut self, login: GdriveLogin) -> Result<(), EngineError>` (async); `init(&mut self, passphrase: &str) -> Result<(InitReport, Zeroizing<String>), EngineError>` (async; the `Zeroizing<String>` is the recovery key's display form, shown once); `join(&mut self, secret: JoinSecret) -> Result<InitReport, EngineError>` (async) with `JoinSecret::{Passphrase(String), Recovery(String)}`; `players_add(&mut self, name: &str) -> Result<(), EngineError>` (async); `players_list(&mut self) -> Result<Vec<String>, EngineError>` (async). Crate-private: `store(&self) -> Result<Store, EngineError>`, `ensure_repo(&mut self) -> Result<(), EngineError>` (async, opens the repo once with the cached master key) and `repo_ref(&self) -> Result<&Repo, EngineError>`. Call `ensure_repo().await?` first, then borrow `repo_ref()?` and `&self.state` side by side (two shared borrows).
  - `DeviceRecord { format: u32, id: DeviceId, name: String, created_at: u64 }` (serde, sealed like every object).

- [ ] **Step 1: Failing tests.** At the bottom of `crates/engine/src/repo.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_crypto::KdfParams;

    const FAST: KdfParams = KdfParams { m_kib: 19_456, t: 1, p: 1 };

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::fs(dir.path()).unwrap();
        (dir, s)
    }

    #[tokio::test]
    async fn create_then_open_three_ways() {
        let (_d, s) = store();
        let (repo, master, recovery) = Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
        let id = repo.id().to_owned();
        let (r1, m1) = Repo::open(s.clone(), Unlock::Passphrase("pass phrase")).await.unwrap();
        assert_eq!((r1.id(), m1.as_bytes()), (id.as_str(), master.as_bytes()));
        let (_, m2) = Repo::open(s.clone(), Unlock::Recovery(&recovery)).await.unwrap();
        assert_eq!(m2.as_bytes(), master.as_bytes());
        assert!(Repo::open(s.clone(), Unlock::Master(master.clone())).await.is_ok());
        assert!(matches!(Repo::open(s.clone(), Unlock::Passphrase("wrong")).await, Err(EngineError::Unlock)));
        assert!(matches!(
            Repo::open(s.clone(), Unlock::Master(Key::from_bytes([0; 32]))).await,
            Err(EngineError::Unlock)
        ));
        assert!(matches!(Repo::create(s, "x", FAST).await, Err(EngineError::Usage(_))), "never two repos");
    }

    #[tokio::test]
    async fn repo_json_never_holds_the_master_key() {
        let (_d, s) = store();
        let (_, master, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let text = String::from_utf8(s.get(REPO_JSON).await.unwrap()).unwrap();
        let hex: String = master.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        assert!(!text.contains(&hex));
        assert!(text.contains("\"algorithm\":\"argon2id\""));
    }

    #[tokio::test]
    async fn blobs_are_content_addressed_and_authenticated() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let a = repo.put_blob(b"save A").await.unwrap();
        assert_eq!(repo.put_blob(b"save A").await.unwrap(), a, "idempotent, dedupes");
        assert_eq!(repo.get_blob(&a).await.unwrap(), b"save A");
        let b = repo.put_blob(b"save B").await.unwrap();
        // A server swapping two objects is caught by the AAD.
        let b_bytes = s.get(&blob_path(&b)).await.unwrap();
        s.put(&blob_path(&a), b_bytes, leemusync_store::PutMode::Overwrite).await.unwrap();
        assert!(repo.get_blob(&a).await.is_err());
    }

    #[tokio::test]
    async fn versions_list_and_read_back() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s, "p", FAST).await.unwrap();
        let stream = repo.stream_id(leemusync_core::PlayerId(VersionId::from_parts(1, [1; 10])), "cemu", "101c9400");
        assert!(repo.list_versions(&stream).await.unwrap().is_empty());
        let id = VersionId::from_parts(5, [5; 10]);
        let v = leemusync_core::manifest::Version {
            format: 1,
            stream,
            id,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(2, [2; 10])),
            player: leemusync_core::PlayerId(VersionId::from_parts(1, [1; 10])),
            created_at: 5,
            emulator: leemusync_core::manifest::EmulatorRef { id: "cemu".into(), version: None },
            files: vec![],
            meta: leemusync_core::manifest::Meta { session_secs: None, changed_files: 0, takeover: false, note: None },
        };
        repo.put_version(&v).await.unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![id]);
        let json = repo.get_version(&stream, &id).await.unwrap();
        assert_eq!(leemusync_core::manifest::Version::from_json(&json, &stream, &id).unwrap(), v);
    }
}
```

At the bottom of `crates/engine/src/repo_config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::PlayerId;

    #[tokio::test]
    async fn concurrent_edits_merge_by_union() {
        let dir = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams { m_kib: 19_456, t: 1, p: 1 };
        let (repo, _, _) = crate::repo::Repo::create(store, "p", fast).await.unwrap();
        let state = crate::State::open(&dir.path().join("data")).unwrap();
        let dev = DeviceId(VersionId::from_parts(1, [1; 10]));
        let mut a = RepoConfig::default();
        a.add_player(PlayerId(VersionId::from_parts(2, [2; 10])), "A").unwrap();
        let mut b = RepoConfig::default();
        b.add_player(PlayerId(VersionId::from_parts(3, [3; 10])), "B").unwrap();
        // Two devices both edit on top of "no config yet": two heads.
        write(&repo, &state, dev, vec![], a).await.unwrap();
        write(&repo, &state, dev, vec![], b).await.unwrap();
        let loaded = load(&repo, &state).await.unwrap();
        assert_eq!(loaded.heads.len(), 2);
        assert_eq!(loaded.config.players.len(), 2);
        let after = update(&repo, &state, dev, |_| Ok(())).await.unwrap();
        assert_eq!(after.players.len(), 2);
        assert_eq!(load(&repo, &state).await.unwrap().heads.len(), 1, "the update merged both heads");
    }
}
```

At the bottom of `crates/engine/src/setup.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tests::test_env;
    use crate::{Engine, JoinSecret};

    #[tokio::test]
    async fn init_on_one_device_and_join_on_another() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let mut a = Engine::open(test_env(&root.path().join("a"))).unwrap();
        a.backend_add_fs(&remote).unwrap();
        let (report, recovery) = a.init("pass phrase").await.unwrap();
        assert_eq!(recovery.split('-').count(), 13);
        a.players_add("Ana").await.unwrap();

        let mut b = Engine::open(test_env(&root.path().join("b"))).unwrap();
        b.backend_add_fs(&remote).unwrap();
        assert!(b.join(JoinSecret::Passphrase("nope".into())).await.is_err());
        let joined = b.join(JoinSecret::Passphrase("pass phrase".into())).await.unwrap();
        assert_eq!(joined.repo, report.repo);
        b.players_add("Beto").await.unwrap();
        assert!(b.players_add("ana").await.is_err(), "names are unique");
        assert_eq!(a.players_list().await.unwrap(), vec!["Ana".to_string(), "Beto".to_string()]);

        let mut c = Engine::open(test_env(&root.path().join("c"))).unwrap();
        c.backend_add_fs(&remote).unwrap();
        c.join(JoinSecret::Recovery(recovery.to_lowercase())).await.unwrap();
        let config = std::fs::read_to_string(root.path().join("c/config/leemusync/config.toml")).unwrap();
        assert!(!config.contains("pass phrase") && !config.to_lowercase().contains(&recovery.to_lowercase()));
    }
}
```

- [ ] **Step 2: Run** `cargo test -p leemusync-engine` → compile errors.
- [ ] **Step 3: Implement `repo.rs`** (above its tests):

```rust
//! The remote repo: `repo.json`, encrypted objects, names (sync-model §Remote layout).

use leemusync_core::config::ConfigVersion;
use leemusync_core::manifest::Version;
use leemusync_core::{BlobId, DeviceId, Hash32, PlayerId, StreamId, VersionId};
use leemusync_crypto::{
    CryptoError, KdfParams, Key, RecoveryKey, RepoKeys, keyed_hash, open, passphrase_kek, random_bytes, seal,
    unwrap_key, wrap,
};
use leemusync_store::{ErrorClass, PutMode, Store};
use serde::{Deserialize, Serialize};

use crate::EngineError;

pub(crate) const REPO_JSON: &str = "leemusync/repo.json";
const CHECK: &[u8] = b"leemusync-repo-check";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoFile {
    format: u32,
    repo_id: String,
    kdf: KdfFile,
    wrapped: Wrapped,
    check: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KdfFile {
    algorithm: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wrapped {
    passphrase: String,
    recovery: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceRecord {
    pub format: u32,
    pub id: DeviceId,
    pub name: String,
    pub created_at: u64,
}

pub(crate) enum Unlock<'a> {
    Passphrase(&'a str),
    Recovery(&'a RecoveryKey),
    Master(Key),
}

pub(crate) struct Repo {
    store: Store,
    keys: RepoKeys,
    id: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, EngineError> {
    if text.len() % 2 != 0 {
        return Err(EngineError::Data("bad hex".into()));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| EngineError::Data("bad hex".into())))
        .collect()
}

fn lower(id: &VersionId) -> String {
    id.to_string().to_lowercase()
}

pub(crate) fn blob_path(id: &BlobId) -> String {
    let h = id.0.to_hex();
    format!("leemusync/blobs/{}/{h}", &h[..2])
}

fn version_dir(stream: &StreamId) -> String {
    format!("leemusync/streams/{stream}/versions/")
}

fn last_id(path: &str) -> Option<VersionId> {
    path.rsplit('/').next()?.parse().ok()
}

fn data_err(e: &CryptoError) -> EngineError {
    EngineError::Data(e.to_string())
}

impl Repo {
    pub(crate) async fn create(store: Store, passphrase: &str, kdf: KdfParams) -> Result<(Self, Key, RecoveryKey), EngineError> {
        if store.stat(REPO_JSON).await?.is_some() {
            return Err(EngineError::Usage("a LeemuSync repo already exists on this storage; use `leemusync join`".into()));
        }
        let master = Key::random()?;
        let recovery = RecoveryKey::generate()?;
        let salt: [u8; 16] = random_bytes()?;
        let repo_id = hex(&random_bytes::<16>()?);
        let kek = passphrase_kek(passphrase.as_bytes(), &salt, &kdf)?;
        let keys = RepoKeys::from_master(&master);
        let file = RepoFile {
            format: 1,
            repo_id: repo_id.clone(),
            kdf: KdfFile { algorithm: "argon2id".into(), m_kib: kdf.m_kib, t: kdf.t, p: kdf.p, salt: hex(&salt) },
            wrapped: Wrapped {
                passphrase: hex(&wrap(&kek, format!("wrap\0passphrase\0{repo_id}").as_bytes(), &master)?),
                recovery: hex(&wrap(&recovery.kek(), format!("wrap\0recovery\0{repo_id}").as_bytes(), &master)?),
            },
            check: hex(&seal(&keys.object, format!("check\0{repo_id}").as_bytes(), CHECK)?),
        };
        let json = serde_json::to_vec(&file).map_err(|e| EngineError::Data(e.to_string()))?;
        match store.put(REPO_JSON, json, PutMode::CreateOnly).await {
            Err(e) if e.class() == ErrorClass::AlreadyExists => {
                return Err(EngineError::Usage("another device just created a repo here; use `leemusync join`".into()));
            }
            other => other?,
        }
        // V-STORE-8: Drive may keep two repo.json files and serve the newest. Read back and compare.
        let back = Self::read_file(&store).await?;
        if back.repo_id != repo_id {
            return Err(EngineError::Usage("another device created a repo at the same time; use `leemusync join`".into()));
        }
        Ok((Self { store, keys, id: repo_id }, master, recovery))
    }

    async fn read_file(store: &Store) -> Result<RepoFile, EngineError> {
        let bytes = store.get(REPO_JSON).await?;
        let file: RepoFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Data(format!("repo.json: {e}")))?;
        if file.format != 1 || file.kdf.algorithm != "argon2id" {
            return Err(EngineError::Data("repo.json has an unsupported format".into()));
        }
        Ok(file)
    }

    pub(crate) async fn open(store: Store, unlock: Unlock<'_>) -> Result<(Self, Key), EngineError> {
        let file = Self::read_file(&store).await?;
        let id = file.repo_id.clone();
        let master = match unlock {
            Unlock::Master(key) => key,
            Unlock::Passphrase(p) => {
                let salt: [u8; 16] =
                    unhex(&file.kdf.salt)?.try_into().map_err(|_| EngineError::Data("repo.json salt".into()))?;
                let params = KdfParams { m_kib: file.kdf.m_kib, t: file.kdf.t, p: file.kdf.p };
                let kek = passphrase_kek(p.as_bytes(), &salt, &params)?;
                unwrap_key(&kek, format!("wrap\0passphrase\0{id}").as_bytes(), &unhex(&file.wrapped.passphrase)?)
                    .map_err(|_| EngineError::Unlock)?
            }
            Unlock::Recovery(r) => {
                unwrap_key(&r.kek(), format!("wrap\0recovery\0{id}").as_bytes(), &unhex(&file.wrapped.recovery)?)
                    .map_err(|_| EngineError::Unlock)?
            }
        };
        let keys = RepoKeys::from_master(&master);
        let check = open(&keys.object, format!("check\0{id}").as_bytes(), &unhex(&file.check)?).map_err(|_| EngineError::Unlock)?;
        if check != CHECK {
            return Err(EngineError::Unlock);
        }
        Ok((Self { store, keys, id }, master))
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn blob_id(&self, data: &[u8]) -> BlobId {
        BlobId(Hash32(keyed_hash(&self.keys.blob_id, data)))
    }

    pub(crate) fn stream_id(&self, player: PlayerId, emulator: &str, game: &str) -> StreamId {
        let canonical = format!("save\0{player}\0{emulator}\0{game}\0default");
        StreamId(Hash32(keyed_hash(&self.keys.stream_id, canonical.as_bytes())))
    }

    pub(crate) async fn put_blob(&self, data: &[u8]) -> Result<BlobId, EngineError> {
        let id = self.blob_id(data);
        let sealed = seal(&self.keys.object, format!("blob\0{id}").as_bytes(), data)?;
        match self.store.put(&blob_path(&id), sealed, PutMode::CreateOnly).await {
            Err(e) if e.class() == ErrorClass::AlreadyExists => Ok(id),
            other => other.map(|()| id).map_err(Into::into),
        }
    }

    pub(crate) async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>, EngineError> {
        let sealed = self.store.get(&blob_path(id)).await?;
        let data = open(&self.keys.object, format!("blob\0{id}").as_bytes(), &sealed).map_err(|e| data_err(&e))?;
        if self.blob_id(&data) != *id {
            return Err(EngineError::Data(format!("blob {id} doesn't match its id")));
        }
        Ok(data)
    }

    pub(crate) async fn put_version(&self, v: &Version) -> Result<(), EngineError> {
        let json = v.to_json().map_err(|e| EngineError::Data(e.to_string()))?;
        let sealed = seal(&self.keys.object, format!("version\0{}\0{}", v.stream, v.id).as_bytes(), &json)?;
        self.store.put(&format!("{}{}", version_dir(&v.stream), lower(&v.id)), sealed, PutMode::CreateOnly).await?;
        Ok(())
    }

    pub(crate) async fn list_versions(&self, stream: &StreamId) -> Result<Vec<VersionId>, EngineError> {
        let mut ids: Vec<VersionId> =
            self.store.list(&version_dir(stream)).await?.iter().filter_map(|e| last_id(&e.path)).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub(crate) async fn get_version(&self, stream: &StreamId, id: &VersionId) -> Result<Vec<u8>, EngineError> {
        let sealed = self.store.get(&format!("{}{}", version_dir(stream), lower(id))).await?;
        open(&self.keys.object, format!("version\0{stream}\0{id}").as_bytes(), &sealed).map_err(|e| data_err(&e))
    }

    pub(crate) async fn put_config(&self, v: &ConfigVersion) -> Result<(), EngineError> {
        let json = v.to_json().map_err(|e| EngineError::Data(e.to_string()))?;
        let sealed = seal(&self.keys.object, format!("config\0{}", v.id).as_bytes(), &json)?;
        self.store.put(&format!("leemusync/config/versions/{}", lower(&v.id)), sealed, PutMode::CreateOnly).await?;
        Ok(())
    }

    pub(crate) async fn list_configs(&self) -> Result<Vec<VersionId>, EngineError> {
        let mut ids: Vec<VersionId> =
            self.store.list("leemusync/config/versions/").await?.iter().filter_map(|e| last_id(&e.path)).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub(crate) async fn get_config(&self, id: &VersionId) -> Result<Vec<u8>, EngineError> {
        let sealed = self.store.get(&format!("leemusync/config/versions/{}", lower(id))).await?;
        open(&self.keys.object, format!("config\0{id}").as_bytes(), &sealed).map_err(|e| data_err(&e))
    }

    pub(crate) async fn put_device(&self, d: &DeviceRecord) -> Result<(), EngineError> {
        let json = serde_json::to_vec(d).map_err(|e| EngineError::Data(e.to_string()))?;
        let sealed = seal(&self.keys.object, format!("device\0{}", d.id).as_bytes(), &json)?;
        match self.store.put(&format!("leemusync/devices/{}", lower(&d.id.0)), sealed, PutMode::CreateOnly).await {
            Err(e) if e.class() == ErrorClass::AlreadyExists => Ok(()),
            other => other.map_err(Into::into),
        }
    }
}
```

- [ ] **Step 4: Implement `repo_config.rs`** (above its tests):

```rust
//! The repo-config stream: load (merging several heads, D30), write, update.

use leemusync_core::config::{CONFIG_FORMAT, ConfigVersion, RepoConfig};
use leemusync_core::heads::{Graph, heads};
use leemusync_core::{DeviceId, VersionId};

use crate::repo::Repo;
use crate::{EngineError, State, new_version_id, now_ms};

pub(crate) struct LoadedConfig {
    pub config: RepoConfig,
    pub heads: Vec<VersionId>,
    pub renamed: Vec<String>,
    /// Config versions that failed to decrypt or parse (ignored, reported as a warning).
    pub invalid: usize,
}

pub(crate) async fn load(repo: &Repo, state: &State) -> Result<LoadedConfig, EngineError> {
    let mut graph = Graph::new();
    let mut versions = std::collections::BTreeMap::new();
    let mut invalid = 0;
    for id in repo.list_configs().await? {
        let json = match state.cached_config(&id)? {
            Some(j) => j,
            None => match repo.get_config(&id).await {
                Ok(j) => j,
                Err(EngineError::Data(_)) => {
                    invalid += 1;
                    continue;
                }
                Err(e) => return Err(e),
            },
        };
        let Ok(v) = ConfigVersion::from_json(&json, &id) else {
            invalid += 1;
            continue;
        };
        state.cache_config(&id, &json)?;
        graph.insert(id, v.parents.clone());
        versions.insert(id, v);
    }
    let heads = heads(&graph);
    let head_configs: Vec<&RepoConfig> = heads.iter().filter_map(|h| versions.get(h)).map(|v| &v.config).collect();
    let (config, renamed) = match head_configs.as_slice() {
        [] => (RepoConfig::default(), Vec::new()),
        [one] => ((*one).clone(), Vec::new()),
        many => RepoConfig::merge(many),
    };
    Ok(LoadedConfig { config, heads, renamed, invalid })
}

pub(crate) async fn write(
    repo: &Repo,
    state: &State,
    device: DeviceId,
    parents: Vec<VersionId>,
    config: RepoConfig,
) -> Result<VersionId, EngineError> {
    let id = new_version_id()?;
    let v = ConfigVersion { format: CONFIG_FORMAT, id, parents, device, created_at: now_ms(), config };
    repo.put_config(&v).await?;
    state.cache_config(&id, &v.to_json().map_err(|e| EngineError::Data(e.to_string()))?)?;
    Ok(id)
}

pub(crate) async fn update(
    repo: &Repo,
    state: &State,
    device: DeviceId,
    f: impl FnOnce(&mut RepoConfig) -> Result<(), EngineError>,
) -> Result<RepoConfig, EngineError> {
    let loaded = load(repo, state).await?;
    let mut config = loaded.config;
    f(&mut config)?;
    write(repo, state, device, loaded.heads, config.clone()).await?;
    Ok(config)
}
```

- [ ] **Step 5: Implement `setup.rs`** (above its tests):

```rust
//! Backend login, repo creation and joining, players (config-and-cli §Commands).

use std::path::Path;

use leemusync_api::{API_VERSION, InitReport};
use leemusync_core::PlayerId;
use leemusync_crypto::{Key, RecoveryKey};
use leemusync_store::oauth::{self, OAuthClient, PendingLogin};
use leemusync_store::{GdriveConfig, Store};
use zeroize::Zeroizing;

use crate::repo::{DeviceRecord, Repo, Unlock};
use crate::secrets::{secret_gdrive_client, secret_gdrive_refresh, secret_master};
use crate::{BackendConfig, Engine, EngineError, new_version_id, now_ms, repo_config};

pub enum JoinSecret {
    Passphrase(String),
    Recovery(String),
}

pub struct GdriveLogin {
    pending: PendingLogin,
    client: OAuthClient,
}

impl GdriveLogin {
    #[must_use]
    pub fn url(&self) -> &str {
        self.pending.url()
    }
}

fn hex32(key: &Key) -> String {
    key.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

impl Engine {
    /// # Errors
    /// `Io` if the config can't be saved.
    pub fn backend_add_fs(&mut self, path: &Path) -> Result<(), EngineError> {
        self.config.backend = Some(BackendConfig::Fs { path: path.to_path_buf() });
        self.repo = None;
        self.save_config()
    }

    /// Starts the Google login (D28). Open `url()` in a browser, then call `gdrive_login_finish`.
    ///
    /// # Errors
    /// `Store` if the loopback port can't be bound.
    pub async fn gdrive_login_start(&mut self, client_id: &str, client_secret: &str) -> Result<GdriveLogin, EngineError> {
        let client = OAuthClient {
            client_id: client_id.trim().to_owned(),
            client_secret: Some(Zeroizing::new(client_secret.trim().to_owned())),
        };
        let pending = oauth::start_login(&client).await?;
        Ok(GdriveLogin { pending, client })
    }

    /// # Errors
    /// `Store(Auth)` for a refused or timed-out login; `Secrets` if the keystore fails.
    pub async fn gdrive_login_finish(&mut self, login: GdriveLogin) -> Result<(), EngineError> {
        let http = leemusync_store::http::http_client()?;
        let tokens = login.pending.finish(&login.client, &http).await?;
        let device = self.device_id();
        self.env.secrets.set(&secret_gdrive_refresh(device), &tokens.refresh_token)?;
        if let Some(secret) = &login.client.client_secret {
            self.env.secrets.set(&secret_gdrive_client(device), secret)?;
        }
        self.config.backend = Some(BackendConfig::Gdrive { root: "/".into(), client_id: login.client.client_id.clone() });
        self.repo = None;
        self.save_config()
    }

    pub(crate) fn store(&self) -> Result<Store, EngineError> {
        match &self.config.backend {
            None => Err(EngineError::NotSetUp("storage: run `leemusync backend add`")),
            Some(BackendConfig::Fs { path }) => Ok(Store::fs(path)?),
            Some(BackendConfig::Gdrive { root, client_id }) => {
                let device = self.device_id();
                let refresh = self
                    .env
                    .secrets
                    .get(&secret_gdrive_refresh(device))?
                    .ok_or(EngineError::NotSetUp("Google Drive login: run `leemusync backend login`"))?;
                let secret = self.env.secrets.get(&secret_gdrive_client(device))?;
                Ok(Store::gdrive(GdriveConfig {
                    root: root.clone(),
                    client: OAuthClient { client_id: client_id.clone(), client_secret: secret },
                    refresh_token: refresh,
                })?)
            }
        }
    }

    pub(crate) async fn ensure_repo(&mut self) -> Result<(), EngineError> {
        if self.repo.is_none() {
            let repo_id = self.config.repo.clone().ok_or(EngineError::NotSetUp("repo: run `leemusync init` or `join`"))?;
            let hex = self
                .env
                .secrets
                .get(&secret_master(&repo_id))?
                .ok_or(EngineError::NotSetUp("repo key: run `leemusync join` again"))?;
            let bytes = leemusync_core::Hash32::from_hex(&hex).map_err(|_| EngineError::Secrets("bad cached key".into()))?;
            let (repo, _) = Repo::open(self.store()?, Unlock::Master(Key::from_bytes(bytes.0))).await?;
            if repo.id() != repo_id {
                return Err(EngineError::Config("the storage holds a different repo than this device joined".into()));
            }
            self.repo = Some(repo);
        }
        Ok(())
    }

    pub(crate) fn repo_ref(&self) -> Result<&Repo, EngineError> {
        self.repo.as_ref().ok_or(EngineError::NotSetUp("repo: run `leemusync init` or `join`"))
    }

    async fn adopt_repo(&mut self, repo: Repo, master: &Key) -> Result<InitReport, EngineError> {
        self.env.secrets.set(&secret_master(repo.id()), &hex32(master))?;
        repo.put_device(&DeviceRecord {
            format: 1,
            id: self.device_id(),
            name: self.config.device.name.clone(),
            created_at: now_ms(),
        })
        .await?;
        self.config.repo = Some(repo.id().to_owned());
        self.save_config()?;
        let report = InitReport { api_version: API_VERSION, repo: repo.id().to_owned(), device: self.config.device.name.clone() };
        self.repo = Some(repo);
        Ok(report)
    }

    /// Creates the repo. Returns the recovery key's display form: show it once, never store it.
    ///
    /// # Errors
    /// `Usage` if this device already has a repo or the storage already holds one.
    pub async fn init(&mut self, passphrase: &str) -> Result<(InitReport, Zeroizing<String>), EngineError> {
        if self.config.repo.is_some() {
            return Err(EngineError::Usage("this device already belongs to a repo".into()));
        }
        if passphrase.chars().count() < 8 {
            return Err(EngineError::Usage("the passphrase needs at least 8 characters".into()));
        }
        let (repo, master, recovery) = Repo::create(self.store()?, passphrase, self.env.kdf).await?;
        let report = self.adopt_repo(repo, &master).await?;
        let device = self.device_id();
        repo_config::write(self.repo_ref()?, &self.state, device, vec![], leemusync_core::config::RepoConfig::default()).await?;
        Ok((report, recovery.to_display()))
    }

    /// # Errors
    /// `Unlock` for a wrong passphrase or recovery key.
    pub async fn join(&mut self, secret: JoinSecret) -> Result<InitReport, EngineError> {
        let store = self.store()?;
        let (repo, master) = match &secret {
            JoinSecret::Passphrase(p) => Repo::open(store, Unlock::Passphrase(p)).await?,
            JoinSecret::Recovery(text) => {
                let key = RecoveryKey::parse(text).map_err(|_| EngineError::Unlock)?;
                Repo::open(store, Unlock::Recovery(&key)).await?
            }
        };
        self.adopt_repo(repo, &master).await
    }

    /// # Errors
    /// `Config` for an invalid or taken name (from `core`).
    pub async fn players_add(&mut self, name: &str) -> Result<(), EngineError> {
        let id = PlayerId(new_version_id()?);
        let device = self.device_id();
        self.ensure_repo().await?;
        let repo = self.repo_ref()?;
        repo_config::update(repo, &self.state, device, |c| {
            c.add_player(id, name).map_err(|e| EngineError::Usage(e.to_string()))
        })
        .await?;
        Ok(())
    }

    /// # Errors
    /// Storage or repo errors.
    pub async fn players_list(&mut self) -> Result<Vec<String>, EngineError> {
        self.ensure_repo().await?;
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        Ok(loaded.config.players.values().map(|p| p.name.clone()).collect())
    }
}
```

Then in `lib.rs`: add `mod repo; mod repo_config; mod setup;`, `pub use setup::{GdriveLogin, JoinSecret};`, add the field `repo: Option<repo::Repo>` to `Engine` (initialised to `None` in `open`), and add `tokio = { workspace = true, features = ["rt", "net"] }` if `oauth` needs it at link time.

Note: `players_list` returns names in player-id order, which is creation order because ids are time-ordered. The test relies on that.

- [ ] **Step 6: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS.
- [ ] **Step 7: Commit.** `docs/specs/sync-model.md` §Remote layout: add "AAD strings and lowercase object names: `crates/engine/src/repo.rs`." `just check`, then:

```bash
git add crates/engine docs/specs/sync-model.md
git commit -m "feat(engine): create, join and unlock the encrypted repo

repo.json with the master key wrapped twice, sealed objects with
kind-and-id AAD, and the repo-config stream with union merges (D30).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: `engine` Cemu discovery: installs, accounts, save folders, orphans, grants

Spec: [emulator-profiles §Discovery, §Linux paths](../../specs/emulator-profiles.md#discovery), [players-and-accounts §Discovery, §Shared folders](../../specs/players-and-accounts.md#discovery), [security §File-system confinement](../../specs/security.md#file-system-confinement). Skills: `leemusync-emulator-profile`, `leemusync-security`.

**Files:**
- Modify: `Cargo.toml` (`regex`, `roxmltree`), `crates/engine/Cargo.toml` (both `{ workspace = true }`)
- Create: `crates/engine/src/profiles.rs`, `crates/engine/src/discover.rs`, `crates/engine/src/grants.rs`, `crates/engine/src/scan.rs` (the `Engine::scan`, `grant_*` methods), `crates/engine/tests/common/mod.rs`, `crates/engine/tests/discovery.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Produces:
  - `profiles::bundled() -> Result<Vec<Profile>, EngineError>` (the Cemu profile via `include_str!("../../../profiles/cemu.toml")`); `profiles::Install { emulator: String, base: PathBuf, config_dir: PathBuf, saves_root: PathBuf, accounts_root: Option<PathBuf>, warnings: Vec<Warning> }`; `profiles::resolve(profile: &Profile, loc: &Locations, override_root: Option<&Path>, granted: &dyn Fn(&Path) -> bool) -> Result<Option<Install>, EngineError>`.
  - `discover::{Account { id, name: Option<String> }, SaveFolder { game: String, account: String, path: PathBuf, orphaned: bool }, Discovery { accounts, saves, shared_not_empty: Vec<PathBuf> }}`; `discover::discover(profile: &Profile, install: &Install) -> Result<Discovery, EngineError>`; `discover::save_folder(profile: &Profile, install: &Install, game: &str, account: &str) -> Result<PathBuf, EngineError>`.
  - `grants::{is_granted(grants: &[PathBuf], path: &Path) -> bool, require(grants: &[PathBuf], path: &Path) -> Result<PathBuf, EngineError>}` (returns the grant root that covers `path`).
  - `Engine::grant_add(&mut self, path: &Path) -> Result<PathBuf, EngineError>` (canonical path; refuses `/`, the home folder itself and non-folders), `Engine::grant_list(&self) -> Vec<PathBuf>`, `Engine::scan(&mut self) -> Result<ScanReport, EngineError>` (async; works without a repo, then `player` stays `None`), crate-private `Engine::installs(&self) -> Result<Vec<(Profile, Install, Option<Discovery>)>, EngineError>` (`None` when the folders aren't granted).

- [ ] **Step 1: Failing integration test.** First the shared helpers, `crates/engine/tests/common/mod.rs` (Tasks 15 and 16 add to this file):

```rust
//! Shared helpers for engine integration tests. Fake data only.
#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};
use std::sync::Arc;

use leemusync_engine::{EngineEnv, FakeProbe, Locations, MemorySecrets, fsx};

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../profiles/fixtures/cemu/flatpak-botw-two-accounts");

pub fn env(root: &Path) -> EngineEnv {
    env_with(root, Arc::new(FakeProbe::default()), Arc::new(MemorySecrets::default()))
}

/// Keeping `secrets` across `Engine::open` calls simulates the OS keystore surviving a restart.
pub fn env_with(root: &Path, probe: Arc<FakeProbe>, secrets: Arc<MemorySecrets>) -> EngineEnv {
    EngineEnv {
        locations: Locations::under(root),
        secrets,
        processes: probe,
        kdf: leemusync_crypto::KdfParams { m_kib: 19_456, t: 1, p: 1 },
    }
}

/// Installs the fixture as a Flatpak (`flatpak = true`) or native Cemu under `root/home`.
/// Returns the Cemu data folder.
pub fn install_fixture(root: &Path, flatpak: bool) -> PathBuf {
    let home = root.join("home");
    let (data, config) = if flatpak {
        (home.join(".var/app/info.cemu.Cemu/data/Cemu"), home.join(".var/app/info.cemu.Cemu/config/Cemu"))
    } else {
        (home.join(".local/share/Cemu"), home.join(".config/Cemu"))
    };
    fsx::copy_tree(&Path::new(FIXTURE).join("data/Cemu"), &data).unwrap();
    fsx::copy_tree(&Path::new(FIXTURE).join("config/Cemu"), &config).unwrap();
    data
}
```

Then `crates/engine/tests/discovery.rs`. It loads the fixture's `expected.toml` (the profile test the emulator-profile skill asks for):

```rust
//! Cemu discovery against profiles/fixtures (fake data only).

mod common;

use std::path::Path;

use common::{FIXTURE, env, install_fixture};
use leemusync_engine::Engine;

#[tokio::test]
async fn flatpak_fixture_matches_expected_toml() {
    let root = tempfile::tempdir().unwrap();
    install_fixture(root.path(), true);
    let mut engine = Engine::open(env(root.path())).unwrap();
    let before = engine.scan().await.unwrap();
    assert!(!before.installs[0].granted, "nothing is read before a grant");
    assert!(before.installs[0].accounts.is_empty());
    engine.grant_add(&root.path().join("home/.var/app/info.cemu.Cemu")).unwrap();
    let report = engine.scan().await.unwrap();
    let cemu = &report.installs[0];
    assert!(cemu.granted && cemu.needs_grant.is_empty());

    let expected: toml::Value = toml::from_str(&std::fs::read_to_string(Path::new(FIXTURE).join("expected.toml")).unwrap()).unwrap();
    let mut want: Vec<(String, String)> = expected["account"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["id"].as_str().unwrap().to_owned(), a["name"].as_str().unwrap().to_owned()))
        .collect();
    want.sort();
    let got: Vec<(String, String)> =
        cemu.accounts.iter().map(|a| (a.id.clone(), a.name.clone().unwrap_or_default())).collect();
    assert_eq!(got, want);
    let mut streams: Vec<(String, String)> = expected["stream"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["title_id"].as_str().unwrap().to_owned(), s["account"].as_str().unwrap().to_owned()))
        .collect();
    streams.sort();
    let saves: Vec<(String, String)> = cemu.saves.iter().map(|s| (s.game.clone(), s.account.clone())).collect();
    assert_eq!(saves, streams);
    assert!(cemu.saves.iter().all(|s| !s.orphaned));
}

#[tokio::test]
async fn native_install_with_a_deleted_account_shows_an_orphan() {
    let root = tempfile::tempdir().unwrap();
    let data = install_fixture(root.path(), false);
    std::fs::remove_dir_all(data.join("mlc01/usr/save/system/act/80000002")).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    engine.grant_add(&data).unwrap();
    let report = engine.scan().await.unwrap();
    let cemu = &report.installs[0];
    assert_eq!(cemu.needs_grant.len(), 1, "settings.xml in ~/.config/Cemu isn't granted yet");
    let orphan = cemu.saves.iter().find(|s| s.account == "80000002").unwrap();
    assert!(orphan.orphaned);
    assert_eq!(cemu.accounts.len(), 1);
    engine.grant_add(&root.path().join("home/.config/Cemu")).unwrap();
    assert!(engine.scan().await.unwrap().installs[0].needs_grant.is_empty());
}

#[tokio::test]
async fn grants_refuse_home_and_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    assert!(engine.grant_add(Path::new("/")).is_err());
    assert!(engine.grant_add(&root.path().join("home")).is_err());
    assert!(engine.grant_add(&root.path().join("missing")).is_err());
}
```

Add `toml = { workspace = true }` and `leemusync-crypto = { path = "../crypto" }` to engine `[dev-dependencies]`.

- [ ] **Step 2: Run** `cargo test -p leemusync-engine --test discovery` → compile errors.
- [ ] **Step 3: Implement `grants.rs`:**

```rust
//! The engine reads and writes only inside folders the user granted (security §File-system confinement).

use std::path::{Path, PathBuf};

use crate::EngineError;

fn canonical_or_nearest(path: &Path) -> Option<PathBuf> {
    let mut probe = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = probe.canonicalize() {
            return Some(rest.iter().rev().fold(c, |acc: PathBuf, part: &std::ffi::OsString| acc.join(part)));
        }
        rest.push(probe.file_name()?.to_os_string());
        probe = probe.parent()?.to_path_buf();
    }
}

#[must_use]
pub fn is_granted(grants: &[PathBuf], path: &Path) -> bool {
    require(grants, path).is_ok()
}

/// # Errors
/// `NotGranted` naming the path.
pub fn require(grants: &[PathBuf], path: &Path) -> Result<PathBuf, EngineError> {
    let target = canonical_or_nearest(path).ok_or_else(|| EngineError::NotGranted(path.display().to_string()))?;
    grants
        .iter()
        .find(|g| target.starts_with(g))
        .cloned()
        .ok_or_else(|| EngineError::NotGranted(path.display().to_string()))
}
```

- [ ] **Step 4: Implement `profiles.rs`:**

```rust
//! Bundled profiles (D8) and where an emulator is installed on this machine.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use leemusync_api::{Warning, WarningCode};
use leemusync_core::profile::{Profile, expand};

use crate::{EngineError, Locations};

const BUNDLED: [&str; 1] = [include_str!("../../../profiles/cemu.toml")];
const MAX_CONFIG_FILE: u64 = 1024 * 1024;

pub struct Install {
    pub emulator: String,
    pub base: PathBuf,
    pub config_dir: PathBuf,
    pub saves_root: PathBuf,
    pub accounts_root: Option<PathBuf>,
    pub warnings: Vec<Warning>,
}

/// # Errors
/// `Config` if a bundled profile is invalid (a release bug).
pub fn bundled() -> Result<Vec<Profile>, EngineError> {
    BUNDLED.iter().map(|t| Profile::from_toml(t).map_err(|e| EngineError::Config(e.to_string()))).collect()
}

fn vars(profile: &Profile, loc: &Locations) -> BTreeMap<String, String> {
    let mut v = BTreeMap::new();
    v.insert("xdg_data".to_owned(), loc.xdg_data.display().to_string());
    v.insert("xdg_config".to_owned(), loc.xdg_config.display().to_string());
    if let Some(linux) = profile.platforms.get("linux") {
        for t in linux.roots.iter().chain(linux.config_roots.iter().flatten()) {
            if let Some(app) = t.strip_prefix("{flatpak:").and_then(|r| r.split('}').next()) {
                v.insert(format!("flatpak:{app}"), loc.home.join(".var/app").join(app).display().to_string());
            }
        }
    }
    v
}

/// First existing root in the profile's order (or the user's override). `granted` gates every read.
///
/// # Errors
/// `Config` for template errors in the profile.
pub fn resolve(
    profile: &Profile,
    loc: &Locations,
    override_root: Option<&Path>,
    granted: &dyn Fn(&Path) -> bool,
) -> Result<Option<Install>, EngineError> {
    let Some(linux) = profile.platforms.get("linux") else { return Ok(None) };
    let mut vars = vars(profile, loc);
    let mut found = None;
    for (i, root) in linux.roots.iter().enumerate() {
        let Ok(base) = expand(root, &vars) else { continue };
        let base = PathBuf::from(base);
        let wanted = override_root.is_none_or(|o| o == base);
        if wanted && base.is_dir() {
            let config = linux.config_roots.as_ref().and_then(|c| c.get(i)).map_or(Ok(base.display().to_string()), |c| expand(c, &vars));
            let config = config.map_err(|e| EngineError::Config(e.to_string()))?;
            found = Some((base, PathBuf::from(config)));
            break;
        }
    }
    if found.is_none() {
        if let Some(o) = override_root.filter(|o| o.is_dir()) {
            found = Some((o.to_path_buf(), o.to_path_buf()));
        }
    }
    let Some((base, config_dir)) = found else { return Ok(None) };
    vars.insert("base".to_owned(), base.display().to_string());
    let mut warnings = Vec::new();
    for key in &profile.config_keys {
        let file = config_dir.join(&key.file);
        let value = if granted(&file) {
            read_xml_key(&file, &key.key)
        } else {
            if file.exists() {
                warnings.push(Warning { code: WarningCode::NotGranted, detail: config_dir.display().to_string() });
            }
            None
        };
        let resolved = match value.filter(|v| Path::new(v).is_absolute()) {
            Some(v) => v,
            None => expand(&key.default, &vars).map_err(|e| EngineError::Config(e.to_string()))?,
        };
        vars.insert(key.var.clone(), resolved);
    }
    let saves_root = PathBuf::from(expand(&profile.saves.root, &vars).map_err(|e| EngineError::Config(e.to_string()))?);
    let accounts_root = profile
        .accounts
        .as_ref()
        .map(|a| expand(&a.root, &vars).map(PathBuf::from))
        .transpose()
        .map_err(|e| EngineError::Config(e.to_string()))?;
    Ok(Some(Install { emulator: profile.id.clone(), base, config_dir, saves_root, accounts_root, warnings }))
}

/// `content/mlc_path`-style element path in a small XML file; `None` when missing, empty or unreadable.
fn read_xml_key(file: &Path, key: &str) -> Option<String> {
    if std::fs::metadata(file).ok()?.len() > MAX_CONFIG_FILE {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let mut node = doc.root_element();
    let mut parts = key.split('/');
    if parts.next()? != node.tag_name().name() {
        return None;
    }
    for part in parts {
        node = node.children().find(|n| n.has_tag_name(part))?;
    }
    let value = node.text().unwrap_or("").trim().to_owned();
    (!value.is_empty()).then_some(value)
}
```

- [ ] **Step 5: Implement `discover.rs`:**

```rust
//! Accounts, save folders and orphaned saves of one install (players-and-accounts §Discovery).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use leemusync_core::account::{decode_mii_name, key_value};
use leemusync_core::profile::{PatternPart, Profile, parse_pattern};

use crate::profiles::Install;
use crate::EngineError;

const MAX_ACCOUNT_FILE: u64 = 64 * 1024;
const TITLE_ID: &str = "^[0-9a-f]{8}$";

pub struct Account {
    pub id: String,
    pub name: Option<String>,
}

pub struct SaveFolder {
    pub game: String,
    pub account: String,
    pub path: PathBuf,
    pub orphaned: bool,
}

pub struct Discovery {
    pub accounts: Vec<Account>,
    pub saves: Vec<SaveFolder>,
    pub shared_not_empty: Vec<PathBuf>,
}

fn regex(pattern: &str) -> Result<regex::Regex, EngineError> {
    regex::Regex::new(pattern).map_err(|e| EngineError::Config(e.to_string()))
}

fn dirs(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

/// # Errors
/// `Config` for an invalid `id_regex` or pattern in the profile.
pub fn discover(profile: &Profile, install: &Install) -> Result<Discovery, EngineError> {
    let account_re = regex(profile.accounts.as_ref().map_or(".*", |a| a.id_regex.as_str()))?;
    let title_re = regex(TITLE_ID)?;
    let mut accounts = Vec::new();
    if let (Some(root), Some(acc)) = (&install.accounts_root, &profile.accounts) {
        for id in dirs(root).into_iter().filter(|d| account_re.is_match(d)) {
            let file = root.join(&id).join(&acc.name_file);
            let name = std::fs::metadata(&file)
                .ok()
                .filter(|m| m.len() <= MAX_ACCOUNT_FILE)
                .and_then(|_| std::fs::read_to_string(&file).ok())
                .and_then(|t| key_value(&t, &acc.name_key).and_then(decode_mii_name));
            accounts.push(Account { id, name });
        }
    }
    let parts = parse_pattern(&profile.saves.pattern).map_err(|e| EngineError::Config(e.to_string()))?;
    let mut found: Vec<(PathBuf, BTreeMap<String, String>)> = vec![(install.saves_root.clone(), BTreeMap::new())];
    for part in &parts {
        let mut next = Vec::new();
        for (path, bound) in found {
            match part {
                PatternPart::Literal(l) => next.push((path.join(l), bound)),
                PatternPart::Var(v) => {
                    for d in dirs(&path) {
                        let ok = match v.as_str() {
                            "title_id" => title_re.is_match(&d),
                            "account" => account_re.is_match(&d),
                            _ => false,
                        };
                        if ok {
                            let mut b = bound.clone();
                            b.insert(v.clone(), d.clone());
                            next.push((path.join(&d), b));
                        }
                    }
                }
            }
        }
        found = next;
    }
    let known: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    let mut saves: Vec<SaveFolder> = found
        .into_iter()
        .filter(|(p, _)| p.is_dir())
        .filter_map(|(path, b)| {
            let game = b.get("title_id")?.to_lowercase();
            let account = b.get("account")?.clone();
            let orphaned = install.accounts_root.is_some() && !known.contains(&account.as_str());
            Some(SaveFolder { game, account, path, orphaned })
        })
        .collect();
    saves.sort_by(|a, b| (&a.game, &a.account).cmp(&(&b.game, &b.account)));
    let mut shared_not_empty = Vec::new();
    for game in saves.iter().map(|s| s.game.clone()).collect::<std::collections::BTreeSet<_>>() {
        for shared in &profile.saves.shared {
            let rel = shared.replace("{title_id}", &game);
            let path = install.saves_root.join(rel);
            if std::fs::read_dir(&path).is_ok_and(|mut d| d.next().is_some()) {
                shared_not_empty.push(path);
            }
        }
    }
    Ok(Discovery { accounts, saves, shared_not_empty })
}

/// Where a (game, account) save folder lives, whether or not it exists yet.
///
/// # Errors
/// `Config` for an invalid pattern; `Usage` for a game or account id the profile rejects.
pub fn save_folder(profile: &Profile, install: &Install, game: &str, account: &str) -> Result<PathBuf, EngineError> {
    let account_re = regex(profile.accounts.as_ref().map_or(".*", |a| a.id_regex.as_str()))?;
    if !regex(TITLE_ID)?.is_match(game) || !account_re.is_match(account) {
        return Err(EngineError::Usage(format!("`{game}`/`{account}` isn't a valid game and account here")));
    }
    let parts = parse_pattern(&profile.saves.pattern).map_err(|e| EngineError::Config(e.to_string()))?;
    Ok(parts.iter().fold(install.saves_root.clone(), |p, part| match part {
        PatternPart::Literal(l) => p.join(l),
        PatternPart::Var(v) if v == "title_id" => p.join(game),
        PatternPart::Var(_) => p.join(account),
    }))
}
```

Note: the fixture's `user/common/.keep` makes `user/common` non-empty. In the Flatpak fixture test, the scan reports it as `shared_not_empty`, so it shows up as a `SharedFolderNotEmpty` warning. The test doesn't assert on warnings. This is expected: M1a doesn't sync shared folders, and the warning tells the user.

- [ ] **Step 6: Implement `scan.rs`:**

```rust
//! `leemusync grant` and `leemusync scan`.

use std::path::{Path, PathBuf};

use leemusync_api::{API_VERSION, AccountReport, InstallReport, SaveReport, ScanReport, Warning, WarningCode};
use leemusync_core::profile::Profile;

use crate::discover::{Discovery, discover};
use crate::profiles::{Install, bundled, resolve};
use crate::{Engine, EngineError, fsx, grants, repo_config};

impl Engine {
    /// # Errors
    /// `Usage` for `/`, the home folder itself, or a path that isn't an existing folder.
    pub fn grant_add(&mut self, path: &Path) -> Result<PathBuf, EngineError> {
        let canonical = path.canonicalize().map_err(|_| EngineError::Usage(format!("{} doesn't exist", path.display())))?;
        let home = self.env.locations.home.canonicalize().unwrap_or_else(|_| self.env.locations.home.clone());
        if !canonical.is_dir() || canonical.parent().is_none() || canonical == home {
            return Err(EngineError::Usage(format!("{} can't be granted; grant the emulator's own folder", canonical.display())));
        }
        if !self.config.grants.contains(&canonical) {
            self.config.grants.push(canonical.clone());
            self.config.grants.sort();
            self.save_config()?;
        }
        Ok(canonical)
    }

    #[must_use]
    pub fn grant_list(&self) -> Vec<PathBuf> {
        self.config.grants.clone()
    }

    pub(crate) fn installs(&self) -> Result<Vec<(Profile, Install, Option<Discovery>)>, EngineError> {
        let grants = self.config.grants.clone();
        let granted = move |p: &Path| grants::is_granted(&grants, p);
        let mut out = Vec::new();
        for profile in bundled()? {
            let override_root = self.config.emulators.get(&profile.id).and_then(|o| o.root.clone());
            let Some(install) = resolve(&profile, &self.env.locations, override_root.as_deref(), &granted)? else {
                continue;
            };
            let readable = granted(&install.saves_root)
                && install.accounts_root.as_deref().is_none_or(|a| granted(a))
                && fsx::ensure_no_symlinks(&grants::require(&self.config.grants, &install.saves_root)?, &install.saves_root).is_ok();
            let discovery = if readable { Some(discover(&profile, &install)?) } else { None };
            out.push((profile, install, discovery));
        }
        Ok(out)
    }

    /// Lists installs, accounts (with mapped players when a repo is open), save folders and orphans.
    ///
    /// # Errors
    /// `Config` for a broken bundled profile; repo errors are ignored (players stay unknown).
    pub async fn scan(&mut self) -> Result<ScanReport, EngineError> {
        let device = self.device_id();
        let config = if self.ensure_repo().await.is_ok() {
            match self.repo_ref() {
                Ok(repo) => repo_config::load(repo, &self.state).await.ok().map(|l| l.config),
                Err(_) => None,
            }
        } else {
            None
        };
        let mut installs = Vec::new();
        let mut warnings = Vec::new();
        for (profile, install, discovery) in self.installs()? {
            let mut needs_grant = Vec::new();
            if !grants::is_granted(&self.config.grants, &install.saves_root) {
                needs_grant.push(install.base.display().to_string());
            }
            if install.config_dir != install.base && !grants::is_granted(&self.config.grants, &install.config_dir) && install.config_dir.exists() {
                needs_grant.push(install.config_dir.display().to_string());
            }
            warnings.extend(install.warnings.iter().cloned());
            let (accounts, saves) = match &discovery {
                None => (Vec::new(), Vec::new()),
                Some(d) => {
                    for p in &d.shared_not_empty {
                        warnings.push(Warning { code: WarningCode::SharedFolderNotEmpty, detail: p.display().to_string() });
                    }
                    let accounts = d.accounts.iter().map(|a| AccountReport {
                        id: a.id.clone(),
                        name: a.name.clone(),
                        player: config.as_ref().and_then(|c| {
                            let pid = c.player_on_account(device, &profile.id, &a.id)?;
                            c.players.get(&pid).map(|p| p.name.clone())
                        }),
                    }).collect();
                    let saves = d.saves.iter().map(|s| {
                        if s.orphaned {
                            warnings.push(Warning { code: WarningCode::OrphanedSave, detail: format!("{}/{}", s.game, s.account) });
                        }
                        let files = fsx::list_files(&s.path).unwrap_or_default();
                        SaveReport {
                            game: s.game.clone(),
                            account: s.account.clone(),
                            orphaned: s.orphaned,
                            files: u32::try_from(files.len()).unwrap_or(u32::MAX),
                            bytes: files.iter().map(|(_, m)| m.len()).sum(),
                        }
                    }).collect();
                    (accounts, saves)
                }
            };
            installs.push(InstallReport {
                emulator: profile.id.clone(),
                base: install.base.display().to_string(),
                granted: discovery.is_some(),
                needs_grant,
                accounts,
                saves,
            });
        }
        Ok(ScanReport { api_version: API_VERSION, installs, warnings })
    }
}
```

`lib.rs`: add `mod discover; mod grants; mod profiles; mod scan;`. Because `fsx` is `pub`, the integration test can call `fsx::copy_tree`.

- [ ] **Step 7: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS.
- [ ] **Step 8: Commit.** In `docs/specs/emulator-profiles.md` §Discovery add "Implemented for `per_game_user_dir` in `crates/engine/src/{profiles,discover}.rs`; fixture test `crates/engine/tests/discovery.rs`." In `players-and-accounts.md` §Discovery add "Scan: `crates/engine/src/scan.rs`." `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/engine profiles docs/specs
git commit -m "feat(engine): discover Cemu installs, accounts and orphaned saves

Reads only inside granted folders; Flatpak and native layouts; fixture
test against expected.toml.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 13: `engine` snapshot and upload

Spec: [sync-model §Snapshot rules](../../specs/sync-model.md#snapshot-rules), [reliability §Upload procedure](../../specs/reliability.md#upload-procedure), [reliability §Local history](../../specs/reliability.md#local-history).

**Files:**
- Create: `crates/engine/src/snapshot.rs`, `crates/engine/src/upload.rs`
- Modify: `crates/engine/src/lib.rs` (`mod snapshot; mod upload;` and `pub(crate) fn history_dir`)

**Interfaces:**
- Produces:
  - `history_dir(data_dir: &Path, stream: &StreamId) -> PathBuf` = `<data>/history/<stream id>` (in `lib.rs`).
  - `snapshot::Quick::{Missing, SameAsBase, Changed}`; `snapshot::quick(src: &Path, base_files: &[FileEntry]) -> Result<Quick, EngineError>` (names, sizes and mtimes only, no reads).
  - `snapshot::Snapshot { dir: PathBuf, files: Vec<FileEntry> }`; `snapshot::take(src: &Path, dest: &Path, max_file_bytes: u64, blob_id: &dyn Fn(&[u8]) -> BlobId) -> Result<Option<Snapshot>, EngineError>`: copies every file into `dest` (local history) and returns entries sorted by path; `None` when `src` has no files; `Busy` when a file changes during the read (re-stat), `Usage` for a file over the limit or a name `RelPath` rejects.
  - `snapshot::same_files(a: &[FileEntry], b: &[FileEntry]) -> bool` (paths and blobs).
  - `upload::UploadMeta<'a> { stream: StreamId, parents: Vec<VersionId>, device: DeviceId, player: PlayerId, emulator: &'a str, base_files: &'a [FileEntry], note: Option<String> }`; `upload::upload(repo: &Repo, snap: &Snapshot, meta: UploadMeta<'_>, fp: &Failpoints) -> Result<Version, EngineError>` (async): blobs first (create-only, idempotent), then failpoint `upload.before_manifest`, then the manifest (the commit point), then failpoint `upload.after_manifest`.

- [ ] **Step 1: Failing tests.** At the bottom of `crates/engine/src/snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::Hash32;

    fn fake_id(d: &[u8]) -> BlobId {
        let mut h = [0u8; 32];
        for (i, b) in d.iter().enumerate() {
            h[i % 32] ^= *b;
        }
        BlobId(Hash32(h))
    }

    #[test]
    fn takes_a_copy_and_lists_entries() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("0")).unwrap();
        std::fs::write(src.path().join("0/game_data.sav"), b"game").unwrap();
        std::fs::write(src.path().join("option.sav"), b"opt").unwrap();
        let hist = tempfile::tempdir().unwrap();
        let snap = take(src.path(), &hist.path().join("s1"), 1024, &fake_id).unwrap().unwrap();
        let paths: Vec<&str> = snap.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["0/game_data.sav", "option.sav"]);
        assert_eq!(std::fs::read(snap.dir.join("0/game_data.sav")).unwrap(), b"game");
        assert_eq!(snap.files[1].size, 3);
        assert!(matches!(quick(src.path(), &snap.files).unwrap(), Quick::SameAsBase));
        std::fs::write(src.path().join("option.sav"), b"changed").unwrap();
        assert!(matches!(quick(src.path(), &snap.files).unwrap(), Quick::Changed));
    }

    #[test]
    fn empty_or_missing_folders_have_no_snapshot() {
        let src = tempfile::tempdir().unwrap();
        let hist = tempfile::tempdir().unwrap();
        assert!(take(src.path(), &hist.path().join("s"), 1024, &fake_id).unwrap().is_none());
        assert!(matches!(quick(&src.path().join("nope"), &[]).unwrap(), Quick::Missing));
    }

    #[test]
    fn refuses_oversized_files() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("big.sav"), vec![0u8; 2048]).unwrap();
        let hist = tempfile::tempdir().unwrap();
        assert!(matches!(take(src.path(), &hist.path().join("s"), 1024, &fake_id), Err(EngineError::Usage(_))));
    }
}
```

At the bottom of `crates/engine/src/upload.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::Repo;
    use leemusync_core::{Hash32, VersionId};

    #[tokio::test]
    async fn blobs_first_manifest_last() {
        let dir = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams { m_kib: 19_456, t: 1, p: 1 };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("option.sav"), b"opt").unwrap();
        let snap = crate::snapshot::take(&src, &dir.path().join("hist"), 1024, &|d| repo.blob_id(d)).unwrap().unwrap();
        let stream = StreamId(Hash32([3; 32]));
        let meta = || UploadMeta {
            stream,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
        };
        let fp = Failpoints::default();
        fp.arm("upload.before_manifest");
        assert!(matches!(upload(&repo, &snap, meta(), &fp).await, Err(EngineError::Injected(_))));
        assert!(repo.list_versions(&stream).await.unwrap().is_empty(), "no manifest, no version");
        assert_eq!(repo.get_blob(&snap.files[0].blob).await.unwrap(), b"opt", "the blob already went up");
        let v = upload(&repo, &snap, meta(), &fp).await.unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![v.id]);
        assert_eq!(v.meta.changed_files, 1);
    }
}
```

- [ ] **Step 2: Run** `cargo test -p leemusync-engine snapshot upload` → compile errors.
- [ ] **Step 3: Implement `snapshot.rs`** (above its tests):

```rust
//! Consistent snapshots: read → hash → re-stat, copied into local history first
//! (sync-model §Snapshot rules, reliability §Upload procedure step 1).

use std::path::{Path, PathBuf};

use leemusync_core::manifest::FileEntry;
use leemusync_core::{BlobId, RelPath};

use crate::{EngineError, fsx};

pub(crate) enum Quick {
    Missing,
    SameAsBase,
    Changed,
}

pub(crate) struct Snapshot {
    pub dir: PathBuf,
    pub files: Vec<FileEntry>,
}

pub(crate) fn quick(src: &Path, base_files: &[FileEntry]) -> Result<Quick, EngineError> {
    if !src.is_dir() {
        return Ok(Quick::Missing);
    }
    let files = fsx::list_files(src)?;
    if files.is_empty() {
        return Ok(Quick::Missing);
    }
    let same = files.len() == base_files.len()
        && files.iter().zip(base_files).all(|((name, meta), base)| {
            name == base.path.as_str() && meta.len() == base.size && fsx::mtime_ms(meta) == base.mtime
        });
    Ok(if same { Quick::SameAsBase } else { Quick::Changed })
}

pub(crate) fn same_files(a: &[FileEntry], b: &[FileEntry]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.path == y.path && x.blob == y.blob)
}

pub(crate) fn take(
    src: &Path,
    dest: &Path,
    max_file_bytes: u64,
    blob_id: &dyn Fn(&[u8]) -> BlobId,
) -> Result<Option<Snapshot>, EngineError> {
    if !src.is_dir() {
        return Ok(None);
    }
    let listed = fsx::list_files(src)?;
    if listed.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::with_capacity(listed.len());
    for (rel, before) in listed {
        let path = RelPath::parse(&rel).map_err(|e| EngineError::Usage(format!("{rel}: {e}")))?;
        if before.len() > max_file_bytes {
            return Err(EngineError::Usage(format!("{rel} is larger than this emulator's file limit")));
        }
        let bytes = std::fs::read(src.join(&rel))?;
        let after = std::fs::symlink_metadata(src.join(&rel))?;
        if after.len() != before.len()
            || fsx::mtime_ms(&after) != fsx::mtime_ms(&before)
            || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != before.len()
        {
            return Err(EngineError::Busy(rel));
        }
        fsx::write_atomic(&dest.join(&rel), &bytes)?;
        files.push(FileEntry { path, size: before.len(), mtime: fsx::mtime_ms(&before), blob: blob_id(&bytes) });
    }
    Ok(Some(Snapshot { dir: dest.to_path_buf(), files }))
}
```

- [ ] **Step 4: Implement `upload.rs`** (above its tests):

```rust
//! Upload procedure: blobs (create-only, idempotent) → manifest (commit point) (reliability §Upload procedure).

use leemusync_core::manifest::{EmulatorRef, FORMAT, FileEntry, Meta, Version};
use leemusync_core::{DeviceId, PlayerId, StreamId, VersionId};

use crate::repo::Repo;
use crate::snapshot::Snapshot;
use crate::{EngineError, Failpoints, new_version_id, now_ms};

pub(crate) struct UploadMeta<'a> {
    pub stream: StreamId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub player: PlayerId,
    pub emulator: &'a str,
    pub base_files: &'a [FileEntry],
    pub note: Option<String>,
}

pub(crate) async fn upload(repo: &Repo, snap: &Snapshot, meta: UploadMeta<'_>, fp: &Failpoints) -> Result<Version, EngineError> {
    for f in &snap.files {
        let data = std::fs::read(snap.dir.join(f.path.as_str()))?;
        if repo.put_blob(&data).await? != f.blob {
            return Err(EngineError::Data(format!("local history copy of {} changed", f.path.as_str())));
        }
    }
    fp.hit("upload.before_manifest")?;
    let changed = snap
        .files
        .iter()
        .filter(|f| !meta.base_files.iter().any(|b| b.path == f.path && b.blob == f.blob))
        .count();
    let v = Version {
        format: FORMAT,
        stream: meta.stream,
        id: new_version_id()?,
        parents: meta.parents,
        device: meta.device,
        player: meta.player,
        created_at: now_ms(),
        emulator: EmulatorRef { id: meta.emulator.to_owned(), version: None },
        files: snap.files.clone(),
        meta: Meta {
            session_secs: None,
            changed_files: u32::try_from(changed).unwrap_or(u32::MAX),
            takeover: false,
            note: meta.note,
        },
    };
    repo.put_version(&v).await?;
    fp.hit("upload.after_manifest")?;
    Ok(v)
}
```

`lib.rs`: add `mod snapshot; mod upload;` and:

```rust
pub(crate) fn history_dir(data_dir: &std::path::Path, stream: &leemusync_core::StreamId) -> std::path::PathBuf {
    data_dir.join("history").join(stream.to_string())
}
```

- [ ] **Step 5: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS.
- [ ] **Step 6: Commit.** `docs/specs/reliability.md` §Upload procedure: add "Implemented in `crates/engine/src/{snapshot,upload}.rs`. Local history pruning is not in M1a: history grows until Phase 1's later plan adds pruning." `just check`, then `feat(engine): take consistent snapshots and upload blobs before the manifest` with the trailer.

---

### Task 14: `engine` journaled restore and crash recovery

Spec: [reliability §Restore procedure, §Invariants](../../specs/reliability.md#restore-procedure) (I1, I3, I4, I5, I6), [security §File-system confinement](../../specs/security.md#file-system-confinement). Skill: `leemusync-security`.

**Files:**
- Create: `crates/engine/src/restore.rs`
- Modify: `crates/engine/src/lib.rs` (`mod restore;`; `Engine::open` runs recovery; `Engine::recover_journal`)

**Interfaces:**
- Consumes: Tasks 10–13.
- Produces:
  - `restore::Source<'a>::{Remote(&'a Version), Local(&'a Path)}` (a version from the repo, or a local-history folder).
  - `restore::Ctx<'a> { repo: Option<&'a Repo>, state: &'a State, data_dir: &'a Path, grants: &'a [PathBuf], probe: &'a dyn ProcessProbe, process_names: &'a [String], fp: &'a Failpoints }`.
  - `restore::restore(ctx: &Ctx<'_>, stream: &StreamId, target: &Path, source: Source<'_>, reason: &str) -> Result<Option<PathBuf>, EngineError>` (async; returns the backup folder, `None` when the target was empty). Failpoints: `restore.after_backup`, `restore.mid_swap` (after the first file), `restore.before_done`.
  - `restore::recover(state: &State, probe: &dyn ProcessProbe) -> Result<Vec<Warning>, EngineError>`: rolls every unfinished swap back to its backup. Leaves the entry if the emulator runs, and returns `EmulatorRunning`.
  - `Engine::recover_journal(&self) -> Result<Vec<Warning>, EngineError>` (crate-private), called by `Engine::open` (an `EmulatorRunning` error there is ignored, so the CLI still starts) and at the start of `sync` and `restore` (where it's fatal).

- [ ] **Step 1: Failing tests** at the bottom of `crates/engine/src/restore.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FakeProbe, State};
    use leemusync_core::{DeviceId, Hash32, PlayerId, VersionId};

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        repo: Repo,
        state: State,
        version: Version,
        target: PathBuf,
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = leemusync_store::Store::fs(&root.join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams { m_kib: 19_456, t: 1, p: 1 };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let state = State::open(&root.join("data")).unwrap();
        let new = root.join("new");
        std::fs::create_dir_all(new.join("0")).unwrap();
        std::fs::write(new.join("0/game_data.sav"), b"NEW GAME").unwrap();
        std::fs::write(new.join("option.sav"), b"NEW OPT").unwrap();
        let snap = crate::snapshot::take(&new, &root.join("hist"), 1 << 20, &|d| repo.blob_id(d)).unwrap().unwrap();
        let meta = crate::upload::UploadMeta {
            stream: StreamId(Hash32([9; 32])),
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
        };
        let version = crate::upload::upload(&repo, &snap, meta, &Failpoints::default()).await.unwrap();
        let target = root.join("grant/user/80000001");
        std::fs::create_dir_all(target.join("0")).unwrap();
        std::fs::write(target.join("0/game_data.sav"), b"OLD GAME").unwrap();
        std::fs::write(target.join("stale.sav"), b"OLD ONLY").unwrap();
        World { _dir: dir, root, repo, state, version, target }
    }

    fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fsx::list_files(dir).unwrap().into_iter().map(|(n, _)| (n.clone(), std::fs::read(dir.join(&n)).unwrap())).collect()
    }

    fn ctx<'a>(w: &'a World, grants: &'a [PathBuf], probe: &'a FakeProbe, fp: &'a Failpoints, data: &'a Path) -> Ctx<'a> {
        Ctx { repo: Some(&w.repo), state: &w.state, data_dir: data, grants, probe, process_names: &[], fp }
    }

    #[tokio::test]
    async fn restore_backs_up_then_replaces_exactly() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (FakeProbe::default(), Failpoints::default(), w.root.join("data"));
        let before = read_tree(&w.target);
        let backup = restore(&ctx(&w, &grants, &probe, &fp, &data), &w.version.stream, &w.target, Source::Remote(&w.version), "pre-restore")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read_tree(&backup), before, "I1: the old files are in local history");
        assert_eq!(
            read_tree(&w.target),
            vec![("0/game_data.sav".into(), b"NEW GAME".to_vec()), ("option.sav".into(), b"NEW OPT".to_vec())]
        );
        assert!(w.state.journal_all().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refuses_outside_grants_and_while_running() {
        let w = world().await;
        let (probe, fp, data) = (FakeProbe::default(), Failpoints::default(), w.root.join("data"));
        let none: Vec<PathBuf> = vec![];
        let r = restore(&ctx(&w, &none, &probe, &fp, &data), &w.version.stream, &w.target, Source::Remote(&w.version), "x").await;
        assert!(matches!(r, Err(EngineError::NotGranted(_))));
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        probe.set(true);
        let r = restore(&ctx(&w, &grants, &probe, &fp, &data), &w.version.stream, &w.target, Source::Remote(&w.version), "x").await;
        assert!(matches!(r, Err(EngineError::EmulatorRunning(_))));
        assert_eq!(std::fs::read(w.target.join("stale.sav")).unwrap(), b"OLD ONLY", "nothing touched");
    }

    #[tokio::test]
    async fn a_crash_at_any_step_rolls_back_to_the_old_files() {
        for point in ["restore.after_backup", "restore.mid_swap", "restore.before_done"] {
            let w = world().await;
            let grants = vec![w.root.join("grant").canonicalize().unwrap()];
            let (probe, fp, data) = (FakeProbe::default(), Failpoints::default(), w.root.join("data"));
            let before = read_tree(&w.target);
            fp.arm(point);
            let r = restore(&ctx(&w, &grants, &probe, &fp, &data), &w.version.stream, &w.target, Source::Remote(&w.version), "x").await;
            assert!(matches!(r, Err(EngineError::Injected(_))), "{point}");
            recover(&w.state, &probe).unwrap();
            assert_eq!(read_tree(&w.target), before, "{point}: never half old, half new");
            assert!(w.state.journal_all().unwrap().is_empty(), "{point}");
            recover(&w.state, &probe).unwrap(); // idempotent
        }
    }
}
```

- [ ] **Step 2: Run** `cargo test -p leemusync-engine restore` → compile errors.
- [ ] **Step 3: Implement** (above the tests):

```rust
//! Staged, journaled, atomic restore with a backup first (reliability §Restore procedure).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use leemusync_api::{Warning, WarningCode};
use leemusync_core::StreamId;
use leemusync_core::manifest::Version;
use serde::{Deserialize, Serialize};

use crate::repo::Repo;
use crate::{EngineError, Failpoints, JournalEntry, ProcessProbe, State, fsx, grants, history_dir, new_version_id, now_ms};

pub(crate) enum Source<'a> {
    Remote(&'a Version),
    Local(&'a Path),
}

pub(crate) struct Ctx<'a> {
    pub repo: Option<&'a Repo>,
    pub state: &'a State,
    pub data_dir: &'a Path,
    pub grants: &'a [PathBuf],
    pub probe: &'a dyn ProcessProbe,
    pub process_names: &'a [String],
    pub fp: &'a Failpoints,
}

#[derive(Serialize, Deserialize)]
struct SwapData {
    target: PathBuf,
    backup: Option<PathBuf>,
    staging: PathBuf,
    process_names: Vec<String>,
}

async fn stage(ctx: &Ctx<'_>, source: &Source<'_>, staging: &Path) -> Result<Vec<String>, EngineError> {
    match source {
        Source::Remote(v) => {
            let repo = ctx.repo.ok_or(EngineError::NotSetUp("repo"))?;
            for f in &v.files {
                let data = repo.get_blob(&f.blob).await?;
                if u64::try_from(data.len()).unwrap_or(u64::MAX) != f.size {
                    return Err(EngineError::Data(format!("{} has the wrong size", f.path.as_str())));
                }
                fsx::write_atomic(&staging.join(f.path.as_str()), &data)?;
            }
            Ok(v.files.iter().map(|f| f.path.as_str().to_owned()).collect())
        }
        Source::Local(dir) => {
            fsx::copy_tree(dir, staging)?;
            Ok(fsx::list_files(staging)?.into_iter().map(|(n, _)| n).collect())
        }
    }
}

pub(crate) async fn restore(
    ctx: &Ctx<'_>,
    stream: &StreamId,
    target: &Path,
    source: Source<'_>,
    reason: &str,
) -> Result<Option<PathBuf>, EngineError> {
    let op = new_version_id()?.to_string().to_lowercase();
    let staging = ctx.data_dir.join("staging").join(&op);
    // 1. Download everything and verify it (I6) before touching the target.
    let files = stage(ctx, &source, &staging).await?;
    // 2. Confinement and I3.
    let grant = grants::require(ctx.grants, target)?;
    fsx::ensure_no_symlinks(&grant, target)?;
    if ctx.probe.running(ctx.process_names) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(EngineError::EmulatorRunning(ctx.process_names.join(", ")));
    }
    // 3. Backup (I1).
    let backup = if target.is_dir() && !fsx::list_files(target)?.is_empty() {
        let dir = history_dir(ctx.data_dir, stream).join(format!("{}-{reason}", now_ms()));
        fsx::copy_tree(target, &dir)?;
        Some(dir)
    } else {
        None
    };
    ctx.fp.hit("restore.after_backup")?;
    // 4. Journal "swap started" (I5).
    let data = SwapData {
        target: target.to_path_buf(),
        backup: backup.clone(),
        staging: staging.clone(),
        process_names: ctx.process_names.to_vec(),
    };
    let entry = JournalEntry {
        op: op.clone(),
        stream: *stream,
        kind: "restore".into(),
        step: "swap".into(),
        data: serde_json::to_string(&data).map_err(|e| EngineError::Io(e.to_string()))?,
    };
    ctx.state.journal_put(&entry)?;
    // 5. Swap each file atomically (I4), then drop files the version doesn't have.
    std::fs::create_dir_all(target)?;
    for (i, rel) in files.iter().enumerate() {
        fsx::write_atomic(&target.join(rel), &std::fs::read(staging.join(rel))?)?;
        if i == 0 {
            ctx.fp.hit("restore.mid_swap")?;
        }
    }
    let keep: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    for (rel, _) in fsx::list_files(target)? {
        if !keep.contains(rel.as_str()) {
            std::fs::remove_file(target.join(&rel))?;
        }
    }
    ctx.fp.hit("restore.before_done")?;
    // 6. Done.
    ctx.state.journal_delete(&op)?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(backup)
}

pub(crate) fn recover(state: &State, probe: &dyn ProcessProbe) -> Result<Vec<Warning>, EngineError> {
    let mut warnings = Vec::new();
    for entry in state.journal_all()?.into_iter().filter(|e| e.kind == "restore") {
        let data: SwapData = serde_json::from_str(&entry.data).map_err(|e| EngineError::Io(e.to_string()))?;
        if probe.running(&data.process_names) {
            return Err(EngineError::EmulatorRunning(data.process_names.join(", ")));
        }
        let old: Vec<String> = match &data.backup {
            Some(b) => fsx::list_files(b)?.into_iter().map(|(n, _)| n).collect(),
            None => Vec::new(),
        };
        if data.target.is_dir() {
            for (rel, _) in fsx::list_files(&data.target)? {
                if !old.contains(&rel) {
                    std::fs::remove_file(data.target.join(&rel))?;
                }
            }
        }
        if let Some(b) = &data.backup {
            for rel in &old {
                fsx::write_atomic(&data.target.join(rel), &std::fs::read(b.join(rel))?)?;
            }
        }
        state.journal_delete(&entry.op)?;
        let _ = std::fs::remove_dir_all(&data.staging);
        warnings.push(Warning { code: WarningCode::RestoreRolledBack, detail: data.target.display().to_string() });
    }
    Ok(warnings)
}
```

In `lib.rs`: `mod restore;`, and:

```rust
impl Engine {
    pub(crate) fn recover_journal(&self) -> Result<Vec<leemusync_api::Warning>, EngineError> {
        restore::recover(&self.state, self.env.processes.as_ref())
    }
}
```

At the end of `Engine::open`, after building `engine`: `match engine.recover_journal() { Ok(_) | Err(EngineError::EmulatorRunning(_)) => {} Err(e) => return Err(e) }`. Remove the temporary `#[allow(dead_code)]` attributes from Task 10 once clippy is clean without them.

- [ ] **Step 4: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS.
- [ ] **Step 5: Commit.** `docs/specs/reliability.md` §Restore procedure: add "Implemented in `crates/engine/src/restore.rs`; crash tests at every journal step (`restore.after_backup`, `restore.mid_swap`, `restore.before_done`)." Invariants table: mark I1, I3–I6 as "tested (M1a)". `just check`, then:

```bash
git add crates/engine docs/specs/reliability.md
git commit -m "feat(engine): restore with backup, journal and crash rollback

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 15: `engine` sync, account mapping, orphan recovery, status, history, restore, doctor

Spec: [sync-model §Reconciliation](../../specs/sync-model.md#reconciliation) (with Task 1's no-base rules), [players-and-accounts §Discovery](../../specs/players-and-accounts.md#discovery) (choices a/b, recovery), [security §Rollback and withholding](../../specs/security.md#rollback-and-withholding).

**Files:**
- Create: `crates/engine/src/sync.rs`, `crates/engine/src/accounts.rs`, `crates/engine/src/query.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 3–14.
- Produces (public `Engine` methods, all `async` except `confirm_first_sync`):
  - `SyncOptions { dry_run: bool }`; `sync(&mut self, opts: SyncOptions) -> Result<SyncReport, EngineError>`. Refuses with `NeedsConfirmation` until `confirm_first_sync(&self) -> Result<(), EngineError>` has been called once on this device (dry runs are always allowed).
  - `MapChoice::{Ask, Keep, Replace}`, `MapOutcome::{Mapped, NeedsChoice { games: Vec<String> }}`; `accounts_map(&mut self, player: &str, emulator: &str, account: &str, choice: MapChoice) -> Result<MapOutcome, EngineError>`.
  - `accounts_recover(&mut self, emulator: &str, account: &str, player: &str) -> Result<StreamReport, EngineError>`.
  - `status(&mut self) -> Result<StatusReport, EngineError>`; `history(&mut self, player: &str, emulator: &str, game: &str) -> Result<HistoryReport, EngineError>`; `restore(&mut self, player: &str, emulator: &str, game: &str, what: &str) -> Result<StreamReport, EngineError>` (`what` is a version id from `history`, or a backup folder name from `history`'s `backups`); `doctor(&mut self) -> Result<DoctorReport, EngineError>`.
- Behaviour that later tasks and the test day rely on:
  - Streams synced per install: every `(player, game)` in the repo config's `games` for that emulator, plus every non-orphaned local save folder whose account is mapped to a player on this device.
  - A player with versions but no account on this device → `NeedsAccount`, nothing else happens.
  - Before reconciling, if the local files equal a head's files, that head becomes `base` (`Adopt`). This makes a crash between "manifest written" and "base saved" harmless (Task 16 tests it).
  - After a fast-forward, `base_files` carries the restored files' on-disk mtimes, so the next `quick` check reports `SameAsBase`.
  - `restore` of an older version or a backup leaves `base` unchanged. The next `sync` then uploads it as a new version on top of the head, and history is never rewritten.
  - When an emulator runs, its streams are `Skipped` with an `EmulatorRunning` warning (I3).
  - `sync` writes one config version at the end when it learned new games, or when it had to merge config heads.

- [ ] **Step 1: Write `sync.rs`:**

```rust
//! `leemusync sync`: reconcile every stream of every granted install (sync-model §Reconciliation).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use leemusync_api::{API_VERSION, StreamAction, StreamReport, SyncReport, Warning, WarningCode};
use leemusync_core::config::RepoConfig;
use leemusync_core::heads::{Graph, heads};
use leemusync_core::manifest::{FileEntry, Version};
use leemusync_core::profile::Profile;
use leemusync_core::reconcile::{Action, Input, Local, reconcile};
use leemusync_core::{PlayerId, StreamId, VersionId};

use crate::discover::save_folder;
use crate::profiles::Install;
use crate::restore::{self, Ctx, Source};
use crate::snapshot::{self, Quick};
use crate::upload::{UploadMeta, upload};
use crate::{Engine, EngineError, StreamRow, fsx, history_dir, now_ms, repo_config};

pub struct SyncOptions {
    pub dry_run: bool,
}

const FIRST_SYNC: &str = "first_sync_confirmed";

pub(crate) fn action_kind(a: &Action) -> StreamAction {
    match a {
        Action::Nothing => StreamAction::Nothing,
        Action::Adopt(_) => StreamAction::Adopt,
        Action::FastForward(_) => StreamAction::FastForward,
        Action::Upload { .. } => StreamAction::Upload,
        Action::UploadThenConflict { .. } => StreamAction::UploadThenConflict,
        Action::Conflict(_) => StreamAction::Conflict,
        Action::LocalMissing => StreamAction::LocalMissing,
        Action::RemoteBehind => StreamAction::RemoteBehind,
    }
}

fn process_names(profile: &Profile) -> Vec<String> {
    profile.platforms.get("linux").map(|p| p.process.clone()).unwrap_or_default()
}

/// Restored files with their on-disk mtimes, so the next quick check sees "same as base".
fn on_disk(folder: &Path, files: &[FileEntry]) -> Vec<FileEntry> {
    files
        .iter()
        .map(|f| {
            let mtime = std::fs::symlink_metadata(folder.join(f.path.as_str())).map_or(f.mtime, |m| fsx::mtime_ms(&m));
            FileEntry { mtime, ..f.clone() }
        })
        .collect()
}

impl Engine {
    /// # Errors
    /// `Io` if the state DB can't be written.
    pub fn confirm_first_sync(&self) -> Result<(), EngineError> {
        self.state.set_meta(FIRST_SYNC, &now_ms().to_string())
    }

    pub(crate) async fn fetch_graph(
        &self,
        stream: &StreamId,
        warnings: &mut Vec<Warning>,
    ) -> Result<BTreeMap<VersionId, Version>, EngineError> {
        let repo = self.repo_ref()?;
        let mut out = BTreeMap::new();
        for id in repo.list_versions(stream).await? {
            let json = match self.state.cached_manifest(stream, &id)? {
                Some(j) => j,
                None => match repo.get_version(stream, &id).await {
                    Ok(j) => j,
                    Err(EngineError::Data(d)) => {
                        warnings.push(Warning { code: WarningCode::InvalidVersion, detail: d });
                        continue;
                    }
                    Err(e) => return Err(e),
                },
            };
            match Version::from_json(&json, stream, &id) {
                Ok(v) => {
                    self.state.cache_manifest(stream, &id, &json)?;
                    out.insert(id, v);
                }
                Err(e) => warnings.push(Warning { code: WarningCode::InvalidVersion, detail: e.to_string() }),
            }
        }
        Ok(out)
    }

    /// # Errors
    /// `NeedsConfirmation` on the first real sync; storage, crypto and file errors abort the run.
    pub async fn sync(&mut self, opts: SyncOptions) -> Result<SyncReport, EngineError> {
        self.ensure_repo().await?;
        if !opts.dry_run && self.state.meta(FIRST_SYNC)?.is_none() {
            return Err(EngineError::NeedsConfirmation);
        }
        let mut warnings = self.recover_journal()?;
        let device = self.device_id();
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        if loaded.invalid > 0 {
            warnings.push(Warning { code: WarningCode::InvalidVersion, detail: format!("{} config versions", loaded.invalid) });
        }
        for name in &loaded.renamed {
            warnings.push(Warning { code: WarningCode::PlayerRenamed, detail: name.clone() });
        }
        let merged = loaded.heads.len() > 1;
        let config = loaded.config;
        let mut streams = Vec::new();
        let mut new_games: Vec<(PlayerId, String, String)> = Vec::new();
        for (profile, install, discovery) in self.installs()? {
            let Some(discovery) = discovery else {
                warnings.push(Warning { code: WarningCode::NotGranted, detail: install.base.display().to_string() });
                continue;
            };
            let names = process_names(&profile);
            let running = self.env.processes.running(&names);
            if running {
                warnings.push(Warning { code: WarningCode::EmulatorRunning, detail: profile.name.clone() });
            }
            let mut keys: BTreeSet<(PlayerId, String)> = BTreeSet::new();
            for player in config.players.keys() {
                for (emu, game) in config.games_of(*player) {
                    if emu == profile.id {
                        keys.insert((*player, game));
                    }
                }
            }
            for save in discovery.saves.iter().filter(|s| !s.orphaned) {
                if let Some(p) = config.player_on_account(device, &profile.id, &save.account) {
                    keys.insert((p, save.game.clone()));
                }
            }
            for (player, game) in keys {
                let report = if running {
                    StreamReport {
                        player: config.players.get(&player).map(|p| p.name.clone()).unwrap_or_default(),
                        emulator: profile.id.clone(),
                        game: game.clone(),
                        account: config.account_of(device, player, &profile.id).map(str::to_owned),
                        action: StreamAction::Skipped,
                        version: None,
                        heads: vec![],
                        done: false,
                    }
                } else {
                    self.sync_stream(&profile, &install, &config, player, &game, opts.dry_run, &mut warnings).await?
                };
                // Adopt counts too: after a crash between manifest and base, the version exists but the
                // game may not be listed yet.
                let has_version = report.done
                    && matches!(report.action, StreamAction::Upload | StreamAction::UploadThenConflict | StreamAction::Adopt);
                if has_version && !config.games_of(player).contains(&(profile.id.clone(), game.clone())) {
                    new_games.push((player, profile.id.clone(), game.clone()));
                }
                streams.push(report);
            }
        }
        if !opts.dry_run && (merged || !new_games.is_empty()) {
            if merged {
                warnings.push(Warning { code: WarningCode::ConfigMerged, detail: String::new() });
            }
            repo_config::update(self.repo_ref()?, &self.state, device, |c| {
                for (p, e, g) in &new_games {
                    c.add_game(*p, e, g);
                }
                Ok(())
            })
            .await?;
        }
        Ok(SyncReport { api_version: API_VERSION, dry_run: opts.dry_run, streams, warnings })
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one stream's whole decision, kept in one place
    async fn sync_stream(
        &self,
        profile: &Profile,
        install: &Install,
        config: &RepoConfig,
        player: PlayerId,
        game: &str,
        dry_run: bool,
        warnings: &mut Vec<Warning>,
    ) -> Result<StreamReport, EngineError> {
        let device = self.device_id();
        let repo = self.repo_ref()?;
        let stream = repo.stream_id(player, &profile.id, game);
        let account = config.account_of(device, player, &profile.id).map(str::to_owned);
        let mut report = StreamReport {
            player: config.players.get(&player).map(|p| p.name.clone()).unwrap_or_default(),
            emulator: profile.id.clone(),
            game: game.to_owned(),
            account: account.clone(),
            action: StreamAction::Nothing,
            version: None,
            heads: vec![],
            done: false,
        };
        let versions = self.fetch_graph(&stream, warnings).await?;
        let graph: Graph = versions.iter().map(|(id, v)| (*id, v.parents.clone())).collect();
        let heads = heads(&graph);
        report.heads = heads.iter().map(ToString::to_string).collect();
        let Some(account) = account else {
            if !heads.is_empty() {
                report.action = StreamAction::NeedsAccount;
            }
            return Ok(report);
        };
        let folder = save_folder(profile, install, game, &account)?;
        let mut row = self.state.stream(&stream)?.unwrap_or(StreamRow {
            stream,
            player,
            emulator: profile.id.clone(),
            game: game.to_owned(),
            base: None,
            base_files: vec![],
            highest_seen: None,
            replace_local: false,
        });
        if let Some(seen) = row.highest_seen {
            if !versions.contains_key(&seen) {
                warnings.push(Warning { code: WarningCode::RemoteBehind, detail: format!("{} {game}", report.player) });
            }
        }
        row.highest_seen = versions.keys().max().copied().max(row.highest_seen);

        let history = history_dir(&self.env.locations.data_dir, &stream);
        let mut snap = None;
        let mut local = match snapshot::quick(&folder, &row.base_files)? {
            Quick::Missing => Local::Missing,
            Quick::SameAsBase if row.base.is_some() => Local::Clean,
            _ => Local::Dirty,
        };
        if local == Local::Dirty {
            let dest = history.join(format!("{}-snapshot", now_ms()));
            snap = snapshot::take(&folder, &dest, profile.max_file_bytes(), &|d| repo.blob_id(d))?;
            match &snap {
                None => local = Local::Missing,
                Some(s) if row.base.is_some() && snapshot::same_files(&s.files, &row.base_files) => {
                    local = Local::Clean;
                    row.base_files.clone_from(&s.files);
                    let _ = std::fs::remove_dir_all(&dest);
                    snap = None;
                }
                Some(_) => {}
            }
        }
        let matches = snap
            .as_ref()
            .and_then(|s| heads.iter().find(|h| versions.get(h).is_some_and(|v| v.same_content(&s.files))).copied());
        let mut adopted = false;
        if let (Some(h), Some(_)) = (matches, row.base) {
            if row.base != Some(h) {
                adopted = true;
            }
            row.base = Some(h);
            row.base_files = snap.as_ref().map(|s| s.files.clone()).unwrap_or_default();
            local = Local::Clean;
        }
        let action = reconcile(&Input { base: row.base, local, graph: &graph, local_matches: matches, replace_local: row.replace_local });
        report.action = if adopted && action == Action::Nothing { StreamAction::Adopt } else { action_kind(&action) };
        if dry_run {
            if let Some(s) = &snap {
                let _ = std::fs::remove_dir_all(&s.dir);
            }
            return Ok(report);
        }
        match &action {
            Action::Nothing | Action::Conflict(_) | Action::LocalMissing | Action::RemoteBehind => {}
            Action::Adopt(h) => {
                row.base = Some(*h);
                row.base_files = snap.as_ref().map(|s| s.files.clone()).unwrap_or_default();
            }
            Action::Upload { parents } | Action::UploadThenConflict { parents } => {
                let s = snap.as_ref().ok_or_else(|| EngineError::Data("upload without a snapshot".into()))?;
                let meta = UploadMeta {
                    stream,
                    parents: parents.clone(),
                    device,
                    player,
                    emulator: &profile.id,
                    base_files: &row.base_files,
                    note: None,
                };
                let v = upload(repo, s, meta, &self.failpoints).await?;
                report.version = Some(v.id.to_string());
                row.base = Some(v.id);
                row.base_files.clone_from(&v.files);
                row.highest_seen = row.highest_seen.max(Some(v.id));
                row.replace_local = false;
            }
            Action::FastForward(h) => {
                let v = versions.get(h).ok_or_else(|| EngineError::Data("head vanished".into()))?;
                let names = process_names(profile);
                let ctx = Ctx {
                    repo: Some(repo),
                    state: &self.state,
                    data_dir: &self.env.locations.data_dir,
                    grants: &self.config.grants,
                    probe: self.env.processes.as_ref(),
                    process_names: &names,
                    fp: &self.failpoints,
                };
                restore::restore(&ctx, &stream, &folder, Source::Remote(v), "pre-restore").await?;
                report.version = Some(h.to_string());
                row.base = Some(*h);
                row.base_files = on_disk(&folder, &v.files);
                row.replace_local = false;
            }
        }
        report.done = true;
        self.state.put_stream(&row)?;
        Ok(report)
    }
}
```

Check the `Adopt` path against core: with no `base` and a matching head, `reconcile` returns `Action::Adopt(h)`. With a `base`, the pre-adopt block sets `base = h` and `local = Clean`, so `reconcile` returns `Nothing`, which is reported as `Adopt`.

- [ ] **Step 2: Write `accounts.rs`:**

```rust
//! `leemusync accounts map` (choices a/b, keep or replace) and `accounts recover`
//! (players-and-accounts §Discovery).

use leemusync_api::{StreamAction, StreamReport};
use leemusync_core::PlayerId;

use crate::discover::save_folder;
use crate::snapshot;
use crate::upload::{UploadMeta, upload};
use crate::{Engine, EngineError, StreamRow, fsx, history_dir, now_ms, repo_config};

pub enum MapChoice {
    Ask,
    Keep,
    Replace,
}

pub enum MapOutcome {
    Mapped,
    /// The account has files and the player has versions of these games: keep or replace?
    NeedsChoice { games: Vec<String> },
}

impl Engine {
    fn player_id(config: &leemusync_core::config::RepoConfig, name: &str) -> Result<PlayerId, EngineError> {
        config.player_named(name).ok_or_else(|| EngineError::Usage(format!("no player `{name}`; run `leemusync players add`")))
    }

    /// # Errors
    /// `Usage` for an unknown player, an account this device doesn't have, or one mapped to another player.
    pub async fn accounts_map(
        &mut self,
        player: &str,
        emulator: &str,
        account: &str,
        choice: MapChoice,
    ) -> Result<MapOutcome, EngineError> {
        self.ensure_repo().await?;
        let device = self.device_id();
        let installs = self.installs()?;
        let (profile, install, discovery) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no {emulator} install found; run `leemusync scan`")))?;
        let discovery = discovery.as_ref().ok_or_else(|| EngineError::NotGranted(install.base.display().to_string()))?;
        if !discovery.accounts.iter().any(|a| a.id == account) {
            return Err(EngineError::Usage(format!(
                "{} has no account {account} on this device; create it in {}, then run `leemusync scan`",
                profile.name, profile.name
            )));
        }
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        let pid = Self::player_id(&loaded.config, player)?;
        let mut relevant = Vec::new();
        for (emu, game) in loaded.config.games_of(pid) {
            if emu != profile.id {
                continue;
            }
            let folder = save_folder(profile, install, &game, account)?;
            let has_files = folder.is_dir() && !fsx::list_files(&folder)?.is_empty();
            let stream = self.repo_ref()?.stream_id(pid, &profile.id, &game);
            if has_files && !self.repo_ref()?.list_versions(&stream).await?.is_empty() {
                relevant.push(game);
            }
        }
        if !relevant.is_empty() && matches!(choice, MapChoice::Ask) {
            return Ok(MapOutcome::NeedsChoice { games: relevant });
        }
        repo_config::update(self.repo_ref()?, &self.state, device, |c| {
            c.map_account(device, pid, &profile.id, account).map_err(|e| EngineError::Usage(e.to_string()))
        })
        .await?;
        if matches!(choice, MapChoice::Replace) {
            for game in relevant {
                let stream = self.repo_ref()?.stream_id(pid, &profile.id, &game);
                let mut row = self.state.stream(&stream)?.unwrap_or(StreamRow {
                    stream,
                    player: pid,
                    emulator: profile.id.clone(),
                    game: game.clone(),
                    base: None,
                    base_files: vec![],
                    highest_seen: None,
                    replace_local: false,
                });
                row.base = None;
                row.base_files = vec![];
                row.replace_local = true;
                self.state.put_stream(&row)?;
            }
        }
        Ok(MapOutcome::Mapped)
    }

    /// Uploads an orphaned save folder as the first version of `player`'s stream for that game.
    ///
    /// # Errors
    /// `Usage` if the folder isn't orphaned or the player already has a version of the game.
    pub async fn accounts_recover(&mut self, emulator: &str, account: &str, player: &str) -> Result<StreamReport, EngineError> {
        self.ensure_repo().await?;
        let device = self.device_id();
        let installs = self.installs()?;
        let (profile, _, discovery) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no {emulator} install found")))?;
        let discovery = discovery.as_ref().ok_or_else(|| EngineError::NotSetUp("grant the emulator folder first"))?;
        let save = discovery
            .saves
            .iter()
            .find(|s| s.account == account && s.orphaned)
            .ok_or_else(|| EngineError::Usage(format!("{account} isn't an orphaned save folder here")))?;
        let names = profile.platforms.get("linux").map(|p| p.process.clone()).unwrap_or_default();
        if self.env.processes.running(&names) {
            return Err(EngineError::EmulatorRunning(profile.name.clone()));
        }
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        let pid = Self::player_id(&loaded.config, player)?;
        let repo = self.repo_ref()?;
        let stream = repo.stream_id(pid, &profile.id, &save.game);
        if !repo.list_versions(&stream).await?.is_empty() {
            return Err(EngineError::Usage(format!(
                "{player} already has a save for {}; recover it to a new player instead",
                save.game
            )));
        }
        let dest = history_dir(&self.env.locations.data_dir, &stream).join(format!("{}-recover", now_ms()));
        let snap = snapshot::take(&save.path, &dest, profile.max_file_bytes(), &|d| repo.blob_id(d))?
            .ok_or_else(|| EngineError::Usage("that folder is empty".into()))?;
        let meta = UploadMeta {
            stream,
            parents: vec![],
            device,
            player: pid,
            emulator: &profile.id,
            base_files: &[],
            note: Some(format!("recovered from orphaned folder {account} on {}", self.config.device.name)),
        };
        let v = upload(repo, &snap, meta, &self.failpoints).await?;
        let game = save.game.clone();
        repo_config::update(self.repo_ref()?, &self.state, device, |c| {
            c.add_game(pid, &profile.id, &game);
            Ok(())
        })
        .await?;
        Ok(StreamReport {
            player: player.to_owned(),
            emulator: profile.id.clone(),
            game,
            account: None,
            action: StreamAction::Upload,
            version: Some(v.id.to_string()),
            heads: vec![v.id.to_string()],
            done: true,
        })
    }
}
```

- [ ] **Step 3: Write `query.rs`:**

```rust
//! `status`, `history`, `restore`, `doctor`.

use leemusync_api::{API_VERSION, Check, DoctorReport, HistoryEntry, HistoryReport, StatusReport, StreamAction, StreamReport, StreamStatus};
use leemusync_core::heads::{Graph, heads};

use crate::discover::save_folder;
use crate::repo::REPO_JSON;
use crate::restore::{self, Ctx, Source};
use crate::{BackendConfig, Engine, EngineError, history_dir, repo_config};

impl Engine {
    /// # Errors
    /// None for a device without a repo (fields stay empty); storage errors otherwise.
    pub async fn status(&mut self) -> Result<StatusReport, EngineError> {
        let device = self.device_id();
        let mut players = Vec::new();
        let mut streams = Vec::new();
        if self.config.repo.is_some() {
            self.ensure_repo().await?;
            let config = repo_config::load(self.repo_ref()?, &self.state).await?.config;
            for (pid, p) in &config.players {
                players.push(p.name.clone());
                for (emu, game) in config.games_of(*pid) {
                    let stream = self.repo_ref()?.stream_id(*pid, &emu, &game);
                    let base = self.state.stream(&stream)?.and_then(|r| r.base).map(|b| b.to_string());
                    let account = config.account_of(device, *pid, &emu).map(str::to_owned);
                    streams.push(StreamStatus { player: p.name.clone(), emulator: emu, game, account, base });
                }
            }
        }
        Ok(StatusReport {
            api_version: API_VERSION,
            device: self.config.device.name.clone(),
            device_id: device.to_string(),
            repo: self.config.repo.clone(),
            backend: self.config.backend.as_ref().map(|b| match b {
                BackendConfig::Gdrive { .. } => "gdrive".to_owned(),
                BackendConfig::Fs { path } => format!("fs {}", path.display()),
            }),
            grants: self.config.grants.iter().map(|g| g.display().to_string()).collect(),
            players,
            streams,
        })
    }

    async fn stream_of(&mut self, player: &str, emulator: &str, game: &str) -> Result<leemusync_core::StreamId, EngineError> {
        self.ensure_repo().await?;
        let config = repo_config::load(self.repo_ref()?, &self.state).await?.config;
        let pid = config.player_named(player).ok_or_else(|| EngineError::Usage(format!("no player `{player}`")))?;
        Ok(self.repo_ref()?.stream_id(pid, emulator, &game.to_lowercase()))
    }

    /// # Errors
    /// `Usage` for an unknown player; storage errors.
    pub async fn history(&mut self, player: &str, emulator: &str, game: &str) -> Result<HistoryReport, EngineError> {
        let stream = self.stream_of(player, emulator, game).await?;
        let mut warnings = Vec::new();
        let versions = self.fetch_graph(&stream, &mut warnings).await?;
        let graph: Graph = versions.iter().map(|(id, v)| (*id, v.parents.clone())).collect();
        let heads = heads(&graph);
        let entries = versions
            .values()
            .rev()
            .map(|v| HistoryEntry {
                id: v.id.to_string(),
                created_at: v.created_at,
                device: v.device.to_string(),
                files: u32::try_from(v.files.len()).unwrap_or(u32::MAX),
                bytes: v.files.iter().map(|f| f.size).sum(),
                parents: v.parents.iter().map(ToString::to_string).collect(),
                head: heads.contains(&v.id),
            })
            .collect();
        let dir = history_dir(&self.env.locations.data_dir, &stream);
        let mut backups: Vec<String> = std::fs::read_dir(&dir)
            .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        backups.sort();
        backups.reverse();
        Ok(HistoryReport { api_version: API_VERSION, versions: entries, backups })
    }

    /// Restores a version or a local backup into the mapped account (backup first). `base` is
    /// unchanged, so the next `sync` uploads the result as a new version.
    ///
    /// # Errors
    /// `Usage` for an unknown version, backup, player or mapping; restore errors.
    pub async fn restore(&mut self, player: &str, emulator: &str, game: &str, what: &str) -> Result<StreamReport, EngineError> {
        let stream = self.stream_of(player, emulator, game).await?;
        self.recover_journal()?;
        let device = self.device_id();
        let config = repo_config::load(self.repo_ref()?, &self.state).await?.config;
        let pid = config.player_named(player).ok_or_else(|| EngineError::Usage(format!("no player `{player}`")))?;
        let account = config
            .account_of(device, pid, emulator)
            .ok_or_else(|| EngineError::Usage(format!("{player} has no {emulator} account on this device")))?
            .to_owned();
        let installs = self.installs()?;
        let (profile, install, _) = installs
            .iter()
            .find(|(p, _, _)| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("no {emulator} install found")))?;
        let folder = save_folder(profile, install, &game.to_lowercase(), &account)?;
        let names = profile.platforms.get("linux").map(|p| p.process.clone()).unwrap_or_default();
        let mut warnings = Vec::new();
        let versions = self.fetch_graph(&stream, &mut warnings).await?;
        let backup_dir = history_dir(&self.env.locations.data_dir, &stream).join(what);
        let ctx = Ctx {
            repo: Some(self.repo_ref()?),
            state: &self.state,
            data_dir: &self.env.locations.data_dir,
            grants: &self.config.grants,
            probe: self.env.processes.as_ref(),
            process_names: &names,
            fp: &self.failpoints,
        };
        if let Some(v) = versions.values().find(|v| v.id.to_string().eq_ignore_ascii_case(what)) {
            restore::restore(&ctx, &stream, &folder, Source::Remote(v), "pre-restore").await?;
        } else if !what.contains(['/', '\\']) && what != ".." && backup_dir.is_dir() {
            restore::restore(&ctx, &stream, &folder, Source::Local(&backup_dir), "pre-restore").await?;
        } else {
            return Err(EngineError::Usage(format!("`{what}` is neither a version nor a backup; see `leemusync history`")));
        }
        Ok(StreamReport {
            player: player.to_owned(),
            emulator: emulator.to_owned(),
            game: game.to_lowercase(),
            account: Some(account),
            action: StreamAction::FastForward,
            version: Some(what.to_owned()),
            heads: vec![],
            done: true,
        })
    }

    /// # Errors
    /// Never for failed checks (they're reported); only for a broken bundled profile.
    pub async fn doctor(&mut self) -> Result<DoctorReport, EngineError> {
        let mut checks = Vec::new();
        let probe = "leemusync-doctor-probe";
        let keystore = self.env.secrets.set(probe, "ok").and_then(|()| self.env.secrets.get(probe)).and_then(|v| {
            self.env.secrets.delete(probe)?;
            Ok(v)
        });
        checks.push(Check { name: "keystore".into(), ok: keystore.is_ok(), detail: keystore.err().map(|e| e.to_string()).unwrap_or_default() });
        let backend = match self.store() {
            Ok(s) => s.stat(REPO_JSON).await.map(|e| if e.is_some() { "repo found" } else { "no repo yet" }).map_err(EngineError::from),
            Err(e) => Err(e),
        };
        checks.push(Check { name: "backend".into(), ok: backend.is_ok(), detail: backend.map_or_else(|e| e.to_string(), str::to_owned) });
        if self.config.repo.is_some() {
            let unlocked = self.ensure_repo().await;
            checks.push(Check { name: "repo".into(), ok: unlocked.is_ok(), detail: unlocked.err().map(|e| e.to_string()).unwrap_or_default() });
        }
        for (profile, install, discovery) in self.installs()? {
            checks.push(Check {
                name: format!("{} folder", profile.id),
                ok: discovery.is_some(),
                detail: install.base.display().to_string(),
            });
            let names = profile.platforms.get("linux").map(|p| p.process.clone()).unwrap_or_default();
            let running = self.env.processes.running(&names);
            checks.push(Check {
                name: format!("{} running", profile.id),
                ok: true,
                detail: if running { "running".into() } else { "closed".into() },
            });
        }
        Ok(DoctorReport { api_version: API_VERSION, checks })
    }
}
```

- [ ] **Step 4: Wire `lib.rs`:** `mod accounts; mod query; mod sync;` and `pub use accounts::{MapChoice, MapOutcome}; pub use sync::SyncOptions;`.
- [ ] **Step 5: Engine-level tests.** Add to `crates/engine/tests/common/mod.rs`:

```rust
use leemusync_engine::{Engine, JoinSecret, MapChoice, MapOutcome, SyncOptions};

pub const SAVE: &str = "mlc01/usr/save/00050000/101c9400/user";

/// One simulated PC: its own config, data, keystore and Cemu tree. Shared remote at `remote`.
pub struct Pc {
    pub root: PathBuf,
    pub cemu: PathBuf,
    pub probe: Arc<FakeProbe>,
    pub secrets: Arc<MemorySecrets>,
    pub engine: Engine,
}

impl Pc {
    /// A PC with one Cemu account `80000001` whose BotW save contains `content`.
    pub fn new(root: &Path, flatpak: bool, content: &[u8]) -> Self {
        let cemu = install_fixture(root, flatpak);
        std::fs::remove_dir_all(cemu.join("mlc01/usr/save/system/act/80000002")).unwrap();
        std::fs::remove_dir_all(cemu.join(SAVE).join("80000002")).unwrap();
        std::fs::write(cemu.join(SAVE).join("80000001/0/game_data.sav"), content).unwrap();
        let probe = Arc::new(FakeProbe::default());
        let secrets = Arc::new(MemorySecrets::default());
        let mut engine = Engine::open(env_with(root, probe.clone(), secrets.clone())).unwrap();
        let grant = if flatpak { root.join("home/.var/app/info.cemu.Cemu") } else { cemu.clone() };
        engine.grant_add(&grant).unwrap();
        if !flatpak {
            engine.grant_add(&root.join("home/.config/Cemu")).unwrap();
        }
        Pc { root: root.to_path_buf(), cemu, probe, secrets, engine }
    }

    /// Simulates a crash and restart: drops the engine (releasing the lock) and opens a new one.
    #[must_use]
    pub fn reopen(self) -> Self {
        let Pc { root, cemu, probe, secrets, engine } = self;
        drop(engine);
        let engine = Engine::open(env_with(&root, probe.clone(), secrets.clone())).unwrap();
        Pc { root, cemu, probe, secrets, engine }
    }

    pub fn save(&self, account: &str) -> Vec<u8> {
        std::fs::read(self.cemu.join(SAVE).join(account).join("0/game_data.sav")).unwrap()
    }

    /// Creates a Cemu account folder by copying 80000001's account.dat (what choice b does by hand).
    pub fn create_account(&self, id: &str) {
        let act = self.cemu.join("mlc01/usr/save/system/act");
        fsx::copy_tree(&act.join("80000001"), &act.join(id)).unwrap();
    }

    pub async fn sync(&mut self) -> leemusync_api::SyncReport {
        self.engine.confirm_first_sync().unwrap();
        self.engine.sync(SyncOptions { dry_run: false }).await.unwrap()
    }
}

pub async fn map(pc: &mut Pc, player: &str, account: &str, choice: MapChoice) -> MapOutcome {
    pc.engine.accounts_map(player, "cemu", account, choice).await.unwrap()
}

pub async fn join(pc: &mut Pc, remote: &Path) {
    pc.engine.backend_add_fs(remote).unwrap();
    pc.engine.join(JoinSecret::Passphrase("pass phrase".into())).await.unwrap();
}
```

Create `crates/engine/tests/sync.rs`:

```rust
//! Sync behaviour on one or two simulated PCs (fake data).

mod common;

use common::{Pc, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{EngineError, MapChoice, SyncOptions};

#[tokio::test]
async fn first_sync_needs_confirmation_and_uploads() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"A's save");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    let dry = a.engine.sync(SyncOptions { dry_run: true }).await.unwrap();
    assert_eq!(dry.streams[0].action, StreamAction::Upload);
    assert!(matches!(a.engine.sync(SyncOptions { dry_run: false }).await, Err(EngineError::NeedsConfirmation)));
    let r = a.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::Upload);
    assert!(r.streams[0].done);
    assert_eq!(a.sync().await.streams[0].action, StreamAction::Nothing, "second sync has nothing to do");
}

#[tokio::test]
async fn emulator_running_skips_everything() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Pc::new(&root.path().join("a"), false, b"x");
    a.engine.backend_add_fs(&root.path().join("remote")).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.probe.set(true);
    let r = a.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::Skipped);
    assert!(r.warnings.iter().any(|w| w.code == leemusync_api::WarningCode::EmulatorRunning));
}

#[tokio::test]
async fn edit_on_one_pc_fast_forwards_the_other() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"v1");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.path().join("b"), false, b"other");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    assert_eq!(b.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(b.save("80000002"), b"v1");
    // A different length, so the size changes even if the mtime lands in the same millisecond.
    std::fs::write(b.cemu.join(common::SAVE).join("80000002/0/game_data.sav"), b"version two").unwrap();
    assert_eq!(b.sync().await.streams[0].action, StreamAction::Upload);
    assert_eq!(a.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(a.save("80000001"), b"version two");
}
```

- [ ] **Step 6: Run** `cargo test -p leemusync-engine && cargo clippy -p leemusync-engine --all-targets -- -D warnings` → PASS. If a test fails, use skill `superpowers:systematic-debugging` before changing code. Never weaken an assertion to make it pass.
- [ ] **Step 7: Commit.** `docs/specs/sync-model.md` **Code:** add `` `crates/engine/src/sync.rs` ``, and Status `Partial` (leases, play sessions and retention are still open). `docs/specs/players-and-accounts.md` **Code:** `` `crates/core/src/config.rs`, `crates/engine/src/accounts.rs` ``, Status `Partial`. `just check`, then:

```bash
git add crates/engine docs/specs
git commit -m "feat(engine): sync streams, map accounts and recover orphaned saves

Reconciliation with adopt-on-equal, backup-first fast-forwards, no-account
reporting, keep/replace when mapping, and status/history/restore/doctor.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 16: Crash tests and the test day, simulated

Spec: [testing §Layers](../../specs/testing.md#layers) (simulation, fault injection), [reliability §Invariants](../../specs/reliability.md#invariants), the runbook in Task 18. Nothing touches real saves.

**Files:**
- Create: `crates/engine/tests/faults.rs`, `crates/engine/tests/two_devices.rs`

**Interfaces:**
- Consumes: `tests/common/mod.rs` (`Pc`, `Pc::reopen`, `join`, `map`, `SAVE`) from Tasks 12 and 15; every public `Engine` method.

- [ ] **Step 1: Write `faults.rs`:**

```rust
//! Kill the engine at every journal step; on restart nothing is half old, half new (I1, I5).

mod common;

use common::{Pc, SAVE, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{EngineError, MapChoice, SyncOptions};

async fn two_pcs(root: &std::path::Path) -> (Pc, Pc) {
    let remote = root.join("remote");
    let mut a = Pc::new(&root.join("a"), true, b"version one");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.join("b"), false, b"b's own");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    b.sync().await;
    std::fs::write(a.cemu.join(SAVE).join("80000001/0/game_data.sav"), b"version two!").unwrap();
    a.sync().await;
    (a, b)
}

#[tokio::test]
async fn a_crash_during_a_fast_forward_rolls_back_then_retries() {
    for point in ["restore.after_backup", "restore.mid_swap", "restore.before_done"] {
        let root = tempfile::tempdir().unwrap();
        let (_a, b) = two_pcs(root.path()).await;
        assert_eq!(b.save("80000002"), b"version one");
        b.engine.arm_failpoint(point);
        let mut b = b;
        let r = b.engine.sync(SyncOptions { dry_run: false }).await;
        assert!(matches!(r, Err(EngineError::Injected(_))), "{point}");
        let mut b = b.reopen();
        assert_eq!(b.save("80000002"), b"version one", "{point}: rolled back on restart");
        let again = b.sync().await;
        assert_eq!(again.streams[0].action, StreamAction::FastForward, "{point}");
        assert_eq!(b.save("80000002"), b"version two!", "{point}");
    }
}

#[tokio::test]
async fn a_crash_around_the_manifest_never_makes_a_conflict() {
    for point in ["upload.before_manifest", "upload.after_manifest"] {
        let root = tempfile::tempdir().unwrap();
        let mut a = Pc::new(&root.path().join("a"), true, b"first");
        a.engine.backend_add_fs(&root.path().join("remote")).unwrap();
        a.engine.init("pass phrase").await.unwrap();
        a.engine.players_add("Ana").await.unwrap();
        map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
        a.engine.confirm_first_sync().unwrap();
        a.engine.arm_failpoint(point);
        assert!(a.engine.sync(SyncOptions { dry_run: false }).await.is_err(), "{point}");
        let mut a = a.reopen();
        let r = a.sync().await;
        let expected = if point == "upload.after_manifest" { StreamAction::Adopt } else { StreamAction::Upload };
        assert_eq!(r.streams[0].action, expected, "{point}");
        assert_eq!(a.sync().await.streams[0].action, StreamAction::Nothing, "{point}");
        let history = a.engine.history("Ana", "cemu", "101c9400").await.unwrap();
        assert_eq!(history.versions.len(), 1, "{point}: exactly one version, no conflict");
    }
}
```

The `after_manifest` case relies on two Task 15 rules. Adopt-on-equal turns the leftover version into `base` instead of a conflict. `sync` also records the game in the repo config on `Adopt`, so other devices still find the stream.

- [ ] **Step 2: Write `two_devices.rs`.** It mirrors the Task 18 runbook step for step:

```rust
//! The M1a test day on two simulated PCs: Fedora (Flatpak Cemu) and CachyOS (native Cemu).

mod common;

use common::{Pc, SAVE, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{MapChoice, MapOutcome};

#[tokio::test]
async fn both_saves_end_up_on_both_pcs() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut fedora = Pc::new(&root.path().join("fedora"), true, b"Fedora's BotW");
    let mut cachy = Pc::new(&root.path().join("cachy"), false, b"CachyOS's BotW");
    // CachyOS has an orphaned folder left by a deleted account (seen on the real laptop).
    let orphan = cachy.cemu.join(SAVE).join("80000002");
    std::fs::create_dir_all(orphan.join("0")).unwrap();
    std::fs::write(orphan.join("0/game_data.sav"), b"old orphan").unwrap();

    // Runbook step 3: CachyOS creates the repo.
    cachy.engine.backend_add_fs(&remote).unwrap();
    let (_, recovery) = cachy.engine.init("pass phrase").await.unwrap();
    assert!(!recovery.is_empty());
    cachy.engine.players_add("Cachy").await.unwrap();
    cachy.engine.players_add("Fedora").await.unwrap();
    let scan = cachy.engine.scan().await.unwrap();
    assert!(scan.installs[0].saves.iter().any(|s| s.account == "80000002" && s.orphaned));
    assert!(matches!(map(&mut cachy, "Cachy", "80000001", MapChoice::Ask).await, MapOutcome::Mapped));

    // Step 4: Fedora joins.
    join(&mut fedora, &remote).await;
    map(&mut fedora, "Fedora", "80000001", MapChoice::Ask).await;

    // Step 5: first pushes.
    assert_eq!(cachy.sync().await.streams[0].action, StreamAction::Upload);
    let r = fedora.sync().await;
    let action_of = |r: &leemusync_api::SyncReport, p: &str| r.streams.iter().find(|s| s.player == p).map(|s| s.action);
    assert_eq!(action_of(&r, "Fedora"), Some(StreamAction::Upload));
    assert_eq!(action_of(&r, "Cachy"), Some(StreamAction::NeedsAccount));

    // Step 6, Fedora: choice b (the account is created in Cemu by hand).
    fedora.create_account("80000002");
    map(&mut fedora, "Cachy", "80000002", MapChoice::Ask).await;
    assert_eq!(action_of(&fedora.sync().await, "Cachy"), Some(StreamAction::FastForward));

    // Step 6, CachyOS: keep the orphan as its own player first, then choice a with Replace.
    cachy.engine.players_add("Old CachyOS").await.unwrap();
    cachy.engine.accounts_recover("cemu", "80000002", "Old CachyOS").await.unwrap();
    cachy.create_account("80000002");
    match map(&mut cachy, "Fedora", "80000002", MapChoice::Ask).await {
        MapOutcome::NeedsChoice { games } => assert_eq!(games, vec!["101c9400".to_string()]),
        MapOutcome::Mapped => panic!("the folder has files, so the CLI must ask keep or replace"),
    }
    map(&mut cachy, "Fedora", "80000002", MapChoice::Replace).await;
    assert_eq!(action_of(&cachy.sync().await, "Fedora"), Some(StreamAction::FastForward));

    // Step 7: both saves on both PCs, byte for byte.
    assert_eq!(fedora.save("80000001"), b"Fedora's BotW");
    assert_eq!(fedora.save("80000002"), b"CachyOS's BotW");
    assert_eq!(cachy.save("80000001"), b"CachyOS's BotW");
    assert_eq!(cachy.save("80000002"), b"Fedora's BotW");
    for pc in [&mut fedora, &mut cachy] {
        for s in pc.sync().await.streams {
            let ok = s.action == StreamAction::Nothing || (s.player == "Old CachyOS" && s.action == StreamAction::NeedsAccount);
            assert!(ok, "{} {:?}", s.player, s.action);
        }
    }
    let old = cachy.engine.history("Old CachyOS", "cemu", "101c9400").await.unwrap();
    assert_eq!(old.versions.len(), 1, "the orphan's save is kept in the repo");
    let fedora_backups = cachy.engine.history("Fedora", "cemu", "101c9400").await.unwrap().backups;
    assert!(fedora_backups.iter().any(|b| b.ends_with("-pre-restore")), "the replaced files went to local history");

    // A forced double edit becomes a conflict with nothing lost (SC3 preview).
    std::fs::write(fedora.cemu.join(SAVE).join("80000002/0/game_data.sav"), b"edited on Fedora").unwrap();
    std::fs::write(cachy.cemu.join(SAVE).join("80000001/0/game_data.sav"), b"edited on CachyOS").unwrap();
    assert_eq!(action_of(&fedora.sync().await, "Cachy"), Some(StreamAction::Upload));
    assert_eq!(action_of(&cachy.sync().await, "Cachy"), Some(StreamAction::UploadThenConflict));
    let h = cachy.engine.history("Cachy", "cemu", "101c9400").await.unwrap();
    assert_eq!(h.versions.iter().filter(|v| v.head).count(), 2);
    assert_eq!(cachy.save("80000001"), b"edited on CachyOS", "the local copy is kept while the conflict is open");
}
```

- [ ] **Step 3: Run** `cargo test -p leemusync-engine --test faults --test two_devices` → PASS. If one fails, debug with `superpowers:systematic-debugging`; the tests are the spec.
- [ ] **Step 4: Commit.** `docs/specs/testing.md` §Layers: add "Simulation: `crates/engine/tests/two_devices.rs` (the M1a test day). Fault injection: `crates/engine/tests/faults.rs` (feature `failpoints`)." `just check`, then `test(engine): simulate the M1a test day and crash at every journal step` with the trailer.

---
### Task 17: `cli`: commands, prompts, `--json`, exit codes, Fluent strings (en + es)

Spec: [config-and-cli §Commands, §Exit codes, §Parity rule](../../specs/config-and-cli.md#commands), [i18n §Rust strings, §Rules, §Terminology, §Parity check](../../specs/i18n.md#rust-strings), D19. Skill: `leemusync-security` (secrets at prompts).

**Files:**
- Modify: `Cargo.toml` (`clap`, `rpassword`, `fluent-bundle`, `unic-langid`), `crates/cli/Cargo.toml`:

```toml
[dependencies]
leemusync-api = { path = "../api" }
leemusync-engine = { path = "../engine" }
clap = { workspace = true }
fluent-bundle = { workspace = true }
rpassword = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true, features = ["rt-multi-thread"] }
unic-langid = { workspace = true }
zeroize = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
tempfile = { workspace = true }
```

- Modify: `crates/cli/src/main.rs`, `crates/cli/tests/version.rs`
- Create: `crates/cli/src/{i18n,prompt,output}.rs`, `crates/cli/tests/cli.rs`, `locales/en/leemusync.ftl`, `locales/es/leemusync.ftl`
- Modify: `xtask/src/i18n.rs` (Fluent parity)

**Interfaces:**
- Consumes: every public `Engine` method and type from Tasks 10–15 (`Engine`, `EngineEnv`, `EngineError`, `Locations`, `JoinSecret`, `MapChoice`, `MapOutcome`, `SyncOptions`, `BackendConfig`); report types from Task 9.
- Produces: the `leemusync` binary. Command tree: `init`; `join [--recovery]`; `backend add gdrive --client-id <id>` / `backend add fs <path>` / `backend login`; `grant add <path>` / `grant list`; `players add <name>` / `players list`; `scan`; `accounts map <player> <emulator> <account> [--keep | --replace]`; `accounts recover <emulator> <account> --player <name>`; `sync [--dry-run] [--yes]`; `status`; `history <player> <emulator> <game>`; `restore <player> <emulator> <game> <version-or-backup> [--yes]`; `doctor`. Global `--json`. Exit codes: the engine's `exit_code()`, plus 4 when any stream ends in a conflict, plus 1 when `doctor` has a failed check.
- `LEEMUSYNC_TEST_ROOT=<dir>` puts config, data and the emulator search under `<dir>` (`Locations::under`). It's for tests only, and the OS keystore is still used.
- Language: the first non-empty of `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE` that starts with `es` selects `es`; anything else selects `en` (i18n §Rules).
- Secrets: passphrases, the recovery key typed at `join` and the OAuth client secret are read with `rpassword` (no echo) into `Zeroizing<String>`. They're never accepted as arguments or environment variables. The recovery key is printed once, by `init`, to stderr in human form, even with `--json`.

- [ ] **Step 1: Failing CLI tests.** Replace `crates/cli/tests/version.rs` with:

```rust
use std::process::Command;

#[test]
fn prints_name_and_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_leemusync")).arg("--version").output().expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), format!("leemusync {}", env!("CARGO_PKG_VERSION")));
}
```

Create `crates/cli/tests/cli.rs`:

```rust
//! The binary end to end, on commands that don't need the OS keystore.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, lang: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_leemusync"))
        .args(args)
        .env("LEEMUSYNC_TEST_ROOT", root)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .env("LANG", lang)
        .output()
        .expect("binary runs")
}

#[test]
fn status_json_on_a_fresh_device() {
    let root = tempfile::tempdir().unwrap();
    let out = run(root.path(), "en_US.UTF-8", &["--json", "status"]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["api_version"], 1);
    assert!(json["repo"].is_null());
}

#[test]
fn human_output_follows_the_locale() {
    let root = tempfile::tempdir().unwrap();
    let en = String::from_utf8(run(root.path(), "en_US.UTF-8", &["status"]).stdout).unwrap();
    let es = String::from_utf8(run(root.path(), "es_MX.UTF-8", &["status"]).stdout).unwrap();
    assert!(en.contains("No repo yet"), "{en}");
    assert!(es.contains("Todavía no hay un repo"), "{es}");
    let help = String::from_utf8(run(root.path(), "es_MX.UTF-8", &["--help"]).stdout).unwrap();
    assert!(help.contains("Mantiene tus partidas guardadas sincronizadas"), "{help}");
}

#[test]
fn errors_use_the_spec_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let out = run(root.path(), "en_US.UTF-8", &["--json", "sync", "--dry-run"]);
    assert_eq!(out.status.code(), Some(2), "not set up yet is a usage error");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "not_set_up");
    assert_eq!(run(root.path(), "en_US.UTF-8", &["grant", "add", "/"]).status.code(), Some(2));
    assert_eq!(run(root.path(), "en_US.UTF-8", &["no-such-command"]).status.code(), Some(2));
}

#[test]
fn backend_fs_and_grants_persist() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("remote");
    std::fs::create_dir_all(root.path().join("home/.local/share/Cemu")).unwrap();
    assert!(run(root.path(), "en_US.UTF-8", &["backend", "add", "fs", store.to_str().unwrap()]).status.success());
    let grant = root.path().join("home/.local/share/Cemu");
    assert!(run(root.path(), "en_US.UTF-8", &["grant", "add", grant.to_str().unwrap()]).status.success());
    let out = run(root.path(), "en_US.UTF-8", &["--json", "status"]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["backend"].as_str().unwrap().starts_with("fs "));
    assert_eq!(json["grants"].as_array().unwrap().len(), 1);
}

#[test]
fn scan_output_is_fully_formatted_in_both_languages() {
    for (lang, word) in [("en_US.UTF-8", "Allow it"), ("es_MX.UTF-8", "Permítelo")] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home/.local/share/Cemu")).unwrap();
        let out = String::from_utf8(run(root.path(), lang, &["scan"]).stdout).unwrap();
        assert!(out.contains(word), "{lang}: {out}");
        assert!(!out.contains("{ $") && !out.contains("scan-"), "{lang}: unformatted text: {out}");
    }
}
```

- [ ] **Step 2: Run** `cargo test -p leemusync-cli` → compile/assert failures.
- [ ] **Step 3: Write `locales/en/leemusync.ftl`:**

```ftl
# LeemuSync Rust strings (English source). Keys describe meaning (i18n §Rules).
# Every key here must exist in es/leemusync.ftl (cargo xtask i18n-check).

## Help
help-about = Keeps your emulator saves in sync across your devices, through storage you own.
help-about-json = Print machine-readable JSON (keys are always in English).
help-init = Create a new encrypted repo on the storage you set up.
help-join = Join an existing repo from this device.
help-join-recovery = Use the recovery key instead of the passphrase.
help-backend = Set up where the repo is stored.
help-backend-add = Choose the storage.
help-backend-add-gdrive = Use Google Drive (LeemuSync only sees the files it created).
help-backend-add-gdrive-client_id = Your OAuth client id (type "Desktop app").
help-backend-add-fs = Use a local or mounted folder.
help-backend-add-fs-path = The folder.
help-backend-login = Log in to Google Drive again.
help-grant = Choose which folders LeemuSync may read and write.
help-grant-add = Allow a folder (an emulator's own folder).
help-grant-add-path = The folder.
help-grant-list = List the allowed folders.
help-players = Manage players.
help-players-add = Add a player.
help-players-add-name = The player's name.
help-players-list = List the players.
help-scan = Find emulators, accounts and saves on this device.
help-accounts = Connect players to emulator accounts on this device.
help-accounts-map = Use an emulator account for a player on this device.
help-accounts-map-player = The player's name.
help-accounts-map-emulator = The emulator id, for example cemu.
help-accounts-map-account = The account id, for example 80000002.
help-accounts-map-keep = Keep the account's current saves (they're uploaded, then you resolve a conflict).
help-accounts-map-replace = Replace the account's saves with the player's latest (the current ones go to local history first).
help-accounts-recover = Keep an orphaned save as a player's first version.
help-accounts-recover-emulator = The emulator id, for example cemu.
help-accounts-recover-account = The orphaned folder's account id.
help-accounts-recover-player = The player who gets this save.
help-sync = Sync now.
help-sync-dry_run = Show what would happen without changing anything.
help-sync-yes = Confirm the first sync on this device.
help-status = Show this device's setup and saves.
help-history = Show a player's version history for a game.
help-history-player = The player's name.
help-history-emulator = The emulator id, for example cemu.
help-history-game = The game id, for example 101c9400.
help-restore = Put back a version or a local backup (the current files are backed up first).
help-restore-player = The player's name.
help-restore-emulator = The emulator id, for example cemu.
help-restore-game = The game id, for example 101c9400.
help-restore-what = A version id or a backup name from `leemusync history`.
help-restore-yes = Don't ask for confirmation.
help-doctor = Check the keystore, the storage and the emulator folders.

## Errors ($detail is never translated: paths, ids, provider text)
error-usage = { $detail }
error-config = The config file is invalid: { $detail }
error-backend = Storage problem: { $detail }
error-backend_auth = The storage login failed ({ $detail }). Run `leemusync backend login`.
error-io = File problem: { $detail }
error-crypto = Encryption problem: { $detail }
error-unlock = Wrong passphrase or recovery key.
error-locked = Another LeemuSync process is running ({ $detail }).
error-emulator_running = { $detail } is running. Close it and try again.
error-not_granted = LeemuSync isn't allowed to use { $detail }. Run `leemusync grant add <folder>`.
error-not_set_up = Not set up yet: { $detail }
error-secrets = The system keyring failed: { $detail }
error-data = The repo has invalid data: { $detail }
error-needs_confirmation = This is the first sync on this device. Back up this device's saves first, then run `leemusync sync --yes`.
error-busy = { $detail } changed while it was being read. Try again when the game is idle.
error-injected = Test fault at { $detail }.
error-needs-choice = Use --keep or --replace (the account already has saves for: { $games }).
error-needs-tty = This needs a terminal to ask you: { $what }.

## Prompts
prompt-passphrase = Passphrase:
prompt-passphrase-again = Type the passphrase again:
prompt-passphrases-differ = The passphrases don't match.
prompt-recovery = Recovery key:
prompt-client-secret = OAuth client secret (it isn't shown):
prompt-first-sync = This is the first sync on this device. Did you back up this device's saves?
prompt-keep-or-replace = Account { $account } already has saves for { $games }. Keep them (they're uploaded and you resolve a conflict) or replace them with { $player }'s latest save (the current files go to local history first)?
prompt-keep-or-replace-keys = [k]eep / [r]eplace:
prompt-restore = Replace { $player }'s current files with { $what }? The current files go to local history first.
prompt-yes-no = [y/N]:
answer-yes = y
answer-keep = k
answer-replace = r

## Results
login-open-url = Open this address in your browser to let LeemuSync use its own Google Drive folder:
login-waiting = Waiting for the browser (up to 5 minutes)…
login-done = Google Drive is connected.
backend-fs-done = The repo will be stored in { $path }.
init-done = Repo { $repo } created on { $device }.
init-recovery = Your recovery key. Write it down now: it's shown only once, and without it or the passphrase your saves can't be recovered.
join-done = { $device } joined repo { $repo }.
grant-done = Allowed: { $path }
grant-none = No folders allowed yet.
players-added = Player { $name } added.
players-none = No players yet.
map-done = { $player } now uses { $emulator } account { $account } on this device.
recover-done = The orphaned save is now { $player }'s first { $game } version.
restore-done = Restored { $what } for { $player }. Run `leemusync sync` to upload it.
sync-line = { $player } · { $game } · { $action }
dry-run = Dry run: nothing was changed.
sync-conflicts = { $count ->
    [one] One save has a conflict. Both versions are kept; resolving comes in the next release.
   *[other] { $count } saves have a conflict. All versions are kept; resolving comes in the next release.
}

action-nothing = up to date
action-adopt = already matches the repo
action-fast_forward = restored the latest version
action-upload = uploaded
action-upload_then_conflict = uploaded, conflict to resolve
action-conflict = conflict to resolve
action-local_missing = the save folder is missing
action-remote_behind = the repo went back in time; nothing changed
action-needs_account = no account on this device: run `leemusync accounts map`
action-skipped = skipped (the emulator is running)

warning-emulator_running = { $detail } is running, so its saves were skipped.
warning-not_granted = Not allowed yet: { $detail }. Run `leemusync grant add`.
warning-shared_folder_not_empty = { $detail } is shared by every account and isn't synced yet.
warning-orphaned_save = Orphaned save { $detail }: its account was deleted. Run `leemusync accounts recover` to keep it.
warning-config_merged = Changes from two devices were merged.
warning-player_renamed = Two players had the same name; one is now { $detail }.
warning-remote_behind = The repo is missing versions this device saw ({ $detail }). Nothing was changed.
warning-restore_rolled_back = An interrupted restore was undone in { $detail }.
warning-invalid_version = Ignored invalid data in the repo: { $detail }

scan-install = { $emulator } at { $base }
scan-needs-grant = Allow it: leemusync grant add { $path }
scan-account = account { $id } · { $name } · { $player }
scan-unknown = (none)
scan-save = save { $game }/{ $account } · { $files ->
    [one] 1 file
   *[other] { $files } files
}
scan-orphan = orphaned save { $game }/{ $account }: run `leemusync accounts recover`

status-device = Device: { $device }
status-repo = Repo: { $repo } on { $backend }
status-no-repo = No repo yet: run `leemusync init` or `leemusync join`.
status-stream = { $player } · { $emulator } { $game } · account { $account }

history-version = { $id } · { $files ->
    [one] 1 file
   *[other] { $files } files
} { $head }
history-head = (latest)
history-backup = local backup { $name }

doctor-ok = ok
doctor-fail = FAILED
doctor-check = { $status } { $name } { $detail }
```

- [ ] **Step 4: Write `locales/es/leemusync.ftl`** with the same keys (neutral Latin American Spanish, "tú", the terminology table):

```ftl
# Textos de LeemuSync para Rust (español neutro latinoamericano). Mismas claves que en/leemusync.ftl.

## Ayuda
help-about = Mantiene tus partidas guardadas sincronizadas entre tus dispositivos, con un almacenamiento que es tuyo.
help-about-json = Muestra JSON para programas (las claves siempre van en inglés).
help-init = Crea un repo cifrado nuevo en el almacenamiento que configuraste.
help-join = Une este dispositivo a un repo existente.
help-join-recovery = Usa la clave de recuperación en lugar de la frase de contraseña.
help-backend = Configura dónde se guarda el repo.
help-backend-add = Elige el almacenamiento.
help-backend-add-gdrive = Usa Google Drive (LeemuSync solo ve los archivos que creó).
help-backend-add-gdrive-client_id = Tu id de cliente OAuth (tipo "App de escritorio").
help-backend-add-fs = Usa una carpeta local o montada.
help-backend-add-fs-path = La carpeta.
help-backend-login = Vuelve a iniciar sesión en Google Drive.
help-grant = Elige qué carpetas puede leer y escribir LeemuSync.
help-grant-add = Permite una carpeta (la carpeta del emulador).
help-grant-add-path = La carpeta.
help-grant-list = Lista las carpetas permitidas.
help-players = Administra los jugadores.
help-players-add = Agrega un jugador.
help-players-add-name = El nombre del jugador.
help-players-list = Lista los jugadores.
help-scan = Busca emuladores, cuentas y partidas guardadas en este dispositivo.
help-accounts = Conecta jugadores con cuentas del emulador en este dispositivo.
help-accounts-map = Usa una cuenta del emulador para un jugador en este dispositivo.
help-accounts-map-player = El nombre del jugador.
help-accounts-map-emulator = El id del emulador, por ejemplo cemu.
help-accounts-map-account = El id de la cuenta, por ejemplo 80000002.
help-accounts-map-keep = Conserva las partidas guardadas de la cuenta (se suben y luego resuelves un conflicto).
help-accounts-map-replace = Reemplaza las partidas guardadas de la cuenta por las más recientes del jugador (las actuales van primero al historial local).
help-accounts-recover = Guarda una partida huérfana como la primera versión de un jugador.
help-accounts-recover-emulator = El id del emulador, por ejemplo cemu.
help-accounts-recover-account = El id de cuenta de la carpeta huérfana.
help-accounts-recover-player = El jugador que recibe esta partida guardada.
help-sync = Sincroniza ahora.
help-sync-dry_run = Muestra qué pasaría sin cambiar nada.
help-sync-yes = Confirma la primera sincronización en este dispositivo.
help-status = Muestra la configuración y las partidas guardadas de este dispositivo.
help-history = Muestra el historial de versiones de un jugador para un juego.
help-history-player = El nombre del jugador.
help-history-emulator = El id del emulador, por ejemplo cemu.
help-history-game = El id del juego, por ejemplo 101c9400.
help-restore = Restaura una versión o una copia local (antes se respaldan los archivos actuales).
help-restore-player = El nombre del jugador.
help-restore-emulator = El id del emulador, por ejemplo cemu.
help-restore-game = El id del juego, por ejemplo 101c9400.
help-restore-what = Un id de versión o el nombre de una copia de `leemusync history`.
help-restore-yes = No pide confirmación.
help-doctor = Revisa el llavero del sistema, el almacenamiento y las carpetas del emulador.

## Errores ($detail nunca se traduce: rutas, ids, texto del proveedor)
error-usage = { $detail }
error-config = El archivo de configuración no es válido: { $detail }
error-backend = Problema con el almacenamiento: { $detail }
error-backend_auth = Falló el inicio de sesión del almacenamiento ({ $detail }). Ejecuta `leemusync backend login`.
error-io = Problema con un archivo: { $detail }
error-crypto = Problema de cifrado: { $detail }
error-unlock = La frase de contraseña o la clave de recuperación no es correcta.
error-locked = Ya hay otro proceso de LeemuSync en ejecución ({ $detail }).
error-emulator_running = { $detail } está abierto. Ciérralo y vuelve a intentarlo.
error-not_granted = LeemuSync no tiene permiso para usar { $detail }. Ejecuta `leemusync grant add <carpeta>`.
error-not_set_up = Todavía falta configurar: { $detail }
error-secrets = Falló el llavero del sistema: { $detail }
error-data = El repo tiene datos no válidos: { $detail }
error-needs_confirmation = Es la primera sincronización en este dispositivo. Primero respalda las partidas guardadas de este dispositivo y luego ejecuta `leemusync sync --yes`.
error-busy = { $detail } cambió mientras se leía. Vuelve a intentarlo cuando el juego esté en reposo.
error-injected = Falla de prueba en { $detail }.
error-needs-choice = Usa --keep o --replace (la cuenta ya tiene partidas guardadas de: { $games }).
error-needs-tty = Esto necesita una terminal para preguntarte: { $what }.

## Preguntas
prompt-passphrase = Frase de contraseña:
prompt-passphrase-again = Escribe la frase de contraseña otra vez:
prompt-passphrases-differ = Las frases de contraseña no coinciden.
prompt-recovery = Clave de recuperación:
prompt-client-secret = Secreto del cliente OAuth (no se muestra):
prompt-first-sync = Es la primera sincronización en este dispositivo. ¿Respaldaste las partidas guardadas de este dispositivo?
prompt-keep-or-replace = La cuenta { $account } ya tiene partidas guardadas de { $games }. ¿Las conservas (se suben y resuelves un conflicto) o las reemplazas por la partida guardada más reciente de { $player } (los archivos actuales van primero al historial local)?
prompt-keep-or-replace-keys = [c]onservar / [r]eemplazar:
prompt-restore = ¿Reemplazar los archivos actuales de { $player } por { $what }? Los archivos actuales van primero al historial local.
prompt-yes-no = [s/N]:
answer-yes = s
answer-keep = c
answer-replace = r

## Resultados
login-open-url = Abre esta dirección en tu navegador para que LeemuSync use su propia carpeta de Google Drive:
login-waiting = Esperando al navegador (hasta 5 minutos)…
login-done = Google Drive está conectado.
backend-fs-done = El repo se guardará en { $path }.
init-done = Se creó el repo { $repo } en { $device }.
init-recovery = Tu clave de recuperación. Anótala ahora: solo se muestra una vez, y sin ella o sin la frase de contraseña no se pueden recuperar tus partidas guardadas.
join-done = { $device } se unió al repo { $repo }.
grant-done = Permitido: { $path }
grant-none = Todavía no hay carpetas permitidas.
players-added = Se agregó el jugador { $name }.
players-none = Todavía no hay jugadores.
map-done = { $player } ahora usa la cuenta { $account } de { $emulator } en este dispositivo.
recover-done = La partida huérfana ahora es la primera versión de { $game } de { $player }.
restore-done = Se restauró { $what } para { $player }. Ejecuta `leemusync sync` para subirla.
sync-line = { $player } · { $game } · { $action }
dry-run = Prueba: no se cambió nada.
sync-conflicts = { $count ->
    [one] Una partida guardada tiene un conflicto. Se conservan ambas versiones; resolverlo llega en la próxima versión.
   *[other] { $count } partidas guardadas tienen un conflicto. Se conservan todas las versiones; resolverlos llega en la próxima versión.
}

action-nothing = al día
action-adopt = ya coincide con el repo
action-fast_forward = se restauró la versión más reciente
action-upload = subida
action-upload_then_conflict = subida, con un conflicto por resolver
action-conflict = conflicto por resolver
action-local_missing = falta la carpeta de la partida guardada
action-remote_behind = el repo volvió a un estado anterior; no se cambió nada
action-needs_account = no hay cuenta en este dispositivo: ejecuta `leemusync accounts map`
action-skipped = omitida (el emulador está abierto)

warning-emulator_running = { $detail } está abierto, así que se omitieron sus partidas guardadas.
warning-not_granted = Todavía sin permiso: { $detail }. Ejecuta `leemusync grant add`.
warning-shared_folder_not_empty = { $detail } lo comparten todas las cuentas y todavía no se sincroniza.
warning-orphaned_save = Partida huérfana { $detail }: se borró su cuenta. Ejecuta `leemusync accounts recover` para conservarla.
warning-config_merged = Se combinaron cambios de dos dispositivos.
warning-player_renamed = Dos jugadores tenían el mismo nombre; uno ahora se llama { $detail }.
warning-remote_behind = Al repo le faltan versiones que este dispositivo vio ({ $detail }). No se cambió nada.
warning-restore_rolled_back = Se deshizo una restauración interrumpida en { $detail }.
warning-invalid_version = Se ignoraron datos no válidos del repo: { $detail }

scan-install = { $emulator } en { $base }
scan-needs-grant = Permítelo: leemusync grant add { $path }
scan-account = cuenta { $id } · { $name } · { $player }
scan-unknown = (ninguno)
scan-save = partida guardada { $game }/{ $account } · { $files ->
    [one] 1 archivo
   *[other] { $files } archivos
}
scan-orphan = partida huérfana { $game }/{ $account }: ejecuta `leemusync accounts recover`

status-device = Dispositivo: { $device }
status-repo = Repo: { $repo } en { $backend }
status-no-repo = Todavía no hay un repo: ejecuta `leemusync init` o `leemusync join`.
status-stream = { $player } · { $emulator } { $game } · cuenta { $account }

history-version = { $id } · { $files ->
    [one] 1 archivo
   *[other] { $files } archivos
} { $head }
history-head = (la más reciente)
history-backup = copia local { $name }

doctor-ok = ok
doctor-fail = FALLÓ
doctor-check = { $status } { $name } { $detail }
```

Have a Spanish-speaking reviewer (the maintainer) read this file once before the commit. "Partida huérfana" and "llavero del sistema" are new terms: add them to [i18n §Terminology](../../specs/i18n.md#terminology) in this task's commit.

- [ ] **Step 5: Implement `crates/cli/src/i18n.rs`:**

```rust
//! Fluent strings for human output (i18n §Rust strings). `--json` never goes through here.

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};

const EN: &str = include_str!("../../../locales/en/leemusync.ftl");
const ES: &str = include_str!("../../../locales/es/leemusync.ftl");

pub struct I18n {
    bundle: FluentBundle<FluentResource>,
}

impl I18n {
    pub fn from_env() -> Self {
        let lang = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        Self::new(if lang.starts_with("es") { "es" } else { "en" })
    }

    pub fn new(lang: &str) -> Self {
        let (text, id) = if lang == "es" { (ES, "es") } else { (EN, "en") };
        let langid: unic_langid::LanguageIdentifier = id.parse().unwrap_or_default();
        let mut bundle = FluentBundle::new(vec![langid]);
        bundle.set_use_isolating(false);
        let resource = FluentResource::try_new(text.to_owned()).unwrap_or_else(|(partial, _)| partial);
        bundle.add_resource_overriding(resource);
        Self { bundle }
    }

    /// The message, or `None` if the key doesn't exist.
    pub fn get(&self, key: &str, args: &[(&str, FluentValue<'static>)]) -> Option<String> {
        let message = self.bundle.get_message(key)?;
        let pattern = message.value()?;
        let mut fargs = FluentArgs::new();
        for (k, v) in args {
            fargs.set(*k, v.clone());
        }
        let mut errors = Vec::new();
        Some(self.bundle.format_pattern(pattern, Some(&fargs), &mut errors).into_owned())
    }

    /// The message; the key itself if it's missing (tests and `i18n-check` keep that from happening).
    pub fn t(&self, key: &str, args: &[(&str, FluentValue<'static>)]) -> String {
        self.get(key, args).unwrap_or_else(|| key.to_owned())
    }
}

/// Shorthand for a string argument.
pub fn s(v: impl Into<String>) -> FluentValue<'static> {
    FluentValue::from(v.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_files_parse_and_plurals_work() {
        assert!(FluentResource::try_new(EN.to_owned()).is_ok());
        assert!(FluentResource::try_new(ES.to_owned()).is_ok());
        let en = I18n::new("en");
        assert_eq!(en.t("history-head", &[]), "(latest)");
        let one = en.t("scan-save", &[("game", s("101c9400")), ("account", s("80000001")), ("files", FluentValue::from(1))]);
        assert!(one.ends_with("1 file"), "{one}");
        let es = I18n::new("es");
        let many = es.t("scan-save", &[("game", s("g")), ("account", s("a")), ("files", FluentValue::from(3))]);
        assert!(many.ends_with("3 archivos"), "{many}");
    }
}
```

- [ ] **Step 6: Implement `crates/cli/src/prompt.rs`:**

```rust
//! Terminal questions. Secrets are read without echo and never come from argv or the environment.

use std::io::{BufRead as _, IsTerminal as _, Write as _};

use leemusync_engine::EngineError;
use zeroize::Zeroizing;

use crate::i18n::{I18n, s};

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn need_tty(t: &I18n, what: &str) -> Result<(), EngineError> {
    if interactive() {
        Ok(())
    } else {
        Err(EngineError::Usage(t.t("error-needs-tty", &[("what", s(what))])))
    }
}

pub fn secret(t: &I18n, key: &str) -> Result<Zeroizing<String>, EngineError> {
    need_tty(t, &t.t(key, &[]))?;
    let value = rpassword::prompt_password(format!("{} ", t.t(key, &[])))?;
    Ok(Zeroizing::new(value.trim().to_owned()))
}

pub fn new_passphrase(t: &I18n) -> Result<Zeroizing<String>, EngineError> {
    let first = secret(t, "prompt-passphrase")?;
    let second = secret(t, "prompt-passphrase-again")?;
    if *first != *second {
        return Err(EngineError::Usage(t.t("prompt-passphrases-differ", &[])));
    }
    Ok(first)
}

fn line(question: &str) -> Result<String, EngineError> {
    eprint!("{question} ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().to_lowercase())
}

pub fn yes(t: &I18n, question: &str) -> Result<bool, EngineError> {
    need_tty(t, question)?;
    let answer = line(&format!("{question} {}", t.t("prompt-yes-no", &[])))?;
    Ok(answer == t.t("answer-yes", &[]))
}

/// `Some(true)` = replace, `Some(false)` = keep, `None` = no clear answer.
pub fn keep_or_replace(t: &I18n, question: &str) -> Result<Option<bool>, EngineError> {
    need_tty(t, question)?;
    let answer = line(&format!("{question}\n{}", t.t("prompt-keep-or-replace-keys", &[])))?;
    Ok(if answer == t.t("answer-replace", &[]) {
        Some(true)
    } else if answer == t.t("answer-keep", &[]) {
        Some(false)
    } else {
        None
    })
}
```

- [ ] **Step 7: Implement `crates/cli/src/output.rs`:**

```rust
//! Human (Fluent) or `--json` output. JSON goes to stdout; human results to stdout, problems to stderr.

use fluent_bundle::FluentValue;
use leemusync_api::{API_VERSION, DoctorReport, ErrorBody, ErrorReport, HistoryReport, ScanReport, StatusReport, SyncReport, Warning};
use leemusync_engine::EngineError;
use serde::Serialize;

use crate::i18n::{I18n, s};

pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("{e}"),
    }
}

fn snake<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// The data part of an error, never translated (paths, ids, provider text).
fn detail(e: &EngineError) -> String {
    match e {
        EngineError::Usage(d)
        | EngineError::Config(d)
        | EngineError::Io(d)
        | EngineError::Locked(d)
        | EngineError::EmulatorRunning(d)
        | EngineError::NotGranted(d)
        | EngineError::Secrets(d)
        | EngineError::Data(d)
        | EngineError::Busy(d) => d.clone(),
        EngineError::Store(s) => s.message().to_owned(),
        EngineError::Crypto(c) => c.to_string(),
        EngineError::NotSetUp(d) | EngineError::Injected(d) => (*d).to_owned(),
        EngineError::Unlock | EngineError::NeedsConfirmation => String::new(),
    }
}

pub fn error(t: &I18n, as_json: bool, e: &EngineError) {
    if as_json {
        json(&ErrorReport { api_version: API_VERSION, error: ErrorBody { code: e.code().to_owned(), message: e.to_string() } });
    } else {
        eprintln!("{}", t.t(&format!("error-{}", e.code()), &[("detail", s(detail(e)))]));
    }
}

pub fn line(t: &I18n, key: &str, args: &[(&str, FluentValue<'static>)]) {
    println!("{}", t.t(key, args));
}

pub fn warnings(t: &I18n, warnings: &[Warning]) {
    for w in warnings {
        eprintln!("{}", t.t(&format!("warning-{}", snake(&w.code)), &[("detail", s(w.detail.clone()))]));
    }
}

pub fn sync(t: &I18n, r: &SyncReport) {
    for st in &r.streams {
        let action = t.t(&format!("action-{}", snake(&st.action)), &[]);
        line(t, "sync-line", &[("player", s(st.player.clone())), ("game", s(st.game.clone())), ("action", s(action))]);
    }
    warnings(t, &r.warnings);
    if r.dry_run {
        line(t, "dry-run", &[]);
    }
}

pub fn scan(t: &I18n, r: &ScanReport) {
    for i in &r.installs {
        line(t, "scan-install", &[("emulator", s(i.emulator.clone())), ("base", s(i.base.clone()))]);
        for p in &i.needs_grant {
            line(t, "scan-needs-grant", &[("path", s(p.clone()))]);
        }
        let unknown = t.t("scan-unknown", &[]);
        for a in &i.accounts {
            line(t, "scan-account", &[
                ("id", s(a.id.clone())),
                ("name", s(a.name.clone().unwrap_or_else(|| unknown.clone()))),
                ("player", s(a.player.clone().unwrap_or_else(|| unknown.clone()))),
            ]);
        }
        for sv in &i.saves {
            let key = if sv.orphaned { "scan-orphan" } else { "scan-save" };
            line(t, key, &[("game", s(sv.game.clone())), ("account", s(sv.account.clone())), ("files", FluentValue::from(sv.files))]);
        }
    }
    warnings(t, &r.warnings);
}

pub fn status(t: &I18n, r: &StatusReport) {
    line(t, "status-device", &[("device", s(format!("{} ({})", r.device, r.device_id)))]);
    match (&r.repo, &r.backend) {
        (Some(repo), backend) => line(t, "status-repo", &[("repo", s(repo.clone())), ("backend", s(backend.clone().unwrap_or_default()))]),
        (None, _) => line(t, "status-no-repo", &[]),
    }
    for g in &r.grants {
        line(t, "grant-done", &[("path", s(g.clone()))]);
    }
    let none = t.t("scan-unknown", &[]);
    for st in &r.streams {
        line(t, "status-stream", &[
            ("player", s(st.player.clone())),
            ("emulator", s(st.emulator.clone())),
            ("game", s(st.game.clone())),
            ("account", s(st.account.clone().unwrap_or_else(|| none.clone()))),
        ]);
    }
}

pub fn history(t: &I18n, r: &HistoryReport) {
    let head = t.t("history-head", &[]);
    for v in &r.versions {
        let marker = if v.head { head.clone() } else { String::new() };
        line(t, "history-version", &[("id", s(v.id.clone())), ("files", FluentValue::from(v.files)), ("head", s(marker))]);
    }
    for b in &r.backups {
        line(t, "history-backup", &[("name", s(b.clone()))]);
    }
}

pub fn doctor(t: &I18n, r: &DoctorReport) {
    for c in &r.checks {
        let status = t.t(if c.ok { "doctor-ok" } else { "doctor-fail" }, &[]);
        line(t, "doctor-check", &[("status", s(status)), ("name", s(c.name.clone())), ("detail", s(c.detail.clone()))]);
    }
}
```

- [ ] **Step 8: Implement `crates/cli/src/main.rs`:**

```rust
//! `leemusync`: command-line interface. Spec: docs/specs/config-and-cli.md.

mod i18n;
mod output;
mod prompt;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory as _, FromArgMatches as _, Parser, Subcommand};
use leemusync_api::StreamAction;
use leemusync_engine::{BackendConfig, Engine, EngineEnv, EngineError, JoinSecret, Locations, MapChoice, MapOutcome, SyncOptions};

use crate::i18n::{I18n, s};

#[derive(Parser)]
#[command(name = "leemusync", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Init,
    Join {
        #[arg(long)]
        recovery: bool,
    },
    Backend {
        #[command(subcommand)]
        cmd: BackendCmd,
    },
    Grant {
        #[command(subcommand)]
        cmd: GrantCmd,
    },
    Players {
        #[command(subcommand)]
        cmd: PlayersCmd,
    },
    Scan,
    Accounts {
        #[command(subcommand)]
        cmd: AccountsCmd,
    },
    Sync {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    Status,
    History {
        player: String,
        emulator: String,
        game: String,
    },
    Restore {
        player: String,
        emulator: String,
        game: String,
        what: String,
        #[arg(long)]
        yes: bool,
    },
    Doctor,
}

#[derive(Subcommand)]
enum BackendCmd {
    Add {
        #[command(subcommand)]
        kind: BackendKind,
    },
    Login,
}

#[derive(Subcommand)]
enum BackendKind {
    Gdrive {
        #[arg(long)]
        client_id: String,
    },
    Fs {
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum GrantCmd {
    Add { path: PathBuf },
    List,
}

#[derive(Subcommand)]
enum PlayersCmd {
    Add { name: String },
    List,
}

#[derive(Subcommand)]
enum AccountsCmd {
    Map {
        player: String,
        emulator: String,
        account: String,
        #[arg(long, conflicts_with = "replace")]
        keep: bool,
        #[arg(long)]
        replace: bool,
    },
    Recover {
        emulator: String,
        account: String,
        #[arg(long)]
        player: String,
    },
}

/// Sets every command's and argument's help from `help-<path>[-<arg id>]` (P6).
/// clap's own "Usage:/Options:" headings and the built-in -h/-V texts stay English (i18n Open questions).
fn localize(cmd: clap::Command, path: &str, t: &I18n) -> clap::Command {
    let key = if path.is_empty() { "help-about".to_owned() } else { format!("help-{path}") };
    let mut cmd = match t.get(&key, &[]) {
        Some(text) => cmd.about(text),
        None => cmd,
    };
    let args: Vec<String> = cmd.get_arguments().map(|a| a.get_id().to_string()).collect();
    for id in args {
        if let Some(text) = t.get(&format!("{key}-{id}"), &[]) {
            cmd = cmd.mut_arg(id, |a| a.help(text));
        }
    }
    let subs: Vec<String> = cmd.get_subcommands().map(|c| c.get_name().to_owned()).collect();
    for name in subs {
        let sub_path = if path.is_empty() { name.clone() } else { format!("{path}-{name}") };
        cmd = cmd.mut_subcommand(&name, |c| localize(c, &sub_path, t));
    }
    cmd
}

fn env() -> Result<EngineEnv, EngineError> {
    let mut env = EngineEnv::system()?;
    if let Some(root) = std::env::var_os("LEEMUSYNC_TEST_ROOT") {
        env.locations = Locations::under(Path::new(&root));
    }
    Ok(env)
}

fn main() -> ExitCode {
    let t = I18n::from_env();
    let matches = localize(Cli::command(), "", &t).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            output::error(&t, cli.json, &EngineError::Io(e.to_string()));
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(run(&cli, &t)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            output::error(&t, cli.json, &e);
            ExitCode::from(e.exit_code())
        }
    }
}

async fn gdrive_login(engine: &mut Engine, t: &I18n, client_id: &str) -> Result<(), EngineError> {
    let secret = prompt::secret(t, "prompt-client-secret")?;
    let login = engine.gdrive_login_start(client_id, &secret).await?;
    eprintln!("{}\n\n    {}\n", t.t("login-open-url", &[]), login.url());
    eprintln!("{}", t.t("login-waiting", &[]));
    engine.gdrive_login_finish(login).await?;
    eprintln!("{}", t.t("login-done", &[]));
    Ok(())
}

#[allow(clippy::too_many_lines)] // one match arm per command, each a few lines
async fn run(cli: &Cli, t: &I18n) -> Result<u8, EngineError> {
    let mut engine = Engine::open(env()?)?;
    let json = cli.json;
    match &cli.command {
        Cmd::Init => {
            let pass = prompt::new_passphrase(t)?;
            let (report, recovery) = engine.init(&pass).await?;
            eprintln!("{}\n\n    {}\n", t.t("init-recovery", &[]), recovery.as_str());
            if json {
                output::json(&report);
            } else {
                output::line(t, "init-done", &[("repo", s(report.repo.clone())), ("device", s(report.device.clone()))]);
            }
        }
        Cmd::Join { recovery } => {
            let secret = if *recovery {
                JoinSecret::Recovery(prompt::secret(t, "prompt-recovery")?.to_string())
            } else {
                JoinSecret::Passphrase(prompt::secret(t, "prompt-passphrase")?.to_string())
            };
            let report = engine.join(secret).await?;
            if json {
                output::json(&report);
            } else {
                output::line(t, "join-done", &[("device", s(report.device.clone())), ("repo", s(report.repo.clone()))]);
            }
        }
        Cmd::Backend { cmd: BackendCmd::Add { kind: BackendKind::Fs { path } } } => {
            engine.backend_add_fs(path)?;
            output::line(t, "backend-fs-done", &[("path", s(path.display().to_string()))]);
        }
        Cmd::Backend { cmd: BackendCmd::Add { kind: BackendKind::Gdrive { client_id } } } => {
            gdrive_login(&mut engine, t, client_id).await?;
        }
        Cmd::Backend { cmd: BackendCmd::Login } => {
            let Some(BackendConfig::Gdrive { client_id, .. }) = engine.local_config().backend.clone() else {
                return Err(EngineError::NotSetUp("Google Drive: run `leemusync backend add gdrive`"));
            };
            gdrive_login(&mut engine, t, &client_id).await?;
        }
        Cmd::Grant { cmd: GrantCmd::Add { path } } => {
            let granted = engine.grant_add(path)?;
            output::line(t, "grant-done", &[("path", s(granted.display().to_string()))]);
        }
        Cmd::Grant { cmd: GrantCmd::List } => {
            let grants: Vec<String> = engine.grant_list().iter().map(|g| g.display().to_string()).collect();
            if json {
                output::json(&grants);
            } else if grants.is_empty() {
                output::line(t, "grant-none", &[]);
            } else {
                for g in grants {
                    output::line(t, "grant-done", &[("path", s(g))]);
                }
            }
        }
        Cmd::Players { cmd: PlayersCmd::Add { name } } => {
            engine.players_add(name).await?;
            output::line(t, "players-added", &[("name", s(name.clone()))]);
        }
        Cmd::Players { cmd: PlayersCmd::List } => {
            let players = engine.players_list().await?;
            if json {
                output::json(&players);
            } else if players.is_empty() {
                output::line(t, "players-none", &[]);
            } else {
                for p in players {
                    println!("{p}");
                }
            }
        }
        Cmd::Scan => {
            let report = engine.scan().await?;
            if json { output::json(&report) } else { output::scan(t, &report) }
        }
        Cmd::Accounts { cmd: AccountsCmd::Map { player, emulator, account, keep, replace } } => {
            let choice = if *replace { MapChoice::Replace } else if *keep { MapChoice::Keep } else { MapChoice::Ask };
            let mut outcome = engine.accounts_map(player, emulator, account, choice).await?;
            if let MapOutcome::NeedsChoice { games } = &outcome {
                let games = games.join(", ");
                if json || !prompt::interactive() {
                    return Err(EngineError::Usage(t.t("error-needs-choice", &[("games", s(games))])));
                }
                let question = t.t("prompt-keep-or-replace", &[
                    ("account", s(account.clone())),
                    ("games", s(games.clone())),
                    ("player", s(player.clone())),
                ]);
                let choice = match prompt::keep_or_replace(t, &question)? {
                    Some(true) => MapChoice::Replace,
                    Some(false) => MapChoice::Keep,
                    None => return Err(EngineError::Usage(t.t("error-needs-choice", &[("games", s(games))]))),
                };
                outcome = engine.accounts_map(player, emulator, account, choice).await?;
            }
            if matches!(outcome, MapOutcome::Mapped) {
                output::line(t, "map-done", &[("player", s(player.clone())), ("emulator", s(emulator.clone())), ("account", s(account.clone()))]);
            }
        }
        Cmd::Accounts { cmd: AccountsCmd::Recover { emulator, account, player } } => {
            let report = engine.accounts_recover(emulator, account, player).await?;
            if json {
                output::json(&report);
            } else {
                output::line(t, "recover-done", &[("player", s(player.clone())), ("game", s(report.game.clone()))]);
            }
        }
        Cmd::Sync { dry_run, yes } => {
            let opts = || SyncOptions { dry_run: *dry_run };
            let report = match engine.sync(opts()).await {
                Err(EngineError::NeedsConfirmation) => {
                    let ok = *yes || (!json && prompt::interactive() && prompt::yes(t, &t.t("prompt-first-sync", &[]))?);
                    if !ok {
                        return Err(EngineError::NeedsConfirmation);
                    }
                    engine.confirm_first_sync()?;
                    engine.sync(opts()).await?
                }
                other => other?,
            };
            let conflicts = report
                .streams
                .iter()
                .filter(|st| matches!(st.action, StreamAction::Conflict | StreamAction::UploadThenConflict))
                .count();
            if json {
                output::json(&report);
            } else {
                output::sync(t, &report);
                if conflicts > 0 {
                    output::line(t, "sync-conflicts", &[("count", fluent_bundle::FluentValue::from(conflicts))]);
                }
            }
            if conflicts > 0 {
                return Ok(4);
            }
        }
        Cmd::Status => {
            let report = engine.status().await?;
            if json { output::json(&report) } else { output::status(t, &report) }
        }
        Cmd::History { player, emulator, game } => {
            let report = engine.history(player, emulator, game).await?;
            if json { output::json(&report) } else { output::history(t, &report) }
        }
        Cmd::Restore { player, emulator, game, what, yes } => {
            if !*yes {
                let question = t.t("prompt-restore", &[("player", s(player.clone())), ("what", s(what.clone()))]);
                if json || !prompt::yes(t, &question)? {
                    return Err(EngineError::Usage(t.t("error-needs-tty", &[("what", s(question))])));
                }
            }
            let report = engine.restore(player, emulator, game, what).await?;
            if json {
                output::json(&report);
            } else {
                output::line(t, "restore-done", &[("what", s(what.clone())), ("player", s(player.clone()))]);
            }
        }
        Cmd::Doctor => {
            let report = engine.doctor().await?;
            if json { output::json(&report) } else { output::doctor(t, &report) }
            if report.checks.iter().any(|c| !c.ok) {
                return Ok(1);
            }
        }
    }
    Ok(0)
}
```

`FluentValue::from(conflicts)` needs `conflicts` as a type Fluent accepts. If `usize` isn't supported, use `u32::try_from(conflicts).unwrap_or(u32::MAX)`.

- [ ] **Step 9: Extend `xtask/src/i18n.rs` with Fluent parity.** Add, next to the ARB constants:

```rust
const FTL_DIR: &str = "locales";
const FTL_FILE: &str = "leemusync.ftl";

/// Message ids of a Fluent file: lines `id = …` at column 0 (comments and continuation lines skipped).
fn ftl_keys(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with(char::is_whitespace) && !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim()))
        .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map(str::to_owned)
        .collect())
}

fn check_ftl(root: &Path) -> Vec<String> {
    let dir = root.join(FTL_DIR);
    let template = match ftl_keys(&dir.join("en").join(FTL_FILE)) {
        Ok(k) => k,
        Err(e) => return vec![e],
    };
    let Ok(entries) = fs::read_dir(&dir) else { return vec![format!("{FTL_DIR}: missing")] };
    let mut errors = Vec::new();
    for entry in entries.flatten() {
        let lang = entry.file_name().to_string_lossy().into_owned();
        if lang == "en" || !entry.path().is_dir() {
            continue;
        }
        match ftl_keys(&entry.path().join(FTL_FILE)) {
            Ok(found) => errors.extend(diff(&format!("{lang}/{FTL_FILE}"), &template, &found)),
            Err(e) => errors.push(e),
        }
    }
    errors
}
```

In `check`, after the ARB loop add `errors.extend(check_ftl(root));`. Update the module doc comment to say it covers ARB and Fluent. Add a unit test in its `tests` module:

```rust
#[test]
fn ftl_keys_skip_comments_and_continuations() {
    let dir = std::env::temp_dir().join(format!("xtask-ftl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("x.ftl");
    std::fs::write(&file, "# c\na = A\nb = { $n ->\n    [one] x\n   *[other] y\n}\n").unwrap();
    let keys = super::ftl_keys(&file).unwrap();
    assert_eq!(keys.into_iter().collect::<Vec<_>>(), ["a", "b"]);
    std::fs::remove_dir_all(dir).unwrap();
}
```

- [ ] **Step 10: Run** `cargo test -p leemusync-cli -p xtask && cargo xtask i18n-check && cargo clippy --workspace --all-targets -- -D warnings` → PASS. Then try it by hand: `LEEMUSYNC_TEST_ROOT=$(mktemp -d) LANG=es_MX.UTF-8 cargo run -q -p leemusync-cli -- --help` shows Spanish command descriptions.
- [ ] **Step 11: Commit.** Docs in the same commit:
  - `docs/specs/i18n.md`: **Code:** `` `app/lib/l10n/`, `locales/` ``; §Parity check: "checks ARB and Fluent (`locales/<lang>/leemusync.ftl`)"; §Terminology: rows "orphaned save | partida huérfana" and "system keyring | llavero del sistema"; §Open questions: "clap's built-in headings (`Usage:`, `Options:`) and `-h`/`-V` descriptions are English. Localise them with a custom help template before 1.0."
  - `docs/specs/config-and-cli.md`: §Commands notes `LEEMUSYNC_TEST_ROOT` as test-only, and that secrets are only typed at prompts.

  Then `just check`, then:

```bash
git add Cargo.toml Cargo.lock crates/cli locales xtask docs/specs
git commit -m "feat(cli): add the first-sync commands with en/es output

init, join, backend, grant, players, scan, accounts, sync, status,
history, restore and doctor; --json everywhere; Fluent parity in
cargo xtask i18n-check.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 18: Test day on the real PCs (manual runbook)

Done by the maintainer, with an agent on each PC if wanted. It needs Tasks 1–17 merged to `main` and `just check` green. Real saves are only read and written by `leemusync`, after the panic backups exist. Agents never copy saves anywhere else and never record save contents. Names, sizes and hashes are fine to record.

- [ ] **Step 1: Panic backups.**
  - **CachyOS laptop:** done on 2026-10-18 at `~/Documents/panic/botw-cachyos-2026-10-18/` (170 files sha256-verified, `RESTORE.md` inside). Check it's still there.
  - **Fedora PC** (Cemu Flatpak). Close Cemu (`pgrep -a -i cemu` prints nothing), then:

    ```sh
    S=~/.var/app/info.cemu.Cemu/data/Cemu/mlc01/usr/save
    D=~/Documents/panic/botw-fedora-$(date +%F)
    mkdir -p "$D"
    cp -a "$S/00050000/101c9400" "$D/101c9400"
    cp -a "$S/system/act" "$D/act"
    (cd "$S/00050000" && find 101c9400 -type f -exec sha256sum {} + | sort -k2) > "$D/SHA256SUMS"
    (cd "$D" && sha256sum -c SHA256SUMS | grep -v ': OK$') || echo "all OK"
    diff -r "$S/system/act" "$D/act" && echo "accounts OK"
    ls "$S/00050000/101c9400/user"
    ```

    Expected: `all OK`, `accounts OK`, and a list of user folders (note whether an orphaned `80000002` is there). Copy `RESTORE.md` from the CachyOS backup and change `S=` to the Flatpak path above. If `80000002` hasn't been deleted on Fedora yet, take this backup first.
- [ ] **Step 2: Build and check on both PCs.**

    ```sh
    git switch main && git pull
    cargo build --release -p leemusync-cli
    alias leemusync="$PWD/target/release/leemusync"
    leemusync doctor
    ```

    Expected: `keystore` ok (KWallet on both). `backend` fails ("not set up yet"), and that's fine at this point.
- [ ] **Step 3: CachyOS creates the repo.** Use the maintainer's Desktop OAuth client from SP6. Its consent screen is in Testing, and the maintainer's Google account must be a test user.

    ```sh
    leemusync backend add gdrive --client-id <client id>   # paste the secret; approve in the browser
    leemusync init                                          # passphrase twice; store the recovery key safely
    leemusync grant add ~/.local/share/Cemu
    leemusync grant add ~/.config/Cemu
    leemusync scan                                          # expect account 80000001 and orphaned 80000002
    leemusync players add "<CachyOS player>"
    leemusync players add "<Fedora player>"
    leemusync accounts map "<CachyOS player>" cemu 80000001
    ```

- [ ] **Step 4: Fedora joins.**

    ```sh
    leemusync backend add gdrive --client-id <client id>
    leemusync join
    leemusync grant add ~/.var/app/info.cemu.Cemu
    leemusync scan
    leemusync accounts map "<Fedora player>" cemu 80000001
    ```

- [ ] **Step 5: First pushes.** On each PC: `leemusync sync --dry-run` (expect `uploaded` for that PC's own player), then `leemusync sync` (it asks whether the panic backup exists; answer yes). The second PC to sync also shows the other player as `no account on this device`.
- [ ] **Step 6: The other player's save.** On each PC:
  1. **The orphan first.** To keep it as its own player: `leemusync players add "Old <PC>"` and `leemusync accounts recover cemu 80000002 --player "Old <PC>"`. To skip it: the panic backup already holds it.
  2. **Create the second account in Cemu** (account settings, under General settings in 2.6; check the menu on the day), then close Cemu. Run `leemusync scan` to see its id, probably `80000002`.
  3. Run `leemusync accounts map "<other player>" cemu <new id>`. If it asks keep or replace (the folder still holds the orphan's files), choose **replace**. The current files go to local history first.
  4. Run `leemusync sync --dry-run` (expect `restored the latest version`), then `leemusync sync`.
- [ ] **Step 7: Check.**
  - Open Cemu on each PC, pick each account in turn, load BotW, and confirm it's the right save (location and play time). Close Cemu.
  - While Cemu is open, run `leemusync doctor` once on each PC. Expect `cemu running running`, which confirms process detection for the Flatpak and the native package.
  - Run `leemusync sync` on both PCs. Expect `up to date` everywhere. An "Old …" player shows `no account on this device`, which is expected.
  - Compare names and hashes only, without copying files. On each PC, for each account folder: `(cd <save>/00050000/101c9400/user/<id> && find . -type f -exec sha256sum {} + | sort -k2)`. Fedora's `80000002` must match CachyOS's `80000001`, and the other way round.
- [ ] **Step 8: If anything looks wrong**, stop syncing, follow `RESTORE.md` on that PC, and keep `~/.local/share/leemusync/history/` (it holds every pre-restore backup). Debug with `superpowers:systematic-debugging`.
- [ ] **Step 9: Record and close.**
  - `docs/progress/roadmap.md` Phase 1: "M1a reached on <date>", plus anything surprising.
  - `docs/specs/emulator-profiles.md` §Linux paths: process detection seen live for the Flatpak and `cemu-bin`.
  - `docs/specs/players-and-accounts.md`: the second account's id as Cemu assigned it, and the orphan outcome.
  - `docs/specs/storage-backends.md` §Google Drive (live): the first real repo (object counts and timings, if notable).
  - `CHANGELOG.md` `[Unreleased]`: "First sync between two PCs through Google Drive (CLI)".
  - Tick this plan and set the board row to Done (awaiting cleanup). Run `cargo xtask docs-check`, then commit `docs(progress): record the M1a test day` with the trailer, and push.

---

### Task 19: Spike SP8: Cemu account creation (option c), behind V-ACC-3

Timebox: 1 day, on branch `spike/sp8-cemu-account`. The output is an answer and a design, not shipped code. Anything written here is throwaway unless a later plan adopts it. **Never on live data:** everything runs with a scratch `HOME`.

- [ ] **Step 1: Scratch Cemu on the CachyOS laptop.**

    ```sh
    X=$(mktemp -d)/home && mkdir -p "$X/.local/share" "$X/.config"
    cp -a ~/.config/Cemu "$X/.config/Cemu"                              # settings.xml (game paths)
    mkdir -p "$X/.local/share/Cemu/mlc01/usr/save/system"
    cp -a ~/.local/share/Cemu/mlc01/usr/save/system/act "$X/.local/share/Cemu/mlc01/usr/save/system/act"
    cp ~/.local/share/Cemu/keys.txt "$X/.local/share/Cemu/" 2>/dev/null || true
    HOME="$X" cemu                                                       # confirm it starts and lists 80000001
    ```

    Make sure `settings.xml`'s `mlc_path` is empty, so the scratch `mlc01` is the one used. Don't copy any `00050000` save folder.
- [ ] **Step 2: Learn what must change.** Compare two accounts Cemu created itself (create one in the scratch Cemu), by key names and value formats only: which keys differ (`PersistentId`, `Uuid`, `MiiName`, `MiiData`, `AccountId`, `PrincipalId`, `TransferableIdBase`, `SimpleAddressId`, …), and how `persisid.dat` changes. Read `src/Cafe/Account/Account.cpp` at Cemu v2.6 for the same answer from the source.
- [ ] **Step 3: Write one account by hand** in the scratch tree, from the template rules found in Step 2, with a throwaway script kept outside the repo. Start the scratch Cemu: does it list the account, can BotW start with it, and does a save create `user/<id>/`?
- [ ] **Step 4: Record.** Put the result in V-ACC-3 ([players-and-accounts §Verify](../../specs/players-and-accounts.md#verify)) with the evidence, and move it into §Cemu accounts if it's proven. If it works, add a short design to that spec: profile fields that describe account creation as data (P3), which keys get new values, and the confirmation the UI shows. The implementation goes into the next Phase 1 plan. If it fails, write down why and leave choices a and b as the only ones. Commit `docs(spike): record SP8 Cemu account creation` with the trailer.

---

## Self-review (done while writing, 2026-10-18)

- **Design coverage:**
  - Components (section 1): Tasks 2–17.
  - Data flow, with panic backups (section 2): Tasks 15, 16 and 18.
  - Error handling, I1–I6 (section 3): Tasks 10, 13, 14 and 16.
  - Testing (section 4): every task, plus Task 16.
  - Test day and SP8 (section 5): Tasks 18 and 19.
  - Docs (W1): Task 1, plus each task's commit step.
- **Verified APIs (compiled against the real crates on 2026-10-18, scratch project, not committed):**
  - OpenDAL 0.59.5 `Gdrive::access_token`, `Operator::new` returning `Operator`, `OperationContext::with_http_transport`, `HttpTransporter::new(ReqwestTransport::new(client))`, fs `write_with(..).if_not_exists(true)` → `ConditionNotMatch`, `info().capability().write_with_if_not_exists`.
  - reqwest 0.13 `tls_backend_preconfigured`; a fake token against Google's API gives `Unexpected … status: 401`.
  - No `aws-lc-sys` and no `openssl-sys` with this feature set.
  - `keyring` 3.6.3 reached KWallet.
  - `fluent-bundle` 0.16 plural formatting, `rusqlite` 0.37 `PRAGMA user_version`, `directories` 6 paths, `roxmltree` 0.21, `std::fs::File::try_lock`, `sysinfo` 0.37 process listing.
  - `argon2` 0.5.3 against the RFC 9106 vector; `chacha20poly1305` 0.10 with AAD.
  - `cargo deny check licenses` passes.
- **Not verified (each task's tests and clippy settle it):** exact clippy pedantic findings, the `FluentValue` integer conversions, and OpenDAL `delete` on a missing path (Task 7, Step 8 says what to do).
