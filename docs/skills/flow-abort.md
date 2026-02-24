---
title: /flow:abort
nav_order: 17
parent: Skills
---

# /flow:abort

**Phase:** Any (no phase gate)

**Usage:** `/flow:abort`

The escape hatch. Abandons the current feature completely — closes the PR,
deletes the remote branch, removes the worktree, and deletes the state file.

Available from any phase, no prerequisites. Best-effort — warns if the state
file is missing, but proceeds after user confirmation.

---

## What It Does

1. Reads `.flow-states/<branch>.json` for feature details
   (or infers from git state if the file is missing)
2. Confirms with the user before any destructive action, including any
   warnings from the entry check
3. Navigates to the project root
4. Closes the PR with `gh pr close` and a comment
5. Removes the worktree with `git worktree remove --force`
6. Deletes the remote branch with `git push origin --delete`
7. Deletes the local branch with `git branch -D`
8. Deletes `.flow-states/<branch>.json`

Steps 3–8 follow a mix of abort-specific actions and the shared cleanup
process (`docs/cleanup-process.md`). Every step after confirmation is
best-effort — if one fails (e.g., PR already closed, worktree already
removed), it continues to the next.

---

## When to Use It

- You started a feature and decided not to pursue it
- The approach is fundamentally wrong and you want a clean slate
- You want to abandon work without going through Review and Reflect

---

## vs /flow:cleanup

| | `/flow:cleanup` | `/flow:abort` |
|---|---|---|
| **When** | After Reflect (Phase 7) | Any phase |
| **PR** | Left open (merge it yourself) | Closed |
| **Remote branch** | Left intact | Deleted |
| **Worktree** | Removed | Removed |
| **State file** | Deleted | Deleted |
| **Missing state** | Warns, proceeds | Warns, proceeds |

Use `/flow:cleanup` for the happy path after a completed feature.
Use `/flow:abort` to walk away from a feature entirely.

---

## Gates

- No phase gate — available from any phase
- State file not required — warns if missing, infers from git state
- Requires explicit user confirmation before any destructive action
- Must run from the project root — never from inside the worktree
- All operations are irreversible