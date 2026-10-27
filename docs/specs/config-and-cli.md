# Config and CLI

> **Status:** Draft · **Related:** [rules](../rules.md) (P5, P6, B2), [architecture](architecture.md), [emulator-profiles](emulator-profiles.md#overrides), [i18n](i18n.md#rust-strings), [platforms/headless](platforms/headless.md) · **Code:** `crates/engine/src/local_config.rs`, `crates/cli/`

## Purpose
Power users and headless devices can do everything without the GUI (P5). The UI, CLI and config file are three views of the same settings.

## Locations
Resolved with the `directories` crate:

| OS | Config | Data (state DB, history, staging) |
|---|---|---|
| Linux | `~/.config/lemusync/` | `~/.local/share/lemusync/` |
| macOS | `~/Library/Application Support/LemuSync/` | same |
| Windows | `%APPDATA%\LemuSync\` | `%LOCALAPPDATA%\LemuSync\` |
| Android / iOS | App-private storage | App-private storage |

## Config file
- `config.toml`, versioned (`config_version = 1`), per device: device id and name, repo id, backend reference (`kind = "gdrive"` with `root` and `client_id`, or `kind = "fs"` with `path`), grants, per-emulator settings `[emulators.<id>]` (`root` override; `exe`, the argv prefix that starts the emulator, e.g. `["/usr/bin/cemu"]` or `["flatpak", "run", "info.cemu.Cemu"]`, never run through a shell; `games."<title id>" = "<absolute path>"`, the file passed as `{game_path}` to the profile's [`launch.args`](emulator-profiles.md#schema-v1)), `[leases] ttl_secs` (lease lifetime, default 300, 120–86400; [sync-model §Leases](sync-model.md#leases)), `conflict_policy = "ask" | "newest"` (default `ask`; [sync-model §Conflicts](sync-model.md#conflicts)). Later phases add schedule, UI language and notification preferences.
- Shared settings (players, mappings, aliases) live in [repo config](sync-model.md#repo-config), not in this file.
- The engine validates on load and hot-reloads on change. If the file is invalid, the engine keeps the last good config and reports the error (status `Error`, CLI exit code 3).
- Secrets are **never** stored here ([security §Credentials](security.md#credentials)).
- The UI writes the same file through the engine API, so hand edits and UI edits never fight.

## Commands
Every command supports `--json` (stable English keys, versioned) and `--help`. Human-readable output is localised.

| Command | Does |
|---|---|
| `lemusync init` | Create a repo on a backend; set passphrase; show recovery key. Once the repo exists the key is always shown: if a later step fails on this device, `init` warns and `lemusync join` finishes it |
| `lemusync join` | Join an existing repo (passphrase or recovery key) |
| `lemusync status` | Engine state, pending transfers, conflicts, leases |
| `lemusync sync [--dry-run]` | Sync now |
| `lemusync play <emulator> <game> [--as <player>] [-- <args>]` | Launcher mode |
| `lemusync games` / `history <game>` / `restore <game> <version>` | Browse and restore |
| `lemusync conflicts` / `resolve <game> <version>` | Conflict handling |
| `lemusync lease status` / `lease release --force` | Lease handling |
| `lemusync players add/list` / `accounts map <player> <emulator> <id> [--keep\|--replace]` | Players and account mappings |
| `lemusync accounts recover <emulator> <id> --player <name>` | Upload an orphaned save folder as a player's first version |
| `lemusync accounts create <player> <emulator> [--name <name>] [--yes]` | Write a new emulator account from profile data, map it to the player, then sync to restore their saves ([players-and-accounts](players-and-accounts.md#creating-a-cemu-account-sp8)) |
| `lemusync scan` | Accounts, save folders, orphaned saves, grants needed |
| `lemusync devices` | Devices in the repo |
| `lemusync backend add/test/show` | Storage setup and probe |
| `lemusync backend add s3 --endpoint <url> --bucket <name> --access-key-id <id> [--region auto]` / `backend add gdrive --client-id <id>` / `backend add fs <path>` / `backend login` | S3-compatible (R2), Google Drive or local-folder setup, with a probe for S3; OAuth login |
| `lemusync grant add/list/revoke` | Folder permissions |
| `lemusync profiles list/show/validate <file>` | Emulator profiles |
| `lemusync service install/uninstall/start/stop/status` | Daemon service management |
| `lemusync logs [--follow]` | Local, redacted logs |
| `lemusync doctor` | Diagnose permissions, paths, keystore, battery/background settings, clock skew |

The Phase 1 first slice ships `init`, `join`, `backend add/login`, `grant add/list`, `players add/list`, `scan`, `accounts map/recover`, `sync [--dry-run] [--yes]`, `status`, `history`, `restore` and `doctor` ([plan](../progress/plans/phase-1-first-sync.md)). M1b adds `play`, `conflicts`, `resolve <game> <version> [--as <player>]`, `lease status`, `lease release --force <game>` and `accounts create` ([plan](../progress/plans/phase-1b-play.md)). `--as` defaults to the only player in the repo. `conflict_policy` and `[emulators.<id>] exe`/`games` are set in `config.toml` only.

**`play` in the terminal.** One line on stderr per step (getting the newest save, waiting for another device, the emulator running, uploading), then the emulator's exit code and the upload result on stdout. Lease renewals are silent unless one fails. On Unix the emulator runs in its own process group: Ctrl-C reaches only LemuSync, which prints a hint and keeps waiting, then uploads and releases the lease when the emulator closes. A held lease or a conflict is asked about only with a terminal and without `--json`; otherwise `play` stops before launch with exit 5 or 4.

**Progress and results.** While `sync`, `restore` and `accounts recover` run, one line on stderr shows the stream being worked on: a spinner, player · game, the phase, a bar, files, MB and MB/s. It's redrawn every 100 ms and cleared before anything else prints. It only appears when stderr is a terminal, `TERM` isn't `dumb` and `--json` is off. `sync` then prints one aligned line per stream (with "N files, X MB" when something moved) and the elapsed time. The action is colored (green done, yellow waiting on the user, red conflict) only when stdout is a terminal and `NO_COLOR` is unset.

**Connection notice.** While a storage request is retried (reliability §Network), the engine sends `ConnectionEvent`s (architecture §Engine API). Any command that talks to the backend shows them on stderr: nothing for one quick retry; from the second retry of an outage on, one line, en "Connection trouble, retrying…" / es "Problemas de conexión, reintentando…"; then "Connection is back." / "La conexión volvió." once a request succeeds. At most those two lines per outage; the live progress line is cleared first. Nothing with `--json`, which doesn't stream events. Code: `crates/cli/src/connection.rs`.

Secrets (passphrase, recovery key, OAuth client secret, S3 secret key) are only typed at no-echo prompts, never taken from arguments or environment variables; without a terminal the command fails with a usage error. The recovery key is printed once by `init`, to stderr, even with `--json`. `LEMUSYNC_TEST_ROOT=<dir>` puts the config, data and emulator search under `<dir>`: it's for tests only (the OS keystore is still used).

## Exit codes
| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Error |
| 2 | Usage error |
| 3 | Invalid config |
| 4 | Conflict needs a decision (`sync` found one; `play` without a terminal or with `--json`) |
| 5 | Locked by another device (`play` found a held lease without a terminal or with `--json`; never a silent "play anyway") |
| 6 | Backend unreachable or auth failed |

## Error codes
Every error has a stable `code` (the `--json` `error.code`, and the key of its localized message `error-<code>` in `locales/`). Storage errors map by class ([storage-backends §Interface](storage-backends.md#interface)):

| Class | Code | Exit | Message says |
|---|---|---|---|
| `transient` (after the store's retries) | `backend_offline` | 6 | Can't reach the storage; check the connection and try again; saves on this device are safe |
| `auth` | `backend_auth` | 6 | The login failed; run `lemusync backend login` |
| `quota` | `backend_quota` | 1 | The storage is full; free up space; saves on this device are safe |
| `not_found`, `already_exists`, `permanent` | `backend` | 1 | The storage returned an error; try again, then `lemusync doctor` |

"Saves on this device are safe" holds because a sync or restore downloads and verifies everything into staging before it touches a save folder, and an upload only reads local files ([reliability §Restore procedure](reliability.md#restore-procedure)). The message never contains provider text. That goes on a second, dim line, `Details: …` / `Detalles: …`, and in `--json` as `error.detail` next to `code` and `message` (the English message for storage errors). It always passes through `crates/store/src/redact.rs` first (B2: `StoreError` redacts its text when it is built, so no path sees it raw), with control characters replaced and at most 500 characters. `redact` removes tokens, keys and signatures; it keeps endpoints and bucket names on purpose, because the line is shown only to this device's own user, who configured them and needs them to fix the problem. Other errors have no `detail`. Code: `crates/engine/src/error.rs`, `crates/cli/src/output.rs`.

## Parity rule
Any feature added to the UI gets its CLI command (desktop/headless) in the same plan, and the other way round. Mobile has no CLI. Its Settings → Advanced exposes the same options.
