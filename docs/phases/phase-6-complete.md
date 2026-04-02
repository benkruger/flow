---
title: "Phase 6: Complete"
nav_order: 7
---

# Phase 6: Complete

**Command:** `/flow-complete` or `/flow-complete --manual`

The final phase. Merges the PR into main, removes the git worktree,
and deletes the state file and log file. This is what fully closes out
a feature and resets the environment for the next one.

By default, skips confirmation and proceeds directly to merge and cleanup.
Use `--manual` to prompt for confirmation before the irreversible merge.
Best-effort on cleanup steps — warns if the state file is missing or
Phase 5 is incomplete.

---

## Steps

### 1. Preflight

`complete-preflight` handles state detection, PR status check, phase
transition entry, mode resolution, Learn phase warning, and merging
main into the branch — all in a single script call. If the PR is
already merged, skips to post-merge (step 6) then cleanup. If there
are merge conflicts, resolves them and self-invokes to continue.

### 2. Run local CI gate

Runs `bin/flow ci --simulate-branch main` locally to catch
branch-dependent test failures (tests that pass on feature branches but
fail on main because `current_branch()` resolves against the host repo).
If it fails, launch the ci-fixer sub-agent to diagnose and fix.

### 3. Check GitHub CI status

Checks the PR's GitHub CI checks via `gh pr checks`. If all pass,
continue to merge. If any are pending, invoke
`/loop 15s /flow:flow-complete` to auto-retry. If any have failed,
launch the ci-fixer sub-agent to diagnose and fix.

### 4. Confirm with user (--manual only)

When `--manual` is passed, explicit confirmation is required before
the irreversible squash merge. Any warnings from the preflight are
included in the confirmation message. Skipped by default.

### 5. Merge PR

`complete-merge` handles the freshness check and squash merge in a
single script call. Verifies the branch is up-to-date with main
before merging. If main has moved, merges the new commits and loops
back to step 2 (CI gate) to re-test. A retry limit of 3 prevents
infinite loops under high contention. Once up-to-date, squash-merges
via `gh pr merge --squash`. Detects branch protection policy blocks
and returns for CI wait.

### 6. Post-merge operations

`complete-post-merge` handles all post-merge work in a single
best-effort script call:

- Phase transition complete (records timing)
- PR body rendering (What, Artifacts, Plan, DAG Analysis, Phase
  Timings, State File, Session Log, Issues Filed)
- Close referenced GitHub issues from the start prompt
- Generate business-friendly summary (feature name, prompt,
  per-phase timeline, artifact counts)
- Remove "Flow In-Progress" labels
- Auto-close parent issues and milestones
- Post Slack notification

### 7. Cleanup

`cleanup --pull` handles all resource cleanup from the project root:
remote and local branch deletion, worktree removal, state file deletion,
log file deletion, CI sentinel deletion, and pulls merged changes to
main. Each step is best-effort — if one fails, the rest still run.

This resets the SessionStart hook — the next session starts clean.

---

## What You Get

By the end of Phase 6:

- PR squash-merged into main
- Referenced GitHub issues closed (extracted from the start prompt)
- Remote branch deleted
- Worktree and all its contents removed
- Business-friendly summary displayed in Done banner: feature name, prompt,
  per-phase timeline, and artifact counts (issues filed, notes captured)
- PR link displayed in Done banner for quick access
- State file deleted — no more session hook injection for this feature
- Log file deleted — no stale logs left behind
- Local main pulled up to date with the merged feature code
- Local environment clean and ready for the next feature

---

## Idempotent Design

The skill is safe to re-invoke (e.g., via `/loop 15s /flow:flow-complete`):

| State | Behavior |
|---|---|
| PR already merged | Runs post-merge operations, then skips to cleanup |
| Main already merged into branch | No-op merge |
| CI already passing | Skips to merge |
| Freshness retry in progress | Loops back through CI gate, respects retry limit |
| State file already deleted | Exits cleanly |

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Phase 5 complete | Normal merge and cleanup — no warnings |
| State file exists, Phase 5 incomplete | Warns, proceeds (confirms if `--manual`) |
| State file missing | Warns, infers from git, proceeds (confirms if `--manual`) |
| PR not open or merged | Hard block, does not proceed |

Every step after the merge (Step 6) is best-effort — if one fails,
continue to the next.

---

## Gates

- PR must be open or already merged — hard block if closed
- Phase 5 complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- CI must pass before merge
- Confirmation only when `--manual` is passed
- Steps 1-6 run from the worktree; Step 7 runs from the project root
