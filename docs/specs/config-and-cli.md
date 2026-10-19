# Config and CLI

> **Status:** Draft · **Related:** [rules](../rules.md) (P5, P6, B2), [architecture](architecture.md), [emulator-profiles](emulator-profiles.md#overrides), [i18n](i18n.md#rust-strings), [platforms/headless](platforms/headless.md) · **Code:** `crates/cli` (planned)

## Purpose
Power users and headless devices can do everything without the GUI (P5). The UI, CLI and config file are three views of the same settings.

## Locations
Resolved with the `directories` crate:

| OS | Config | Data (state DB, history, staging) |
|---|---|---|
| Linux | `~/.config/leemusync/` | `~/.local/share/leemusync/` |
| macOS | `~/Library/Application Support/LeemuSync/` | same |
| Windows | `%APPDATA%\LeemuSync\` | `%LOCALAPPDATA%\LeemuSync\` |
| Android / iOS | App-private storage | App-private storage |

## Config file
- `config.toml`, versioned (`config_version = 1`), per device: device id and name, repo id, backend reference (`kind = "gdrive"` with `root` and `client_id`, or `kind = "fs"` with `path`), grants, emulator root overrides. Later phases add schedule, UI language and notification preferences.
- Shared settings (players, mappings, aliases) live in [repo config](sync-model.md#repo-config), not in this file.
- The engine validates on load and hot-reloads on change. If the file is invalid, the engine keeps the last good config and reports the error (status `Error`, CLI exit code 3).
- Secrets are **never** stored here ([security §Credentials](security.md#credentials)).
- The UI writes the same file through the engine API, so hand edits and UI edits never fight.

## Commands
Every command supports `--json` (stable English keys, versioned) and `--help`. Human-readable output is localised.

| Command | Does |
|---|---|
| `leemusync init` | Create a repo on a backend; set passphrase; show recovery key |
| `leemusync join` | Join an existing repo (passphrase or recovery key) |
| `leemusync status` | Engine state, pending transfers, conflicts, leases |
| `leemusync sync [--dry-run]` | Sync now |
| `leemusync play <emulator> <game> [--as <player>] [-- <args>]` | Launcher mode |
| `leemusync games` / `history <game>` / `restore <game> <version>` | Browse and restore |
| `leemusync conflicts` / `resolve <game> <version>` | Conflict handling |
| `leemusync lease status` / `lease release --force` | Lease handling |
| `leemusync players add/list` / `accounts map <player> <emulator> <id> [--keep\|--replace]` | Players and account mappings |
| `leemusync accounts recover <emulator> <id> --player <name>` | Upload an orphaned save folder as a player's first version |
| `leemusync scan` | Accounts, save folders, orphaned saves, grants needed |
| `leemusync devices` | Devices in the repo |
| `leemusync backend add/test/show` | Storage setup and probe |
| `leemusync backend add gdrive --client-id <id>` / `backend add fs <path>` / `backend login` | Google Drive or local-folder setup; OAuth login |
| `leemusync grant add/list/revoke` | Folder permissions |
| `leemusync profiles list/show/validate <file>` | Emulator profiles |
| `leemusync service install/uninstall/start/stop/status` | Daemon service management |
| `leemusync logs [--follow]` | Local, redacted logs |
| `leemusync doctor` | Diagnose permissions, paths, keystore, battery/background settings, clock skew |

The Phase 1 first slice ships `init`, `join`, `backend add/login`, `grant add/list`, `players add/list`, `scan`, `accounts map/recover`, `sync [--dry-run] [--yes]`, `status`, `history`, `restore` and `doctor` ([plan](../progress/plans/phase-1-first-sync.md)).

## Exit codes
| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Error |
| 2 | Usage error |
| 3 | Invalid config |
| 4 | Conflict needs a decision |
| 5 | Locked by another device |
| 6 | Backend unreachable or auth failed |

## Parity rule
Any feature added to the UI gets its CLI command (desktop/headless) in the same plan, and the other way round. Mobile has no CLI. Its Settings → Advanced exposes the same options.
