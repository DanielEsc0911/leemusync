---
name: leemusync-docs
description: Use when reading or changing anything under docs/ in LeemuSync, when a code change alters documented behaviour, when looking for where a feature, rule, decision or term is described, or when creating, updating or closing a progress plan.
---

# LeemuSync docs

Docs are code (rule W1). A behaviour change and its spec update ship in the same commit.

## Find things (cheapest path first)
1. `docs/index.md`: pick the doc from "Reading paths". Never bulk-read the specs.
2. Outline: `grep -n "^#" docs/specs/<file>.md`, then read only that section (offset/limit).
3. Search by keyword (`grep -rn "<term>" docs/`) before opening files.
4. Terms → `docs/glossary.md`. Rules → cite IDs from `docs/rules.md` (F1–F3, B1–B3, P1–P7, W1–W5); don't restate them. Past choices → `docs/decisions.md`.

## Change a spec
Every spec starts with this header (`cargo xtask docs-check` enforces it):
```
# Title

> **Status:** Draft|Decided|Partial|Implemented · **Related:** [rules](../rules.md) (IDs), … · **Code:** `path` or —
```
Section order: Purpose (1–3 lines) → design/behaviour → edge cases and quirks → Verify → Open questions.

- State each fact once and link to it everywhere else. Use tables for matrices and mappings.
- Link related specs both ways. Add new specs to `docs/index.md` (table row, plus a reading path if it's useful).
- Fill **Code:** with real paths once code exists. Keep Status honest.
- Unproven vendor or platform facts go under **Verify** with an ID (`V-AREA-n`) and the spike (SPn) or phase that will prove them. Once proven, move the fact into the body with its evidence or source and delete the Verify line.
- A changed decision gets a new `decisions.md` entry ("Supersedes Dn"). Never edit old entries.
- Over ~300 lines → split by topic and update the index and links.
- UI wording in English or Spanish → `docs/specs/i18n.md` §Terminology.

## Progress plans (`docs/progress/`)
- New multi-step work → `plans/<name>.md` in writing-plans format, plus a row on the board in `progress/README.md`.
- Tick checkboxes in the same commit as the work. Update the board's Status and Updated columns.
- Finished and merged → add an entry to CHANGELOG `[Unreleased]`, then delete the plan file and its board row.
- Durable results (spike outcomes, decisions) go into specs and decisions, never only into a plan.

## Before committing
`cargo xtask docs-check` is green · index updated · Status and Code current · no fact duplicated.

## Common mistakes
| Mistake | Fix |
|---|---|
| Reading whole specs "for context" | Index → outline → section |
| Restating a rule | Cite its ID |
| New spec missing from the index | Add the row (docs-check fails otherwise) |
| Spike results left in a plan | Move them to a spec or decision before closing |
| Link examples in inline code | Put them in fenced blocks (docs-check skips fences) |
