# Sync model

> **Status:** Draft · **Related:** [rules](../rules.md) (B2, B3, P3), [reliability](reliability.md), [security](security.md), [players-and-accounts](players-and-accounts.md), [emulator-profiles](emulator-profiles.md), [storage-backends](storage-backends.md), [glossary](../glossary.md) · **Code:** —

## Purpose
How saves move between devices without loss: what is synced (streams), how history is stored (versions), how divergence is detected (heads), and how simultaneous play is coordinated (leases, play sessions).

## Streams
A stream is the unit of sync: `(player, game key, kind, compat group)`, where kind is `save` or `state`.
- The stream id is a keyed hash of the canonical tuple, so it's opaque on the remote ([security §Keys](security.md#keys)).
- Granularity follows the tier ([emulator-profiles §Tiers](emulator-profiles.md#tiers)). Full and Standard have one stream per game per player. Basic has one stream per container per player, and its lease covers the whole container.
- Game key comes from the profile capture (title ID, serial, normalised ROM name). Cross-device aliases ("Pokemon Emerald (USA)" ≙ "pokemon emerald") live in [repo config](#repo-config).

## Versions
- **Immutable** snapshot of a stream. Manifest fields: `format`, `stream`, `id`, `parents[]`, `device`, `player`, `created_at`, `emulator {id, version}`, `files[{path, size, mtime, blob}]`, `meta {session_secs, changed_files, takeover, note}`.
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

## Conflicts
- Never merged automatically; saves are opaque binary ([D7](../decisions.md#d7-no-game-save-format-parsing-in-core-2026-10-07)).
- The player sees each head with device, player, time, session length, changed files, size and emulator version.
- Choosing writes a **resolution version**: content of the chosen head, parents = all heads. The others stay in history and can be restored any time.
- Policy setting `conflict_policy = "ask" | "newest"` (default `ask`). `newest` still writes a resolution version and keeps the others.
- While a conflict is unresolved, the device keeps its local files. Launcher mode asks for a choice before starting that game, or the player can "play this device's copy".

## Leases
Advisory per-stream locks so two devices don't play the same player's save at once ([D6](../decisions.md#d6-leases-are-advisory-divergence-detection-is-the-safety-net-2026-10-07)).
- Object `locks/<streamId>/<deviceId>` (encrypted) holds `{device, player, session, acquired_at, renewed_at, ttl}`. Each device writes only its own file.
- **Acquire:** list leases. Any live lease from another device → warn ("BotW is in use on Desktop, active 2 min ago"). Otherwise write our lease and list again. If another live lease appeared meanwhile, the earliest `(acquired_at, device id)` wins and the other device backs off.
- **Renew** every 60 s. **TTL** 5 min (configurable). **Release** at session end by deleting our lease.
- **Liveness** uses the backend's server-side modified time when the backend reports it; otherwise device clocks with 2 min skew tolerance.
- **Stale** (expired) leases can be taken over after confirmation. The takeover is recorded in the next version's `meta`.
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

### Watch mode (fallback)
- **Desktop:** the profile's process names mark when the emulator is running. On the first save change, LeemuSync takes the lease for that stream. When the emulator exits (or files go quiet): snapshot, upload, release.
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

## Remote layout
```
<root>/leemusync/
  repo.json                              format version, KDF params, wrapped master keys
  config/versions/<versionId>            repo-config stream
  devices/<deviceId>                     device record
  streams/<streamId>/versions/<versionId>
  locks/<streamId>/<deviceId>
  blobs/<2 hex>/<blobId>
```
- Every name is an opaque id. Every object except the public fields of `repo.json` is encrypted ([security](security.md#cryptography)).
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
- **V-SYNC-1** Server-side modified time for lease liveness. Local services are answered: `s3` (SeaweedFS, RustFS), `webdav` (Nextcloud), `sftp` and `fs` return it from `stat` and `list`, and a renewal (overwrite) advances it, at 1 s granularity on the network services ([storage-backends §Tested capabilities](storage-backends.md#tested-capabilities-sp6)). Clock skew on a real remote server and the cloud providers are pending accounts (SP6).
- **V-SYNC-2** Listing delay and consistency on Google Drive, Dropbox and OneDrive (SP6).

## Open questions
- Watch mode on desktop: should LeemuSync take the lease for the *last played* game as soon as the emulator starts? Decide with Phase 2 usage data.
