---
title: /flow-complete
nav_order: 12
parent: Skills
---

# /flow-complete

**Phase:** 6 — Complete

**Usage:** `/flow-complete`, `/flow-complete --auto`, `/flow-complete --manual`, or `/flow-complete --continue-step`

The final phase. Merges the PR into main, removes the git worktree,
and deletes the state file. Mode is configurable via `.flow.json`
(default: auto, skips confirmation). Use `--manual` to prompt for
confirmation before the irreversible merge. The `--continue-step`
flag is used for self-invocation after mid-phase commits (merge
conflict resolution or CI fix) — it skips the Announce banner and
SOFT-GATE and dispatches via the Resume Check.

---

## What It Does

1. Reads `.flow-states/<branch>.json` for worktree, feature name, and PR number
   (or infers from git state if the file is missing)
2. Checks PR status — if already merged, skips to archive (step 6), then cleanup
3. Merges `origin/main` into the feature branch, resolving any conflicts.
   If conflicts exist, sets `_continue_pending=commit` before invoking
   `/flow:flow-commit`, then self-invokes with `--continue-step` to resume
   at Step 4
4. Checks CI status — first runs `bin/flow ci --force --simulate-branch main`
   locally to catch branch-dependent test failures before merge, then checks
   GitHub CI via `gh pr checks` (waits for checks to pass, suggests `/loop`
   for pending). If CI fails at either stage, ci-fixer commits a fix and
   self-invokes to re-check
5. Confirms with the user (only when `--manual` is passed). Offers three
   options: approve merge, decline, or provide feedback on the code. If the
   user gives feedback, processes the fix, commits via `/flow:flow-commit`,
   then self-invokes with `--continue-step --manual` to re-prompt for
   merge approval
6. Archives artifacts to the PR body: session log link (from transcript path),
   phase timings table (non-collapsible), state file, and session log dump.
   Generates a business-friendly summary via `format-complete-summary`
   showing feature name, prompt, per-phase timeline, and artifact counts.
   After the Done banner, writes a brief prose session summary describing what was accomplished
7. Squash-merges the PR via `gh pr merge --squash`
8. Removes the "Flow In-Progress" label from any issues referenced in the start prompt
9. Closes any GitHub issues referenced in the start prompt (`#N` patterns)
10. Runs the cleanup process: remove worktree, delete branches, delete state file, log, and CI sentinel
11. Pulls `origin main` so local main has the merged feature code

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
skip to archive then cleanup, up-to-date branches skip the merge,
passing CI skips the wait. After cleanup completes, the next invocation finds no state
file and exits cleanly.

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Phase 5 complete | Normal merge and cleanup — no warnings |
| State file exists, Phase 5 incomplete | Warns, proceeds (confirms if `--manual`) |
| State file missing | Warns, infers from git state, proceeds (confirms if `--manual`) |
| PR closed but not merged | Hard block, does not proceed |

Every step after the merge (Steps 8-11) is best-effort. If label removal
or issue closing fails, it continues to state file deletion. If the
state file doesn't exist, it notes that and finishes.

---

## Gates

- PR must be open or already merged — hard block if closed
- CI must pass before merge
- Phase 5 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Confirmation only when mode is manual (via `--manual` or `.flow.json`)
- Steps 1-9 run from the worktree; Steps 10-11 run from the project root
- Merge is irreversible; branch deletion is handled by the cleanup script
- If merge fails, stop and report — never retry with additional flags or elevated privileges
