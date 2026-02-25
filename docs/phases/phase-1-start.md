---
title: "Phase 1: Start"
nav_order: 2
---

# Phase 1: Start

**Command:** `/flow:start <feature name words>`

**Example:** `/flow:start app payment webhooks`

This is always the first phase, for every feature without exception. It establishes an isolated workspace, verifies the health of the codebase, upgrades all dependencies, configures workspace permissions, and opens the PR before any feature work begins.

---

## Steps

### 1. Check for existing features

Scans for active `.flow-states/*.json` files. If any exist, asks whether to proceed or cancel.

### 2. Set up workspace

A single Python script (`hooks/start-setup.py`) handles all mechanical setup in one process:

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

### 4. Upgrade gems

```bash
bundle update --all
```

Upgrades all gems to their latest compatible versions. Runs inside the worktree so `Gemfile.lock` changes stay on the feature branch.

### 5. Post-update `bin/ci`

Run `bin/ci` again after the gem upgrade. Gem updates commonly introduce:

- New RuboCop rules requiring code changes
- Breaking API changes causing test failures
- Deprecation warnings promoted to errors

If failures occur, the same CI fix sub-agent handles diagnosis and repair.

### 6. Fix breakage (if needed)

A general-purpose Sonnet sub-agent handles CI failures from Steps 3 and 5:

1. **RuboCop violations** — `rubocop -A` to auto-fix
2. **Test failures** — read the failing test and fix the code
3. **Coverage gaps** — read `test/coverage/uncovered.txt` and write the missing test

Max 3 attempts. Will not proceed until `bin/ci` is green.

### 7. Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` and any gem-related fixes).

---

## What You Get

By the end of Phase 1:

- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote with CI running
- An open PR
- Workspace permissions configured in `.claude/settings.json`
- All gems upgraded and `bin/ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

Phase 2: Research (`/flow:research`) — read all affected code before writing any.
