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

1. **Preflight** — `complete-preflight` handles state detection, PR status
   check, phase transition entry, mode resolution, Learn phase warning, and
   merging main into the branch in a single script call. If the PR is already
   merged, skips directly to post-merge (step 6) then cleanup
2. **Local CI gate** — `bin/flow ci --simulate-branch main` catches
   branch-dependent test failures. If it fails, ci-fixer commits a fix and
   self-invokes to re-check
3. **GitHub CI check** — `gh pr checks` waits for checks to pass. If pending,
   invokes `/loop` to auto-retry. If failed, ci-fixer commits a fix
4. **Confirm** (manual mode only) — explicit confirmation before the
   irreversible merge. Offers approve, decline, or feedback options. Skipped
   by default
5. **Merge** — `complete-merge` handles the freshness check and squash merge.
   If main moved, loops back through CI. Detects branch protection policy
   blocks and merge conflicts
6. **Post-merge** — `complete-post-merge` handles phase completion, PR body
   rendering, issues summary, closing referenced issues, summary generation,
   label removal, auto-close parent issues, and Slack notification — all
   best-effort in a single call
7. **Cleanup** — `cleanup --pull` removes the worktree, deletes branches,
   state file, log, and all artifacts, then pulls merged changes to main

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
skip to post-merge then cleanup, up-to-date branches skip the merge,
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

Every step after the merge (Step 6) is best-effort. If label removal
or issue closing fails, it continues to state file deletion. If the
state file doesn't exist, it notes that and finishes.

---

## Gates

- PR must be open or already merged — hard block if closed
- CI must pass before merge
- Phase 5 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Confirmation only when mode is manual (via `--manual` or `.flow.json`)
- Steps 1-6 run from the worktree; Step 7 runs from the project root
- Merge is irreversible; branch deletion is handled by the cleanup script
- If merge fails, stop and report — never retry with additional flags or elevated privileges
