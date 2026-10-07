# Players and accounts

> **Status:** Draft · **Related:** [rules](../rules.md) (P3, P5), [sync-model](sync-model.md), [emulator-profiles](emulator-profiles.md), [glossary](../glossary.md), [navigation](ui/navigation.md) · **Code:** —

## Purpose
Map people (players) to emulator user folders on every device, so each player's saves stay separate and follow them from machine to machine.

## Concepts
- **Player:** a person in the repo. A solo user has exactly one player and only ever sees their own name (progressive disclosure, P5).
- **Device:** one install. Random device id, user-chosen name, **default player**.
- **Account mapping:** (device, emulator, player) → emulator account id. Stored in [repo config](sync-model.md#repo-config).

## Multi-user emulators (Full tier)
| Emulator | Account id | Location (Verify) |
|---|---|---|
| Cemu (Wii U) | persistent id `8xxxxxxx` (first account: `80000001`) | accounts `mlc01/usr/save/system/act/<id>/`; saves `mlc01/usr/save/00050000/<titleid>/user/<id>/` |
| RPCS3 (PS3) | user `0000000N` | `dev_hdd0/home/<user>/savedata/<serial>/` |
| Yuzu-family forks (Switch: Eden, Citron…) | 128-bit profile UUID | `nand/user/save/0000000000000000/<uuid>/<titleid>/` |

### The same-ID trap
Every Cemu install creates its first account as `80000001`. Two machines with differently *named* accounts usually still share that id, so syncing raw folders would mix two players' saves. LeemuSync never syncs by account id. Each device maps a player to its own local id:

```toml
# repo config (edited through UI/CLI; TOML shown for clarity)
[players.daniel.accounts.cemu]
desktop = "80000001"
laptop  = "80000002"

[players.ana.accounts.cemu]
laptop  = "80000001"
desktop = "80000002"
```

### Discovery
A scan lists the accounts it finds (id, plus the display name if the profile says how to read it). LeemuSync proposes mappings by matching names and the player confirms. Unmatched accounts → ask. Two players can't map to the same id on one device.

### Shared folders
Some games write to a folder shared by all accounts (Wii U `user/common`). Per-player separation is impossible for those files. That part of the game drops to Basic (one lease for all players), and the UI says so. Profiles list such paths under `saves.shared`.

## Single-user emulators (Standard tier)
The emulator has one save location per game. Per-player saves work by **swapping**:
- **Launcher mode:** before launch, the chosen player's latest version is restored into the emulator's save location. The current content is backed up first, and if it's dirty it's uploaded to its own stream first. After the emulator exits, the snapshot goes to the chosen player's stream.
- **Watch mode:** changes belong to the device's default player. "Play as Ana" is an explicit action that performs the swap.
- **Never swap while the emulator runs.**

## Basic tier
Swapping works the same way, but at container granularity: the whole memory card is swapped, and the lease covers the container.

## Edge cases
| Case | Behaviour |
|---|---|
| Player removed | Mappings removed; streams and history kept |
| Device renamed | Mappings follow the device id |
| Account deleted in the emulator | Mapping flagged on next scan; asks the player |
| New device, existing players | Join flow asks for this device's mappings ([navigation](ui/navigation.md)) |

## Verify
- **V-ACC-1** Cemu account folder layout, how to read the display name from `account.dat`, and whether BotW uses `user/common` (spike SP7).
- **V-ACC-2** RPCS3 and Yuzu-family layouts (Phase 6 profile work).
