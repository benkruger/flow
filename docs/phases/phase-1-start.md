---
title: "Phase 1: Start"
nav_order: 2
---

# Phase 1: Start

**Command:** `/flow:start <feature name words>`

**Example:** `/flow:start app payment webhooks`

This is always the first phase, for every feature without exception. It establishes an isolated workspace, verifies the health of the codebase, configures workspace permissions, and opens the PR before any feature work begins. Framework-specific setup (dependency upgrades, CI fixes) is handled by the framework fragment.

---

## Steps

### 1. Check for existing features

Scans for active `.flow-states/*.json` files. If any exist, asks whether to proceed or cancel.

### 2. Set up workspace

A single Python script (`lib/start-setup.py`) handles all mechanical setup in one process:

1. `git pull origin main`
2. Create/merge `.claude/settings.json` with workspace permissions
3. Create a git worktree at `.worktrees/app-payment-webhooks`
4. Configure `info/exclude` with `.flow-states/` and `.worktrees/`
5. Empty commit, push branch, and open a PR via `gh pr create`
6. Create `.flow-states/app-payment-webhooks.json` (initial state)

The script returns JSON with the worktree path, PR URL, and PR number. Claude then `cd`s into the worktree for all remaining steps.

### 3. Baseline `bin/ci`

Run `bin/ci` inside the worktree to capture the health of the codebase before any changes.

- **Passes** — note it as the baseline and continue
- **Fails** — launch a sub-agent to diagnose and fix. If not fixable after three attempts, stop and report.

### 4. Framework-specific setup

Runs framework-specific steps defined in the framework fragment (e.g., dependency upgrades for Rails, baseline checks for Python). A general-purpose Sonnet sub-agent handles any CI failures — max 3 attempts before escalating to the user.

### 5. Commit and push

Use `/flow:commit` to review and commit any changes from the framework-specific setup.

---

## What You Get

By the end of Phase 1:

- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote with CI running
- An open PR
- Workspace permissions configured in `.claude/settings.json`
- Dependencies current and `bin/ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

Phase 2: Research (`/flow:research`) — read all affected code before writing any.
