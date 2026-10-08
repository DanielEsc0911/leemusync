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
- While a transfer or restore is in flight, the daemon **inhibits idle sleep only**: Windows `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, macOS `IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleSystemSleep)`, Linux logind `Inhibit("idle", …, "block")` over D-Bus. The inhibition is released as soon as the work ends, with a maximum of 10 min. A suspend the user asks for always wins, as on Windows and macOS: a logind `block` lock on `sleep` would also refuse a user-requested suspend ([systemd inhibitor locks](https://systemd.io/INHIBITOR_LOCKS/), [`org.freedesktop.login1`](https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html)), so it is never taken; B3 is protected instead by the delay lock below, leases and divergence detection.
- **Linux checkpoint before suspend:** while the daemon runs it holds a logind `Inhibit("sleep", …, "delay")` lock. On `PrepareForSleep(true)` it journals, pauses transfers, releases leases and closes the lock, all within `InhibitDelayMaxUSec` (5 s by default). On `PrepareForSleep(false)` it takes the delay lock again and resumes.
- **Linux desktop idle suspend:** desktops may run their own idle-suspend timer instead of logind's `IdleAction`, and some ignore logind `idle` inhibitors. So during transfers the daemon also takes the desktop inhibit: `org.freedesktop.PowerManagement.Inhibit.Inhibit` on XFCE (measured: [desktop §Linux results](platforms/desktop.md#linux-results-sp2-xfce)), and the xdg-desktop-portal `org.freedesktop.portal.Inhibit` (flags suspend=4 | idle=8) elsewhere (V-REL-5).
- **Linux logind (SP2, Fedora 44 XFCE, systemd 259):** a `block` lock on `sleep:idle` was taken without a prompt both from a terminal and from a systemd user service, showed in `systemd-inhibit --list` (`LeemuSync SP2 … sleep:idle … block`) and was gone as soon as the fd closed. `delay` mode is rejected for `idle` (`InvalidArgs: Delay inhibitors only supported for shutdown and sleep`). A `delay` lock on `sleep` alone works but only buys `InhibitDelayMaxUSec` (5 s by default). `block` on `sleep` was not kept: it would also refuse a suspend the user asks for (see above). Real suspend from a systemd user service (`systemd-run --user … daemon`, suspend from the XFCE menu for ~17 s, resume): the journal showed `14:05:03 sleep: subscribed to PrepareForSleep`, `14:05:19 sleep: PrepareForSleep start=true`, `14:05:36 sleep: PrepareForSleep start=false`, so a user service receives both edges. Polkit defaults (`org.freedesktop.login1.policy`): `inhibit-block-sleep` is `yes` for active and inactive sessions but `auth_admin_keep` for `allow_any` (no session, e.g. lingering without a login); `inhibit-delay-sleep` and `inhibit-block-idle` are `yes` everywhere.
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
- **V-REL-4** Whether polkit refuses logind inhibitors for a lingering service with no session (`allow_any` is `auth_admin_keep` for `inhibit-block-sleep`; the `idle` block and `sleep` delay locks used now are `yes`, but untested without a session) (SP2).
- **V-REL-5** Desktop idle suspend actually waits while LeemuSync holds its inhibit (needs an idle-timeout run): XFCE with `org.freedesktop.PowerManagement.Inhibit`; GNOME (gnome-settings-daemon follows `org.gnome.SessionManager` inhibitors, which the portal's `Inhibit` feeds) and KDE (PowerDevil's policy agent, which also reads logind inhibitors) from documentation only, unmeasured (SP2).
- **V-REL-2** Windows logon task restart behaviour and tray from a scheduled task (SP2). Deferred by the maintainer on 2026-10-08.
- **V-REL-3** Budgets are achievable with OpenDAL + tokio + SQLite (Phase 1 benchmarks).
