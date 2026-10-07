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

## Verify
- **V-DESK-1** `tray-icon` + `notify-rust` (or native equivalents) from the daemon on all three OSes, with action buttons (SP2).
- **V-DESK-2** Flatpak permission strategy (SP2).
- **V-DESK-3** `SMAppService` agent from a notarised bundle with a Rust helper (SP2).
- **V-DESK-4** Flutter Windows arm64 build support (SP1).
