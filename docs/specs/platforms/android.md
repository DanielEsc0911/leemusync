# Android

> **Status:** Draft · **Related:** [rules](../../rules.md) (B3, P4, F1), [architecture §Process model](../architecture.md#process-model), [status-and-notifications §Android](../ui/status-and-notifications.md#android), [emulator-profiles](../emulator-profiles.md), [reliability](../reliability.md) · **Code:** `app/android/` (planned)

## Purpose
Android is the most common portable emulation platform. It also restricts file access and background work the most. This spec covers how LeemuSync works within those limits.

## Baseline
- `minSdk` 26 (Android 8.0), `targetSdk` latest stable. Features above `minSdk` are detected at runtime.
- The engine runs in the app process through the `bridge` cdylib. Kotlin calls it via UniFFI; Dart via FRB ([D4](../../decisions.md#d4-mobile-one-engine-in-the-app-process-two-thin-bindings-2026-10-07)).
- File access goes through a `SaveFs` abstraction: direct paths where allowed, Storage Access Framework (SAF) tree URIs otherwise.

## Storage access
| Location | Access | Notes |
|---|---|---|
| Public shared storage (e.g., `/storage/emulated/0/RetroArch`) | SAF tree grant, or "All files access" (`MANAGE_EXTERNAL_STORAGE`) | Play only allows All-files access for qualifying app types. Backup/sync qualification must be verified (SP3). F-Droid/GitHub builds can offer it freely |
| SD cards / USB | SAF tree grant | Persistable permissions |
| `Android/data/<other app>` | **Not accessible** to other apps on Android 11+ (and SAF blocks it on 13+) | Users must switch the emulator to a public folder. Profiles' `recommend` explains how per emulator |

SAF is slow for large trees, so only the specific save subtrees are scanned.

## Background work
| Need | Mechanism |
|---|---|
| Play session (launcher mode) | Launch the emulator via intent. Foreground service while pulling/pushing. Detect return via our activity resuming, or usage access if the user grants it (optional) |
| Active transfers | Foreground service type `dataSync`. On Android 15+ it's limited to 6 h per 24 h, which is plenty for short transfers. The limit resets when the user opens the app |
| Large user-initiated transfers | User-initiated data transfer jobs (Android 14+) |
| Periodic checks | WorkManager periodic work (minimum interval 15 min) |
| Reliability | Ask for battery-optimisation exemption with an explanation. Show OEM-specific guidance (Xiaomi autostart, Samsung "sleeping apps") through `doctor` and onboarding |

## Native integrations
Live Updates (Android 16+; Now Bar on One UI 8; Super Island on HyperOS 3 global) · ongoing progress notification (older versions) · Quick Settings tile · app shortcuts · later a widget. Details in [status-and-notifications §Android](../ui/status-and-notifications.md#android).

## Emulators to inventory (SP3)
RetroArch, Dolphin, DuckStation, PPSSPP, Azahar, melonDS, NetherSX2/ARMSX2, Eden/Citron, Cemu (Android port), Vita3K, Lemuroid. For each: save path, whether it's user-movable, launch intent and extras, and its tier.

## Distribution
Google Play (AAB) · F-Droid (reproducible build from source: Flutter + Rust NDK, Verify) · GitHub APK.

## Verify
- **V-AND-1** All-files-access eligibility on Play for a save-sync app (SP3).
- **V-AND-2** Emulator inventory above (SP3).
- **V-AND-3** Launch intents with a game path for each emulator (SP3).
- **V-AND-4** Live Update promotion rules and OEM rendering (SP4).
- **V-AND-5** F-Droid build of a Flutter + Rust app (Phase 4).
