---
title: /flow-complete
nav_order: 12
parent: Skills
---

# /flow-complete

**Phase:** 6 — Complete

**Usage:** `/flow-complete`, `/flow-complete --auto`, or `/flow-complete --manual`

The final phase. Merges the PR into main, removes the git worktree,
and deletes the state file. Mode is configurable via `.flow.json`
(default: auto, skips confirmation). Use `--manual` to prompt for
confirmation before the irreversible merge.

---

## What It Does

1. Reads `.flow-states/<branch>.json` for worktree, feature name, PR number,
   and session\_id (or infers from git state if the file is missing)
2. Checks PR status — if already merged, skips to cleanup
3. Merges `origin/main` into the feature branch, resolving any conflicts
4. Checks CI status — waits for checks to pass (suggests `/loop` for pending)
5. Confirms with the user (only when `--manual` is passed)
6. Archives artifacts to the PR body: phase timings table (non-collapsible),
   session log link (if session\_id is set), state file, and session log
7. Squash-merges the PR via `gh pr merge --squash`
8. Runs the cleanup process: remove worktree, delete branches, delete state file, log, and CI sentinel
9. Pulls `origin main` so local main has the merged feature code

---

## Why State File Deletion Matters

Deleting `.flow-states/<branch>.json` is what resets the
SessionStart hook. Without it, every new session would detect a
feature in progress that no longer exists. This is the clean exit
from the FLOW workflow.

---

## Idempotent Design

The skill is safe to re-invoke (e.g., via `/loop 15s /flow:flow-complete`).
Each step checks its precondition and skips if already done: merged PRs
skip to cleanup, up-to-date branches skip the merge, passing CI skips
the wait. After cleanup completes, the next invocation finds no state
file and exits cleanly.

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Phase 5 complete | Normal merge and cleanup — no warnings |
| State file exists, Phase 5 incomplete | Warns, proceeds (confirms if `--manual`) |
| State file missing | Warns, infers from git state, proceeds (confirms if `--manual`) |
| PR closed but not merged | Hard block, does not proceed |

Every step after the merge (Steps 8-9) is best-effort. If worktree removal
fails (already removed), it continues to state file deletion. If the
state file doesn't exist, it notes that and finishes.

---

## Gates

- PR must be open or already merged — hard block if closed
- CI must pass before merge
- Phase 5 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Confirmation only when mode is manual (via `--manual` or `.flow.json`)
- Must run from the project root — never from inside the worktree
- Merge is irreversible; branch deletion is handled by the cleanup script
- If merge fails, stop and report — never retry with additional flags or elevated privileges
