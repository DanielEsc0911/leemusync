# Release

> **Status:** Draft · **Related:** [rules](../rules.md) (B2, F2, P7), [security §Supply chain](security.md#supply-chain), [testing](testing.md), [git-workflow](git-workflow.md), [progress](../progress/README.md) · **Code:** `.github/workflows/`, `packaging/` (planned)

## Purpose
Ship signed, reproducible builds for every platform without ever shipping something half-wired (F2).

## Versioning
| Thing | Scheme |
|---|---|
| App (all platforms together) | SemVer `MAJOR.MINOR.PATCH`, one version for every artifact |
| Repo format | Integer `format` in `repo.json`. Newer clients migrate. Older clients refuse to write and explain why |
| Profile schema, config, IPC `api_version` | Integers with tested migrations (B1) |

## CI (GitHub Actions)
| Workflow | Trigger | Runs |
|---|---|---|
| `ci.yml` | PR, push to `main` | fmt, clippy `-D warnings`, tests (Linux/Windows/macOS), `cargo deny`, `xtask layers/docs-check/i18n-check`, Flutter format/analyze/test |
| `nightly.yml` | Schedule | Fuzzing, Docker backend conformance, mobile builds, integration tests |
| `release.yml` | Tag `v*` | Build matrix → sign → checksums + SBOM + Sigstore → draft GitHub Release |

## Targets
Windows x64/arm64 (MSIX + portable zip) · macOS universal (notarised DMG) · Linux x86_64/arm64 (Flatpak + tarball; `.deb`/`.rpm` for the daemon) · Linux armv7 (headless daemon/CLI) · Android (AAB for Play, APK for GitHub/F-Droid) · iOS (App Store / TestFlight).

## Channels (planned)
GitHub Releases · Flathub · winget · Homebrew cask · F-Droid · Google Play · App Store. Each one has a short guide under `packaging/` when it's added.

## Signing
- Apple: Developer Program membership needed for iOS distribution and macOS notarisation (a maintainer cost to plan for).
- Windows: Authenticode, for example through a free code-signing programme for open-source projects (Verify when applying).
- Android: an upload key held by the maintainer. F-Droid signs its own builds.
- Every artifact: SHA-256 checksums plus a Sigstore signature.

## Release checklist
1. Every progress plan in the release is done. Specs say **Implemented** where true.
2. `CHANGELOG.md` `[Unreleased]` → version section.
3. CI and nightly are green. The manual device pass is done ([testing](testing.md)).
4. Strings complete in en and es, store listings included.
5. Tag `vX.Y.Z` → `release.yml` → review the draft → publish.
6. Clean up the published plans ([progress](../progress/README.md)).
