---
name: leemusync-backend
description: Use when adding, configuring or debugging a LeemuSync storage backend (S3, WebDAV/Nextcloud, SFTP, Google Drive, OneDrive, Dropbox, MEGA, local folder), or when hitting backend auth, rate-limit, listing-consistency or conditional-write problems.
---

# Storage backends

The store moves opaque encrypted objects and knows nothing about saves. Spec: `docs/specs/storage-backends.md`. Credentials: `docs/specs/security.md` §Credentials. Why it's append-only: D5.

## Adding an adapter
1. Prefer an OpenDAL service (D10). A custom adapter needs a decision entry (security and maintenance review).
2. Implement the `store` trait: `get`, `put(create_only | overwrite)`, `list`, `delete` (idempotent), `stat`, `capabilities()`.
3. Classify every error: `transient` (backoff + jitter, honour `Retry-After`), `auth`, `quota`, `permanent`.
4. Report capabilities honestly: native or emulated `create_only`; `server_mtime` only if the provider returns it.
5. Run the conformance suite (`crates/store/tests/conformance.rs`) against CI services or a real account, and record the results.
6. Credentials go in the OS keystore. OAuth uses authorization code + PKCE, with no client secrets in the app. Never log tokens.
7. Setup flow: UI wizard + `leemusync backend add` + config (P5), with a probe step and strings in en + es.
8. Update the adapters table, quirks and Verify lines in the spec.

## Debugging
| Symptom | Look at |
|---|---|
| A version appears late on another device | Listing consistency (V-SYNC-2); version cache in the state DB |
| A lease shows wrongly as stale or live | `server_mtime` capability; clock skew (`leemusync doctor`) |
| 429 / throttling | Backoff, per-prefix listing, batch size |
| Repeated auth prompts | Token refresh path, keystore entry, OAuth scope |
