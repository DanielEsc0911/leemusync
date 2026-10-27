# Network resilience: synthetic SC5 suite, connection notice, friendly storage errors

> **For agentic workers:** follow the project skills `lemusync-feature` (always), `lemusync-backend` (Tasks 1–3) and `lemusync-security` (Task 1). Steps use checkbox (`- [ ]`) syntax.

**Goal:** turn the M1b SC5 spot check into synthetic tests that cover the wider picture: cuts, lost responses, short and long outages and slow links, on upload, download and a whole `play` session. While the store retries, tell the user the connection is having trouble. When it gives up, show a friendly, localized error instead of the raw OpenDAL text.

**Branch:** `feat/network-resilience`, from the previous branch in the chain.

**Already true (don't redo):**
- The retry layer wraps `get/put/list/stat/delete`, `list` included (`crates/store/src/lib.rs` ~225, `retry.rs`).
- A kill -9 matrix covers every failpoint, including each restore step (`crates/engine/tests/kill.rs`).
- `faults.rs` covers a cut or lost response at every request of one sync (`crates/engine/tests/faults.rs` ~140).
- The fault wrapper is `crates/store/src/faults.rs`.

**Spec:** [reliability](../../specs/reliability.md) §Restore, §Upload, SC5; [storage-backends](../../specs/storage-backends.md) §Interface (error classes); [config-and-cli](../../specs/config-and-cli.md) §Exit codes; [testing](../../specs/testing.md) §Layers.

## Global constraints

- Everything in the [first-sync plan's Global constraints](phase-1-first-sync.md#global-constraints) and the [phase-1b-play Global constraints](phase-1b-play.md#global-constraints) still holds: toolchain, clippy pedantic as errors, layers (B1), `core` has no I/O/clock/randomness, I1–I6, secrets, no real saves in tests, W1, W2.
- Formats stay v1. New fields are additive with `#[serde(default)]`; `API_VERSION` and `config_version` don't change.
- Every user-facing string goes into `locales/{en,es}/lemusync.ftl` (neutral Latin American Spanish, terminology from `docs/specs/i18n.md` §Terminology). `cargo xtask i18n-check` must pass.
- Anything read from the emulator, the repo or the disk is data, never code (B2). Cap the size, reject path escapes, strip control characters from anything printed.
- W3: a vendor or emulator fact without evidence goes under the spec's **Verify** with an ID.
- W5: Conventional Commits. The author is the configured git user (Daniel). The only trailer is `Co-Authored-By: Claude <noreply@anthropic.com>`. No session URLs, no `Claude-Session:` trailer, no "Generated with" lines. Never push.
- Every Bash call starts with `export PATH="$HOME/.local/bin:$HOME/development/flutter/bin:$HOME/.cargo/bin:$PATH" && `.
- Daniel's real emulator data (`~/.var/app/info.cemu.Cemu`, `~/.var/app/org.DolphinEmu.dolphin-emu`, `~/.config/lemusync`, `~/.local/share/lemusync`) is read-only. Never launch his emulators and never copy saves or `account.dat` values.
- The first commit on the branch adds this plan file and its board row in `docs/progress/README.md` (`docs(progress): plan <name>`). Each task ticks its own checkboxes in its own commit. The last task adds the CHANGELOG `[Unreleased]` entry and sets the board row to "Done (awaiting merge)". The plan file is deleted after merge, not on this branch.
- Each task ends with `just check` green, then its commit.

---

### Task 1: Synthetic network suite

**Why:** the M1b test was one cable pull. B3 needs a repeatable matrix.

**Files:** `crates/store/src/faults.rs`, `crates/engine/tests/faults.rs` (or a new `network.rs` beside it if `faults.rs` passes ~400 lines), spec `testing.md` and `reliability.md` (an SC5 coverage table: scenario × direction × test name).

- [x] **Step 1: list what's already covered.** Read `faults.rs` and `kill.rs`. Write the coverage table in `reliability.md` §SC5 with what exists, and mark the gaps.
- [x] **Step 2: fault primitives.** Add to `Faults`, test-only behind the existing `faults` feature: a per-request delay (slow link), and an outage that heals by itself after K failed requests. Unit-test both in `store`.
- [x] **Step 3: failing tests, then the fixes they expose.** Fill only the gaps from Step 1. The target matrix, all on `Store::fs` with the fast test retry policy:
  - a download-only sync and a full `play` session (lease → download → fake emulator → upload → release; reuse the fake-emulator helper the existing play tests use, found with `grep -rn "fn \|exe" crates/engine/tests/common* crates/engine/tests/play*`), each at every request N × {cut, lost response}. Then heal and run again. Expect: no corruption, files byte-identical to either the old or the new version, empty journal, no spurious conflict, lease released or expired.
  - an outage shorter than the retry budget on upload and on download: the operation completes with no error. This is what Daniel saw.
  - an outage longer than the budget: exit code 6 (Transient), and the next run converges.
  - a slow link (a few ms per request) on upload and download: it completes, with the same result as a fast run.
  - a store-level unit test that `list` is retried, if one doesn't exist yet.

  Keep the whole suite under ~30 s on this machine. Each scenario runs at most 12 request indices N: all of them when the run has ≤ 12 requests, otherwise a sample from a fixed seed that always includes the first and last request.
- [x] **Step 3 outcome:** no fixes were needed; every new case passed first time. The new cases live in `crates/engine/tests/network.rs`. The existing cut/lost-response matrices already run every N and stay exhaustive (each round makes ≤ 20 requests, under 3 s).
- [x] **Step 4:** fill in the coverage table, run `just check`, commit `test(engine): synthetic network fault suite for SC5`.

### Task 2: "Connection trouble" notice while retrying

**Why:** when a request is being retried, the CLI goes quiet for up to the whole retry budget.

**Files:** `crates/store/src/retry.rs` and `lib.rs` (an observer hook), the engine progress plumbing that the CLI already reads (`crates/cli/src/progress.rs`), `locales/{en,es}/lemusync.ftl`, specs `reliability.md` and `config-and-cli.md`.

- [x] **Step 1: failing test.** A store with a scripted transient fault reports one notice (operation, attempt, max attempts, delay) to an observer set on the `Store`, and a recovery notice when a later request succeeds.
- [x] **Step 2: implement.** The observer is a plain `Arc<dyn Fn(RetryNotice) + Send + Sync>` on `Store` (no new crate). The engine forwards notices as progress events. **That event is the contract**: the CLI is one consumer, and the daemon/tray (P4) will be another. The CLI shows nothing for a single quick retry. It prints one line to stderr from the second retry of an outage on (en "Connection trouble, retrying…" / es "Problemas de conexión, reintentando…"), then "Connection is back." once a request succeeds. That's at most those two lines per outage. In `--json` mode the CLI prints nothing extra, because it doesn't stream JSON events today.
- [x] **Step 2 outcome:** the engine side is `Engine::set_connection_sink` with a new additive `ConnectionEvent` in `api` (a separate sink beside `set_progress`, so `ProgressSink` keeps its signature).
- [x] **Step 3:** a CLI test (the notice state machine fed the events a fault makes; the fault → event path is tested in `crates/engine/tests/network.rs`), specs, `just check`, commit `feat(cli): tell the user when the connection is being retried`.

### Task 3: Friendly, localized storage errors

**Why:** a network cut printed the raw OpenDAL error (M1b test day).

**Files:** `crates/engine/src/error.rs` (`code()`), `crates/cli/src/output.rs`, `locales/{en,es}/lemusync.ftl`, specs `config-and-cli.md` §Exit codes (add an error-code table) and `storage-backends.md` (link to it).

- [x] **Step 1: failing tests.** Each `ErrorClass` maps to its own code: Transient → `backend_offline`, Quota → `backend_quota`, Auth → `backend_auth` (exists), NotFound/AlreadyExists/Permanent → `backend`. Exit codes stay as they are.
- [x] **Step 2: messages.** Each code gets an en and es message that says what happened and what to do, with no raw text. Only say "your saves are safe" where B3 actually guarantees it (a failed upload or download never touches local saves; check the restore path before claiming it). Don't add a `--verbose` flag (P1). The raw detail goes on one dim second line, `Details: …`, and into the `--json` output as a `detail` field next to `code` and `message`. It **always** goes through `crates/store/src/redact.rs` first (B2: OpenDAL text can contain endpoints, bucket names and signed query strings). Test it with a fake token in a URL.
- [x] **Step 2 outcome:** `redact` already scrubs signed query strings and tokens, not endpoints or bucket names; the detail line is shown only to the device's own user, so those stay. `error-backend_auth` lost its inline detail (it moved to the Details line). The CLI unit tests use `lemusync-store` as a dev-dependency to build `StoreError`s.
- [x] **Step 3:** CLI tests for the human and JSON output, specs, `just check`, commit `feat(cli): friendly storage errors in English and Spanish`.

### Task 4: Close out

- [ ] In [phase-1b-play](phase-1b-play.md), mark the open items as done with a link to this plan: "SC5 at every journal step", "raw OpenDAL error", "retries cover list". Update the board row for phase-1b-play. CHANGELOG, then commit `docs(progress): close the M1b network follow-ups`.
