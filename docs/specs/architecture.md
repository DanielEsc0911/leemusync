# Architecture

> **Status:** Decided · **Related:** [rules](../rules.md) (B1, B2, B3), [decisions](../decisions.md) (D1–D4, D10, D12, D18), [sync-model](sync-model.md), [security](security.md), [reliability](reliability.md) · **Code:** `crates/`, `app/` (planned)

## Purpose
How the pieces fit together: components, crate layers, process model per platform, the engine API, and the repository tree.

## Components
```
                 ┌────────────── Flutter UI (app/) — same Dart code everywhere ──────────────┐
                 └───────┬───────────────────────────────────────────────┬───────────────────┘
          desktop: IPC   │                                               │  mobile: in-process
                 ┌───────▼────────┐                              ┌───────▼────────────────────┐
                 │ leemusyncd     │  CLI ──IPC──►                │ leemusync-bridge (cdylib)  │
                 │ engine + tray  │                              │  FRB (Dart) + UniFFI       │
                 │ + notifications│                              │  (Kotlin/Swift) → 1 engine │
                 └───────┬────────┘                              └───────┬────────────────────┘
                         └─────────────► leemusync-engine ◄──────────────┘
                                          │      │       │
                               core ◄─────┘   crypto   store ──► user's storage (S3, WebDAV, SFTP…)
```

## Layers
Crates live under `crates/` with package names `leemusync-<dir>`. Dependencies only point down. `cargo xtask layers` enforces this table:

| Crate | Responsibility | May depend on (internal) |
|---|---|---|
| `core` | Domain model and pure logic: streams, versions, heads, conflict detection, path validation, profile schema. **No I/O, no async** | — |
| `crypto` | Key derivation, AEAD, keyed hashing, key wrapping. Pure | — |
| `store` | Backend interface and adapters (OpenDAL and custom); raw object I/O | — |
| `api` | Engine API: request/response/event types and the `EngineApi` trait. Stable and versioned | — |
| `engine` | Orchestration: scanning, watching, snapshots, transfers, leases, journal, state DB, scheduler. Implements `EngineApi` | core, crypto, store, api |
| `ipc` | Local-socket transport: framing, handshake, `EngineApi` client and server | api |
| `daemon` | `leemusyncd` binary: service lifecycle, tray, notifications, sleep inhibition | api, engine, ipc |
| `cli` | `leemusync` binary | api, engine, ipc |
| `bridge` | Mobile/desktop UI bindings: flutter_rust_bridge + UniFFI in one cdylib | api, engine, ipc |

Rules:
- `unsafe_code = "forbid"` in every crate except `bridge` (generated FFI) ([B2](../rules.md#b2-security-first-no-backdoors-no-exploits)).
- `core` and `crypto` are fully unit- and property-testable without mocks.
- Dev-dependencies may cross layers (test helpers). Normal dependencies may not.

## Process model
| Platform | Processes | Engine lives in |
|---|---|---|
| Windows, macOS, Linux desktop | `leemusyncd` (user-level service, owns tray and notifications) + Flutter app (on demand) + `leemusync` CLI | daemon |
| Headless (Pi, server) | `leemusyncd --headless` + CLI | daemon |
| Android | App process. Kotlin service/jobs keep it alive during work | bridge (in-process) |
| iOS | App process (also used by App Intents and background tasks) | bridge (in-process) |

- **One engine per user profile.** The engine takes an exclusive lock on its state directory. A second instance exits with a clear error. On desktop the CLI talks to the daemon. If no daemon is running, the CLI may run the engine itself for one command, holding the same lock.
- **Desktop UI ↔ daemon:** the Flutter app → FRB → `bridge` (IPC client) → `ipc` → daemon. The UI uses the same `EngineApi` as mobile, so the Dart code doesn't change between platforms.
- **Tray on the main thread:** the tray event loop needs the main thread (macOS, GTK). Async work runs on a tokio runtime on other threads.

## Engine API
Defined in `crates/api`, versioned (`api_version`), and identical in-process and over IPC.

- **Commands:** status · sync now · pause/resume · list games · history · restore version · resolve conflict · start/end play session · players CRUD · account mappings · devices · backends CRUD/test · grants CRUD · settings get/set · profiles list/validate · diagnostics.
- **Events (stream):** `StatusChanged` · `SessionChanged` · `ConflictDetected` · `LeaseWarning` · `TransferProgress` · `Error`.
- **One status model** feeds every surface: tray, notifications, Live Updates, Live Activities, UI ([status-and-notifications](ui/status-and-notifications.md)).

## IPC
- Unix domain socket in the user runtime dir (mode `0600`, dir `0700`). Windows named pipe with a DACL for the current user only.
- Peer check: the same UID/SID as the daemon (`SO_PEERCRED` / `getpeereid` / pipe client token). Mismatch → close.
- Frames: u32 big-endian length + JSON (serde), max 1 MiB. Handshake: `{api_version, client}`. Unknown versions are rejected with an upgrade hint.

## Engine internals
| Module | Job |
|---|---|
| `scan` | Resolve profiles + grants → candidate streams |
| `watch` | File watching (notify) + periodic rescans |
| `session` | Launcher/watch sessions, process detection |
| `snapshot` | Quiet-period check, consistent copy, hashing |
| `transfer` | Upload/download with retry and backoff |
| `restore` | Staged, journaled, atomic restore with backup |
| `lease` | Acquire, renew, release, stale handling |
| `heads` | Version graph, divergence → conflicts |
| `repo` | Remote layout, encryption, manifests |
| `state` | SQLite state DB + journal |
| `retention` | Remote retention and blob GC, local history pruning |
| `power` | Sleep inhibition, resume handling (via platform hooks) |
| `events` | Status model and event bus |

Async runtime: tokio. Errors: typed per crate (`thiserror`). Applications map them to user-facing messages through i18n keys.

## Repository tree
```
AGENTS.md  CLAUDE.md  README.md  LICENSE  SECURITY.md  CONTRIBUTING.md  CHANGELOG.md
Cargo.toml  rust-toolchain.toml  justfile  deny.toml  clippy.toml  .cargo/config.toml
crates/{core,crypto,store,api,engine,ipc,daemon,cli,bridge}/
xtask/                    repo checks: docs-check, layers, i18n-check
app/                      Flutter app
  lib/main.dart
  lib/app/                router, theme bootstrap, providers
  lib/design/             tokens + components (design system)
  lib/features/<name>/    screens, widgets, state for one feature
  lib/l10n/               ARB files (+ generated/)
  lib/bridge/             generated FRB bindings
  android/ ios/ macos/ windows/ linux/   native code: services, notifications, Live Activities, App Intents
profiles/                 emulator profiles (TOML) + fixtures/
locales/{en,es}/          Fluent files for Rust (tray, notifications, CLI)
packaging/                systemd units, launchd plists, Windows tasks, Flatpak/installer manifests
docs/                     this documentation
.claude/skills/           project skills
.github/workflows/        CI
```
