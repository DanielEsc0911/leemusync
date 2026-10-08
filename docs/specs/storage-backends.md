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
| Any SSH server (a Pi, a NAS) | OpenDAL `sftp` | Key-based auth recommended. Linux and macOS only ([D25](../decisions.md#d25-sftp-only-on-linux-and-macos-for-now-2026-10-08)); needs the system OpenSSH client. See [SFTP on mobile and Windows](#sftp-on-mobile-and-windows) |
| Local folder, USB, mounted NAS/SMB | OpenDAL `fs` | Also serves iCloud Drive/OneDrive folders synced by the OS |
| Google Drive | OpenDAL `gdrive` | Phase 6. Scope `drive.file` (only files LeemuSync created) |
| OneDrive, Dropbox | OpenDAL `onedrive`, `dropbox` | Phase 6 |
| MEGA | see [MEGA](#mega) | Phase 6 |

## MEGA
MEGA isn't an OpenDAL service. In order of preference:
1. **MEGA S4** (MEGA's S3-compatible storage) through the `s3` adapter. Per [mega.io/objectstorage](https://mega.io/objectstorage) (read 2026-10-08), S4 is "available on Pro Lite, Pro I, Pro Flexi and Business at no extra cost", so free MEGA accounts can't use it. Not tested live (V-STORE-3).
2. A native adapter using an existing Rust MEGA crate, after a security and maintenance review. Candidates on crates.io (2026-10-08): [`mega`](https://crates.io/crates/mega) 0.8.0, last release 2024-10-09, one owner (Hirevo); [`megalib`](https://crates.io/crates/megalib) 0.11.1, last release 2026-07-06, one owner (11philip22). Neither has been reviewed yet.
3. MEGAcmd's local WebDAV server (desktop only).

Decide in spike SP6.

## Provider quirks
- **Google Drive:** Google's [Drive scope guide](https://developers.google.com/workspace/drive/api/guides/api-specific-auth) (read 2026-10-08) lists `drive.file` under "Non-sensitive scopes", "recommended for most use cases". Full `drive` is listed as restricted and needs restricted-scope OAuth app verification. Whether the public client still needs brand verification is open (V-STORE-2). Official builds ship the project's public OAuth client id (PKCE, no secret). Power users can supply their own. Caveat: OpenDAL's `gdrive` builder requires `client_secret` whenever `refresh_token` is set, and the tested code exchange sent the Desktop client's secret; a Desktop-client exchange without the secret is untested (V-STORE-2). Drive also keeps duplicate names in one folder (OpenDAL resolves the newest), `delete` moves files to the trash, `list` entries carry no mtime, and OpenDAL puts the refresh token and client secret in the token URL, which network errors print (V-STORE-7). Details: [Google Drive (live)](#google-drive-live).
- **Eventual consistency** (Drive, Dropbox, OneDrive listings): a new version may appear late. This is safe thanks to append-only writes, but lease liveness must tolerate it (V-SYNC-2).
- **Rate limits:** batch listings per stream prefix and cache known versions in the state DB.
- **WebDAV servers** differ in conditional-request support. The capability is probed at setup and stored. OpenDAL's `webdav` service doesn't expose `if_not_exists` or `if_match` at all, and Nextcloud's own `If-None-Match: *` is not race-safe ([tested](#tested-capabilities-sp6)).

## Tested capabilities (SP6)
Measured 2026-10-08 on Fedora 44 x86_64 with OpenDAL **0.59.4** (latest on crates.io, tag [`v0.59.4`](https://github.com/apache/opendal/tree/v0.59.4)) and a throwaway test binary (not in the repo). Servers ran locally in rootless podman, bound to 127.0.0.1, except the Google Drive row, which is a live cloud test ([Google Drive (live)](#google-drive-live)). Each service got two independent `Operator`s (two "devices").

| Service | Server | `if_not_exists` | Race: one winner of 2 | `if_match` | mtime (stat + list) | Overwrite moves mtime | ETag | List after write | copy / rename |
|---|---|---|---|---|---|---|---|---|---|
| `s3` | SeaweedFS 4.48 (30GB build) | yes | **100/100** | yes | yes, 1 s | yes | yes (MD5) | 20/20 at once, 5/5 rounds | yes / no |
| `s3` | RustFS 1.0.1 | yes | **100/100** | yes | yes, 1 s (list: ms) | yes | yes (MD5) | 20/20 at once, 5/5 rounds | yes / no |
| `webdav` | Nextcloud 35.0.1 | **not exposed** (`Unsupported`) | n/a | not exposed | yes, 1 s | yes | yes | 20/20 at once, 5/5 rounds | yes / yes |
| `sftp` | OpenSSH 8.4p1 (`atmoz/sftp`) | yes (`create_new`) | **100/100** | no | yes, 1 s | yes | no | 20/20 at once, 5/5 rounds | fails / yes |
| `fs` | tmpfs temp dir, default | yes (`O_EXCL`) | **100/100** | no | yes, ns | yes | no | 20/20 at once, 5/5 rounds | yes / yes |
| `fs` | with `atomic_write_dir` | declared | **0/100 (both won every round)** | no | yes, ns | yes | no | 20/20 at once, 5/5 rounds | yes / yes |
| `gdrive` | Google Drive API v3, **live cloud** | **not exposed** (`Unsupported`) | n/a | not exposed | stat: yes, ms; list: **none** | yes | none | 10/10 at once, 5/5 rounds | yes / yes |

How it was measured:
- **Race:** 100 rounds per service. Each round, both clients `write_with(key).if_not_exists(true)` to the same new key at once. "Winner" means `Ok`, loser means `ConditionNotMatch`. Each single winner's content was read back and matched.
- **Emulated `create_only`** (`exists` then `write`, the fallback in [Interface](#interface)) produced two winners in 100/100 rounds on `s3` and `fs`, 78/100 on `sftp` and 15/100 on `webdav`. It's only safe because version and blob names are unique.
- **Raw WebDAV:** Nextcloud answers `If-None-Match: *` (201, then 412) and `If-Match` (412 stale, 204 current) over plain HTTP. Under the same 100-round race it gave one winner 82 times, two winners 2 times, and `423 Locked` to both 16 times. So it is not a safe atomic create either.
- **List after write:** client A writes 20 objects under a new prefix, client B lists immediately. Every object was visible on the first list in every round (local servers, no network delay; cloud providers are V-SYNC-2).
- **mtime:** `stat` and `list` both return `last_modified` on all four services. S3, WebDAV and SFTP report whole seconds; an overwrite 2.1 s later advanced it on every service. Clock skew couldn't be measured because the servers shared the host clock.
- **SFTP copy** failed with `Unexpected` on this server (OpenDAL uses an SFTP extension that this `internal-sftp` server doesn't offer). LeemuSync doesn't need copy.
- **Max object size** (declared by OpenDAL, not tested): `s3` 5 GiB per part. The others declare no limit.
- **Images** (pulled 2026-10-08): `docker.io/chrislusf/seaweedfs@sha256:4e61d15fd35994cb1e43e1e553dff106794841fd9a99ade2fc8c8bfce4d7872d`, `docker.io/rustfs/rustfs@sha256:1803faef57627e2d9c2e7d89d655d712ddded5389040054987163043fecb6a3c`, `docker.io/library/nextcloud@sha256:f4e0ee28ac9e54cad6e04e148489d79a609acba86dfbff69c8f666d50f96e414`, `docker.io/atmoz/sftp@sha256:0960390462a4441dbb63698d7c185b76a41ffcee7b78ff4adf275f3e66f9c475`.
- **MinIO couldn't be tested:** on 2026-10-08 `docker.io/minio/minio` (latest and a pinned `RELEASE.` tag) returned "access denied", `quay.io/minio/minio` returned "unauthorized", and `dl.min.io` returned HTTP 410. SeaweedFS and RustFS stood in as S3 servers.

### Google Drive (live)
Run 2026-10-08 on Fedora 44 x86_64 with OpenDAL 0.59.4 `gdrive` (Drive API v3) on the maintainer's personal Google account. Scope `drive.file` only. OAuth client type "Desktop app", consent screen in Testing status. Two independent `Operator`s. Rounds were reduced for Drive quotas: race 20 rounds, list-after-write 5 rounds of 10 files. Test root `/leemusync-sp6-test/<run id>/`.

- **List after write:** all 10 files were visible on the first list in 5/5 rounds. That one list call took 0.5–2.6 s over the internet. `list` entries return no `last_modified`; `stat` returns it with millisecond precision.
- **Emulated `create_only`** (`exists` then `write`): both clients got `Ok` in 20/20 rounds. Inspected on Drive afterwards, 17 of the 20 names held two files with the same name in the same folder; the other 3 ended as one file (the second write overwrote).
- **Duplicate names:** Drive allows several files with the same name in one folder. OpenDAL resolves a path by querying `name = '<n>' and '<parent>' in parents and trashed = false` with `orderBy=modifiedTime desc,createdTime desc&pageSize=1` ([`core.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/core.rs), fn `query`), so with duplicates it silently picks the newest. Probe: clients A and B wrote the same new name `dup/k` at once. Both got `Ok` and Drive held 2 files named `k`. A third, fresh client's `list` showed 1 entry for `dup/k` and `read` returned "A". Deleting `dup/k` by name trashed one copy, and `stat dup/k` still succeeded afterwards (the other copy surfaced). Once a client has cached a file's id, its later writes overwrite in place (no new duplicate).
- **Delete is trash:** `delete` sends a PATCH with `{"trashed": true}` ([`deleter.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/deleter.rs) → `gdrive_trash` in [`core.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/core.rs)), not a permanent delete. `delete_with("/").recursive(true)` on the run root trashed only the run folder itself; its 91 files and 8 folders were trashed through the parent. The lister filters `trashed = false`, so trashed items vanish from listings. The empty top folder `leemusync-sp6-test` stays in the user's Drive. Trash retention and quota impact were not tested (V-STORE-9).
- **Rename onto an existing target** first trashes the target, then patches the source's metadata ([`backend.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/backend.rs) `rename` → `trash_path_if_exists`): two steps, not atomic.
- **Secrets in the token URL:** to refresh the access token, OpenDAL POSTs to `https://oauth2.googleapis.com/token?refresh_token=…&client_id=…&client_secret=…&grant_type=refresh_token` with an empty body, values not percent-encoded ([`core.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/core.rs)). When a request fails to send, the reqwest transport adds the full URL as error context `url`, and OpenDAL's `Error` Display prints it. An offline probe with fake credentials and an unreachable proxy (`HTTPS_PROXY=http://127.0.0.1:9`) printed:
  ```
  Unexpected (temporary) at stat, context: { url: https://oauth2.googleapis.com/token?refresh_token=FAKE-REFRESH-TOKEN&client_id=FAKE-CLIENT-ID&client_secret=FAKE-CLIENT-SECRET&grant_type=refresh_token, called: reqwest::send, service: gdrive, path: a } => send http request, source: error sending request
  ```
  So any network failure during a refresh puts the refresh token and client secret into error text, which would reach logs (V-STORE-7, V-SEC-4). The builder also requires `client_secret` whenever `refresh_token` is set ([`backend.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/gdrive/src/backend.rs)).
- **OAuth (desktop):** loopback redirect to `http://127.0.0.1:<port>`, PKCE S256 and `state`, Desktop-app client. Testing status shows Google's unverified-app warning, and Testing-mode refresh tokens expire after 7 days ([Google OAuth 2.0](https://developers.google.com/identity/protocols/oauth2#expiration)). The code exchange sent the Desktop client's secret in the form body.

### Documented, not tested
What OpenDAL 0.59.4 declares in each service's `capability` block (source at the tag). `gdrive` is now tested ([Google Drive (live)](#google-drive-live)). The provider must also honour the header; that is untested.

| Service | `write_with_if_not_exists` | `write_with_if_match` | Source |
|---|---|---|---|
| `s3` (AWS, R2, B2, Wasabi, MEGA S4) | yes | yes | [`services/s3/src/backend.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/s3/src/backend.rs). One flag set for every S3-compatible endpoint. The [compatible services notes](https://github.com/apache/opendal/blob/v0.59.4/core/services/s3/src/compatible_services.md) mention no R2 or B2 exception for conditional writes |
| `onedrive` | no | yes | [`services/onedrive/src/backend.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/onedrive/src/backend.rs) |
| `dropbox` | no | no | [`services/dropbox/src/backend.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/dropbox/src/backend.rs) |

AWS documents `If-None-Match: *` and `If-Match` on `PutObject`. When several conditional writes race, "the first write operation to finish succeeds" and the rest get 412 ([AWS conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html)). R2 and B2 support is unconfirmed (V-STORE-1).

### SFTP on mobile and Windows
OpenDAL's `sftp` service (0.59.4) depends on the [`openssh`](https://docs.rs/openssh/0.11.6/openssh/) crate 0.11.6 and `openssh-sftp-client`. `openssh` wraps the system `ssh` binary ("all commands are executed through the `ssh` command") and has `compile_error!("This crate can only be used on unix")` for non-Unix targets ([src/lib.rs](https://github.com/openssh-rust/openssh/blob/v0.11.6/src/lib.rs)). Consequences:
- **Windows:** the `sftp` service doesn't compile.
- **Android and iOS:** there's no `ssh` binary to launch, and iOS apps can't spawn processes. So it can't work there as-is.
- **Decision:** SFTP is offered on Linux and macOS only ([D25](../decisions.md#d25-sftp-only-on-linux-and-macos-for-now-2026-10-08)).
- **Alternatives** (deferred by D25): pure-Rust SSH with [`russh`](https://crates.io/crates/russh) 0.64.1 + [`russh-sftp`](https://crates.io/crates/russh-sftp) 3.0.1, or libssh2 bindings via [`ssh2`](https://crates.io/crates/ssh2) 0.9.6 (versions from crates.io, 2026-10-08). Either would need a custom store adapter.

### `fs` and atomic writes
OpenDAL's `fs` gets an atomic `if_not_exists` only without `atomic_write_dir`: it opens the target with `create_new` (`O_EXCL`) and writes in place, so a crash can leave a partial file. With `atomic_write_dir`, it writes a temp file and renames it, but `if_not_exists` becomes a check-then-rename and both racers won every round ([`services/fs/src/writer.rs`](https://github.com/apache/opendal/blob/v0.59.4/core/services/fs/src/writer.rs)). Neither mode gives atomic content and atomic create together (V-STORE-5).

## Setup and testing a backend
1. Enter the details (UI wizard, CLI `backend add`, or config).
2. **Probe:** write/read/list/delete a test object under `leemusync/.probe/`. Detect capabilities and measure latency.
3. Store the credentials ([security §Credentials](security.md#credentials)).

## Conformance suite
Every adapter must pass the shared suite in `crates/store/tests/conformance.rs`: round-trip, `create_only` refuses to overwrite, list after write (within the documented delay), delete is idempotent, 0-byte and 64 MiB objects, server mtime if claimed. It runs in CI against MinIO, a WebDAV server and an SFTP server in Docker. Cloud providers are tested manually before each release ([testing](testing.md)).

## Verify
Local services are answered in [Tested capabilities](#tested-capabilities-sp6). The rest is pending accounts (deferred by the maintainer).
- **V-STORE-1** Live conditional writes, server mtime, list-after-write delay and rate limits on AWS S3, Cloudflare R2, Backblaze B2, OneDrive and Dropbox. Pending accounts (SP6). Google Drive done live 2026-10-08 ([Google Drive (live)](#google-drive-live)).
- **V-STORE-2** Google Drive `drive.file` OAuth: desktop flow done 2026-10-08 ([Google Drive (live)](#google-drive-live)). Open: the mobile flow, whether Google accepts a Desktop-client code exchange without the secret (and how that fits OpenDAL requiring `client_secret` with `refresh_token`), and whether the public client needs brand verification (SP6).
- **V-STORE-3** MEGA: live S4 test, security review of `mega`/`megalib`, MEGAcmd WebDAV. Pending accounts (SP6).
- **V-STORE-4** A pure-Rust SFTP adapter for Windows, Android and iOS (deferred by [D25](../decisions.md#d25-sftp-only-on-linux-and-macos-for-now-2026-10-08)).
- **V-STORE-5** `fs`: how `crates/store` gets both crash-safe writes and an atomic `create_only` (e.g. its own temp file + `link`/`renameat2(RENAME_NOREPLACE)`), and whether it works on SMB and synced folders (Phase 1).
- **V-STORE-6** CI S3 server for the conformance suite, since MinIO images weren't pullable on 2026-10-08 (SeaweedFS and RustFS worked). Decide in Phase 1.
- **V-STORE-7** OpenDAL `gdrive` sends the refresh token and client secret in the token URL query, and network errors print that URL ([Google Drive (live)](#google-drive-live)). Options, not decided: `crates/store` refreshes tokens itself with a form body and gives OpenDAL only an `access_token` (rebuilding the operator on expiry), or fix it upstream in OpenDAL. In every case the log redaction layer must scrub OAuth query parameters (V-SEC-4). Decide in Phase 1.
- **V-STORE-8** Drive duplicate names: two devices creating a shared name such as `repo.json` at once leave two files, and readers see the newest. Decide how `crates/store` handles this and whether to detect duplicates by listing raw file ids (Phase 1).
- **V-STORE-9** Drive trash: pruned data goes to the user's Drive trash. Retention and quota impact are untested. Decide whether pruning should permanently delete (Phase 6).

## Open questions
- **Follow-up (D25):** the Linux `.deb` and `.rpm` should *recommend* `openssh-client` / `openssh-clients` so SFTP works out of the box. Packaging isn't changed yet.
- How a backend that's unavailable on the current platform (SFTP on Windows, Android, iOS) appears in the UI wizard and when a synced config names it.
- WebDAV has no race-safe atomic create through OpenDAL or Nextcloud. Only the [lease](sync-model.md#leases) and `repo.json` overwrite, and the lease doesn't depend on atomic create, but anything that later wants a real lock on WebDAV can't rely on it.
