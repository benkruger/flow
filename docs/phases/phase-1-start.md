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

### 1. Pull main

```bash
git pull origin main
```

Ensure the starting point is current. If this fails, stop and report why.

### 2. Create the worktree

```bash
git worktree add .worktrees/app-payment-webhooks -b app-payment-webhooks
```

The worktree name and branch are derived from the command arguments joined with hyphens. All subsequent work happens inside the worktree — main is never modified.

### 3. Push branch to remote immediately

```bash
git push -u origin app-payment-webhooks
```

Establishes the branch remotely before any code changes.

### 4. Open the PR

```bash
gh pr create \
  --title "App Payment Webhooks" \
  --body "..." \
  --base main
```

A real PR, not a draft. The PR body is auto-generated from the feature name:

```
## What

App payment webhooks.
```

Phase progress is tracked in `.claude/flow-states/<branch>.json`, not in the PR body.

### 5. Configure workspace permissions

Check if `.claude/settings.json` exists in the project root.

**If it does not exist**, create it:

```json
{
  "permissions": {
    "allow": [
      "Bash(git add *)",
      "Bash(git commit *)",
      "Bash(git push)",
      "Bash(git push -u *)"
    ]
  }
}
```

**If it exists**, read it and merge in any missing entries. Existing entries are never removed or overwritten. No duplicates are added.

### 6. Baseline `bin/ci`

Run `bin/ci` inside the worktree to capture the health of the codebase before any changes.

- **Passes** — note it as the baseline and continue
- **Fails** — report the failures clearly. These are pre-existing issues, not caused by this work. Ask the user whether to proceed or stop.

### 7. Upgrade gems

```bash
bundle update
```

Upgrades all gems to their latest compatible versions. Runs inside the worktree so `Gemfile.lock` changes stay on the feature branch.

### 8. Post-update `bin/ci`

Run `bin/ci` again after the gem upgrade. Gem updates commonly introduce:

- New RuboCop rules requiring code changes
- Breaking API changes causing test failures
- Deprecation warnings promoted to errors

### 9. Fix breakage (if needed)

**RuboCop violations** — run the auto-fixer first:

```bash
rubocop -A
```

Then run `bin/ci` again. Fix any remaining violations manually.

**Test failures** — read the output carefully. Common causes:
- Changed gem APIs (update call sites)
- New validation behaviour (update fixtures or assertions)
- Deprecation warnings promoted to errors (follow the deprecation message)

Repeat until `bin/ci` is green. If not fixed after three attempts, stop and report what is failing and what was tried.

### 10. Commit and push

Use `/flow:commit` to review and commit the changes (`Gemfile.lock` and any gem-related fixes), then mark Phase 0 complete on the PR.

---

## What You Get

By the end of Phase 0:

- An isolated worktree at `.worktrees/<feature-name>`
- A branch pushed to remote with CI running
- An open PR with a phase progress checklist
- Workspace permissions configured in `.claude/settings.json`
- All gems upgraded and `bin/ci` green
- A clean, known-good baseline to build from

---

## What Comes Next

Phase 1: Research (`/flow:research`) — read all affected code before writing any.
