---
name: leemusync-emulator-profile
description: Use when adding, fixing or reviewing a LeemuSync emulator profile, when an emulator's saves are not detected or land in the wrong place, or when supporting a new emulator, platform path, Flatpak or portable install, or account layout.
---

# Emulator profiles

Profiles are data, never code (D8, B2). Spec: `docs/specs/emulator-profiles.md` (schema, Tiers, Validation). Accounts: `docs/specs/players-and-accounts.md`.

## Research (record the evidence)
For each OS you support (Windows, macOS, Linux native/Flatpak/portable, Android, iOS):
- Base folder(s), plus the config file and key that customise the path
- Save layout → `per_game_user_dir` | `per_game_dir` | `per_game_file` | `container`
- Accounts: id format, account folder, where the display name lives, shared folders (`common`)
- Game key source (title ID, serial, ROM name) and how to normalise it
- Process name(s) or Android package; launch arguments for a game
- Settings that raise the tier (per-game memory cards, folder cards) → `recommend`
- Save-state location (opt-in only)

## Write
1. `profiles/<id>.toml`. The first lines are a comment with the emulator version, OS and date verified.
2. Use only allowed template variables and relative patterns. No `..`, no executable paths.
3. Fixtures: `profiles/fixtures/<id>/<case>/` (a fake tree, never real user saves) + `expected.toml` listing the streams.
4. Test first: the fixture test fails → add the profile → it passes (`just test`).
5. Once the CLI exists: `leemusync profiles validate profiles/<id>.toml`.
6. Update the emulator table in `emulator-profiles.md` (and the platform spec's inventory). Remove the Verify lines this resolves.

## Never
Parse game save contents (D7) · download profiles at runtime · add a field that runs a program · guess a path without evidence (W3: put it under Verify).

## Quick tier check
| Layout | Tier |
|---|---|
| Per game + per account | Full |
| Per game, no accounts | Standard (player swap in launcher mode) |
| One container for many games | Basic → add a `recommend` that unlocks Standard, if the emulator has one |
