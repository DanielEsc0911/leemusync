# Desktop: Windows, macOS, Linux

> **Status:** Draft · **Related:** [rules](../../rules.md) (B3, P4), [architecture §Process model](../architecture.md#process-model), [reliability](../reliability.md#always-on-247), [status-and-notifications](../ui/status-and-notifications.md#desktop-tray-and-menu-bar), [emulator-profiles](../emulator-profiles.md), [security](../security.md) · **Code:** `crates/daemon`, `packaging/` (planned)

## Purpose
Desktop is where most emulators run. The daemon gives true 24/7 sync with native tray and notifications. The Flutter app is a client of it.

## Matrix
| | Windows 10/11 | macOS | Linux |
|---|---|---|---|
| Arch | x64, arm64 | Apple Silicon + Intel (universal) | x86_64, arm64 |
| Service | Task Scheduler logon task | LaunchAgent via `SMAppService` | systemd user unit |
| Tray | Notification area | Menu bar extra | StatusNotifierItem / AppIndicator |
| Notifications | Toasts with actions | `UNUserNotificationCenter` | freedesktop notifications with actions |
| Sleep inhibition | `SetThreadExecutionState` | IOPM assertion | logind `Inhibit` |
| Process detection | Process list | Process list | `/proc` (Flatpak apps visible too) |
| Keystore | Credential Manager | Keychain | Secret Service |
| Packaging (now, [release §Artifacts](../release.md#artifacts)) | `.msi`, `.exe` installer | `.dmg` (ad-hoc signed until a Developer ID exists) | `.deb`, `.rpm`, `.AppImage`, `.tar.gz` |
| Packaging (later) | winget | Homebrew cask, notarised DMG | Flatpak, AUR |

## Quirks
- **Windows toasts** need an AppUserModelID registered by the installer (Start Menu shortcut). Portable mode falls back to tray balloons (Verify SP2).
- **macOS notifications** need an app identity, so `leemusyncd` ships inside the app bundle as a helper and is registered as a LaunchAgent from there. Distribute outside the Mac App Store, because sandboxing would block reading other apps' save folders.
- **macOS privacy (TCC):** `~/Documents` (e.g., RetroArch's default) triggers a permission prompt. Other apps' sandbox containers trigger the "access data from other apps" prompt. The grant flow explains this before the OS asks.
- **Linux GNOME:** no tray without the AppIndicator extension (see [status](../ui/status-and-notifications.md#desktop-tray-and-menu-bar)). XFCE and KDE show StatusNotifier icons natively. Test machines: [testing §Device lab](../testing.md#device-lab).
- **Flatpak packaging vs. access:** a sandboxed LeemuSync needs filesystem permissions for emulator folders, including other Flatpaks' `~/.var/app/<id>/`. Spike SP2 decides between narrow static permissions, the document portal, or a native daemon package (Verify).
- **Steam Deck (Game Mode):** no tray is visible, but the daemon still runs. A Decky plugin is a later idea.
- **Portable emulators:** profiles use `{exe_dir}`. The user picks the executable in launcher setup.

## Linux results (SP2, XFCE)
Measured 2026-10-08 on Fedora 44, XFCE 4.20 on X11 (`xfce4-panel` 4.20.7, `xfce4-notifyd` 0.9.7, systemd 259, KWallet `ksecretd` from `kf6-kwallet` 6.30.0). Throwaway prototype, crates [`ksni` 0.3.6](https://docs.rs/ksni/0.3.6), [`notify-rust` 4.18.2](https://docs.rs/notify-rust/4.18.2), [`zbus` 5.19.0](https://docs.rs/zbus/5.19.0), [`keyring` 3.6.3](https://docs.rs/keyring/3.6.3), tokio 1.53.2.

- **Tray:** `ksni` (pure Rust StatusNotifierItem over D-Bus, no GTK) was chosen over `tray-icon`, which needs a GTK main loop and libappindicator on Linux. The item registered with `org.kde.StatusNotifierWatcher` (served by XFCE's "Status Tray Plugin", `libsystray.so`, panel plugin `systray`) as `org.kde.StatusNotifierItem-<pid>-1/StatusNotifierItem`. The icon and tooltip swapped idle → syncing → error every 3 s and screenshots of the panel showed each icon. A menu with 3 items plus a separator was exposed. XFCE needs nothing beyond its default systray plugin; it remembers the item in `/plugins/plugin-N/known-items`.
- **Started by systemd:** the same tray registered when the prototype ran as a systemd user unit. The unit inherits `DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` and `XDG_*` because Fedora's `/etc/X11/xinit/xinitrc.d/50-systemd-user.sh` runs `systemctl --user import-environment DISPLAY XAUTHORITY` and `dbus-update-activation-environment --systemd` at X login. Without a watcher on the bus (private `dbus-run-session`), `ksni` fails with `Watcher(ServiceUnknown(...))`, so the daemon must treat the tray as optional and retry when a watcher appears.
- **Unit target:** `graphical-session.target` stays **inactive** under XFCE (it is only reached by sessions that start it, e.g. GNOME and KDE), so `WantedBy=graphical-session.target` would never start the daemon there. Use `WantedBy=default.target`, and create the tray lazily. `Restart=on-failure` restarted the unit 2 s after `kill -9` (`NRestarts=1`, new PID, tray re-registered).
- **Linger:** `loginctl enable-linger` works for the user without a polkit prompt and reports `Linger=yes`. It starts the user manager (and `default.target` units) at boot without a login, so sync runs headless, with no tray and no notifications until a graphical session imports its environment.
- **Notifications:** `GetServerInformation` → `Xfce Notify Daemon`, `0.9.7`, spec `1.2`. `GetCapabilities` → `action-icons actions body body-hyperlinks body-markup icon-static sound x-canonical-private-icon-only` (no `persistence`). A notification with 2 actions was shown. Without a click it expired and `notify-rust` reported `__closed`. Clicking an action (`ActionInvoked`) is listed under Verify.

## Verify
- **V-DESK-1** Tray + actionable notifications from the daemon on Windows and macOS (SP2). Linux XFCE is done ([above](#linux-results-sp2-xfce)), except the action click (V-DESK-5). Windows and macOS deferred by the maintainer on 2026-10-08.
- **V-DESK-5** Manual: run the prototype's `notify` mode, click "Keep this device", and check it prints `notify: action=keep-local` (SP2, XFCE).
- **V-DESK-2** Flatpak permission strategy (SP2). Research so far: inside the sandbox an app sees only its own `~/.var/app/$FLATPAK_ID`, and `--filesystem=home` excludes `~/.var/app`, which must be requested per path ([Flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)). Flathub wants static permissions "kept to an absolute minimum" and makes a portal mandatory when one covers the use case ([Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)). No test Flatpak was built. The decision entry is left to the maintainer.
- **V-DESK-3** `SMAppService` agent from a notarised bundle with a Rust helper (SP2).
- **V-DESK-4** Flutter Windows arm64 build support (SP1).
