# Security

> **Status:** Draft · **Related:** [rules](../rules.md) (B2 is the rule this spec implements; also P2, P7, W3), [architecture](architecture.md#ipc), [sync-model](sync-model.md), [storage-backends](storage-backends.md), [emulator-profiles](emulator-profiles.md#validation), [release](release.md), [skill: leemusync-security](../../.claude/skills/leemusync-security/SKILL.md) · **Code:** `crates/crypto`, `crates/core` (validation), `crates/ipc` (planned)

## Purpose
LeemuSync handles people's data, credentials and file systems on millions of possible machines. It must never become a way to attack those machines. This spec is the threat model and the required controls. [rules.md](../rules.md) B2 is the non-negotiable summary.

## Threat model
| Adversary | Can | Must not be able to |
|---|---|---|
| Storage provider / server operator | Read, delete, reorder, replay, withhold objects | Read saves or names; forge or alter versions undetected; make a device write outside its grants |
| Network attacker | Observe and tamper with traffic | Anything beyond what the provider can do (TLS) |
| Malicious profile contributor | Submit a profile PR | Run code; write outside grants; read non-granted files |
| Malicious website / app | Open `leemusync://` links | Trigger actions without user confirmation |
| Other local users | Use the same computer | Talk to someone else's daemon or read their state |
| Compromised device with the repo key | Write valid encrypted objects | Write outside other devices' grants or exceed size limits |
| Attacker targeting the project | Compromise a dependency or CI | Ship a malicious release unnoticed (signing, reproducibility, review) |

Out of scope: malware already running as the user (it can read anything the user can), and physical access to an unlocked device.

## No global blast radius
LeemuSync runs no server, auto-updater, remote profile feed or telemetry endpoint. There's no central component whose compromise reaches all users. Releases are the only shared channel, so they're signed and reproducible ([Supply chain](#supply-chain)).

## Cryptography
Use audited RustCrypto/BLAKE3 crates only. No custom primitives ([D21](../decisions.md#d21-encryption-from-rustcryptoblake3-primitives-not-the-age-file-format-2026-10-07)).

| Purpose | Algorithm |
|---|---|
| Object encryption | XChaCha20-Poly1305 (`chacha20poly1305`), random 192-bit nonce per object |
| Passphrase → key-encryption key | Argon2id (`argon2`), default m = 64 MiB, t = 3, p = 1. Parameters stored in `repo.json` |
| Content ids, stream ids, subkeys | BLAKE3 keyed hash / `derive_key` with fixed context strings |
| Randomness | OS CSPRNG (`getrandom`) |
| Secret memory | `zeroize` on drop |

- **AAD** binds every ciphertext to its kind and id (e.g., `"blob" ‖ blobId`), so the server can't swap objects.
- **Blob ids** are a keyed hash of the plaintext. After decryption the id is recomputed and must match.
- **Manifests** carry file names and metadata, and they're encrypted. The provider sees only object counts, sizes and timing (accepted leak).

### Keys
- A random 256-bit **master key** per repo. Subkeys come from BLAKE3 `derive_key`: `"leemusync v1 object"`, `"leemusync v1 blob-id"`, `"leemusync v1 stream-id"`.
- `repo.json` stores the master key **wrapped** twice: under the passphrase KEK, and under a **recovery key** (random, shown once at setup for the user to print or store).
- On each device the master key is cached in the OS keystore ([Credentials](#credentials)).
- **Passphrase change** rewraps the master key and doesn't revoke devices. **Revoking a device** requires key rotation (re-encrypting the repo), planned for Phase 7. The UI says this plainly.
- Losing both the passphrase and the recovery key means losing the data, by design. The UI says this before setup completes.

### Rollback and withholding
A provider can serve an old state or hide new versions. Devices remember the highest version seen per stream (state DB) and warn if the remote goes backwards. Withholding can't be fully prevented without a trusted server, and that's accepted and documented.

## Network
- **No inbound listeners**, ever. Outbound connections only to configured backends and OAuth endpoints.
- TLS via `rustls` with the platform verifier. No OpenSSL. Certificate errors are never ignorable from the UI.
- `http://` endpoints are refused unless the user explicitly allows them for a LAN host. A persistent warning stays visible.

## Local IPC
Same-user only: socket permissions plus a peer credential check ([architecture §IPC](architecture.md#ipc)). Frame size limit. Every request is validated in `api`/`ipc` before the engine sees it.

## File-system confinement
- The engine reads and writes only inside **grants**: folders the user approved, which are canonicalised when stored.
- Every path from a manifest or profile is validated in `core`: relative; no `..`; not absolute; no drive letters, UNC paths, NUL bytes, `:` (alternate data streams) or Windows reserved names (`CON`, `NUL`, …); length limits.
- Writes never follow symlinks (open with no-follow / capability-based directory handles). A symlink that resolves outside a grant is refused.
- Per-file and per-stream size limits (profile `max_file_mb`) stop disk-filling.
- Restores always back up first ([reliability](reliability.md#invariants)).

## Profiles, deep links, launching
- Profiles are data: schema-validated, allow-listed variables, linear-time regex, no executable fields ([emulator-profiles §Validation](emulator-profiles.md#validation)). Bundled profiles change only through reviewed PRs and signed releases. Nothing is downloaded at runtime.
- `leemusync://` deep links only **navigate**. Any action they lead to needs an explicit confirmation in the UI.
- Emulators are launched with the user-chosen executable and an argv array. Never through a shell; arguments are never parsed by a shell.

## Credentials
- Stored in the OS keystore: macOS/iOS Keychain, Windows Credential Manager, Linux Secret Service, Android Keystore-backed encrypted storage.
- Headless without a keystore: a file with permissions `0600`, enabled only by explicit opt-in (`--allow-file-secrets`), with a warning.
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
- **V-SEC-2** `rustls` platform verifier works on every target, Android and iOS included (SP1).
- **V-SEC-3** Keystore access from the daemon on each desktop OS, including headless Linux without a Secret Service (SP2).
