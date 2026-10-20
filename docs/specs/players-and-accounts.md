# Players and accounts

> **Status:** Draft · **Related:** [rules](../rules.md) (P3, P5), [sync-model](sync-model.md), [emulator-profiles](emulator-profiles.md), [glossary](../glossary.md), [navigation](ui/navigation.md) · **Code:** —

## Purpose
Map people (players) to emulator user folders on every device, so each player's saves stay separate and follow them from machine to machine.

## Concepts
- **Player:** a person in the repo. A solo user has exactly one player and only ever sees their own name (progressive disclosure, P5).
- **Device:** one install. Random device id, user-chosen name, **default player**.
- **Account mapping:** (device, emulator, player) → emulator account id. Stored in [repo config](sync-model.md#repo-config).

## Multi-user emulators (Full tier)
| Emulator | Account id | Location (Cemu verified on Linux in SP7; others Verify) |
|---|---|---|
| Cemu (Wii U) | persistent id `8xxxxxxx` (first account: `80000001`) | accounts `mlc01/usr/save/system/act/<id>/`; saves `mlc01/usr/save/00050000/<titleid>/user/<id>/` |
| RPCS3 (PS3) | user `0000000N` | `dev_hdd0/home/<user>/savedata/<serial>/` |
| Yuzu-family forks (Switch: Eden, Citron…) | 128-bit profile UUID | `nand/user/save/0000000000000000/<uuid>/<titleid>/` |

### Cemu accounts (SP7)
Verified on Cemu 2.6 (Linux Flatpak) against the [v2.6 source][cemu-acc], 2026-10-08:
- One folder per account: `mlc01/usr/save/system/act/<persistent id, 8 hex digits>/account.dat`. Next to them, `act/persisid.dat` stores the id counter.
- `account.dat` is a line-based text file. The first line is `AccountInstance_20120705`, then `Key=value` lines (`PersistentId`, `Uuid`, `MiiData`, `MiiName`, `AccountId`, `Country`, …).
- **Display name:** key `MiiName`. The value is exactly 44 hex characters: 11 UTF-16 code units, each written as 4 hex digits (most significant first), padded with `0000`. Decode until the first `0000`. Cemu rejects any other length. The maintainer's real file matches this format (value not recorded).
- **BotW and `user/common`:** BotW (US, `00050000/101c9400`) creates `user/common/`. On the maintainer's long-played install (Fedora 44, Cemu 2.6 Flatpak) it exists but is empty. All game data (`0/`–`5/` slots, `option.sav`, `album/`, `pict_book/`, `tracker/`) is under `user/80000001/`. So BotW is fully per-player. The profile still lists `user/common` under `saves.shared` for games that use it.
- **Second Linux install (CachyOS, 2026-10-18):** native package `cemu-bin` 2.6 (`/usr/bin/cemu`), data root `~/.local/share/Cemu`, config `~/.config/Cemu/settings.xml` with an empty `mlc_path`. Two accounts, `80000001` and `80000002`. Both `account.dat` files have the same first line, the same key set and a 44-character `MiiName`. BotW US has full game data under each `user/<id>/`, and `user/common/` is empty. Everything matches the Fedora install (names and formats only, no values recorded). The maintainer then deleted `80000002` on both machines for M1 ([roadmap](../progress/roadmap.md#phase-1-sync-engine--cli-desktop)). Deleting an account in Cemu removes only `system/act/80000002/`. BotW's `user/80000002/` save folder stays behind, so discovery can find save folders with no matching account.
- **BotW title ids** ([WiiUBrew title database][wiiubrew]): JP `00050000-101C9300` (`WUP-P-ALZJ`), US `00050000-101C9400` (`WUP-P-ALZE`), EU `00050000-101C9500` (`WUP-P-ALZP`). Updates use high part `0005000E`, DLC `0005000C`. Cemu writes folder names in lowercase.

[cemu-acc]: https://github.com/cemu-project/Cemu/blob/v2.6/src/Cafe/Account/Account.cpp
[wiiubrew]: https://wiiubrew.org/wiki/Title_database

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
A scan lists the accounts it finds (id, plus the display name if the profile says how to read it). LeemuSync proposes mappings by matching names and the player confirms. Unmatched accounts → ask. Two players can't map to the same id on one device. Scan: `crates/engine/src/scan.rs`.

**Orphaned saves.** A scan also lists save folders whose account no longer exists (for Cemu, `user/<id>/` with no `system/act/<id>/`). Deleting an account in Cemu leaves them behind ([above](#cemu-accounts-sp7)). The scan never ignores or deletes them. It offers to recover one: the player picks who it belongs to, and LeemuSync syncs it into that player's stream. The folder is only read; LeemuSync writes it to a live account only through a normal restore. Seen on the maintainer's CachyOS laptop (2026-10-18).

**A player with no account here.** When a player's stream exists in the repo but the player has no account mapped on this device, `sync` reports it and does nothing for that stream. The player chooses:
- **a. Use an existing account:** `leemusync accounts map <player> cemu <id>`. If that account already has files and the player has a version in the repo, the CLI asks whether to keep them (they're uploaded and become a second head, a conflict) or replace them (`--replace`): the current files go to local history first (I1), then the player's latest version is restored.
- **b. Create the account in the emulator first,** then `leemusync scan` and `accounts map`.
- **c. Let LeemuSync create it:** only after spike SP8 proves Cemu accepts it (V-ACC-3).

**Recovering an orphaned save:** `leemusync accounts recover cemu <id> --player <name>` uploads the orphaned folder as the first version of that player's stream. It's refused if the player already has a version of that game, so a recovered save can never replace one. The folder itself is only read.

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
| Account deleted in the emulator | Mapping flagged on next scan; asks the player. Its save folders stay and show up as orphaned saves ([Discovery](#discovery)) |
| New device, existing players | Join flow asks for this device's mappings ([navigation](ui/navigation.md)) |

## Verify
- **V-ACC-1** Same Cemu layout and `account.dat` format on Windows and macOS. Deferred by the maintainer on 2026-10-08 (spike SP7 device pass). The second Linux install (CachyOS) is done ([above](#cemu-accounts-sp7)).
- **V-ACC-2** RPCS3 and Yuzu-family layouts (Phase 6 profile work).
- **V-ACC-3** Cemu 2.6 accepts an `account.dat` written by LeemuSync from an existing account as the template (new `PersistentId` from `persisid.dat`, new `Uuid`, new `MiiName`): it lists the account and BotW saves under it (spike SP8, [Phase 1 first-sync plan](../progress/plans/phase-1-first-sync.md)).
