---
title: "Phase 7: Cleanup"
nav_order: 8
---

# Phase 7: Cleanup

**Command:** `/flow:cleanup` or `/flow:cleanup --manual`

The final phase. Removes the git worktree and deletes the state file
and log file. This is what fully closes out a feature and resets the
environment for the next one.

By default, skips confirmation and proceeds directly to cleanup.
Use `--manual` to prompt for confirmation before any destructive action.
Best-effort — warns if the state file is missing or Phase 6 is incomplete.

---

## Steps

### 1. Read state

Read `.flow-states/<branch>.json` for the worktree path and feature name.
If the state file is missing, infer from git state (branch name, worktree list).

### 2. Check PR merge status

Verify the PR has been merged. If the PR is not merged, stop — the user
must merge the PR before cleanup can proceed.

### 3. Confirm with user (--manual only)

When `--manual` is passed, explicit confirmation is required before any
destructive action. Any warnings from the entry check are included in the
confirmation message. Skipped by default.

### 4. Run cleanup

`bin/flow cleanup` handles all three resources from the project root:
worktree removal, state file deletion, and log file deletion. Each step
is best-effort — if one fails, the rest still run.

This resets the SessionStart hook — the next session starts clean.

---

## What You Get

By the end of Phase 7:

- Worktree and all its contents removed
- State file deleted — no more session hook injection for this feature
- Log file deleted — no stale logs left behind
- Local environment clean and ready for the next feature

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Phase 6 complete | Normal cleanup — no warnings |
| State file exists, Phase 6 incomplete | Warns, proceeds (confirms if `--manual`) |
| State file missing | Warns, infers from git, proceeds (confirms if `--manual`) |
| PR not merged | Hard block, does not proceed |

Every step after confirmation is best-effort — if one fails, continue to the next.

---

## Gates

- PR must be merged — hard block if not
- Phase 6 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Confirmation only when `--manual` is passed
- Must run from project root
