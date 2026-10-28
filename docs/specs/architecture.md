# Architecture

> **Status:** Decided · **Related:** [rules](../rules.md) (B1, B2, B3), [decisions](../decisions.md) (D1–D4, D10, D12, D18), [sync-model](sync-model.md), [security](security.md), [reliability](reliability.md) · **Code:** `crates/`, `app/`, `crates/bridge`, `app/hook/build.dart`

## Purpose
How the pieces fit together: components, crate layers, process model per platform, the engine API, and the repository tree.

## Components
```
                 ┌────────────── Flutter UI (app/) — same Dart code everywhere ──────────────┐
                 └───────┬───────────────────────────────────────────────┬───────────────────┘
          desktop: IPC   │                                               │  mobile: in-process
                 ┌───────▼────────┐                              ┌───────▼────────────────────┐
                 │ lemusyncd     │  CLI ──IPC──►                │ lemusync-bridge (cdylib)  │
                 │ engine + tray  │                              │  FRB (Dart) + UniFFI       │
                 │ + notifications│                              │  (Kotlin/Swift) → 1 engine │
                 └───────┬────────┘                              └───────┬────────────────────┘
                         └─────────────► lemusync-engine ◄──────────────┘
                                          │      │       │
                               core ◄─────┘   crypto   store ──► user's storage (S3, WebDAV, SFTP…)
```

## Layers
Crates live under `crates/` with package names `lemusync-<dir>`. Dependencies only point down. `cargo xtask layers` enforces this table:

| Crate | Responsibility | May depend on (internal) |
|---|---|---|
| `core` | Domain model and pure logic: streams, versions, heads, conflict detection, path validation, profile schema. **No I/O, no async** | — |
| `crypto` | Key derivation, AEAD, keyed hashing, key wrapping. Pure | — |
| `store` | Backend interface and adapters (OpenDAL and custom); raw object I/O | — |
| `api` | Engine API: request/response/event types and the `EngineApi` trait. Stable and versioned | — |
| `engine` | Orchestration: scanning, watching, snapshots, transfers, leases, journal, state DB, scheduler. Implements `EngineApi` | core, crypto, store, api |
| `ipc` | Local-socket transport: framing, handshake, `EngineApi` client and server | api |
| `daemon` | `lemusyncd` binary: service lifecycle, tray, notifications, sleep inhibition | api, engine, ipc |
| `cli` | `lemusync` binary | api, engine, ipc |
| `bridge` | Mobile/desktop UI bindings: flutter_rust_bridge + UniFFI in one cdylib | api, engine, ipc |

