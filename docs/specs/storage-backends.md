# Storage backends

> **Status:** Draft · **Related:** [rules](../rules.md) (P2, B2, B3), [decisions](../decisions.md) (D5, D10), [sync-model](sync-model.md#remote-layout), [security](security.md#credentials), [testing](testing.md) · **Code:** `crates/store` (planned)

## Purpose
Let users keep their repo on storage they already own. The store layer moves opaque encrypted objects and knows nothing about saves.

## Interface
`crates/store` exposes one trait, sketched here:

| Operation | Semantics |
|---|---|
| `get(path)` | Read the whole object |
| `put(path, bytes, mode)` | `mode`: `create_only` or `overwrite` (only leases and `repo.json` overwrite) |
| `list(prefix)` | Names + size + server modified time (if known) |
| `delete(path)` | Idempotent |
| `stat(path)` | Exists, size, server modified time |
| `capabilities()` | `{create_only: native\|emulated, server_mtime: bool, max_object_mb}` |

- `create_only` uses the backend's native conditional write where available (OpenDAL `if_not_exists`). Otherwise it's emulated with stat-then-write. That's safe because version and blob names are unique ([D5](../decisions.md#d5-append-only-content-addressed-repo-with-derived-heads-2026-10-07)).
- All errors are classified as `transient` (retry with exponential backoff + jitter, honouring `Retry-After`), `auth` (ask the user), `quota`, or `permanent`.

## Adapters
| Provider | Adapter | Notes |
|---|---|---|
| AWS S3, MinIO, Backblaze B2, Cloudflare R2, Wasabi, MEGA S4 | OpenDAL `s3` | "Amazon" means S3. Amazon Drive shut down in 2023 |
| Nextcloud, ownCloud, Synology, `rclone serve webdav` | OpenDAL `webdav` | The most common self-hosted path |
| Any SSH server (a Pi, a NAS) | OpenDAL `sftp` | Key-based auth recommended. Not available on iOS/Android builds unless verified (SP6) |
| Local folder, USB, mounted NAS/SMB | OpenDAL `fs` | Also serves iCloud Drive/OneDrive folders synced by the OS |
| Google Drive | OpenDAL `gdrive` | Phase 6. Scope `drive.file` (only files LeemuSync created) |
| OneDrive, Dropbox | OpenDAL `onedrive`, `dropbox` | Phase 6 |
| MEGA | see [MEGA](#mega) | Phase 6 |

## MEGA
MEGA isn't an OpenDAL service. In order of preference:
1. **MEGA S4** (MEGA's S3-compatible storage) through the `s3` adapter. Works today, but it's a paid MEGA feature.
2. A native adapter using an existing Rust MEGA crate, after a security and maintenance review.
3. MEGAcmd's local WebDAV server (desktop only).

Decide in spike SP6.

## Provider quirks
- **Google Drive:** OAuth with `drive.file` avoids the restricted-scope security assessment that full `drive` access requires (Verify SP6). Official builds ship the project's public OAuth client id (PKCE, no secret). Power users can supply their own.
- **Eventual consistency** (Drive, Dropbox, OneDrive listings): a new version may appear late. This is safe thanks to append-only writes, but lease liveness must tolerate it (V-SYNC-2).
- **Rate limits:** batch listings per stream prefix and cache known versions in the state DB.
- **WebDAV servers** differ in conditional-request support. The capability is probed at setup and stored.

## Setup and testing a backend
1. Enter the details (UI wizard, CLI `backend add`, or config).
2. **Probe:** write/read/list/delete a test object under `leemusync/.probe/`. Detect capabilities and measure latency.
3. Store the credentials ([security §Credentials](security.md#credentials)).

## Conformance suite
Every adapter must pass the shared suite in `crates/store/tests/conformance.rs`: round-trip, `create_only` refuses to overwrite, list after write (within the documented delay), delete is idempotent, 0-byte and 64 MiB objects, server mtime if claimed. It runs in CI against MinIO, a WebDAV server and an SFTP server in Docker. Cloud providers are tested manually before each release ([testing](testing.md)).

## Verify
- **V-STORE-1** OpenDAL conditional-write and server-mtime support per service (SP6).
- **V-STORE-2** Google Drive `drive.file` scope classification and OAuth verification needs (SP6).
- **V-STORE-3** MEGA options (SP6).
- **V-STORE-4** SFTP on Android/iOS builds (SP6).
