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

**Prerequisite:** `/flow:init` must be run once per project (and again after each FLOW upgrade) before `/flow:start` will work. The setup script checks for a matching version marker at `.flow.json`.

---

## What It Does

1. Checks for existing active FLOW features
2. Runs `lib/start-setup.py` — verifies `/flow:init` version gate, git pull, worktree creation, empty commit + push + PR, and state file creation
3. Runs `bin/ci` as a baseline health check
4. Runs `bundle update --all` to upgrade all gems
5. Runs `bin/ci` again and auto-fixes any breakage (via a Sonnet sub-agent)
6. Commits via `/flow:commit`
7. Marks Phase 1 complete and transitions to Phase 2: Research

---

## Naming

Words after `/flow:start` are joined with hyphens to form the feature name:

| Part | Value |
|------|-------|
| Branch | `app-payment-webhooks` |
| Worktree | `.worktrees/app-payment-webhooks` |
| PR title | `App Payment Webhooks` |

Branch names are capped at 32 characters, truncated at word boundaries.

---

## Gates

- Stops immediately if no feature name is provided
- Stops if `git pull` fails
- Will not proceed past gem upgrade until `bin/ci` is green
- Escalates to the user if `bin/ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
