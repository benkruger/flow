---
title: /flow-start
nav_order: 1
parent: Skills
---

# /flow-start

**Phase:** 1 — Start

**Usage:** `/flow-start <feature name words>`, `/flow-start --auto <words>`, or `/flow-start --manual <words>`

**Example:** `/flow-start app payment webhooks`

**Auto mode example:** `/flow-start --auto invoice pdf export`

Begins a new feature. This is always the first command run for any piece of work. It sets up an isolated environment, ensures dependencies are current, and establishes the PR before any feature code is written.

**Prerequisite:** `/flow-prime` must be run once per project (and again after each FLOW upgrade) before `/flow-start` will work. The setup script checks for a matching version marker at `.flow.json`.

---

## What It Does

1. Checks the version gate and notifies if a newer FLOW release is available on GitHub
2. Checks for existing active FLOW features
3. Runs `bin/flow ci` on main to verify the codebase is healthy
4. Runs `lib/start-setup.py` — verifies `/flow-prime` version gate, git pull, worktree creation, empty commit + push + PR, and state file creation
5. Runs `bin/dependencies` if it exists (created by `/flow-prime`), then `bin/flow ci`, with sub-agent CI fix if needed
6. Marks Phase 1 complete and transitions to Phase 2: Research

---

## Naming

Words after `/flow-start` are joined with hyphens to form the feature name:

| Part | Value |
|------|-------|
| Branch | `app-payment-webhooks` |
| Worktree | `.worktrees/app-payment-webhooks` |
| PR title | `App Payment Webhooks` |

Branch names are capped at 32 characters, truncated at word boundaries.

---

## Mode

Mode is configurable via `.flow.json` (default: manual) and copied into the state file at start. In auto mode, the existing-feature warning auto-proceeds and the phase transition advances to Plan without asking.

---

## Gates

- Stops immediately if no feature name is provided
- Stops if `bin/flow ci` fails on main before creating worktree
- Stops if `git pull` fails
- Will not proceed past dependency upgrade until `bin/flow ci` is green
- Escalates to the user if `bin/flow ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
