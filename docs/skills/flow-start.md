---
title: /flow-start
nav_order: 1
parent: Skills
---

# /flow-start

**Phase:** 1 — Start

**Usage:** `/flow-start <prompt>`, `/flow-start --auto <prompt>`, or `/flow-start --manual <prompt>`

**Example:** `/flow-start app payment webhooks`

**Auto mode example:** `/flow-start --auto fix login timeout when session expires`

Begins a new feature. This is always the first command run for any piece of work. It sets up an isolated environment, ensures dependencies are current, and establishes the PR before any feature code is written.

**Prerequisite:** `/flow-prime` must be run once per project (and again after each FLOW upgrade) before `/flow-start` will work. The setup script checks for a matching version marker at `.flow.json`.

---

## What It Does

1. **start-init** — acquires start lock, runs version gate and upgrade check, creates early state file via `init-state`, labels referenced issues with "Flow In-Progress" (concurrent starts poll via `/loop` every 15 seconds until the lock is released)
2. **start-gate** — pulls latest main, runs `bin/flow ci` baseline with retry (3 attempts), updates dependencies, runs post-deps CI with retry if deps changed. Falls back to ci-fixer sub-agent for dep-induced breakage
3. **start-workspace** — creates worktree, opens PR, backfills state file with PR fields, releases the start lock as its final action (lock release is after worktree creation, closing a race condition)
4. Changes to the worktree directory
5. **phase-finalize** — completes the phase transition, sends Slack notification, returns timing and continue mode

---

## Naming

Claude derives a concise branch name (2-5 words) from the prompt:

| Prompt | Branch |
|--------|--------|
| `app payment webhooks` | `app-payment-webhooks` |
| `fix login timeout when session expires` | `fix-login-timeout` |

The derived name is hyphenated and used for the branch, worktree (`.worktrees/<name>`), and PR title (title-cased). Branch names are capped at 32 characters, truncated at word boundaries.

When the prompt contains `#N` issue references (e.g., `work on issue #309`), `start-init` automatically fetches the first issue's title and derives the branch name and PR title from it. This produces descriptive names like `organize-settings-allow-list` rather than generic names like `work-on-issue-309`. If the issue fetch fails, start-init returns a hard error.

If the referenced issue already carries the "Flow In-Progress" label, `start-init` stops with a hard error before creating the state file — another flow (on this machine or another engineer's machine) is already working on that issue. The user should resume the existing flow in its worktree, or reference a different issue.

---

## Mode

Mode is configurable via `.flow.json` (default: manual) and cached in the state file during setup. The Done section reads the resolved mode from the state file, not `.flow.json` directly. In auto mode, the phase transition advances to Plan without asking.

When `--auto` is passed to `/flow-start`, it overrides ALL skill autonomy settings to fully autonomous for this feature — not just flow-start's own continue mode. Every phase will auto-commit and auto-continue. The override is written to the state file by `start-init` and propagates to all downstream phases automatically. This is equivalent to the "Fully autonomous" preset from `/flow-prime`, applied per-feature without changing `.flow.json`.

---

## Gates

- Stops immediately if no feature name is provided
- Serializes starts with a lock — only one start runs at a time
- Stops if CI baseline on main cannot be fixed
- Stops if `git pull` fails
- Stops if a referenced `#N` issue already carries the "Flow In-Progress" label — cross-machine WIP detection prevents concurrent flows on the same issue
- Will not proceed past dependency upgrade until `bin/flow ci` is green
- Escalates to the user if `bin/flow ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
