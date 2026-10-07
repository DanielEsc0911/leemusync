# iOS and iPadOS

> **Status:** Draft · **Related:** [rules](../../rules.md) (B3, P4), [architecture §Process model](../architecture.md#process-model), [status-and-notifications §iOS](../ui/status-and-notifications.md#ios), [emulator-profiles](../emulator-profiles.md), [reliability](../reliability.md) · **Code:** `app/ios/` (planned)

## Purpose
Bring LeemuSync to iPhone and iPad within Apple's sandbox. Be honest about what isn't possible, and use every native feature that is.

## Baseline
- Minimum iOS 17. Newer APIs, such as `BGContinuedProcessingTask` on iOS 26, are used when available.
- The engine runs in the app process via the `bridge` cdylib (static library on iOS). Swift calls it via UniFFI; Dart via FRB ([D4](../../decisions.md#d4-mobile-one-engine-in-the-app-process-two-thin-bindings-2026-10-07)).

## File access
- An app can't read another app's sandbox. LeemuSync reaches saves only when the emulator exposes its folder in the Files app ("On My iPhone/iPad › Emulator"). The user picks that folder once, and LeemuSync stores a security-scoped bookmark.
- Coverage depends on each emulator (SP5): RetroArch, PPSSPP, Delta, Gamma, Provenance, Folium, DolphiniOS, MeloNX… For each: does it expose saves in Files, where, and is the format compatible with desktop?
- Emulators that keep saves only inside their own database or sandbox can't be synced. The UI says so instead of failing silently.

## Background work
| Trigger | Mechanism |
|---|---|
| App opened | Full sync |
| Opportunistic | `BGAppRefreshTask` (short), `BGProcessingTask` (longer, when the system allows) |
| User-started sync that should finish in the background | `BGContinuedProcessingTask` (iOS 26+), shows system progress UI |
| Emulator closed | Shortcuts personal automation "When ⟨emulator⟩ is closed → Sync Now" (App Intent). Set up through a guided onboarding step |
| Siri, Spotlight, widgets | App Intents |

No continuous background process is possible. Lock warnings arrive when one of the triggers above runs.

## Native integrations
Live Activity + Dynamic Island + Lock Screen/StandBy · App Intents (Sync Now, Play Game, Pull Saves) · local notifications · later widgets and a Control Center control. Details in [status-and-notifications §iOS](../ui/status-and-notifications.md#ios).

## Distribution
Now: no distribution. Development and testing run on the maintainer's iPhone with free Apple ID provisioning: apps expire after 7 days, and some capabilities may be unavailable (Verify SP5) ([D23](../../decisions.md#d23-distribution-before-store-accounts-2026-10-07)). Later: App Store + TestFlight (Apple Developer membership); alternative EU marketplaces.

## Verify
- **V-IOS-1** Which emulators expose saves in Files, and their layouts (SP5).
- **V-IOS-2** Security-scoped bookmarks to other apps' Files folders survive app restarts and updates (SP5).
- **V-IOS-3** An App Intent triggered by a Shortcuts "app closed" automation can run the Rust engine to completion (SP5).
- **V-IOS-4** Live Activity updates from `BGContinuedProcessingTask` and the duration limits (SP5).
- **V-IOS-5** Which capabilities (Live Activities, App Groups, App Intents, background tasks) work with free Apple ID provisioning (SP5).
