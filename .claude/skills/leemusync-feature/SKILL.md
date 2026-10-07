---
name: leemusync-feature
description: Use when starting, implementing or finishing a LeemuSync feature or bug fix that touches more than one file, layer or platform, or before claiming any LeemuSync work is complete.
---

# Shipping a LeemuSync change

A change is done only when it works on every platform where it appears (F2), with docs, strings and tests.

## Before code
1. Find the specs: `docs/index.md` → reading path (skill `leemusync-docs`).
2. Multi-step work → a plan in `docs/progress/plans/` (writing-plans format) plus a board row.
3. Behaviour not in a spec yet → write or adjust the spec first (Status: Draft).
4. Check `docs/decisions.md`. Don't reopen a decision without new facts (W4).

## While coding
- TDD (W2): failing test → minimal code → green → refactor.
- Respect the layers (B1); `cargo xtask layers` checks them. Logic without I/O belongs in `core`.
- Touches paths, network, crypto, IPC, parsing, dependencies or process launch → skill `leemusync-security`.
- UI → `leemusync-ui`. Emulator → `leemusync-emulator-profile`. Storage → `leemusync-backend`.

## Parity checklist (copy into the PR)
- [ ] Every surface: UI ⇄ CLI ⇄ config file (P5); mobile Settings → Advanced
- [ ] Every platform where it appears: Windows, macOS, Linux, Android, iOS, headless. If it's not on one, the spec says why
- [ ] Strings in en + es (P6); `cargo xtask i18n-check`
- [ ] Status-related → status model + tray + Android notification/Live Update + iOS Live Activity (`docs/specs/ui/status-and-notifications.md`)
- [ ] Reliability invariants I1–I6 still hold (`docs/specs/reliability.md`)
- [ ] Spec(s) updated in the same commit; Status and Code fields current (W1)
- [ ] Plan checkboxes ticked; board updated

## Finish
1. Run `just check` and show the output. Never claim green without running it.
2. Commit with a Conventional Commits message: the human is the author, the AI adds a trailer:
```
feat(engine): resume interrupted restores from the journal

Co-Authored-By: Claude <noreply@anthropic.com>
```
3. Once merged and the plan is complete → CHANGELOG `[Unreleased]` entry; delete the plan and its board row.

## Red flags: stop
"Works on desktop, mobile later" in a release · a string that exists only in English · a spec left untouched after a behaviour change · "tests later" · claiming done without `just check` output.
