# Roadmap

> **Status:** Decided · **Related:** [product](../specs/product.md) (scenarios SC1–SC7), [progress](README.md), [architecture](../specs/architecture.md), [rules](../rules.md) · **Code:** —

Each phase ends with a demo of its exit criteria on real devices. Scenario IDs (SC1–SC7) are defined in [product](../specs/product.md#core-scenarios). Spike IDs (SP1–SP7) are defined in the [Phase 0 plan](plans/phase-0-foundations.md).

## Phase 0: Foundations and spikes
- **Goal:** a CI-checked skeleton, plus answers to every platform risk before any feature code is written.
- **Deliverables:** Rust workspace + xtask checks; Flutter app shell (en/es); CI; release pipeline producing `.apk` `.msi` `.exe` `.dmg` `.deb` `.rpm` `.AppImage` `.tar.gz`; spikes SP1–SP7 recorded in the specs; Phase 1 plan.
- **Exit:** `just check` green on Linux, Windows and macOS CI. Every Verify item targeted by a spike is resolved or re-scoped.

## Phase 1: Sync engine + CLI (desktop)
- **Goal:** the whole sync model working headless.
- **Deliverables:** `core` (model, heads, reconciliation, path validation, profile schema), `crypto`, `store` (fs, S3, WebDAV, SFTP + conformance), `engine` (scan, snapshot, transfer, restore, leases, journal, local history, retention), the five initial profiles, `cli` with `--json`, the simulation test harness.
- **Exit (milestone M1, "BotW handoff"):** two players play BotW on Cemu on two PCs at the same time (SC1); one player hands off between PCs (SC2); a forced double-open becomes a conflict that's resolved with nothing lost (SC3); `kill -9` and network cuts at every journal step leave no corruption (SC5). Daemon budgets measured.

## Phase 2: Desktop daemon
- **Goal:** always-on desktop sync with native presence.
- **Deliverables:** `leemusyncd` + `ipc`; service install on all three OSes; tray/menu bar; actionable notifications (Fluent en/es); launcher and watch modes with process detection; sleep inhibition and resume handling; `doctor`.
- **Exit (M2):** 72-hour soak on Windows, macOS and Linux with sleep/resume cycles. No missed or late sync beyond the budgets. Tray states verified.

## Phase 3: Flutter app (desktop)
- **Goal:** the beautiful, parity-locked UI.
- **Deliverables:** design exploration (brand, fonts, icon set); design system tokens + components; navigation shell; every flow in [navigation](../specs/ui/navigation.md); FRB bridge over IPC; goldens; performance tests; accessibility pass.
- **Exit (M3):** a new user goes from install to first synced save in under 3 minutes unaided. Goldens pass for every size class, theme and locale. Frame budgets met.

## Phase 4: Android
- **Deliverables:** engine in-process (FRB + UniFFI); `SaveFs` with SAF; launcher mode via intents; foreground service and WorkManager; Live Updates (Now Bar, Super Island) and ongoing notifications; QS tile; app shortcuts; Android profiles; Play + F-Droid + APK builds.
- **Exit (M4):** SC1–SC3 between an Android device and a PC with RetroArch and one standalone emulator. Live Update verified on Pixel, Samsung and Xiaomi.

## Phase 5: iOS
- **Deliverables:** Files bookmarks; Live Activity + Dynamic Island; App Intents + guided Shortcuts automation; background tasks; iOS profiles; TestFlight.
- **Exit (M5):** SC2 between an iPhone and a PC with at least one emulator that exposes saves in Files.

## Phase 6: Breadth
More backends (Google Drive, OneDrive, Dropbox, MEGA); more profiles (RPCS3, Yuzu-family forks, Azahar, melonDS, PPSSPP, mGBA, Ryujinx-family index reader, Vita3K, xemu, Flycast…); opt-in save states; headless packages and Linux handhelds; compat groups; storage how-to guides.

## Phase 7: Hardening and 1.0
External security audit (look for funding, e.g. open-source security grants); key rotation and device revocation; extended fuzzing; reproducible builds; signing on every channel; store submissions; docs site.

## Later ideas (not committed)
UnifiedPush / optional push relay for instant lock warnings · QR device pairing · widgets and Control Center controls · chunked dedupe for large save states · Steam Deck Decky plugin · opt-in, sandboxed per-game progress readers.
