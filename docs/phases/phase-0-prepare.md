---
title: "Phase 0: Prepare"
nav_order: 2
parent: Phases
---

# Phase 0: Prepare

**Command:** `/ror:start <feature name words>`

**Example:** `/ror:start app payment webhooks`

This is always the first phase, for every feature without exception. It establishes an isolated workspace, verifies the health of the codebase, upgrades all dependencies, and opens the PR before any feature work begins.

---

## What It Does

### 1. Pull main
```bash
git pull origin main
```
Ensure the starting point is current.

### 2. Create the worktree
```bash
git worktree add .worktrees/app-payment-webhooks -b app-payment-webhooks
```
The worktree name is derived from the command arguments joined with hyphens. All work happens here — main is never modified.

### 3. Push branch to remote
```bash
git push -u origin app-payment-webhooks
```
Establishes the branch remotely immediately, before any code changes.

### 4. Open the PR
```bash
gh pr create --title "app payment webhooks" --body "..."
```
A real PR, not a draft. The work is visible and trackable from the first moment.

### 5. Baseline `bin/ci`
Run `bin/ci` inside the worktree to capture the health of the branch before any changes. If this fails, it means main has pre-existing issues that must be noted.

### 6. Upgrade gems
```bash
bundle update
```
Upgrades all gems to their latest compatible versions. This runs inside the worktree, so `Gemfile.lock` changes stay on the feature branch.

### 7. Post-update `bin/ci`
Run `bin/ci` again. Gem updates often introduce:
- New RuboCop rules requiring code changes
- Breaking API changes causing test failures

### 8. Fix breakage (if needed)
If `bin/ci` fails after `bundle update`, fix the violations and failures now. These are gem-upgrade fixes, not feature work.

### 9. Final `bin/ci` (if fixes were applied)
Confirm green after fixing breakage.

### 10. Commit and push
```bash
git add Gemfile.lock
git commit -m "chore: bundle update"
git push
```
Commits the `Gemfile.lock` update and any gem-related fixes. The PR now reflects the updated dependency baseline.

---

## What You Get

By the end of Phase 0:
- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote
- An open PR with CI running
- All gems upgraded and `bin/ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

[Phase 1: Research](phase-1-research) — read all affected code before writing any.
