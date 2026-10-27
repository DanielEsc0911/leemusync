# Reliability

> **Status:** Draft · **Related:** [rules](../rules.md) (B3), [sync-model](sync-model.md), [security](security.md), [testing](testing.md), [platforms](platforms/desktop.md) · **Code:** `crates/engine` (`fsx.rs`, `snapshot.rs`, `upload.rs`, `restore.rs`)

## Purpose
A save must never be lost or corrupted, and sync must keep running around the clock wherever the OS allows.

## Invariants
Each invariant has tests (unit, simulation with injected faults) that must pass before any release.

| ID | Invariant | Tests |
|---|---|---|
| I1 | No local save is overwritten unless its previous content is in local history first | tested (M1a) |
| I2 | The remote is append-only. Only leases and `repo.json` are overwritten, and only retention GC deletes | — |
| I3 | Never read for a snapshot, or write a restore, while the emulator runs (when detectable) or before the quiet period ends | tested (M1a) |
| I4 | Every local write is atomic: temp file in the same folder → write → fsync → rename → fsync folder (not done on Windows; see V-REL-6) (`crates/engine/src/fsx.rs`) | tested (M1a) |
| I5 | Every multi-step operation is journaled and resumes or rolls back after a crash | tested (M1a) |
| I6 | Nothing is committed or restored until it's verified (AEAD authentication + content hash) | tested (M1a) |

## Restore procedure
1. Download all blobs to staging (`<data>/staging/<op>/`) and verify them (I6).
2. Re-check that the emulator is closed and the target files are quiet (I3).
3. Copy the current local files to local history (I1).
4. Journal "swap started", listing the files.
5. Swap each file atomically (I4).
6. Journal "done" and set `base` = restored version.

A crash between 4 and 6 → on startup the journal restores every file from the step-3 backup, then retries. A stream is never left half old, half new.

Implemented in `crates/engine/src/restore.rs`; crash tests at every journal step (`restore.after_backup`, `restore.mid_swap`, `restore.before_done`).
- Every staged file is checked against its manifest size and the profile's per-file limit before anything is written. Writes stay inside the grant and never follow symlinks.
- The step-3 backup (`<data>/history/<stream>/<ms>-<reason>/`) is read consistently, built in a temp folder, fsynced (files and folders) and renamed into place before the journal entry is written.
- Files the version doesn't have are removed from the save folder after the swap; they are in the backup.
- An I/O error during the swap rolls back at once. A crash is rolled back when the engine next opens (postponed while the emulator runs) and at the start of `sync` and `restore`.
- A rollback keeps I1 too: if the folder differs from the backup, its current files go to `<ms>-pre-rollback` in local history first. Rolling back is idempotent.

