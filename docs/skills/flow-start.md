---
title: /flow:start
nav_order: 1
parent: Skills
---

# /flow:start

**Phase:** 1 — Start

**Usage:** `/flow:start <feature name words>`

**Example:** `/flow:start app payment webhooks`

Begins a new feature. This is always the first command run for any piece of work. It sets up an isolated environment, ensures dependencies are current, and establishes the PR before any feature code is written.

---

## What It Does

1. Pulls main to ensure a current starting point
2. Creates a git worktree at `.worktrees/<feature-name>`
3. Pushes the branch to remote immediately
4. Opens a real PR with an auto-generated phase checklist
5. Merges required permissions into `.claude/settings.json`
6. Runs `bin/ci` as a baseline health check
7. Runs `bundle update` to upgrade all gems
8. Runs `bin/ci` again and auto-fixes any breakage
9. Commits via `/flow:commit` and marks Phase 0 complete on the PR

---

## Naming

Words after `/flow:start` are joined with hyphens to form the feature name:

| Part | Value |
|------|-------|
| Branch | `app-payment-webhooks` |
| Worktree | `.worktrees/app-payment-webhooks` |
| PR title | `App Payment Webhooks` |

---

## Gates

- Stops immediately if no feature name is provided
- Stops if `git pull` fails
- Will not proceed past gem upgrade until `bin/ci` is green
- Escalates to the user if `bin/ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
