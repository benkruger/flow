---
title: "Phase 1: Start"
nav_order: 2
---

# Phase 1: Start

**Command:** `/flow:start <feature name words>`

**Example:** `/flow:start app payment webhooks`

This is always the first phase, for every feature without exception. It establishes an isolated workspace, verifies the health of the codebase, configures workspace permissions, and opens the PR before any feature work begins. Framework-specific setup (dependency upgrades, CI fixes) is handled by the framework instructions in the skill.

---

## Steps

### 1. Version gate

Run `bin/flow init-check` to verify `/flow:init` has been run with the current plugin version. Cheapest check — runs first so a missing init doesn't waste time on slower steps.

### 2. Check for existing features

Scans for active `.flow-states/*.json` files. If any exist, asks whether to proceed or cancel.

### 3. Verify main is green

Run `bin/flow ci` on main. If it fails, stop — fix CI before starting a feature.
No worktree, PR, or state file is created if main is broken.

### 4. Set up workspace

A single Python script (`lib/start-setup.py`) handles all mechanical setup in one process:

1. `git pull origin main`
2. Create a git worktree at `.worktrees/app-payment-webhooks`
3. Empty commit, push branch, and open a PR via `gh pr create`
4. Create `.flow-states/app-payment-webhooks.json` (initial state)

The script returns JSON with the worktree path, PR URL, and PR number. Claude then `cd`s into the worktree for all remaining steps.

### 5. Framework-specific setup

**Rails:** Upgrade gems with `bundle update --all`, then run `bin/flow ci`. If it fails, a Sonnet sub-agent diagnoses and fixes (max 3 attempts). Commit changes via `/flow:commit`.

**Python:** No additional setup — Step 3 verified `bin/flow ci` on main.

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

Phase 2: Plan (`/flow:plan`) — explore the codebase, design the approach, and produce an ordered implementation plan.
