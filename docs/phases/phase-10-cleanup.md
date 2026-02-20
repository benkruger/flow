---
title: "Phase 10: Cleanup"
nav_order: 12
---

# Phase 10: Cleanup

**Command:** `/sdlc:cleanup`

The final phase. Removes the git worktree and deletes the state file.
This is what fully closes out a feature and resets the environment for
the next one.

---

## Steps

### 1. Read state
Read `.claude/sdlc-states/<branch>.json` for the worktree path and feature name.

### 2. Confirm with user
Explicit confirmation required before any destructive action.

### 3. Navigate to project root
All cleanup must run from the project root, not from inside the worktree.

### 4. Remove the worktree
```bash
git worktree remove .worktrees/<feature-name> --force
```

### 5. Delete the state file
```bash
rm .claude/sdlc-states/<branch>.json
```

This resets the SessionStart hook — the next session starts clean.

---

## What You Get

By the end of Phase 10:

- Worktree and all its contents removed
- State file deleted — no more session hook injection for this feature
- Local environment clean and ready for the next feature

---

## Gates

- Requires Phase 9: Reflect to be complete
- Requires explicit user confirmation
- Must run from project root
