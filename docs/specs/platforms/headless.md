# Headless: Raspberry Pi, servers, Linux handhelds

> **Status:** Draft · **Related:** [rules](../../rules.md) (B3, P5, P7), [config-and-cli](../config-and-cli.md), [reliability](../reliability.md#always-on-247), [security §Credentials](../security.md#credentials) · **Code:** `crates/daemon`, `crates/cli` (planned)

## Purpose
Run LeemuSync on devices without a desktop session: always-on boxes, emulation consoles and handhelds.

## Targets
| Target | Build | UI |
|---|---|---|
| Raspberry Pi OS 64-bit (Pi 3/4/5, Zero 2 W) | `aarch64-unknown-linux-gnu` | CLI; Flutter app if a desktop is present |
| Raspberry Pi OS 32-bit | `armv7-unknown-linux-gnueabihf` | CLI only (Flutter doesn't ship linux-arm32) |
| Generic Linux servers | x86_64 / aarch64 | CLI |
| Linux handheld firmwares (Batocera, Knulli, ROCKNIX, muOS, ArkOS) | Static musl builds (Verify per firmware) | CLI. Later idea: on-device menu integration |

## Setup
1. Install the binary (package or tarball).
2. `leemusync init` or `leemusync join` (interactive or flags).
3. `leemusync grant add <folder>` for each emulator folder.
4. `leemusync service install` → systemd user unit + `loginctl enable-linger`. A system unit with a dedicated user is also supported.

## Constraints
- **Secrets:** a Secret Service is usually absent, so the user explicitly opts into a `0600` file ([security §Credentials](../security.md#credentials)).
- **SD-card wear:** SQLite WAL with batched commits. Local history kept small by default on these targets.
- **Memory:** daemon budgets from [reliability §Budgets](../reliability.md#budgets) apply; Argon2 parameters are checked on a Pi Zero 2 W (V-SEC-1).

## Not in scope
LeemuSync doesn't host storage. A Pi can *be* the storage by running other software (MinIO, OpenSSH for SFTP, Nextcloud, `rclone serve webdav`). A how-to guide is planned with Phase 6.
