# Progress

> **Status:** Decided · **Related:** [roadmap](roadmap.md), [rules](../rules.md) (W1, W4), [git-workflow](../specs/git-workflow.md), [release](../specs/release.md#release-checklist), [skill: leemusync-docs](../../.claude/skills/leemusync-docs/SKILL.md) · **Code:** —

## How it works
- **[roadmap.md](roadmap.md):** phases, milestones and exit criteria. It changes only when scope changes.
- **`plans/<name>.md`:** one file per active plan (a whole phase, or one feature inside a phase), in the writing-plans format: goal, global constraints, tasks with checkboxes.
- **Progress is the ticked checkboxes**, committed together with the work they describe.
- **Durable knowledge never lives only in a plan.** Spike results, design changes and new decisions go into specs and `decisions.md` in the same commit (W1).
- **Cleanup:** when every task in a plan is done and merged to `main`, add its user-visible changes to `CHANGELOG.md` `[Unreleased]`, delete the plan file, and remove its board row. Git history keeps the plan.
- **Next plan:** written at the end of the previous phase, once that phase's results are known (no speculative plans, P1).

## Status board
| Plan | Phase | Status | Updated |
|---|---|---|---|
| [phase-0-foundations](plans/phase-0-foundations.md) | 0 | In progress (Tasks 1–5 done; SP7 Linux done; SP6 in progress, Google Drive live done; SP1 in progress, Fedora, Android emulator and CachyOS done; SP3 skipped until an Android device is available; next M1 sync Fedora ↔ CachyOS) | 2026-10-18 |
| [phase-1-first-sync](plans/phase-1-first-sync.md) | 1 | In progress (Tasks 1–16 done, Task 8 Step 8 live Drive run is the maintainer's; M1a: both BotW saves on both PCs through Google Drive; Tasks 18–19 are manual) | 2026-10-19 |

Status values: Not started · In progress · Blocked (reason) · Done (awaiting cleanup).
