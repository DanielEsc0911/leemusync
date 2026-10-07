# Testing

> **Status:** Draft · **Related:** [rules](../rules.md) (W2, F1, F2, F3, B3), [reliability](reliability.md#invariants), [storage-backends](storage-backends.md#conformance-suite), [design-system](ui/design-system.md#performance), [release](release.md), [platforms](platforms/desktop.md) · **Code:** — (planned per crate/app)

## Purpose
Prove the rules automatically: no lost saves, identical UI, smooth frames, same flows on every platform.

## Layers
| Layer | Tool | Covers |
|---|---|---|
| Unit | `cargo test`, `flutter test` | Pure logic in `core`, `crypto`, Dart state |
| Property | `proptest` | Version graph and heads, reconciliation table, path validation, template expansion |
| Simulation | Deterministic multi-device harness in `crates/engine/tests/sim/` | N devices + fake backend with injected faults: crash at every journal step, network errors, eventual-consistency delays, clock skew, concurrent leases. Checks invariants I1–I6 after every step |
| Backend conformance | `crates/store/tests/conformance.rs` | Every adapter. Docker services (MinIO, WebDAV, SFTP) in CI |
| Profile fixtures | `profiles/fixtures/` | Each profile's scan finds exactly the expected streams |
| Fuzz | `cargo-fuzz` (nightly CI, timeboxed) | Manifest decode, profile parse, path validation, IPC frames, lease decode |
| Widget | `flutter test` | Components and screens with fake engine |
| Golden | `flutter test --update-goldens` (reviewed) | Each screen × size class (compact/medium/expanded) × light/dark × en/es. Rendered on Linux CI with bundled fonts for determinism |
| Integration | `integration_test` on Android emulator, iOS simulator, desktop | Every flow in [navigation](ui/navigation.md) per size class |
| Performance | `flutter drive --profile` + timeline summary; Rust `criterion` | Frame budgets ([design-system](ui/design-system.md#performance)), daemon budgets ([reliability](reliability.md#budgets)) |
| Native | Kotlin unit/instrumented, XCTest | Services, notifications, Live Activities, App Intents |
| Manual release pass | Checklist in [release](release.md) | Real devices from the [Device lab](#device-lab) |

## Device lab
What the project can test on today (2026-10-07). Update this table when devices change.

| Device | Availability | Covers |
|---|---|---|
| Fedora 44 PC, XFCE on X11 (the maintainer's main machine) | Always | Linux x86_64, XFCE tray (StatusNotifier), X11 |
| PC dual-boot: Windows | Always | Windows x64: tray, toasts, MSI/EXE installers |
| PC dual-boot: CachyOS (Arch-based; record the desktop environment) | Always | Linux x86_64 on Arch, AppImage/tar.gz, a second desktop environment |
| Samsung Galaxy A25 | Always | Android, One UI (record the version; Now Bar availability is Verify SP4) |
| MacBook Air M4 | Occasional | macOS Apple Silicon, `.dmg`, building and running iOS from Xcode |
| iPhone 15 Pro | Occasional | iOS 17+, Dynamic Island, Live Activities (free Apple ID provisioning) |
| Redmi Note 15 Pro (a friend's) | Occasional | Xiaomi HyperOS (record version and region; Super Island is Verify SP4) |
| Android emulator, API 36 Pixel image | Local/CI | Stock Android 16 Live Updates |
| GitHub ARM runners (`ubuntu-22.04-arm`, `windows-11-arm`) | CI | arm64 builds and tests. There's no Raspberry Pi yet, so test on a real Pi before any headless release |
| GNOME (VM or live USB) | On demand | GNOME tray behaviour (no AppIndicator by default) |

## Rules
- TDD: write the failing test first (W2).
- Every bug fix adds a test that fails without the fix.
- Golden diffs need human review. They're never auto-accepted.
- Tests never touch real user folders. They use temp dirs, fixtures and fake backends.
- Flaky tests are bugs: fix them or quarantine them with an issue the same day.
