---
title: /flow:cleanup
nav_order: 12
parent: Skills
---

# /flow:cleanup

**Phase:** 9 — Cleanup

**Usage:** `/flow:cleanup`

The final phase. Removes the git worktree and deletes the state file.
Requires Phase 8: Reflect to be complete before it will run.

---

## What It Does

1. Reads `.claude/flow-states/<branch>.json` for worktree and feature name
2. Confirms with the user before any destructive action
3. Navigates to the project root
4. Runs `git worktree remove .worktrees/<feature-name> --force`
5. Deletes `.claude/flow-states/<branch>.json`
6. Marks all phases complete

---

## Why State File Deletion Matters

Deleting `.claude/flow-states/<branch>.json` is what resets the
SessionStart hook. Without it, every new session would detect a
feature in progress that no longer exists. This is the clean exit
from the FLOW workflow.

---

## Gates

- Requires Phase 8: Reflect to be complete
- Requires explicit user confirmation before removing the worktree
- Must run from the project root — never from inside the worktree
- Worktree removal is irreversible