## Upload procedure
1. Take a snapshot (quiet, consistent read, [sync-model §Snapshot rules](sync-model.md#snapshot-rules)) into local history.
2. Upload the blobs the base version doesn't already reference (its manifest was committed after them, so they're on the remote), once per blob id, 4 at a time (create-only, idempotent: the same content gives the same id). A blob already on the remote is read back and verified, never overwritten.
3. Write the manifest (create-only) — this is the commit point.
4. Update `base`. A crash anywhere → retry. Orphaned blobs are collected by GC after the grace period.

Implemented in `crates/engine/src/{snapshot,upload}.rs`. Local history pruning is not in M1a: history grows until Phase 1's later plan adds pruning.

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
- **Linux logind (SP2, Fedora 44 XFCE, systemd 259):** a `block` lock on `sleep:idle` was taken without a prompt both from a terminal and from a systemd user service, showed in `systemd-inhibit --list` (`LemuSync SP2 … sleep:idle … block`) and was gone as soon as the fd closed. `delay` mode is rejected for `idle` (`InvalidArgs: Delay inhibitors only supported for shutdown and sleep`). A `delay` lock on `sleep` alone works but only buys `InhibitDelayMaxUSec` (5 s by default). `block` on `sleep` was not kept: it would also refuse a suspend the user asks for (see above). Real suspend from a systemd user service (`systemd-run --user … daemon`, suspend from the XFCE menu for ~17 s, resume): the journal showed `14:05:03 sleep: subscribed to PrepareForSleep`, `14:05:19 sleep: PrepareForSleep start=true`, `14:05:36 sleep: PrepareForSleep start=false`, so a user service receives both edges. Polkit defaults (`org.freedesktop.login1.policy`): `inhibit-block-sleep` is `yes` for active and inactive sessions but `auth_admin_keep` for `allow_any` (no session, e.g. lingering without a login); `inhibit-delay-sleep` and `inhibit-block-idle` are `yes` everywhere.
- **On resume:** wait for the network, refresh leases (they may have expired), rescan, sync.
- **On shutdown or logout:** finish the current atomic step, journal the rest, and release leases if the network allows.

## Network
Offline → queue locally and keep working. Transient errors → exponential backoff with jitter (1 s → 5 min). Auth errors → stop that backend and notify. Uploads are idempotent (content-addressed). Inside one sync the store retries each request (storage-backends §Interface); the daemon's 1 s → 5 min backoff is for whole syncs that still fail (Phase 2). Blobs upload 4 at a time.

## SC5 coverage
What the tests prove for [SC5](product.md) (crash, power loss or network drop mid-sync). All network cases run on `Store::fs` through `Store::faulty` (3 tries, millisecond backoff). Each scenario runs every request index N when the round makes 12 requests or fewer (today: 5 for an upload sync, 6 for a download sync), otherwise a fixed-seed sample of 12 that keeps the first and the last. Checks: no mixed file (the old or the new version only), a rerun converges both devices, one head, no conflict.

| Scenario | Upload sync | Download sync | `play` (lease → download → emulator → upload → release) |
|---|---|---|---|
| `kill -9` at every journal step | `kill.rs` `a_kill_around_the_manifest_never_makes_a_conflict` | `kill.rs` `a_kill_during_a_fast_forward_leaves_the_old_or_new_files_and_a_backup` | `kill.rs` `a_kill_after_writing_a_lease_lets_this_device_take_it_again` |
| In-process error at a journal step | `faults.rs` `a_crash_around_the_manifest_never_makes_a_conflict` | `faults.rs` `a_crash_during_a_fast_forward_rolls_back_then_retries` | covered by the sync steps it runs |
| Cut from request N (until healed) | `faults.rs` `a_network_fault_at_any_request_of_a_sync_never_corrupts_or_conflicts` | same test | `play.rs` `a_network_fault_at_any_request_of_a_play_never_corrupts_or_conflicts` (at most B's own stale lease left, which expires) |
| Lost response at request N | same test | same test | same test |
| Outage shorter than the retry budget: completes, no error | `network.rs` `a_short_outage_at_any_request_completes_without_error` | same test | not separate: each request is retried the same way |
| Outage longer than the retry budget: exit code 6, next run converges | `network.rs` `a_long_outage_at_any_request_fails_transient_then_converges` | same test | covered by the cut case |
| Slow link (3 ms per request): same result as a fast run | `network.rs` `a_slow_link_gives_the_same_result_as_a_fast_one` | same test | not separate |
| `list` is retried | `store` unit test `a_transient_list_is_retried` | | |

## Budgets
Targets for the daemon on desktop. Measured in Phase 1/2 benchmarks and enforced in CI on regression (not yet: Phase 2, with the daemon).

Measured 2026-10-21 on the Fedora PC (AMD Ryzen AI 7 350), release build, `just bench` (`crates/engine/benches/budgets.rs`). These are the engine-side numbers the daemon will inherit; there is no daemon yet.

| Metric | Target | Measured (engine, 2026-10-21) |
|---|---|---|
| Idle CPU | ≈ 0% (event-driven, no busy polling) | not measured (needs the daemon) |
| Idle memory (RSS) | ≤ 30 MB | 23 MiB: whole bench process (criterion included) with an open engine, from `/proc/self/statm` |
| Cold start to ready | ≤ 300 ms | 0.22 ms: `Engine::open` on a populated state DB, repo unlocked from the keystore, one remote read (local-folder backend) |
| Change detected → upload started | quiet period + ≤ 2 s | not measured (needs the daemon's watcher) |
| Rescan of 10 000 files (warm) | ≤ 1 s | 21 ms: dry-run sync of one unchanged 10 001-file save folder |
| Binary size (daemon) | ≤ 15 MB | 14.0 MB (14 016 632 bytes): `lemusync` CLI, Linux x86-64; close to the target |

## Verify
- **V-REL-1** logind sleep inhibition from a user service on GNOME and KDE (SP2). XFCE is done ([Sleep and resume](#sleep-and-resume)). GNOME and KDE deferred by the maintainer on 2026-10-08.
- **V-REL-4** Whether polkit refuses logind inhibitors for a lingering service with no session (`allow_any` is `auth_admin_keep` for `inhibit-block-sleep`; the `idle` block and `sleep` delay locks used now are `yes`, but untested without a session) (SP2).
- **V-REL-5** Desktop idle suspend actually waits while LemuSync holds its inhibit (needs an idle-timeout run): XFCE with `org.freedesktop.PowerManagement.Inhibit`; GNOME (gnome-settings-daemon follows `org.gnome.SessionManager` inhibitors, which the portal's `Inhibit` feeds) and KDE (PowerDevil's policy agent, which also reads logind inhibitors) from documentation only, unmeasured (SP2).
- **V-REL-2** Windows logon task restart behaviour and tray from a scheduled task (SP2). Deferred by the maintainer on 2026-10-08.
- **V-REL-3** Budgets are achievable with OpenDAL + tokio + SQLite. Engine side measured 2026-10-21 ([Budgets](#budgets)): cold start, rescan and RSS are well inside; binary size is 14.0 of 15 MB. Still open: idle CPU, change-to-upload latency and RSS of the daemon itself (Phase 2), and cold start against a remote backend.
- **V-REL-6** Whether the I4 rename survives power loss on Windows without a folder fsync. `fsx::write_atomic` doesn't do that step on Windows (SP2).
