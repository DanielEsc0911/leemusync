# Changelog

All notable changes to this project. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- The project is renamed from LeemuSync to LemuSync (lemur + emu + sync). Binaries, app id, env vars, local folders and the remote storage prefix change with no migration ([D33](docs/decisions.md#d33-the-project-is-named-lemusync-2026-10-27)).

### Added
- First sync between two PCs through Cloudflare R2 (CLI): both players' Cemu saves on both PCs, with live progress while syncing.
- Project rules (`AGENTS.md`, `docs/rules.md`), documentation set, roadmap and Phase 0 plan.
