---
title: "Phase 8: Cleanup"
nav_order: 11
---

# Phase 8: Cleanup

**Command:** `/flow:cleanup`

The final phase. Removes the git worktree and deletes the state file.
This is what fully closes out a feature and resets the environment for
the next one.

Best-effort — warns if the state file is missing or Phase 7 is incomplete,
but proceeds after user confirmation.

---

## Steps

### 1. Read state
Read `.flow-states/<branch>.json` for the worktree path and feature name.
If the state file is missing, infer from git state (branch name, worktree list).

### 2. Confirm with user
Explicit confirmation required before any destructive action. Any warnings
from the entry check are included in the confirmation message.

### 3. Navigate to project root
All cleanup must run from the project root, not from inside the worktree.

### 4. Remove the worktree
```bash
git worktree remove .worktrees/<feature-name> --force
```
If this fails (already removed), note it and continue.

### 5. Delete the state file
```bash
rm .flow-states/<branch>.json
```
If this doesn't exist, note it and continue.

This resets the SessionStart hook — the next session starts clean.

---

## What You Get

By the end of Phase 8:

- Worktree and all its contents removed
- State file deleted — no more session hook injection for this feature
- Local environment clean and ready for the next feature

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Phase 7 complete | Normal cleanup — no warnings |
| State file exists, Phase 7 incomplete | Warns, proceeds after confirmation |
| State file missing | Warns, infers from git, proceeds after confirmation |

Every step after confirmation is best-effort — if one fails, continue to the next.

---

## Gates

- Phase 7 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Requires explicit user confirmation
- Must run from project root