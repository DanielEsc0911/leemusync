# Git workflow

> **Status:** Decided · **Related:** [rules](../rules.md) (W1, W2, W5), [release](release.md), [progress](../progress/README.md), [CONTRIBUTING](../../CONTRIBUTING.md) · **Code:** —

## Branches
- `main` is always releasable and protected (PR + green CI).
- Work branches: `feat/<slug>`, `fix/<slug>`, `docs/<slug>`, `chore/<slug>`, `spike/<id>-<slug>`.

## Commit messages
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): `type(scope): summary`.
- **Types:** `feat` `fix` `docs` `test` `refactor` `perf` `build` `ci` `chore` `spike`.
- **Scopes:** crate or area names: `core` `crypto` `store` `api` `engine` `ipc` `daemon` `cli` `bridge` `app` `android` `ios` `profiles` `docs` `xtask`.
- Imperative summary of 72 characters or fewer. The body explains *why*. Reference rules by ID when relevant.

## Authorship
- **Author:** the human maintainer directing the work (their git identity).
- **AI co-author:** every commit an AI agent helped write ends with a trailer naming the agent:
  ```
  Co-Authored-By: Claude <noreply@anthropic.com>
  ```
  Other agents use their own name and no-reply address. Add any session trailer the agent's tool requires (for example Claude Code's `Claude-Session:` line) after it.
- Human contributors co-authoring with each other use the same trailer.

## What goes in a commit
- Code + tests + spec updates + progress-plan checkbox updates, together (W1).
- `just check` passes (W2).
- Never: secrets, generated build output, personal paths, real user save files.

## Pull requests
Checklist in the PR description:
- [ ] Spec(s) updated; index updated if a spec was added
- [ ] Tests first; `just check` green
- [ ] en + es strings
- [ ] UI changes: goldens reviewed for every size class
- [ ] Security-relevant change: `leemusync-security` checklist done
- [ ] Progress plan updated
