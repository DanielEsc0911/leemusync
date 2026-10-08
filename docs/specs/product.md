# Product

> **Status:** Decided · **Related:** [rules](../rules.md), [architecture](architecture.md), [sync-model](sync-model.md), [roadmap](../progress/roadmap.md) · **Code:** —

## Purpose
LeemuSync keeps emulator saves in sync across all of a household's devices, through storage they own, without ever losing a save.

## Users
- **Players.** Install, pick storage, play. Common emulators need no configuration.
- **Power users.** Custom paths and profiles, headless servers, CLI with JSON, hand-edited config ([P5](../rules.md#product)).
- **Households.** Several players sharing devices and games.

## Core scenarios
These are acceptance-level. Every phase's exit criteria refer to them.

| ID | Scenario | Expected result |
|---|---|---|
| SC1 | Two players play the same game (e.g., Zelda: BotW on Cemu) on two machines at the same time | Separate streams per player; no interference; both stay synced |
| SC2 | A player switches machines | Close the game on A → upload. Open it on B → latest save restored before the emulator starts |
| SC3 | The same player's save is opened on two machines | The second machine warns ("in use on Desktop"). If the player continues anyway, both versions are kept and they choose later. Nothing is lost |
| SC4 | A new device joins | Storage + passphrase (or recovery key) → map emulator accounts → saves appear |
| SC5 | Crash, power loss or network drop mid-sync | No corruption. The operation resumes or rolls back, and a local backup exists |
| SC6 | Always-on headless device (Pi, server, Linux handheld) | Daemon + CLI + config file, no GUI needed |
| SC7 | Unusual setup (portable install, Flatpak, custom save folder, unknown emulator) | Path override or local custom profile, with full safety features |

## Platforms
Windows (x64, arm64), macOS (Apple Silicon and Intel), Linux (x86_64, arm64), Raspberry Pi (arm64 with GUI; armv7 headless), Android, iOS/iPadOS. Details: [platforms/](platforms/desktop.md).

## Systems
Any emulator that stores saves as files, which covers every major console (Nintendo NES → Switch, PlayStation 1–3, PSP, Vita, Sega, Xbox, arcade…). Coverage grows by adding profiles ([emulator-profiles](emulator-profiles.md)).

## Scope
- **v1 (Phases 0–5):** saves for the initial emulator set on desktop, Android and iOS. Backends: S3, WebDAV, SFTP (Linux and macOS only, [D25](../decisions.md#d25-sftp-only-on-linux-and-macos-for-now-2026-10-08)), local folder. Launcher and watch modes. English and Spanish.
- **Later (Phase 6+):** Google Drive, OneDrive, Dropbox, MEGA; more emulators; save states (opt-in); Linux handheld firmwares; optional push.

## Non-goals
- Syncing ROMs, BIOS, firmware or console keys (legal risk and size; keys are secrets).
- Hosting storage or running servers. LeemuSync is a client and the user brings the storage.
- Parsing game save contents (progress, achievements) in core ([D7](../decisions.md#d7-no-game-save-format-parsing-in-core-2026-10-07)).
- Telemetry, LeemuSync accounts, ads.
- Cheats or save editing.

## Market gap
Researched 2026-10-07.

| Tool | Has | Lacks |
|---|---|---|
| [1Retro](https://www.timeextension.com/news/2026/09/i-built-1retro-so-we-would-stop-losing-save-files-this-cloud-sync-for-retro-players-could-be-a-game-changer) | Many emulators and handhelds, Android, SteamOS | Its own paid cloud only |
| [CrossSave Cloud](https://github.com/h1dr0nn/CrossSave-Cloud) | Emulator-aware, version history | Linux and Android only |
| [Syncthing](https://en.wikipedia.org/wiki/Syncthing) / Syncthing-Fork | Peer-to-peer, popular | Not emulator-aware, no official iOS app, no locking |
| RetroArch cloud sync | Built in, WebDAV | RetroArch only |
| Ludusavi | rclone backends, Rust | Desktop backup tool; emulators need manual paths |
| EmuDeck cloud saves | rclone | SteamOS/Windows focus |

No existing tool combines emulator awareness, user-owned storage, every platform including iOS, E2E encryption, and multi-player locking with handoff.
