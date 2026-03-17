---
title: "Phase 1: Start"
nav_order: 2
---

# Phase 1: Start

**Command:** `/flow-start <feature name words>`

**Example:** `/flow-start app payment webhooks`

This is always the first phase, for every feature without exception. It establishes an isolated workspace, verifies the health of the codebase, configures workspace permissions, and opens the PR before any feature work begins. Framework-specific setup (dependency upgrades, CI fixes) is handled by the framework instructions in the skill.

---

## Steps

### 1. Version gate

Run `bin/flow prime-check` to verify `/flow-prime` has been run with the current plugin version. Cheapest check — runs first so a missing prime doesn't waste time on slower steps.

Also checks GitHub for newer FLOW releases and displays upgrade instructions if one is available. This check is informational — it never blocks.

### 2. Prepare main (locked)

Acquires a lock (`lib/start-lock.py`) so only one flow-start runs at a time. Under the lock:

1. `git pull origin main`
2. `bin/flow ci` — establish a clean baseline
3. If CI fails, the ci-fixer sub-agent diagnoses and fixes, then commits to main
4. `bin/dependencies` — update dependencies on main (not in a worktree)
5. If dependencies changed, `bin/flow ci` again — catches dep-induced breakage (rubocop, breaking changes)
6. If CI fails, ci-fixer again, then commits to main
7. Release the lock

This ensures every worktree starts from a clean, current main. Concurrent starts wait for the lock — the second start finds main already clean and breezes through.

### 3. Set up workspace

A single Python script (`lib/start-setup.py`) handles all mechanical setup in one process:

1. `git pull origin main` (no-op — already pulled in Step 2)
2. Create a git worktree at `.worktrees/app-payment-webhooks`
3. Empty commit, push branch, and open a PR via `gh pr create`
4. Create `.flow-states/app-payment-webhooks.json` (initial state)

---

## What You Get

By the end of Phase 1:

- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote with CI running
- An open PR
- Workspace permissions configured in `.claude/settings.json`
- Dependencies current and `bin/flow ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

Phase 2: Plan (`/flow-plan`) — explore the codebase, design the approach, and produce an ordered implementation plan.
