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

1. Pre-flight: runs version gate and upgrade check in parallel
2. Prepare main (locked): acquires a lock so only one start runs at a time, pulls main, runs `bin/flow ci` for a clean baseline, updates dependencies on main via `bin/dependencies`, runs `bin/flow ci` again to catch dep-induced breakage, commits everything to main, releases lock. The ci-fixer sub-agent handles failures at both CI gates
3. Runs `lib/start-setup.py` — worktree creation, empty commit + push + PR, and state file creation. The `--prompt` flag passes the user's raw input (including `#N` issue references) so it is preserved verbatim in the state file for issue closing at completion

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

Mode is configurable via `.flow.json` (default: manual) and copied into the state file at start. In auto mode, the phase transition advances to Plan without asking.

---

## Gates

- Stops immediately if no feature name is provided
- Serializes starts with a lock — only one start runs at a time
- Stops if CI baseline on main cannot be fixed
- Stops if `git pull` fails
- Will not proceed past dependency upgrade until `bin/flow ci` is green
- Escalates to the user if `bin/flow ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
