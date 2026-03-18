---
title: /flow-abort
nav_order: 17
parent: Skills
---

# /flow-abort

**Phase:** Any (no phase gate)

**Usage:** `/flow-abort`, `/flow-abort --auto`, or `/flow-abort --manual`

The escape hatch. Abandons the current feature completely — closes the PR,
deletes the remote branch, removes the worktree, and deletes the state file.

Available from any phase, no prerequisites. Best-effort — warns if the state
file is missing.

- `/flow-abort` — uses configured mode from `.flow.json` (default: auto)
- `/flow-abort --auto` — skips confirmation and proceeds directly
- `/flow-abort --manual` — prompts for user confirmation before any destructive action

---

## What It Does

1. Reads `.flow-states/<branch>.json` for feature details
   (or infers from git state if the file is missing)
2. Removes the "Flow In-Progress" label from any issues referenced in the prompt (if state file exists)
3. Confirms with the user before any destructive action, including any
   warnings from the entry check
4. Navigates to the project root
5. Closes the PR with `gh pr close` and a comment
6. Removes the worktree with `git worktree remove --force`
7. Deletes the remote branch with `git push origin --delete`
8. Deletes the local branch with `git branch -D`
9. Deletes `.flow-states/<branch>.json` and CI sentinel

Steps 4–9 follow a mix of abort-specific actions and cleanup operations.
Every step after confirmation is best-effort — if one fails (e.g., PR
already closed, worktree already removed), it continues to the next.

---

## When to Use It

- You started a feature and decided not to pursue it
- The approach is fundamentally wrong and you want a clean slate
- You want to abandon work without going through Review and Learn

---

## vs /flow-complete

| | `/flow-complete` | `/flow-abort` |
|---|---|---|
| **When** | After Learn (Phase 5) | Any phase |
| **PR** | Squash-merged into main | Closed |
| **Remote branch** | Deleted (via cleanup) | Deleted (via cleanup) |
| **Worktree** | Removed | Removed |
| **State file** | Deleted | Deleted |
| **Missing state** | Warns, proceeds | Warns, proceeds |

Use `/flow-complete` for the happy path after a completed feature.
Use `/flow-abort` to walk away from a feature entirely.

---

## Gates

- No phase gate — available from any phase
- State file not required — warns if missing, infers from git state
- Requires user confirmation when mode is manual (via `--manual` or `.flow.json`)
- Must run from the project root — never from inside the worktree
- All operations are irreversible