Rules:
- `unsafe_code = "forbid"` in every crate except `bridge` (generated FFI) ([B2](../rules.md#b2-security-first-no-backdoors-no-exploits)).
- `core` and `crypto` are fully unit- and property-testable without mocks.
- Dev-dependencies may cross layers (test helpers). Normal dependencies may not.

## Process model
| Platform | Processes | Engine lives in |
|---|---|---|
| Windows, macOS, Linux desktop | `lemusyncd` (user-level service, owns tray and notifications) + Flutter app (on demand) + `lemusync` CLI | daemon |
| Headless (Pi, server) | `lemusyncd --headless` + CLI | daemon |
| Android | App process. Kotlin service/jobs keep it alive during work | bridge (in-process) |
| iOS | App process (also used by App Intents and background tasks) | bridge (in-process) |

- **One engine per user profile.** The engine takes an exclusive lock on its state directory. A second instance exits with a clear error. On desktop the CLI talks to the daemon. If no daemon is running, the CLI may run the engine itself for one command, holding the same lock. While the daemon runs, it answers `status`, `pause`, `resume` and event subscriptions at once from its own status model and runs other commands one at a time ([D35](../decisions.md#d35-the-cli-is-the-daemons-client-while-it-runs-2026-10-28)).
- **Desktop UI ↔ daemon:** the Flutter app → FRB → `bridge` (IPC client) → `ipc` → daemon. The UI uses the same `EngineApi` as mobile, so the Dart code doesn't change between platforms.
- **Tray on the main thread:** the tray event loop needs the main thread (macOS, GTK). Async work runs on a tokio runtime on other threads.

## Engine API
Defined in `crates/api`, versioned (`api_version`), and identical in-process and over IPC.

- **Commands:** status · sync now · pause/resume · list games · history · restore version · resolve conflict · start/end play session · players CRUD · account mappings · devices · backends CRUD/test · grants CRUD · settings get/set · profiles list/validate · diagnostics.
- **Events (stream):** `StatusChanged` · `SessionChanged` · `ConflictDetected` · `LeaseWarning` · `TransferProgress` · `ConnectionEvent` · `Error`.
- **`TransferProgress`** (`crates/api/src/lib.rs`): player, emulator, game, phase (`checking`, `uploading`, `downloading`) and files and bytes done out of the phase's total. The engine sends it to the sink set with `Engine::set_progress` during `sync`, `restore` and `accounts recover`. Sync and restore reports carry what each stream moved (`files`, `bytes`).
- **`ConnectionEvent`** (`crates/api/src/lib.rs`): `retrying` (operation, attempt, max attempts, delay in ms) while the store retries a transient error, and `recovered` once, on a success when no other request is still retrying. The engine sends it to the sink set with `Engine::set_connection_sink` for every backend call ([reliability §Network](reliability.md#network)).
- **One status model** feeds every surface: tray, notifications, Live Updates, Live Activities, UI ([status-and-notifications](ui/status-and-notifications.md)).

## IPC
- Unix domain socket in the user runtime dir (mode `0600`, dir `0700`). Windows named pipe with a DACL for the current user only.
- Peer check: the same UID/SID as the daemon (`SO_PEERCRED` / `getpeereid` / pipe client token). Mismatch → close.
- Frames: u32 big-endian length + JSON (serde), max 1 MiB. Handshake: `{api_version, client}`. Unknown versions are rejected with an upgrade hint.

## Bridge
`crates/bridge` builds one library (`cdylib` + `staticlib`) with both bindings ([D4](../decisions.md#d4-mobile-one-engine-in-the-app-process-two-thin-bindings-2026-10-07)):

| Part | How |
|---|---|
| Dart | [flutter_rust_bridge](https://cjycode.com/flutter_rust_bridge/) 2.13.0 (Rust `=2.13.0`, Dart `2.13.0`, codegen `2.13.0`, all three must match). Public items in `crates/bridge/src/api/` get bindings in `app/lib/bridge/`. Config: `app/flutter_rust_bridge.yaml`. Regenerate with `flutter_rust_bridge_codegen generate` in `app/`, then `just fmt` |
| Build | FRB's **native-assets** backend: `app/hook/build.dart` (Dart build hook, `flutter_rust_bridge_hooks` → `native_toolchain_rust` 1.0.4) runs Cargo for the target and bundles the library. No per-platform glue (cargokit) in `android/`, `ios/`, `linux/`… The hook requires `crates/bridge/rust-toolchain.toml` with an exact channel and every target it may build. Keep its channel equal to the root one |
| Kotlin, Swift | [UniFFI](https://mozilla.github.io/uniffi-rs/) 0.32.2, proc-macro mode (`setup_scaffolding!("lemusync")`, `#[uniffi::export]`), surface in `crates/bridge/src/native.rs`. Bindings: `cargo run -p lemusync-bridge --features bindgen --bin uniffi-bindgen -- generate --library <lib> --language kotlin --language swift --out-dir <dir>` |
| `unsafe` | The crate sets `unsafe_code = "deny"` (not `forbid`) in its own `[lints]` and allows it only on `mod frb_generated` and `mod native`. Lints are off for the generated module |
| TLS | `rustls` 0.23 with the `ring` provider (no `aws-lc-rs`, no CMake for cross builds) and `rustls-platform-verifier` 0.7.1 ([security §Network](security.md#network)) |
| Android glue | Kotlin `Bridge` (`app/android/.../Bridge.kt`) calls `System.loadLibrary("lemusync_bridge")`, the same `.so` Dart opens, then `Bridge.initTls(context)` (JNI entry in `crates/bridge/src/android.rs`, `jni` 0.22) before any TLS. Gradle adds `net.java.dev.jna:jna:5.19.1@aar` (UniFFI's Kotlin bindings use JNA) and `org.rustls:rustls-platform-verifier:0.2.0`, which is published only to a Maven repo on GitHub; that repo is restricted to the `org.rustls` group. Its version must match `rustls-platform-verifier-android` in `Cargo.lock`. Generated Kotlin bindings: `app/android/app/src/main/kotlin/uniffi/lemusync/` |
| R8 (release) | `app/android/app/proguard-rules.pro` keeps `com.sun.jna.**`, `uniffi.**` and `org.rustls.platformverifier.**`. Without it the release app crashes at the first UniFFI call (`UnsatisfiedLinkError: Can't obtain peer field ID for class com.sun.jna.Pointer`) and the verifier fails (`failed to call native verifier`). Debug and profile builds don't shrink, so they hide this |

**SP1 results, Linux (2026-10-17).** Fedora 44 PC, XFCE on X11, AMD Radeon 860M, 1920×1200 at 60 Hz, Flutter 3.47.6 (Impeller, OpenGL ES), Rust 1.99.0. Probe: `app/tool/sp1_probe.dart` (spike only; `flutter run --profile -d linux -t tool/sp1_probe.dart`).

| Measure | Result | Budget |
|---|---|---|
| Dart → Rust sync call | Works. Two calls returned `#1`, `#2` from the shared counter | — |
| Cold start to first rasterized frame (profile, `--trace-startup`, 3 runs, includes `RustLib.init`) | 82–88 ms | ≤ 1 s ([design-system](ui/design-system.md#performance)) |
| Frames during a 10 s rotation animation (profile) | 619 frames; build p50 0.24 ms / p99 0.38 ms; raster p50 0.43 ms / p99 0.71 ms; 4 frames over 8.3 ms total span | ≤ 8 ms per frame |
| Release bundle (`flutter build linux --release`) | 23 MB: Flutter engine 17 MB, app AOT 3.2 MB, `liblemusync_bridge.so` 2.0 MB | — |
| Idle RSS, release app after 6 s | 143 MiB (profile probe: 169 MiB max) | none for the UI (the daemon's is in [reliability](reliability.md)) |
| Rust dependency graph | 112 crates for Linux (FRB, UniFFI, rustls, platform verifier) | — |

The 120 Hz case was not measurable (60 Hz display). It was measured on CachyOS (below).

**SP1 results, CachyOS (2026-10-18).** CachyOS (kernel 7.2.9-2-cachyos), KDE Plasma 6.8.0 on Wayland, Intel Iris Xe (Alder Lake-P) + NVIDIA GeForce RTX 3050 Mobile, 1920×1080 at 144 Hz, Flutter 3.47.6, Rust 1.99.0. Same probe and commands as on Fedora.

| Measure | Result | Budget |
|---|---|---|
| Dart → Rust sync call | Works (`#1`, `#2`). HTTPS through the platform verifier returns a response | — |
| Cold start to first rasterized frame (3 runs) | 127–135 ms | ≤ 1 s |
| Frames during the 10 s animation (profile) | 611 frames (about 61 fps) while `refresh_hz` reports 144. Build p50 0.19 ms / p99 0.32 ms; raster p50 0.33 ms / p99 0.62 ms; 3 frames over 8.3 ms. Same count with `GDK_BACKEND=wayland` (618) and `GDK_BACKEND=x11` (617; raster p99 6.0 ms, 19 over 8.3 ms) | 120 fps on high-refresh displays ([design-system](ui/design-system.md#performance)) |
| Release bundle | 23 MB: engine 17 MB, app AOT 3.2 MB, bridge 2.0 MB (same as Fedora) | — |
| Idle RSS, release app after 6 s | 186 MiB (profile probe: 239 MiB) | none for the UI |

Frame work fits well inside the 120 Hz budget, but the Linux app is paced at about 60 fps on a 144 Hz display. The cause is not confirmed (V-DESK-5 in [desktop](platforms/desktop.md#verify)).

**SP1 results, Android emulator (2026-10-18).** Pixel 8 AVD, Android 14 (API 34, x86_64, Google Play image), emulator 36.5.10 on the Fedora PC, NDK 28.2.13676358, JDK 21 (Android Studio's JBR). Emulator frame and start times use a software GPU path, so they say nothing about real devices.

| Check | Result |
|---|---|
| One instance, two bindings | Dart `core_version` → `#1`, Kotlin `enginePing()` through UniFFI → `pong #2`, Dart again → `#3`. Profile and release builds. This confirms D4 on Android |
| TLS with Android's verifier (release build) | `www.rust-lang.org` accepted. Expired → `invalid peer certificate: Expired`; self-signed and untrusted root → `UnknownIssuer`; wrong host → `certificate not valid for name` |
| Real app, `flutter build apk --release --split-per-abi` | Builds the bridge for all three ABIs. arm64-v8a 17.4 MB (bridge 1.9 MB), armeabi-v7a 13.8 MB (1.2 MB), x86_64 19.0 MB (2.2 MB). Launches with no crash |

Windows, macOS, iOS and a real Android device (Galaxy A25) are still to run ([Phase 0 plan](../progress/plans/phase-0-foundations.md#sp1-flutter--rust-bridge-on-every-target)). Swift bindings generate on Linux (`enginePing()`); the in-app check needs the iOS run.

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
  hook/build.dart         builds crates/bridge for the target (Dart build hook)
  android/ ios/ macos/ windows/ linux/   native code: services, notifications, Live Activities, App Intents
profiles/                 emulator profiles (TOML) + fixtures/
locales/{en,es}/          Fluent files for Rust (tray, notifications, CLI)
packaging/                systemd units, launchd plists, Windows tasks, Flatpak/installer manifests
docs/                     this documentation
.claude/skills/           project skills
.github/workflows/        CI
```
