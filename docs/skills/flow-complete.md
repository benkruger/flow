---
title: /flow-complete
nav_order: 12
parent: Skills
---

# /flow-complete

**Phase:** 5 — Complete

**Usage:** `/flow-complete` or `/flow-complete --continue-step`

The final phase. Merges the PR into the integration branch (`base_branch`),
removes the git worktree, and deletes the state file. Mode is resolved
from the state file's `skills.flow-complete` config — the single source
of truth for skill autonomy (default: manual, prompts for confirmation
before the irreversible merge; auto skips confirmation and merges
directly). There are no `--auto`/`--manual` flags. The `--continue-step`
flag is used for self-invocation after mid-phase commits (merge
conflict resolution or CI fix) — it skips the Announce banner and the
SOFT-GATE's warning recording, but mode resolution still runs first on
every entry (including `--continue-step` re-entries), and the run then
dispatches via the Resume Check.

---

## What It Does

1. **Run complete-fast** — consolidates phase entry, state detection, PR
   status check, merge of the integration branch into the feature branch,
   local CI dirty check, and squash merge into a single call. Returns a `path` field for dispatch:
   `"merged"` (auto happy path), `"already_merged"`, `"confirm"` (manual
   mode), `"ci_stale"`, `"ci_failed"`, `"not_mergeable"`,
   `"conflict"`, or `"max_retries"`. It makes no GitHub-CI determination of
   its own — `gh pr merge --squash` is the authority, and when it refuses
   the merge (a required GitHub check is failing or pending) the verbatim
   `gh` stderr is surfaced as `not_mergeable` with a `reason` field for the
   skill to report and stop. If the PR is already merged, skips to finalize (step 5)
2. **Local CI gate** — `bin/flow ci` catches test failures after merging
   the integration branch into the feature branch. If it fails, ci-fixer
   commits a fix and self-invokes to re-check
3. **Confirm** (manual mode only) — explicit confirmation before the
   irreversible merge. On approval, `bin/flow confirm-merge` writes a
   single-use merge-approval marker that the merge step requires.
   Offers approve, decline, or feedback options. Skipped by default
4. **Merge** — `complete-merge` handles the freshness check and squash merge.
   When `flow-complete` is configured manual, it requires and consumes the
   merge-approval marker; with no marker the merge is refused
   (`merge_not_confirmed`) and the skill loops back to confirmation.
   If the integration branch moved, loops back through CI. Surfaces a
   `not_mergeable` stop-and-report when `gh pr merge` refuses, and detects
   merge conflicts
5. **Finalize** — `complete-finalize` handles phase completion, PR body
   rendering, issues summary, closing referenced issues, summary generation,
   label removal, cascade-close of downstream issues whose blockers are now
   all closed (walks the native blocked-by graph), Slack notification,
   worktree removal, state/log deletion, and git pull — all best-effort in
   a single call
6. **Cleanup results** — reports what `complete-finalize` cleaned up: what
   was removed, what was already gone, and what failed

---

## Why State File Deletion Matters

Deleting `.flow-states/<branch>/state.json` is the clean exit from the
FLOW workflow. It removes the branch-scoped state that other FLOW
commands (phase gates, status, TUI) rely on to detect an active flow.

---

## Idempotent Design

The skill is safe to re-invoke. Each step checks its precondition and
skips if already done: merged PRs
skip to finalize, up-to-date branches skip the merge, passing CI skips
the wait. After finalize completes, the next invocation finds no state
file and exits cleanly.

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Review (Phase 3) complete | Normal merge and cleanup — no warnings |
| State file exists, Review (Phase 3) incomplete | Warns, proceeds (confirms in manual mode) |
| State file missing | Warns, infers from git state, proceeds (confirms in manual mode) |
| PR closed but not merged | Hard block, does not proceed |

Every operation inside `complete-finalize` (Step 5) is best-effort. If
label removal or issue closing fails, it continues to cleanup. If the
state file doesn't exist, it notes that and finishes.

---

## Gates

- PR must be open or already merged — hard block if closed
- CI must pass before merge
- Review (Phase 3) complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- Confirmation only when mode is manual (via `.flow.json`)
- Manual-mode merge requires a single-use confirmation marker — both merge
  surfaces resolve the mode and structurally refuse the squash-merge
  without it, so a lost mode flag cannot merge unconfirmed
- Steps 1-4 run from the worktree; Step 5 (finalize) runs from the project root
- Merge is irreversible; branch and worktree deletion is handled by `complete-finalize`
- If merge fails, stop and report — never retry with additional flags or elevated privileges
