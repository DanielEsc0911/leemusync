# Sync model

> **Status:** Partial (leases, play sessions and retention are still open) · **Related:** [rules](../rules.md) (B2, B3, P3), [reliability](reliability.md), [security](security.md), [players-and-accounts](players-and-accounts.md), [emulator-profiles](emulator-profiles.md), [storage-backends](storage-backends.md), [glossary](../glossary.md) · **Code:** `crates/core/src/{manifest,heads,reconcile,config}.rs`, `crates/engine/src/{repo,repo_config}.rs`, `crates/engine/src/sync.rs`, `crates/core/src/lease.rs`, `crates/engine/src/lease.rs`, `crates/engine/src/resolve.rs`, `crates/engine/src/{play,process}.rs`

## Purpose
How saves move between devices without loss: what is synced (streams), how history is stored (versions), how divergence is detected (heads), and how simultaneous play is coordinated (leases, play sessions).

## Streams
A stream is the unit of sync: `(player, game key, kind, compat group)`, where kind is `save` or `state`.
- The stream id is a keyed hash of the canonical tuple, so it's opaque on the remote ([security §Keys](security.md#keys)).
- Granularity follows the tier ([emulator-profiles §Tiers](emulator-profiles.md#tiers)). Full and Standard have one stream per game per player. Basic has one stream per container per player, and its lease covers the whole container.
- Game key comes from the profile capture (title ID, serial, normalised ROM name). Cross-device aliases ("Pokemon Emerald (USA)" ≙ "pokemon emerald") live in [repo config](#repo-config).

## Versions
- **Immutable** snapshot of a stream. Manifest fields: `format`, `stream`, `id`, `parents[]`, `device`, `player`, `created_at`, `emulator {id, version}`, `files[{path, size, mtime, blob}]`, `meta {session_secs, changed_files, takeover, note}`.
  `created_at` and `files[].mtime` are Unix milliseconds. `blob` is the lowercase hex of the keyed BLAKE3 hash. Serialised as JSON with unknown fields rejected; `format` is `1`.
- **Version id:** time-ordered and unique (48-bit milliseconds + 80-bit random, ULID-style). Never reused, sorts by creation.
- **Parents:** the version(s) the local copy was based on. The first version has none. A conflict resolution has all the heads it resolves.
- **Commit point:** the manifest object, written create-only after all its blobs exist. Readers ignore versions whose blobs are missing or fail authentication.

## Heads and divergence
- A head is a version no other version lists as a parent. One head means clean; more than one means a conflict.
- This is correct on any backend: unique, never-overwritten object names mean no write can clobber another, so no compare-and-swap is needed ([D5](../decisions.md#d5-append-only-content-addressed-repo-with-derived-heads-2026-10-07)).

## Reconciliation
Per stream, the device tracks `base` (the version its local files match) and whether local files are `clean` or `dirty` (changed since base).

| Local | Remote heads | Action |
|---|---|---|
| clean | one head = base | Nothing |
| clean | one head, descends from base | Fast-forward: restore head (with backup) |
| dirty | one head = base | Upload new version, parent = base |
| dirty | one head ≠ base | Upload local as a version (parent = base) → two heads → conflict |
| any | several heads | Conflict |

Uploading before asking means the local work is safe on the remote even if this device dies before the player decides.

With no `base` yet: no local files and one head → fast-forward. Local files and one head whose files are identical → adopt that head as `base` (clean). Local files and one different head → upload with no parents, which makes two heads (conflict): nothing is overwritten. `replace_local` (set when the player chooses to replace an account's current files, [players-and-accounts §Discovery](players-and-accounts.md#discovery)) turns that last case into a fast-forward with backup. With a `base`: if the local files equal a head, that head becomes `base` (this also makes a crash between writing a manifest and saving `base` harmless). A `base` the remote no longer has → warn that the remote went backwards ([security §Rollback and withholding](security.md#rollback-and-withholding)), no action.

## Conflicts
- Never merged automatically; saves are opaque binary ([D7](../decisions.md#d7-no-game-save-format-parsing-in-core-2026-10-07)).
- The player sees each head with device, player, time, session length, changed files, size and emulator version. M1b (`Engine::conflicts`, `HeadInfo`) shows device, player, time, files, size and changed files; session length and emulator version wait for manifest fields that record them.
- Choosing writes a **resolution version**: content of the chosen head, parents = all heads. The others stay in history and can be restored any time. The resolution reuses the chosen head's blobs (nothing is re-uploaded) and is committed create-only like any manifest; it is then restored on this device with the current files backed up to local history first (`<ms>-pre-resolve`, I1). A crash after the commit is safe: the next sync fast-forwards to it with backup. Choosing a version that isn't a head, or a stream without a conflict, is a usage error.
- Policy setting `conflict_policy = "ask" | "newest"` (default `ask`). `newest` still writes a resolution version and keeps the others. "Newest" is the head with the latest `created_at`, which is its writer's clock (ties: the larger version id); `sync` reports the stream as `Resolved`.
- While a conflict is unresolved, the device keeps its local files. Launcher mode asks for a choice before starting that game, or the player can "play this device's copy".

## Leases
Advisory per-stream locks so two devices don't play the same player's save at once ([D6](../decisions.md#d6-leases-are-advisory-divergence-detection-is-the-safety-net-2026-10-07)).
- Object `locks/<streamId>/<deviceId>` (encrypted, AAD `lease\0<streamId>\0<deviceId>`) holds JSON `{format: 1, device, player, session, acquired_at_ms, renewed_at_ms, ttl_ms}` (device-clock Unix ms; `session` is a random id per acquisition). Each device writes only its own file, and it is the only object besides `repo.json` that is overwritten.
- **Acquire:** list leases. Any live lease from another device → warn ("BotW is in use on Desktop, active 2 min ago"). Otherwise write our lease and list again. If another live lease appeared meanwhile, the earliest `(acquired_at, device id)` wins and the other device deletes its own lease and backs off.
- **Renew** every 60 s. **TTL** 5 min (`config.toml` `[leases] ttl_secs`, 120–86400). **Release** at session end by deleting our lease; a forced release deletes only this device's lease.
- **Liveness:** a lease is live until its last renewal + TTL + 2 min skew. The last renewal is the backend's server-side modified time when the backend reports it (`Entry.modified`), otherwise the holder's `renewed_at_ms`.
- **Stale** (expired) leases can be taken over after confirmation (`takeover`). The stale file stays; it is its device's to delete. The takeover is recorded in the next version's `meta` (`takeover: true`, `takeover_from: <deviceId>`).
- A lease that doesn't decrypt, parse, or names another device than its object name is ignored with a `lease_unreadable` warning and never trusted (B2).
- Leases never weaken safety. Anything they miss becomes a conflict, never a loss.

## Play sessions
### Launcher mode (primary)
1. Resolve the stream(s) for (player, game).
2. Pull: fast-forward if needed (staged restore with backup, [reliability](reliability.md)).
3. Acquire the lease. If it's held elsewhere: wait / play anyway / take over if stale.
4. Launch the emulator (argv, never a shell).
5. Renew the lease while it runs.
6. On exit: wait the quiet period, snapshot, upload.
7. Release the lease.

Steps 2 and 6 drive the live status surfaces ([status-and-notifications](ui/status-and-notifications.md)).

M1b (`Engine::play`, `PlayReport`):
- The game's path comes from the device's game catalog (`Engine::games`, [emulator-profiles §Game discovery](emulator-profiles.md#game-discovery)): the user's `games.<key>` first, then a game found in the emulator's own game folders, then one in the user's `game_dirs`. So `play` works for any installed game without config. A game no source knows is a usage error `game_not_found` that suggests `lemusync games` and `lemusync config set emulators.<id>.games.<key> <path>`. Names in the catalog are found on each device, never stored in the repo ([D34](../decisions.md#d34-game-names-are-found-on-each-device-never-stored-in-the-repo-2026-10-27)); a device without the game shows the bare key. Sync itself is driven by save folders and doesn't need the catalog.
- Refused with `emulator_running` if the emulator is already open (the profile's process names), checked before the pull and again right before launch (I3).
- Steps 2 and 6 are a sync of that one stream. A conflict at step 2 asks the player for a head (a resolution version) or "play this device's copy".
- Held lease: **wait** polls every 10 s, **play anyway** plays without a lease (a divergence becomes a conflict, D6), **take over** works only on a stale lease.
- The emulator's blocking wait runs on its own thread while the engine renews every 60 s. A failed renewal is a `lease_renew_failed` warning and is retried; it never stops the game.
- Step 6 waits until no save file changed for the quiet period, then snapshots and uploads; no change uploads nothing.
- Release runs on every path once the lease is taken, errors included. A failed release is a `lease_release_failed` warning. If LemuSync itself is killed mid-game, the lease goes stale after its TTL and the next acquire on this device overwrites it.
- A failed upload after exit keeps the files and the stream's base, so the next sync sees them as changed and uploads them (B3).

### Watch mode (fallback)
- **Desktop:** the profile's process names mark when the emulator is running. On the first save change, LemuSync takes the lease for that stream. When the emulator exits (or files go quiet): snapshot, upload, release.
- **No process info (mobile):** a file change plus the quiet period triggers snapshot and upload. The lease is held from the first change until the upload finishes.
- **Pull** happens at emulator start (desktop), on app open, and on scheduled syncs (mobile).

### Switching machines
Launcher mode on both ends makes handoff automatic (SC2). With watch mode the handoff is as good as the pull triggers above.

## Snapshot rules
- Only when the emulator is closed (if the profile says `requires_closed` and running state is detectable) and files have been quiet for the quiet period (default 10 s).
- Read → hash → re-stat. If size or mtime changed during the read, retry later (no torn snapshots).
- Files over the profile's size limit are refused with an error (disk-fill guard).

## Repo config
Players, account mappings, game aliases and repo-wide settings. Stored as a special `repo-config` stream using the same versions and heads, so conflicts are handled the same way.

Several heads are merged automatically by union ([D30](../decisions.md#d30-repo-config-heads-merge-automatically-2026-10-18)). A config version is `{format, id, parents[], device, created_at, config}` stored at `config/versions/<versionId>`. The config holds players, per-device account mappings, and each player's games (`"<emulator>/<game key>"`), so a device can find a player's streams although stream ids are opaque.

## Remote layout
```
<root>/lemusync/
  repo.json                              format version, KDF params, wrapped master keys
  config/versions/<versionId>            repo-config stream
  devices/<deviceId>                     device record
  streams/<streamId>/versions/<versionId>
  locks/<streamId>/<deviceId>
  blobs/<2 hex>/<blobId>
```
- Every name is an opaque id. Every object except the public fields of `repo.json` is encrypted ([security](security.md#cryptography)).
- AAD strings and lowercase object names: `crates/engine/src/repo.rs`.
- Blobs are whole files, content-addressed with a keyed hash, so identical files dedupe across versions and players. Chunking for large save states is deferred until it's needed (P1).

## Retention
- **Keep:** all heads, the last 50 versions per stream (configurable), and anything referenced by an unresolved conflict.
- **Blob GC:** delete unreferenced blobs older than 7 days. The grace period protects in-flight uploads. Only the device holding the `gc` lease runs GC.
- Local history on the device: [reliability §Local history](reliability.md#local-history).

## Save states
A separate stream kind (`state`), opt-in per emulator or game. The manifest records the emulator version and core. Restoring warns on a mismatch.

## Offline
Snapshots queue in local history and upload when the device is back online. Parents are kept as they were, so divergence is still detected.

## Verify
- **V-SYNC-1** Server-side modified time for lease liveness. Local services are answered: `s3` (SeaweedFS, RustFS), `webdav` (Nextcloud), `sftp` and `fs` return it from `stat` and `list`, and a renewal (overwrite) advances it, at 1 s granularity on the network services ([storage-backends §Tested capabilities](storage-backends.md#tested-capabilities-sp6)). Clock skew on a real remote server and the cloud providers are pending accounts (SP6). R2 (SP9, 2026-10-21): `list` and `stat` return `LastModified` and an overwrite advances it; R2's clock was +1.5 s from Fedora and −0.3 s from CachyOS (both NTP-synced), far inside the 2 min skew tolerance. Leases on R2 use the server time.
- **V-SYNC-2** Listing delay and consistency on Google Drive, Dropbox and OneDrive (SP6). Google Drive data point 2026-10-08: listings were consistent on the first list in 5/5 rounds of 10 files (small sample). Drive `list` entries carry no mtime, so lease liveness on Drive can't come from listing mtimes; it needs a `stat` per lease or the lease's own `renewed_at` ([storage-backends §Google Drive (live)](storage-backends.md#google-drive-live)).

## Open questions
- Watch mode on desktop: should LemuSync take the lease for the *last played* game as soon as the emulator starts? Decide with Phase 2 usage data.
