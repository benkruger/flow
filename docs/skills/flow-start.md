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

1. Pre-flight: runs version gate and upgrade check in parallel
2. Prepare main (locked): acquires a lock so only one start runs at a time (concurrent starts wait internally via `--wait` with a 5-minute timeout), pulls main, runs `bin/flow ci` for a clean baseline, updates dependencies on main via `bin/dependencies`, runs `bin/flow ci` again to catch dep-induced breakage, commits everything to main, releases lock. The ci-fixer sub-agent handles failures at both CI gates
3. Runs `lib/start-setup.py` — worktree creation, empty commit + push + PR, and state file creation. The user's raw input (including `#N` issue references) is written to `.flow-states/<branch>-start-prompt` and passed via `--prompt-file` so it is preserved verbatim in the state file for issue closing at completion
4. Labels referenced issues — if the prompt contains `#N` issue references, adds the "Flow In-Progress" label so other engineers can see these issues are being worked on

---

## Naming

Claude derives a concise branch name (2-5 words) from the prompt:

| Prompt | Branch |
|--------|--------|
| `app payment webhooks` | `app-payment-webhooks` |
| `fix login timeout when session expires` | `fix-login-timeout` |

The derived name is hyphenated and used for the branch, worktree (`.worktrees/<name>`), and PR title (title-cased). Branch names are capped at 32 characters, truncated at word boundaries.

When the prompt contains `#N` issue references (e.g., `work on issue #309`), Claude fetches the issue title and derives the branch name from it instead of the prompt words. This produces descriptive names like `organize-settings-allow-list` rather than generic names like `work-on-issue-309`. If the issue fetch fails, it falls back to deriving from the prompt words.

---

## Mode

Mode is configurable via `.flow.json` (default: manual) and cached in the state file during setup. The Done section reads the resolved mode from the state file, not `.flow.json` directly. In auto mode, the phase transition advances to Plan without asking.

When `--auto` is passed to `/flow-start`, it overrides ALL skill autonomy settings to fully autonomous for this feature — not just flow-start's own continue mode. Every phase will auto-commit and auto-continue, and the code review plugin step is skipped. The override is written to the state file by `lib/start-setup.py` and propagates to all downstream phases automatically. This is equivalent to the "Fully autonomous" preset from `/flow-prime`, applied per-feature without changing `.flow.json`.

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
