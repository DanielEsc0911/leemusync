# Changelog

All notable changes to this project. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- The project is renamed from LeemuSync to LemuSync (lemur + emu + sync). Binaries, app id, env vars, local folders and the remote storage prefix change with no migration ([D33](docs/decisions.md#d33-the-project-is-named-lemusync-2026-10-27)).

### Added
- `lemusync games` lists the games installed on this device with their names; `play` finds a game's path by itself (Cemu's own game folders, or `emulators.<id>.game_dirs`), and `status`, `history`, `conflicts` and `play` show game names next to their ids.
- `lemusync config path|list|get|set|unset` reads and changes `config.toml` from the CLI, keeping comments; managed keys (`repo`, `backend`, `grants`) point to the command that owns them.
- While storage requests are retried, the CLI says "Connection trouble, retrying…" and then "Connection is back." (en/es); the engine sends the same `ConnectionEvent` for the daemon and tray.
- Storage errors are friendly and localized (`backend_offline`, `backend_quota`, `backend_auth`, `backend`), with the redacted provider text on a `Details:` line and in `--json` `error.detail`.
- Synthetic network test suite for SC5: short and long outages and a slow link on upload and download, on top of the cut and lost-response matrices.
- First sync between two PCs through Cloudflare R2 (CLI): both players' Cemu saves on both PCs, with live progress while syncing.
- Project rules (`AGENTS.md`, `docs/rules.md`), documentation set, roadmap and Phase 0 plan.
