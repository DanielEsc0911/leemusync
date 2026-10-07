# Decisions

> **Status:** Decided · **Related:** [rules](rules.md), [architecture](specs/architecture.md), [product](specs/product.md) · **Code:** —

Append-only. To change a decision, add a new entry that says `Supersedes Dn`, and mark the old one `Superseded by Dm`. Format: decision · why · alternatives rejected.

### D1 Rust for the engine, daemon and CLI (2026-10-07)
Memory safety (B2), native performance and small binaries (B3, P7), and one codebase that compiles for every target, including Android, iOS and ARM Linux. Rejected: Go (weaker mobile story), C++ (memory safety), Kotlin Multiplatform (weak headless/Pi story).

### D2 Flutter for every UI (2026-10-07)
Flutter draws every pixel itself, so the look is identical everywhere (F1). Impeller gives smooth animation on mobile (F2). One Dart codebase means one navigation implementation (F3). Rust is reached through flutter_rust_bridge. This replaces the early chat suggestion of Tauri or native UIs. Rejected: Tauri (the webview differs per OS; WebKitGTK performance on Linux), Compose Multiplatform (JVM on desktop is heavy), Slint (iOS support immature), three native UIs (they would drift apart, breaking F1 and F3).

### D3 Desktop: headless daemon owns sync and tray; UI is a client (2026-10-07)
Sync must run 24/7 without the UI open (B3) and stay light (P7). `leemusyncd` runs as a user-level service and owns the tray, notifications and engine. The Flutter app connects over same-user IPC. Headless installs run the same daemon without a tray.

### D4 Mobile: one engine in the app process, two thin bindings (2026-10-07)
On Android and iOS, background work, notifications, Live Updates and Live Activities are native (Kotlin/Swift) and must work with no Flutter UI running (B3). The single `leemusync-bridge` cdylib exports flutter_rust_bridge for Dart **and** UniFFI for Kotlin/Swift, so the process has exactly one engine instance. Rejected: headless Flutter engines for background work (heavier, more fragile). Verify in spike SP1 that both bindings work from one library.

### D5 Append-only, content-addressed repo with derived heads (2026-10-07)
Objects get unique names and are never overwritten, so correctness doesn't depend on atomic compare-and-swap. That's required to support Google Drive, WebDAV and similar backends. Divergence is detected from the parent links. See [sync-model](specs/sync-model.md).

### D6 Leases are advisory; divergence detection is the safety net (2026-10-07)
Not every backend can provide a perfect distributed lock. Leases give the UX ("in use on Desktop"); heads and parents guarantee nothing is lost.

### D7 No game save-format parsing in core (2026-10-07)
Conflicts are compared by metadata (device, time, session length, changed files, size). This works for every game and avoids parsing untrusted binary data (B2, P3). Narrow emulator *metadata* readers (e.g., an index that maps opaque folders to title IDs) are allowed only as small, fuzzed parsers in core, each with its own decision entry.

### D8 Emulator profiles are declarative TOML, shipped inside releases (2026-10-07)
No scripts, plugins or remote downloads (B2). Users may add local custom profiles. Community profiles arrive through reviewed PRs and signed releases.

### D9 Capability tiers instead of an allow-list (2026-10-07)
Every emulator gets the safety features. The engine derives Full, Standard or Basic from the profile's layout ([emulator-profiles §Tiers](specs/emulator-profiles.md#tiers)).

### D10 Storage through Apache OpenDAL, plus custom adapters where needed (2026-10-07)
OpenDAL covers S3, WebDAV, SFTP, Google Drive, OneDrive, Dropbox, local filesystem and more, with conditional-write options. MEGA isn't in OpenDAL; options are in [storage-backends](specs/storage-backends.md#mega).

### D11 End-to-end encryption is mandatory (2026-10-07)
Having one mode is simpler (P1) and safer (B2). Users can't browse saves on the storage directly; they restore through LeemuSync.

### D12 SQLite for local state and the journal (2026-10-07)
Battle-tested crash safety (WAL), available on every target, bundled in the binary.

### D13 No server, telemetry or accounts in v1; push only optional and later (2026-10-07)
P2, B2. Lock warnings come from devices' own checks. UnifiedPush (self-hosted ntfy) on Android and an optional relay are Phase 6+ ideas.

### D14 Launcher mode is primary; watch mode is the fallback (2026-10-07)
Launching the emulator is the only way to reliably know when a session starts and ends on every OS, especially mobile.

### D15 Save states are opt-in (2026-10-07)
They're large and tied to the emulator version. They're stored as a separate stream kind.

### D16 License MPL-2.0; hosting GitHub (2026-10-07)
MPL-2.0 is file-level copyleft and compatible with the App Store and Play Store. GitHub provides free macOS, Windows, Linux and ARM runners, which iOS and macOS builds need.

### D17 Spanish is neutral Latin American (`es`) (2026-10-07)
Uses `tú`, no `vosotros`, and neutral vocabulary. One Spanish locale.

### D18 Flutter app conventions: Riverpod (no codegen) and go_router (2026-10-07)
Riverpod handles the async streams that come from Rust. go_router's `StatefulShellRoute` gives one navigation tree with adaptive containers (F3). No code generation (P1).

### D19 i18n sources: ARB for Dart, Fluent for Rust, native files for extensions (2026-10-07)
Each layer uses its platform-standard format, and `cargo xtask i18n-check` enforces en/es parity across all of them. See [i18n](specs/i18n.md).

### D20 "Player" is the term for a person (2026-10-07)
Used in code, docs and UI (`es`: "jugador"). One term everywhere avoids translation drift.

### D21 Encryption from RustCrypto/BLAKE3 primitives, not the `age` file format (2026-10-07)
The early chat suggested `age`. The repo needs AEAD with associated data bound to each object's kind and id, plus keyed content ids. `age` provides neither. The construction in [security §Cryptography](specs/security.md#cryptography) uses audited primitives only, with no custom algorithms.

### D22 App and bundle id `io.github.danielesc0911.leemusync` (2026-10-07)
The project has no domain. A reverse-DNS id under the maintainer's GitHub namespace is unique to them and valid on Android, iOS, macOS, Windows and Linux (lowercase). It's only permanent after the first store publication, so it can still change before then if a domain is bought.

### D23 Distribution before store accounts (2026-10-07)
There's no Apple Developer or Google Play account yet. GitHub Releases ship `.apk` (self-generated release key), `.msi`, `.exe`, `.dmg` (ad-hoc signed), `.deb`, `.rpm`, `.AppImage` and `.tar.gz`, each with SHA-256 checksums and GitHub build-provenance attestations. iOS is limited to developer testing on the maintainer's own device until a membership exists. Refines D16. Details: [release](specs/release.md).

### D24 Standard per-format packaging tools (2026-10-07)
WiX (`.msi`), Inno Setup (`.exe`), `hdiutil` (`.dmg`), nfpm (`.deb`/`.rpm`), appimagetool (`.AppImage`), `tar`. Each is the de-facto tool for its format, and the release path keeps few third-party layers (B2). Rejected for now: all-in-one packagers (e.g. fastforge, cargo-packager). None was confirmed to cover all eight requested formats, and each adds a layer to the supply chain. Revisit if V-PKG-5 finds one that does.
