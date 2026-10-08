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
| [phase-0-foundations](plans/phase-0-foundations.md) | 0 | In progress (Tasks 1–4 done; Task 5 Steps 1–8 done, next Steps 9–10 with the maintainer) | 2026-10-07 |

Status values: Not started · In progress · Blocked (reason) · Done (awaiting cleanup).
