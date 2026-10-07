# Emulator profiles

> **Status:** Draft · **Related:** [rules](../rules.md) (P3, B2), [decisions](../decisions.md) (D7, D8, D9), [players-and-accounts](players-and-accounts.md), [sync-model](sync-model.md), [security](security.md#file-system-confinement), [platforms](platforms/desktop.md) · **Code:** `profiles/` (planned)

## Purpose
Describe, as data, where each emulator keeps saves and how to recognise games, so the engine can sync any emulator safely. Profiles are TOML, ship inside releases, and never contain code ([D8](../decisions.md#d8-emulator-profiles-are-declarative-toml-shipped-inside-releases-2026-10-07)). Only save folders are synced, never an emulator's whole data folder (e.g. not Cemu's `mlc01` with its updates and DLC).

## Layouts
| Layout | Meaning | Examples (Verify) |
|---|---|---|
| `per_game_user_dir` | A folder per game per emulator account | Cemu, RPCS3, Yuzu-family forks |
| `per_game_dir` | A folder per game, no accounts | Dolphin Wii NAND, PPSSPP `SAVEDATA`, Azahar |
| `per_game_file` | A file per game | RetroArch `.srm`, melonDS `.sav`, mGBA, DuckStation per-game cards |
| `container` | One file holds many games | PS1/PS2 shared memory cards, Dolphin GameCube `.raw` cards |

## Tiers
The engine derives the tier. Profiles don't declare it ([D9](../decisions.md#d9-capability-tiers-instead-of-an-allow-list-2026-10-07)).

| Tier | When | Gets |
|---|---|---|
| **Full** | `per_game_user_dir` | Per-game + per-player streams via account mapping, per-game leases, launcher handoff |
| **Standard** | `per_game_dir` / `per_game_file` | Per-game streams and leases. Per-player saves through swapping ([players-and-accounts](players-and-accounts.md#single-user-emulators-standard-tier)) |
| **Basic** | `container`, or game folders with opaque names | One stream and lease for the whole container. Suggests a setting that unlocks Standard |

**Every tier** gets version history, conflict detection, leases, atomic restore with backup, and E2E encryption.

## Schema (v1)
| Field | Type | Meaning |
|---|---|---|
| `schema` | int | Profile schema version (`1`) |
| `id` | string `[a-z0-9-]+` | Stable id. Never renamed |
| `name` | string | Display name (proper noun, not translated) |
| `systems` | [string] | Console ids, e.g. `wiiu`, `ps2` |
| `layout` | enum | See [Layouts](#layouts) |
| `requires_closed` | bool, default `true` | Never snapshot or restore while the emulator runs |
| `platforms.<os>.roots` | [template] | Candidate base folders, in priority order (`os`: windows, macos, linux, android, ios) |
| `platforms.<os>.process` | [string] | Process names for detection (desktop) |
| `platforms.android.package` | string | Package name for launching and detection |
| `config_keys` | [{file, format, key, var}] | Read a user-customised path from the emulator's config (`format`: xml, ini, json) into variable `var` |
| `saves.root` | template | Save root under the base folder |
| `saves.pattern` | template | Path per stream, with captures `{title_id}`, `{account}`, `{game}` |
| `saves.shared` | [template] | Folders shared by all accounts |
| `saves.include` / `saves.exclude` | [glob] | Files that belong to a stream |
| `saves.max_file_mb` | int, default `64` | Size guard |
| `accounts` | {root, id_regex, name_file?, name_key?} | Account discovery (multi-user emulators) |
| `game_key` | {from, normalize[]} | Which capture identifies the game, and how to normalise it |
| `compat` | [string] | Compat groups whose files are interchangeable |
| `states` | {pattern, include} | Save states (opt-in, [D15](../decisions.md#d15-save-states-are-opt-in-2026-10-07)) |
| `recommend` | [{setting, value, unlocks, how}] | Emulator settings that raise the tier, shown in the UI |
| `launch.args` | [template] | Arguments to start a game. The executable is always chosen by the user |

### Templates
Allowed variables: `{home}` `{xdg_data}` `{xdg_config}` `{appdata}` `{localappdata}` `{documents}` `{mac_app_support}` `{flatpak:<app-id>}` `{android_storage}` `{exe_dir}`, plus `config_keys` variables and pattern captures. Anything else is a validation error.

### Example (illustrative; paths are Verify SP7)
```toml
schema = 1
id = "cemu"
name = "Cemu"
systems = ["wiiu"]
layout = "per_game_user_dir"
requires_closed = true

[platforms.linux]
roots = ["{xdg_data}/Cemu", "{flatpak:info.cemu.Cemu}/data/Cemu"]
process = ["Cemu"]

[platforms.windows]
roots = ["{exe_dir}", "{appdata}/Cemu"]
process = ["Cemu.exe"]

[[config_keys]]
file = "settings.xml"
format = "xml"
key = "content/mlc_path"
var = "mlc"

[saves]
root = "{mlc}/usr/save"            # falls back to "{base}/mlc01/usr/save"
pattern = "00050000/{title_id}/user/{account}"
shared = ["00050000/{title_id}/user/common"]

[accounts]
root = "{mlc}/usr/save/system/act"
id_regex = "^8[0-9a-f]{7}$"

[game_key]
from = "title_id"
```

## Validation
Rejected at load time, with a clear error. Enforced in `core` and fuzzed:
- Unknown fields (`deny_unknown_fields`), wrong types, unsupported `schema`.
- Template variables not on the allow list. Absolute paths or `..` in patterns.
- Regexes over the size limit. Regexes use the linear-time `regex` crate, which can't backtrack catastrophically.
- Any field that would name an executable. The emulator executable is always picked by the user.

## Discovery
1. For each profile on the current OS: expand `roots` in order. The first that exists is the base.
2. Apply `config_keys` (the user's custom paths inside the emulator's own config).
3. Apply user overrides ([Overrides](#overrides)).
4. Propose grants for the resolved folders. Nothing is read until the user grants it ([security](security.md#file-system-confinement)).
5. Enumerate streams from `saves.pattern` and accounts.

## Overrides
Per device: base path, extra roots, include/exclude tweaks, disable a profile. **Local custom profiles** (same schema) live in `<config dir>/profiles/` and are trusted because the user created them. Both are editable in UI → Settings → Emulators → Advanced, in the CLI, and in the config file ([config-and-cli](config-and-cli.md)).

## Initial set (Phase 1)
| Emulator | Layout → tier | Platforms (Verify SP3/SP5/SP7) |
|---|---|---|
| Cemu | `per_game_user_dir` → Full | Windows, Linux, macOS (Android port: verify) |
| RetroArch | `per_game_file` → Standard (honours "sort saves by core") | All, including Android and iOS |
| Dolphin | Wii: `per_game_dir` → Standard. GC: `container` (`.raw`) → Basic, or GCI folders → Standard | Windows, Linux, macOS, Android |
| DuckStation | Shared card → Basic. Per-game cards → Standard (recommended setting) | Windows, Linux, macOS, Android |
| PCSX2 | `.ps2` file → Basic. Folder memory cards → Standard (recommended setting) | Windows, Linux, macOS |

These five cover every layout and both ways of handling players.

## Known special cases
- **Ryujinx and forks (Switch):** save folders have opaque names, and only an internal index maps them to title IDs → Basic until an index reader exists (its own decision entry per [D7](../decisions.md#d7-no-game-save-format-parsing-in-core-2026-10-07)).
- **Portable installs:** `{exe_dir}` root. The executable path comes from the user's launcher setup.
- **Flatpak / Snap / AppImage / EmuDeck / RetroDECK:** listed as extra roots.
- **Android `Android/data`:** not readable by other apps on modern Android ([platforms/android](platforms/android.md#storage-access)). The profile's `recommend` tells users how to move the emulator's folder.

## Authoring and tests
- Fixture trees: `profiles/fixtures/<id>/<case>/`, with `expected.toml` listing the streams the scan must find.
- Each profile file starts with a comment recording the verification evidence: emulator version, OS, date.
- Workflow: the `leemusync-emulator-profile` skill.

## Verify
- **V-PROF-1** Desktop paths, config keys and process names for the initial set (spike SP7).
- **V-PROF-2** Android save locations and accessibility for each emulator (SP3).
- **V-PROF-3** Which iOS emulators expose saves in the Files app (SP5).
