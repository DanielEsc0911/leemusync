# Reliability

> **Status:** Draft · **Related:** [rules](../rules.md) (B3), [sync-model](sync-model.md), [security](security.md), [testing](testing.md), [platforms](platforms/desktop.md) · **Code:** `crates/engine` (planned)

## Purpose
A save must never be lost or corrupted, and sync must keep running around the clock wherever the OS allows.

## Invariants
Each invariant has tests (unit, simulation with injected faults) that must pass before any release.

| ID | Invariant |
|---|---|
| I1 | No local save is overwritten unless its previous content is in local history first |
| I2 | The remote is append-only. Only leases and `repo.json` are overwritten, and only retention GC deletes |
| I3 | Never read for a snapshot, or write a restore, while the emulator runs (when detectable) or before the quiet period ends |
| I4 | Every local write is atomic: temp file in the same folder → write → fsync → rename → fsync folder |
| I5 | Every multi-step operation is journaled and resumes or rolls back after a crash |
| I6 | Nothing is committed or restored until it's verified (AEAD authentication + content hash) |

## Restore procedure
1. Download all blobs to staging (`<data>/staging/<op>/`) and verify them (I6).
2. Re-check that the emulator is closed and the target files are quiet (I3).
3. Copy the current local files to local history (I1).
4. Journal "swap started", listing the files.
5. Swap each file atomically (I4).
6. Journal "done" and set `base` = restored version.

A crash between 4 and 6 → on startup the journal restores every file from the step-3 backup, then retries. A stream is never left half old, half new.

## Upload procedure
1. Take a snapshot (quiet, consistent read, [sync-model §Snapshot rules](sync-model.md#snapshot-rules)) into local history.
2. Upload missing blobs (create-only, idempotent: the same content gives the same id).
3. Write the manifest (create-only) — this is the commit point.
4. Update `base`. A crash anywhere → retry. Orphaned blobs are collected by GC after the grace period.

## Local history
`<data>/history/<streamId>/<versionId or timestamp>/`. Keeps the last 10 snapshots per stream plus every pre-restore backup from the last 30 days (both configurable). Browsable and restorable from the UI and CLI, even offline.

## Always on (24/7)
| Platform | Mechanism |
|---|---|
| Linux | systemd **user** unit, `Restart=on-failure`, `WantedBy=default.target` (not `graphical-session.target`, which XFCE never reaches). Headless: `loginctl enable-linger`. Evidence: [desktop §Linux results](platforms/desktop.md#linux-results-sp2-xfce) |
| macOS | LaunchAgent (`KeepAlive`), registered from the app bundle via `SMAppService` |
| Windows | Task Scheduler task at logon with restart-on-failure. A user-session process is needed for the tray and notifications |
| Android | Launcher sessions + foreground service during transfers + WorkManager periodic sync ([platforms/android](platforms/android.md#background-work)) |
| iOS | No continuous background. App open, background tasks, Shortcuts automations ([platforms/ios](platforms/ios.md#background-work)) |

- **Watchdog:** the daemon exposes a health status. The service manager restarts it on crash. A crash loop (5 in 10 min) → back off and show a persistent error.
- **Watchers miss events:** full rescan every 15 min (desktop), on resume, on network regained, and at emulator exit.

## Sleep and resume
- While a transfer or restore is in flight, the daemon **inhibits idle sleep**: Windows `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, macOS `IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleSystemSleep)`, Linux logind `Inhibit("sleep:idle", …, "block")` over D-Bus. The inhibition is released as soon as the work ends, with a maximum of 10 min.
- **Linux logind (SP2, Fedora 44 XFCE, systemd 259):** a `block` lock on `sleep:idle` was taken without a prompt both from a terminal and from a systemd user service, showed in `systemd-inhibit --list` (`LeemuSync SP2 … sleep:idle … block`) and was gone as soon as the fd closed. `delay` mode is rejected for `idle` (`InvalidArgs: Delay inhibitors only supported for shutdown and sleep`). A `delay` lock on `sleep` alone works but only buys `InhibitDelayMaxUSec` (5 s by default). So: `block` on `sleep:idle` during transfers, plus a `delay` lock on `sleep` held while idle, to journal and release leases on `PrepareForSleep(true)`. Polkit defaults (`org.freedesktop.login1.policy`): `inhibit-block-sleep` is `yes` for active and inactive sessions but `auth_admin_keep` for `allow_any` (no session, e.g. lingering without a login); `inhibit-delay-sleep` and `inhibit-block-idle` are `yes` everywhere.
- **On resume:** wait for the network, refresh leases (they may have expired), rescan, sync.
- **On shutdown or logout:** finish the current atomic step, journal the rest, and release leases if the network allows.

## Network
Offline → queue locally and keep working. Transient errors → exponential backoff with jitter (1 s → 5 min). Auth errors → stop that backend and notify. Uploads are idempotent (content-addressed).

## Budgets
Targets for the daemon on desktop. Measured in Phase 1/2 benchmarks and enforced in CI on regression.

| Metric | Target |
|---|---|
| Idle CPU | ≈ 0% (event-driven, no busy polling) |
| Idle memory (RSS) | ≤ 30 MB |
| Cold start to ready | ≤ 300 ms |
| Change detected → upload started | quiet period + ≤ 2 s |
| Rescan of 10 000 files (warm) | ≤ 1 s |
| Binary size (daemon) | ≤ 15 MB |

## Verify
- **V-REL-1** logind sleep inhibition from a user service on GNOME and KDE (SP2). XFCE is done ([Sleep and resume](#sleep-and-resume)). GNOME and KDE deferred by the maintainer on 2026-10-08.
- **V-REL-4** `PrepareForSleep` delivery (SP2, manual on XFCE): `systemd-run --user ~/sp2-spike/target/debug/sp2-spike daemon`, suspend from the XFCE menu, resume, then `journalctl --user -b | grep "sleep: PrepareForSleep"` must show `start=true` then `start=false`. Also unverified: whether `block` on `sleep` is refused (polkit `allow_any`) for a lingering service with no session.
- **V-REL-2** Windows logon task restart behaviour and tray from a scheduled task (SP2). Deferred by the maintainer on 2026-10-08.
- **V-REL-3** Budgets are achievable with OpenDAL + tokio + SQLite (Phase 1 benchmarks).
