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

Four consolidated Rust commands handle the Start phase. Steps 1-3 serialize all main-branch work behind a lock — only one flow-start runs at a time. Concurrent starts poll via `/loop` until the lock is released.

### 1. Initialize (`start-init`)

Acquires a queue-based lock, runs version gate and upgrade check, creates the early state file via `init-state`, and labels referenced issues with "Flow In-Progress". If the lock is already held, invokes `/loop 15s /flow:flow-start` to poll every 15 seconds. If version checks or init-state fail, releases the lock and stops.

### 2. CI and dependency gate (`start-gate`)

Pulls latest main, runs `bin/flow ci` baseline with retry (up to 3 attempts), updates dependencies via `bin/dependencies`, and runs post-deps CI with retry if deps changed. Files Flaky Test issues for intermittent failures. Falls back to the ci-fixer sub-agent for consistent dep-induced breakage.

### 3. Create workspace (`start-workspace`)

Creates a git worktree at `.worktrees/<branch>`, makes an empty commit, pushes the branch, opens a PR via `gh pr create`, backfills the state file with PR fields, and releases the start lock as its final action. The lock is released even on error — main is untouched by worktree operations.

### 4. Change to worktree

Changes the working directory to the new worktree so all subsequent phases run in the isolated workspace.

### 5. Finalize (`start-finalize`)

Completes the phase transition, sends the initial Slack notification (if configured), and returns the formatted time and continue mode for the transition to Phase 2.

---

## What You Get

By the end of Phase 1:

- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote with CI running
- An open PR
- Referenced issues labeled "Flow In-Progress" (visible to all engineers)
- Workspace permissions configured in `.claude/settings.json`
- Dependencies current and `bin/flow ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

Phase 2: Plan (`/flow-plan`) — explore the codebase, design the approach, and produce an ordered implementation plan.
