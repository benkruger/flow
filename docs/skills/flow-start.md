---
title: /flow-start
nav_order: 1
parent: Skills
---

# /flow-start

**Phase:** 1 — Start

**Usage:** `/flow-start #N`

**Example:** `/flow-start #1234`

Begins a new feature against a pre-decomposed GitHub issue. The argument must match `^#[1-9][0-9]*$` — a literal `#` followed by a positive integer. `start-init` fetches the issue title and derives the branch name from it; `plan-from-issue` then extracts the implementation plan from the issue body's `<!-- FLOW-PLAN-BEGIN -->`/`<!-- FLOW-PLAN-END -->` sentinels. This is always the first command run for any piece of work. It sets up an isolated environment, ensures dependencies are current, and establishes the PR before any feature code is written.

**Prerequisite:** `/flow-prime` must be run once per project (and again after each FLOW upgrade) before `/flow-start` will work. The setup script checks for a matching version marker at `.flow.json`.

---

## What It Does

1. **start-init** — acquires start lock, runs version gate and upgrade check, creates early state file via `init-state`. Consults the "Flow In-Progress" label on referenced issues as a pre-lock cross-machine WIP guard; the label apply happens later in `start-workspace` so a failed start-gate or start-workspace leaves no sticky label. (concurrent starts poll via `/loop` every 15 seconds until the lock is released)
2. **start-gate** — pulls latest main, runs `bin/flow ci` baseline with retry (3 attempts), updates dependencies, runs post-deps CI with retry if deps changed. Falls back to ci-fixer sub-agent for dep-induced breakage
3. **start-workspace** — creates worktree, opens PR, backfills state file with PR fields, applies the "Flow In-Progress" label to referenced issues (best-effort, success path only), and releases the start lock as its final action (lock release is after worktree creation, closing a race condition). The label apply lands here — not in `start-init` — so the label means "a flow is live, worktree exists, PR exists" rather than "a flow was attempted"
4. Changes to the worktree directory
5. **plan-from-issue** — fetches the issue body via `gh issue view`, extracts the plan content between `<!-- FLOW-PLAN-BEGIN -->` and `<!-- FLOW-PLAN-END -->` sentinels, writes it to `.flow-states/<branch>/plan.md`, and records `code_tasks_total` in the state file via `set-timestamp` so the TUI can render the Code-phase X-of-Y task counter
6. **phase-finalize** — completes the phase transition, sends Slack notification, returns timing and continue mode

---

## Naming

`start-init` fetches the referenced issue's title and derives a concise hyphenated branch name from it:

| Argument | Issue title | Derived branch |
|----------|-------------|----------------|
| `#309` | "Organize settings.json allow list" | `organize-settings-allow-list` |
| `#42` | "Add dark mode toggle to settings page" | `dark-mode-settings-toggle` |

The derived name is hyphenated and used for the branch, worktree (`.worktrees/<name>`), and PR title (title-cased). Branch names are capped at **32 characters**; when the hyphenated name exceeds 32 characters the value is truncated at the last whole word (hyphen boundary) that fits and any trailing hyphen is stripped. If the issue fetch fails, `start-init` returns a hard error.

If the referenced issue already carries the "Flow In-Progress" label, `start-init` stops with a hard error before creating the state file — another flow (on this machine or another engineer's machine) is already working on that issue. The user should resume the existing flow in its worktree, or reference a different issue.

---

## Mode

Mode is configurable via `.flow.json` — the single source of truth for skill autonomy — and cached in the state file during setup. There are no `--auto`/`--manual` flags. The Done section reads the Start phase's resolved `continue` mode from the state file's `skills.flow-start` config (via `phase-finalize`'s `continue_action`). In auto mode, the phase transition advances to Code without asking.

Each phase reads its own `skills.<phase>` configuration, seeded verbatim from `.flow.json` into the state file at flow-start. To run every phase autonomously, configure the skills block in `.flow.json` (the "Fully autonomous" preset from `/flow-prime`).

---

## Gates

- Stops immediately if no `#N` argument is provided or if it does not match the strict `^#[1-9][0-9]*$` format
- Serializes starts with a lock — only one start runs at a time
- Stops if CI baseline on the integration branch cannot be fixed
- Stops if `git pull` fails
- Stops if the referenced `#N` issue already carries the "Flow In-Progress" label — cross-machine WIP detection prevents concurrent flows on the same issue
- Will not proceed past dependency upgrade until `bin/flow ci` is green
- Escalates to the user if `bin/flow ci` cannot be fixed after three attempts

---

## See Also

- [Phase 1: Start](../phases/phase-1-start.md) — full phase documentation
