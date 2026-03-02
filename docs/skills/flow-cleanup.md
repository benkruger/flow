---
title: /flow:cleanup
nav_order: 12
parent: Skills
---

# /flow:cleanup

**Phase:** 9 — Cleanup

**Usage:** `/flow:cleanup`

The final phase. Removes the git worktree and deletes the state file.
Best-effort — warns if the state file is missing or Phase 8 is incomplete,
but proceeds after user confirmation.

---

## What It Does

1. Reads `.flow-states/<branch>.json` for worktree and feature name
   (or infers from git state if the file is missing)
2. Checks PR merge status — hard block if the PR has not been merged
3. Confirms with the user before any destructive action, including any
   warnings from the entry check
4. Runs the cleanup process:
   navigate to root, remove worktree, delete state file and log, report results

---

## Why State File Deletion Matters

Deleting `.flow-states/<branch>.json` is what resets the
SessionStart hook. Without it, every new session would detect a
feature in progress that no longer exists. This is the clean exit
from the FLOW workflow.

---

## Best-Effort Behavior

Cleanup handles three scenarios gracefully:

| Scenario | Behavior |
|---|---|
| State file exists, Phase 8 complete | Normal cleanup — no warnings |
| State file exists, Phase 8 incomplete | Warns, proceeds after confirmation |
| State file missing | Warns, infers from git state, proceeds after confirmation |
| PR not merged | Hard block, does not proceed |

Every step after user confirmation is best-effort. If worktree removal
fails (already removed), it continues to state file deletion. If the
state file doesn't exist, it notes that and finishes.

---

## Gates

- PR must be merged — hard block if not
- Phase 8 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Requires explicit user confirmation before removing the worktree
- Must run from the project root — never from inside the worktree
- Worktree removal is irreversible
