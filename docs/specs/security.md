# Security

> **Status:** Draft · **Related:** [rules](../rules.md) (B2 is the rule this spec implements; also P2, P7, W3), [architecture](architecture.md#ipc), [sync-model](sync-model.md), [storage-backends](storage-backends.md), [emulator-profiles](emulator-profiles.md#validation), [release](release.md), [skill: lemusync-security](../../.claude/skills/lemusync-security/SKILL.md) · **Code:** `crates/crypto/src/lib.rs`, `crates/core` (validation), `crates/ipc` (planned)

## Purpose
LemuSync handles people's data, credentials and file systems on millions of possible machines. It must never become a way to attack those machines. This spec is the threat model and the required controls. [rules.md](../rules.md) B2 is the non-negotiable summary.

## Threat model
| Adversary | Can | Must not be able to |
|---|---|---|
| Storage provider / server operator | Read, delete, reorder, replay, withhold objects | Read saves or names; forge or alter versions undetected; make a device write outside its grants |
| Network attacker | Observe and tamper with traffic | Anything beyond what the provider can do (TLS) |
| Malicious profile contributor | Submit a profile PR | Run code; write outside grants; read non-granted files |
| Malicious website / app | Open `lemusync://` links | Trigger actions without user confirmation |
| Other local users | Use the same computer | Talk to someone else's daemon or read their state |
| Compromised device with the repo key | Write valid encrypted objects | Write outside other devices' grants or exceed size limits |
| Attacker targeting the project | Compromise a dependency or CI | Ship a malicious release unnoticed (signing, reproducibility, review) |

Out of scope: malware already running as the user (it can read anything the user can), and physical access to an unlocked device.

## No global blast radius
LemuSync runs no server, auto-updater, remote profile feed or telemetry endpoint. There's no central component whose compromise reaches all users. Releases are the only shared channel, so they're signed and reproducible ([Supply chain](#supply-chain)).

## Cryptography
Use audited RustCrypto/BLAKE3 crates only. No custom primitives ([D21](../decisions.md#d21-encryption-from-rustcryptoblake3-primitives-not-the-age-file-format-2026-10-07)).

| Purpose | Algorithm |
|---|---|
| Object encryption | XChaCha20-Poly1305 (`chacha20poly1305`), random 192-bit nonce per object |
| Passphrase → key-encryption key | Argon2id (`argon2`), default m = 64 MiB, t = 3, p = 1. Parameters stored in `repo.json` |
| Content ids, stream ids, subkeys | BLAKE3 keyed hash / `derive_key` with fixed context strings |
| Randomness | OS CSPRNG (`getrandom`) |
| Secret memory | `zeroize` on drop |

`repo.json` KDF parameters are bounded on read (`m` 19 456–1 048 576 KiB, `t` 1–10, `p` 1–4), so a tampered file can neither weaken a rewrap below OWASP's minimum nor exhaust memory. The recovery key is 32 random bytes shown as Crockford base32 in groups of 4; its key-encryption key is BLAKE3 `derive_key("lemusync v1 recovery-kek", recovery key)` (it's already full-entropy, so no Argon2id).

- **AAD** binds every ciphertext to its kind and id (e.g., `"blob" ‖ blobId`), so the server can't swap objects.
- **Blob ids** are a keyed hash of the plaintext. After decryption the id is recomputed and must match.
- **Manifests** carry file names and metadata, and they're encrypted. The provider sees only object counts, sizes and timing (accepted leak).

### Keys
- A random 256-bit **master key** per repo. Subkeys come from BLAKE3 `derive_key`: `"lemusync v1 object"`, `"lemusync v1 blob-id"`, `"lemusync v1 stream-id"`.
- `repo.json` stores the master key **wrapped** twice: under the passphrase KEK, and under a **recovery key** (random, shown once at setup for the user to print or store).
- On each device the master key is cached in the OS keystore ([Credentials](#credentials)).
- **Passphrase change** rewraps the master key and doesn't revoke devices. **Revoking a device** requires key rotation (re-encrypting the repo), planned for Phase 7. The UI says this plainly.
- Losing both the passphrase and the recovery key means losing the data, by design. The UI says this before setup completes.

### Rollback and withholding
A provider can serve an old state or hide new versions. Devices remember the highest version seen per stream (state DB) and warn if the remote goes backwards. Withholding can't be fully prevented without a trusted server, and that's accepted and documented.

## Network
- **No inbound listeners**, ever, except the OAuth loopback redirect during login ([D28](../decisions.md#d28-oauth-loopback-listener-during-login-only-2026-10-18)). Outbound connections only to configured backends and OAuth endpoints.
- TLS via `rustls` with the platform verifier. No OpenSSL. Certificate errors are never ignorable from the UI.
- `http://` endpoints are refused unless the user explicitly allows them for a LAN host. A persistent warning stays visible.

## Local IPC
Same-user only: socket permissions plus a peer credential check ([architecture §IPC](architecture.md#ipc)). Frame size limit. Every request is validated in `api`/`ipc` before the engine sees it.

## File-system confinement
- The engine reads and writes only inside **grants**: folders the user approved, which are canonicalised when stored.
- Every path from a manifest or profile is validated in `core`: relative; no `..`; not absolute; no drive letters, UNC paths, NUL bytes, `:` (alternate data streams) or Windows reserved names (`CON`, `NUL`, …); length limits. Implemented in `crates/core/src/path.rs`.
- Writes never follow symlinks (open with no-follow / capability-based directory handles). A symlink that resolves outside a grant is refused.
- Per-file and per-stream size limits (profile `max_file_mb`) stop disk-filling.
- Restores always back up first ([reliability](reliability.md#invariants)).

## Profiles, deep links, launching
- Profiles are data: schema-validated, allow-listed variables, linear-time regex, no executable fields ([emulator-profiles §Validation](emulator-profiles.md#validation)). Bundled profiles change only through reviewed PRs and signed releases. Nothing is downloaded at runtime.
- `lemusync://` deep links only **navigate**. Any action they lead to needs an explicit confirmation in the UI.
- Emulators are launched with the user-chosen executable and an argv array. Never through a shell; arguments are never parsed by a shell.

## Credentials
- Stored in the OS keystore: macOS/iOS Keychain, Windows Credential Manager, Linux Secret Service, Android Keystore-backed encrypted storage.
- Desktop backends through `keyring` 3.6.3, per OS: Linux `sync-secret-service` + `crypto-rust` + `vendored` (libdbus compiled from source, so no system D-Bus dev package and no zbus 4, whose crates are unmaintained: RUSTSEC-2026-0370/0371/0372), macOS `apple-native`, Windows `windows-native` (maintainer, 2026-10-19).
- **Linux evidence (SP2, Fedora 44 XFCE):** `org.freedesktop.secrets` was served by KWallet's `ksecretd` (`kf6-kwallet` 6.30.0), default collection `kdewallet`, unlocked at login (`--pam-login`). `keyring` 3.6.3 (`sync-secret-service`) wrote, read and deleted an item from the session and from a systemd user service without a prompt. With no Secret Service on the bus the call fails with `PlatformFailure(Dbus(... org.freedesktop.secrets was not provided by any .service files (ServiceUnknown)))`. Where `gnome-keyring` is D-Bus-activatable but has no unlocked login keyring (private bus, no PAM), activation succeeds and the write fails with `NoStorageAccess(NoResult)`.
- Headless without a keystore: a file with permissions `0600`, enabled only by explicit opt-in (`--allow-file-secrets`), with a warning.
- S3 keys (D31): the secret access key is typed at a no-echo prompt and kept in the keystore (`device-<id>-s3-secret-access-key`); the access key id, endpoint and bucket are in `config.toml`. Endpoints must be `https://`. The store never reads `AWS_*` variables, `~/.aws` files or instance metadata.
- OAuth: authorization code + PKCE. Loopback redirect on desktop, system browser sessions on mobile. Refresh tokens live in the keystore.
- Secrets never appear in logs, crash output, config files or IPC events. A redaction layer runs in the logger.

## Supply chain
- `cargo-deny` (advisories, licenses compatible with MPL-2.0, bans, crates.io as the only source) and `cargo audit` in CI. `Cargo.lock` and `pubspec.lock` are committed.
- New dependencies need a justification in the PR (P1). Prefer well-maintained, audited crates.
- Pinned toolchains (`rust-toolchain.toml`, Flutter version in `pubspec.yaml`). GitHub Actions pinned by commit SHA, with least-privilege `permissions`.
- Dependabot for cargo, pub and actions. GitHub secret scanning and push protection on.
- Releases: reproducible builds where the platform allows, checksums, Sigstore-backed build-provenance attestations, SBOM (CycloneDX), plus platform signing (Apple notarisation, Authenticode, APK signing). What ships when, given there are no store accounts yet: [release §Signing](release.md#signing).
- `unsafe` forbidden outside `bridge` (generated FFI).
- Fuzzing (`cargo-fuzz`) for every parser of untrusted input: manifest decode, profile parse, path validation, IPC frames, lease decode.

## Privacy
No telemetry, analytics or crash upload. Logs stay local. Users can export them for bug reports after reviewing them (redacted).

## Disclosure
[SECURITY.md](../../SECURITY.md): private reporting through GitHub advisories.

## Verify
- **V-SEC-1** Argon2id parameters stay fast enough on low-end Android and Pi Zero 2 (target under 2 s). Benchmark in Phase 1.
- **V-SEC-2** `rustls` platform verifier works on every target, Android and iOS included (SP1). Linux is done (2026-10-17, Fedora 44, `rustls-platform-verifier` 0.7.1, `ring` provider): a valid certificate (`www.rust-lang.org`) is accepted, and expired, self-signed and wrong-host certificates (`*.badssl.com`) are rejected with a certificate error. Test: `cargo test -p lemusync-bridge -- --include-ignored`. Android done on the API 34 emulator (2026-10-18, release build, same four bad-certificate cases rejected; [architecture §Bridge](architecture.md#bridge)). CachyOS done (2026-10-18, same test, all pass). Windows, macOS, iOS and a real Android device are still to run.
- **V-SEC-3** Keystore access from the daemon on Windows and macOS (SP2). Linux with a Secret Service is done ([Credentials](#credentials)). Windows and macOS deferred by the maintainer on 2026-10-08.
- **V-SEC-4** OpenDAL's `gdrive` puts the OAuth refresh token and client secret in the token URL, and they appear in error text when a refresh fails to send ([storage-backends §Google Drive (live)](storage-backends.md#google-drive-live), V-STORE-7). The log redaction layer must cover it (Phase 1). Store errors pass through `crates/store/src/redact.rs`; tests cover the token URL, JSON bodies and Bearer headers. Since D27 the store refreshes tokens itself with a form body (`crates/store/src/oauth.rs`), so OpenDAL no longer builds that URL.

## Open questions
- **Headless Linux secret store** (evidence from SP2; not decided). (1) Kernel keyutils via `keyring`'s `linux-native` backend: worked with no D-Bus service, but the keys are lost on reboot ([keyring docs](https://docs.rs/keyring/3.6.3/keyring/keyutils/index.html)), so a human must re-enter secrets after each boot. (2) The `0600` opt-in file above, optionally encrypted with a passphrase, which has the same re-entry problem. (3) `systemd-creds --user encrypt/decrypt` (systemd ≥ 256; worked here without root), which binds the secret to the host key or TPM and the user and survives reboots ([systemd-creds](https://www.freedesktop.org/software/systemd/man/latest/systemd-creds.html)). Recommendation: (3) where available, falling back to (2). Needs a decision entry.
