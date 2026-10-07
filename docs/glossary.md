# Glossary

> **Status:** Decided · **Related:** [sync-model](specs/sync-model.md), [players-and-accounts](specs/players-and-accounts.md), [emulator-profiles](specs/emulator-profiles.md), [i18n §Terminology](specs/i18n.md#terminology) · **Code:** —

The same term is used in code, docs and UI. Spanish UI terms are in [i18n §Terminology](specs/i18n.md#terminology).

| Term | Meaning |
|---|---|
| **Account mapping** | Per device: which emulator account id belongs to which player (e.g., Cemu `80000002` = Ana on Desktop) |
| **Backend** | A storage adapter (S3, WebDAV, SFTP, local, Google Drive…) implementing the store interface |
| **Base** | The version a device's local files correspond to for a stream |
| **Blob** | Encrypted, content-addressed file content in the repo |
| **Capability** | What a backend supports (e.g., create-only writes, server timestamps) |
| **Compat group** | Emulators/cores whose save files are interchangeable (e.g., a standalone emulator and its RetroArch core) |
| **Conflict** | A stream with more than one head. The player chooses; nothing is merged or lost |
| **Container** | One save file holding many games (e.g., a PS2 memory card `.ps2`) |
| **Daemon** | `leemusyncd`, the always-on desktop/headless process that runs the engine, tray and notifications |
| **Device** | One LeemuSync install: random device id, user-chosen name, default player |
| **Emulator profile** | Declarative TOML describing where an emulator keeps saves and how to recognise games |
| **Engine** | The Rust sync engine (`crates/engine`): scanning, snapshots, transfers, leases, journal |
| **Game key** | Stable game identity from the profile (title ID, serial, or normalised ROM name) |
| **Grant** | A folder the user allowed LeemuSync to read and write. All writes stay inside grants |
| **Head** | A version that no other version lists as a parent |
| **Journal** | Crash-safe intent log in the local state DB. Unfinished operations resume or roll back |
| **Launcher mode** | LeemuSync starts the emulator: pull → lease → launch → wait → push → release |
| **Layout** | How a profile stores saves: `per_game_user_dir`, `per_game_dir`, `per_game_file`, `container` |
| **Lease** | Advisory lock with heartbeat and expiry. Says "this stream is being played on device X" |
| **Local history** | On-device copies of recent snapshots and pre-restore backups |
| **Manifest** | Encrypted description of a version: files, blobs, parents, device, player, metadata |
| **Play session** | One period of playing a stream on a device, from launch to final upload |
| **Player** | A person in the repo. Each player has their own streams |
| **Quiet period** | Time with no file writes before a snapshot is taken (default 10 s) |
| **Repo** | The user's LeemuSync data on their storage: `repo.json`, versions, blobs, leases |
| **Repo config** | Shared settings (players, mappings, game aliases) stored as a special stream |
| **Save state** | Emulator memory snapshot. A separate, opt-in stream kind |
| **Snapshot** | Consistent copy of a stream's files taken locally, which becomes a version when uploaded |
| **Spike** | A timeboxed experiment that proves or disproves a **Verify** item |
| **Stream** | Unit of sync: (player, game key, kind, compat group) |
| **Tier** | What an emulator's layout allows: Full, Standard, Basic ([emulator-profiles §Tiers](specs/emulator-profiles.md#tiers)) |
| **Version** | Immutable snapshot of a stream in the repo, with parent links |
| **Watch mode** | LeemuSync detects changes itself (file watching, process detection) without launching the emulator |
