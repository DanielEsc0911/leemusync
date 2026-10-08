# Release

> **Status:** Draft · **Related:** [rules](../rules.md) (B2, F2, P6, P7), [decisions](../decisions.md) (D16, D22, D23, D24), [security §Supply chain](security.md#supply-chain), [testing](testing.md#device-lab), [git-workflow](git-workflow.md), [progress](../progress/README.md) · **Code:** `.github/workflows/release.yml`, `packaging/`, `xtask/src/version.rs`

## Purpose
Ship verifiable builds for every platform from one pipeline, without ever shipping something half-wired (F2). Repository: `github.com/DanielEsc0911/leemusync`.

## Versioning
| Thing | Scheme |
|---|---|
| App (all platforms together) | SemVer `MAJOR.MINOR.PATCH[-pre]`, one version for every artifact |
| Repo format | Integer `format` in `repo.json`. Newer clients migrate. Older clients refuse to write and explain why |
| Profile schema, config, IPC `api_version` | Integers with tested migrations (B1) |

A release tag `vX.Y.Z[-pre]` must match `workspace.package.version` in `Cargo.toml` and `version` in `app/pubspec.yaml` (build number after `+` ignored). `cargo xtask version-check <tag>` enforces it, and the release workflow fails otherwise.

## CI (GitHub Actions)
| Workflow | Trigger | Runs |
|---|---|---|
| `ci.yml` | PR, push to `main` | fmt, clippy `-D warnings`, tests (Linux/Windows/macOS), `cargo deny`, `xtask layers/docs-check/i18n-check`, Flutter format/analyze/test |
| `release.yml` | Tag `v*`; manual (`workflow_dispatch`); PRs touching `release.yml` or `packaging/**` | Build every artifact below → `SHA256SUMS` → build-provenance attestations (tag and manual runs) → draft GitHub Release (tag), or workflow artifacts only (manual, PR) |
| `nightly.yml` | Schedule (later) | Fuzzing, Docker backend conformance, integration tests |

## Artifacts
Every desktop package contains the Flutter app plus the `leemusync` CLI and `leemusyncd` daemon. File names: `leemusync-<version>-<os>-<arch>.<ext>`.

| Format | Platform | Runner | Tool |
|---|---|---|---|
| `.msi` (one per language: `en`, `es`) | Windows x64 | `windows-latest` | WiX Toolset 5.0.2 (dotnet global tool) |
| `.exe` installer (English/Spanish selectable) | Windows x64 | `windows-latest` | Inno Setup |
| `.dmg` | macOS universal (Apple Silicon + Intel) | `macos-latest` | `hdiutil` (built into macOS) |
| `.deb`, `.rpm` | Linux x86_64, arm64 | `ubuntu-22.04`, `ubuntu-22.04-arm` | nfpm |
| `.AppImage` | Linux x86_64, arm64 | same | appimagetool |
| `.tar.gz` | Linux desktop bundle (x86_64, arm64), plus headless CLI + daemon (x86_64, aarch64, armv7; file names end in `-headless`) | same | `tar` |
| `.apk` (one per ABI: arm64-v8a, armeabi-v7a, x86_64) | Android | `ubuntu-latest` | `flutter build apk --split-per-abi` |
| `SHA256SUMS` | all | release job | `sha256sum` |

- Manual and PR runs use the version `0.0.0-dev.<run number>` in file names. Pre-release versions sort before the final release: nfpm writes `0.0.1-alpha.1` as `0.0.1~alpha.1` in both `.deb` and `.rpm` (checked locally with nfpm 2.47.0: `rpm.vercmp("0.0.1~alpha.1", "0.0.1")` = -1).
- The `.msi` `Version` is the numeric `X.Y.Z` (MSI rejects pre-release suffixes). The pre-release only appears in the file name.
- Linux builds run on the oldest supported Ubuntu runner (22.04), so the glibc baseline works on Fedora, Arch/CachyOS, Debian and Ubuntu.
- WiX builds both cultures (`en-US`, `es-ES`) from the same source with `-loc` `.wxl` files, and `<Files Include="…\**">` harvests the Flutter Release folder. Each Rust binary needs its own Component (WiX error WIX0367: a multi-file Component with an unversioned keypath can't get an auto-generated GUID).
- The Windows job's "Install Inno Setup and WiX if missing" step works on `windows-latest` (10 s); `ISCC` runs from `C:\Program Files (x86)\Inno Setup 6`. Whether the runner image already shipped either tool wasn't recorded; the install-if-missing step makes it irrelevant.
- On `ubuntu-22.04-arm` the workflow skips `subosito/flutter-action` and clones Flutter from source at the version pinned in `app/pubspec.yaml`; `flutter build linux --release` works there (70 s). Whether flutter-action itself supports linux-arm64 wasn't tested (not needed).
- appimagetool 1.9.1 runs extracted (no FUSE on runners) and builds the aarch64 `.AppImage` on `ubuntu-22.04-arm`.
- Windows arm64 artifacts are added once SP1 confirms Flutter support (V-DESK-4).
- No iOS artifact until an Apple Developer membership exists ([D23](../decisions.md#d23-distribution-before-store-accounts-2026-10-07)).

## Dry run results (2026-10-08)
Workflow `release`, [run 37789852214](https://github.com/DanielEsc0911/leemusync/actions/runs/37789852214) (PR #4, commit `feeb76f`, version `0.0.0-dev.2`): all 7 build jobs and `pr-bundle` succeeded; `publish` was skipped, as designed for PRs. The earlier run 37684498198 failed on WIX0367, fixed in `feeb76f`.

Manual install pass on the [Device lab](testing.md#device-lab):

| Format | Device | Result | By |
|---|---|---|---|
| `.rpm` | Fedora 44 x86_64 (XFCE/X11) | Installs; app opens showing "All saves synced"; `leemusync --version` → `leemusync 0.0.0` | maintainer + agent |
| `.AppImage`, `.tar.gz` | Fedora 44 x86_64 | Window maps (X11 `WM_CLASS` `io.github.danielesc0911.leemusync`); `bin/leemusync` and `bin/leemusyncd --version` → 0.0.0 | agent |
| `.deb` | Debian machine (external) | Installs and opens | maintainer |
| `.apk` (arm64-v8a, debug-signed) | Galaxy A25 | Installs and opens in English and Spanish ("Todas las partidas están sincronizadas") | maintainer |
| `.dmg` (universal) | MacBook Air M4, macOS 26 | Opens and runs | maintainer |

Findings:
- The window title and Android label said "leemusync". Fixed in PR #5 (`80dcb25`).
- On XFCE without a running `xdg-desktop-portal`, the GTK runner logs harmless `CRITICAL … Failed to read XDG desktop portal settings` lines. The app still runs.
- Not yet tested (deferred by the maintainer on 2026-10-08): V-PKG-6.

## Install layout
GUI and CLI never share a folder, because Windows and macOS file systems are case-insensitive (`LeemuSync` ≙ `leemusync`).

| Package | GUI | CLI + daemon |
|---|---|---|
| Windows `.msi` / `.exe` | `%ProgramFiles%\LeemuSync\LeemuSync.exe` + Start Menu shortcut with AppUserModelID (needed for toasts) | `%ProgramFiles%\LeemuSync\bin\` (added to `PATH`) |
| macOS `.dmg` | `LeemuSync.app` | `LeemuSync.app/Contents/Helpers/` |
| Linux `.deb` / `.rpm` | `/opt/leemusync/leemusync-gui`, symlink `/usr/bin/leemusync-gui`, desktop file + icon under `/usr/share` | `/usr/bin/leemusync`, `/usr/bin/leemusyncd` |
| `.AppImage` | AppRun → `leemusync-gui` (bundle at the image root) | `usr/bin/` inside the image |
| `.tar.gz` | `leemusync/leemusync-gui` | `leemusync/bin/` |

Display name: "LeemuSync" on every platform (window title, launcher label, Start Menu, bundle display name).

GUI executable names: `LeemuSync.exe` (Windows), `LeemuSync.app` (macOS), `leemusync-gui` (Linux). App and bundle id: `io.github.danielesc0911.leemusync` ([D22](../decisions.md#d22-app-and-bundle-id-iogithubdanielesc0911leemusync-2026-10-07)).

## Signing
| Platform | Now ([D23](../decisions.md#d23-distribution-before-store-accounts-2026-10-07)) | Later |
|---|---|---|
| Android | A release key generated by the maintainer (`keytool`), stored as GitHub secrets. **Back the keystore up offline: losing it means existing installs can never update.** PR runs never receive the key, and they and manual runs without the secrets produce debug-signed APKs named `-debug`. Only tag and manual runs get write permissions (`publish` job). PR runs use the read-only `pr-bundle` job | Play App Signing (upload key) once a Play account exists |
| macOS | Ad-hoc signed, not notarised → Gatekeeper warning. Users open it via System Settings → Privacy & Security → Open Anyway | Developer ID + notarisation (Apple Developer membership) |
| Windows | Unsigned → SmartScreen warning | Authenticode through a free open-source signing programme (Verify when applying) |
| Linux | Packages unsigned | Repository signing with Flatpak/AUR |
| Every artifact | SHA-256 in `SHA256SUMS` + GitHub build-provenance attestation (Sigstore-backed, verify with `gh attestation verify <file> --repo DanielEsc0911/leemusync`) | + CycloneDX SBOM (Phase 7) |

## Channels
| Channel | When |
|---|---|
| GitHub Releases (every format above) | From Phase 0 |
| Flathub, AUR, winget, Homebrew cask | Phase 7, or earlier on request |
| F-Droid | Phase 4 or later |
| Google Play | Once a Play developer account exists |
| App Store / TestFlight | Once an Apple Developer membership exists |

## Release checklist
1. Every progress plan in the release is done. Specs say **Implemented** where true.
2. Bump the version in `Cargo.toml` and `app/pubspec.yaml`. Move `CHANGELOG.md` `[Unreleased]` → version section.
3. CI is green. The manual device pass is done ([testing §Device lab](testing.md#device-lab)).
4. Strings complete in en and es, including installers and store listings.
5. Tag `vX.Y.Z` → `release.yml` → review the draft release (all files, `SHA256SUMS`, attestations) → publish.
6. Clean up the published plans ([progress](../progress/README.md)).

## Verify
- **V-PKG-3** The aarch64 `.AppImage` (built on `ubuntu-22.04-arm`, see [Artifacts](#artifacts)) runs on arm64 Linux hardware. No arm64 Linux device in the [Device lab](testing.md#device-lab) yet (Phase 0 Task 6 or later).
- **V-PKG-5** Whether an all-in-one packager now covers all eight formats, which would let us revisit D24 (Task 5).
- **V-PKG-6** Windows `.exe` and both `.msi`: install, Start Menu entry, `leemusync` on `PATH` in a new terminal, uninstall leaves nothing, Spanish installer UI. CachyOS: `.AppImage` and `.tar.gz` open and run. Deferred by the maintainer on 2026-10-08 (Phase 0 Task 6).
