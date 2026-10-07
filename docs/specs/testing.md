# Testing

> **Status:** Draft · **Related:** [rules](../rules.md) (W2, F1, F2, F3, B3), [reliability](reliability.md#invariants), [storage-backends](storage-backends.md#conformance-suite), [design-system](ui/design-system.md#performance), [release](release.md) · **Code:** — (planned per crate/app)

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
| Manual release pass | Checklist in [release](release.md) | Real devices: Pixel, Samsung (One UI 8+), Xiaomi (HyperOS 3), iPhone with Dynamic Island, Pi |

## Rules
- TDD: write the failing test first (W2).
- Every bug fix adds a test that fails without the fix.
- Golden diffs need human review. They're never auto-accepted.
- Tests never touch real user folders. They use temp dirs, fixtures and fake backends.
- Flaky tests are bugs: fix them or quarantine them with an issue the same day.
