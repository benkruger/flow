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

### 1. Pre-flight checks

Run `bin/flow prime-check` to verify `/flow-prime` has been run with the current plugin version. Cheapest check — runs first so a missing prime doesn't waste time on slower steps. Also checks GitHub for newer FLOW releases and displays upgrade instructions if one is available. This check is informational — it never blocks.

Steps 2–8 serialize all main-branch work behind a lock. Only one flow-start runs this section at a time. Concurrent starts wait for the lock — the second start finds main already clean and breezes through.

### 2. Acquire start lock

Acquires a lock (`lib/start-lock.py`) so only one flow-start runs at a time. The lock command must not run in the background — its `--wait` flag blocks until acquired or timed out.

### 3. Pull latest main

`git pull origin main` — ensures the worktree starts from the latest code.

### 4. CI baseline gate

`bin/flow ci` — establish a clean baseline. Main is pristine, so any failure is a flaky test. Retries up to 3 times; if a subsequent attempt passes, files a Flaky Test issue and continues. If all 3 fail, stops and reports to the user.

### 5. Update dependencies

`bin/dependencies` — update dependencies on main (not in a worktree). If nothing changed, skip to Step 8.

### 6. CI post-deps gate

If dependencies changed, `bin/flow ci` again — catches dep-induced breakage (rubocop, breaking changes). Retries up to 3 times to detect flaky tests. If all retries fail consistently, launches the ci-fixer sub-agent to diagnose and fix.

### 7. Commit to main

If there are any uncommitted changes (dependency updates + CI fixes), commits them to main.

### 8. Release start lock

Releases the lock so other concurrent starts can proceed.

This ensures every worktree starts from a clean, current main.

### 9. Set up workspace

A single Python script (`lib/start-setup.py`) handles all mechanical setup in one process:

1. Create a git worktree at `.worktrees/app-payment-webhooks`
2. Empty commit, push branch, and open a PR via `gh pr create`
3. Create `.flow-states/app-payment-webhooks.json` (initial state)

### 10. Label referenced issues

If the start prompt contains `#N` issue references (e.g., `fix #83 and #89`), adds the "Flow In-Progress" label to those issues on GitHub. This signals to other engineers (on other machines) that these issues are being worked on. Best-effort — labeling failures do not block the Start phase.

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
