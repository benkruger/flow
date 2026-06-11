---
title: "Phase 5: Complete"
nav_order: 6
---

# Phase 5: Complete

**Command:** `/flow-complete`

The final phase. Merges the PR into the integration branch (the
`base_branch` captured at flow-start — `main` for standard repos,
`staging`/`develop`/etc. for non-main-trunk repos), removes the git
worktree, and deletes the state file and log file. This is what fully
closes out a feature and resets the environment for the next one.

The autonomy mode is resolved from the state file's `skills.flow-complete`
config — the single source of truth for skill autonomy. In manual mode
(the default) it prompts for confirmation before the irreversible merge;
in auto mode it skips confirmation and proceeds directly to merge and
cleanup. Best-effort on cleanup steps — warns if the state file is missing or
Phase 5 is incomplete.

---

## Steps

### 1. Run complete-fast

`complete-fast` consolidates phase entry, state detection, PR status
check, merge of the integration branch into the feature branch, local
CI dirty check, and squash merge into a single call. Returns a `path` field for dispatch:
`"merged"` (auto happy path), `"already_merged"`, `"confirm"` (manual
mode), `"ci_stale"`, `"ci_failed"`, `"not_mergeable"`,
`"conflict"`, or `"max_retries"`. If the PR is already merged, skips
to finalize (step 5). If there are merge conflicts, resolves them and
self-invokes to continue.

`complete-fast` makes no GitHub-CI determination of its own. The full
local CI gate runs at every commit, so the only authority on whether
the PR can merge is `gh pr merge --squash`. When that command refuses
the merge (a required GitHub check is failing or still pending), the
verbatim `gh` stderr is surfaced as the `not_mergeable` path with a
`reason` field, and the skill reports it and stops — FLOW does not
poll GitHub CI on the PR's behalf. `ci_failed` covers only local-CI
failure. See `skills/flow-complete/SKILL.md` Step 1 for the full
dispatch sequence per `.claude/rules/docs-with-behavior.md`.

### 2. Run local CI gate

Runs `bin/flow ci` locally to catch test failures after merging the
integration branch into the feature branch.
If it fails, launch the ci-fixer sub-agent to diagnose and fix.

### 3. Confirm with user (manual mode)

In manual mode (the default), explicit confirmation is required
before the irreversible squash merge. Any warnings from the preflight
are included in the confirmation message. On approval,
`bin/flow confirm-merge` writes a single-use merge-approval marker
that the merge step requires. Skipped in auto mode.

### 4. Merge PR

`complete-merge` handles the freshness check and squash merge in a
single script call. Verifies the branch is up-to-date with the
integration branch before merging. When `flow-complete` is configured
manual, it resolves the mode from the state file and requires the
single-use merge-approval marker — with no marker the merge is refused
(`merge_not_confirmed`) and the skill loops back to step 3 to
re-confirm. This structural gate means a lost mode flag cannot merge a
manual-configured flow unconfirmed. If the integration branch has
moved, merges the new commits, pushes, and runs sentinel-gated CI
inline on the freshly-merged tree before deferring: a pass returns
`ci_rerun` (the skill loops back to step 2, where CI skips on the
sentinel match, then re-attempts the merge); a failure returns
`ci_failed` with the CI output so the skill launches ci-fixer
directly rather than deferring an untested tree. A retry limit of 3
prevents infinite loops under high contention. Once up-to-date,
squash-merges via `gh pr merge --squash`. When `gh pr merge` refuses
(a required GitHub check is failing or pending), surfaces the verbatim
stderr as `not_mergeable` and the skill reports it and stops.

### 5. Finalize: post-merge + cleanup

`complete-finalize` handles all post-merge work AND cleanup in a single
best-effort call. Self-gates before any side effect: when the caller's
canonicalized cwd equals or sits beneath the canonicalized `--worktree`
argument, the command returns
`{"status":"error","reason":"cwd_inside_worktree"}` instead of removing
the worktree, so a missed `cd <project_root>` produces a clean error
rather than stranding the shell in a deleted directory.

- Phase transition complete (records timing)
- PR body rendering (What, Artifacts, Plan, Phase Timings, Token
  Cost, Review Findings, Learn Findings, State File, Session Log,
  Issues Filed)
- Close referenced GitHub issues from the start prompt
- Generate business-friendly summary (feature name, prompt,
  per-phase timeline, artifact counts)
- Remove "Flow In-Progress" labels
- Cascade-close downstream issues whose blockers are now all closed
  (walks GitHub's native blocked-by dependency graph) and close
  empty milestones
- Post Slack notification
- Worktree tmp directory removal, worktree removal, remote and
  local branch deletion, and deletion of the state file, plan file,
  log file, frozen-phases file, CI sentinel, timings
  file, closed-issues file, issues file, and adversarial test file
  (glob-matched as `.flow-states/<branch>/adversarial_test.*`),
  followed by `git pull origin <base_branch>` (the integration branch)
- Integration-branch CI: when `--pull` was passed AND the pull
  completed cleanly, runs sentinel-gated `ci::run_impl` against the
  integration branch rather than fabricating a sentinel. It runs
  format/lint/build/test on the merged local tree and writes the
  base-branch sentinel ONLY on a real pass (so the next `start-gate`
  can skip CI). A failure is surfaced in the result's `base_ci`
  field as a warning — the squash merge already landed, so this is
  not a rollback. The next `/flow:flow-start` re-runs CI on the base
  branch under the start lock and routes the same failure to
  ci-fixer.

Each cleanup step is best-effort — if one fails, the rest still run.

### 6. Cleanup results

Reports what `complete-finalize` cleaned up in Step 5: what was
removed, what was already gone, and what failed.

---

## What You Get

By the end of Phase 5:

- PR squash-merged into the integration branch
- Referenced GitHub issues closed (extracted from the start prompt)
- Remote branch auto-deleted by GitHub after merge
- Worktree and all its contents removed
- Business-friendly summary displayed in Done banner: feature name, prompt,
  per-phase timeline, and artifact counts (issues filed, notes captured)
- PR link displayed in Done banner for quick access
- State file deleted — no more session hook injection for this feature
- Log file deleted — no stale logs left behind
- Local integration branch pulled up to date with the merged feature code
- Local environment clean and ready for the next feature

---

## Idempotent Design

The skill is safe to re-invoke:

| State | Behavior |
|---|---|
| PR already merged | Runs finalize (post-merge + cleanup) |
| Main already merged into branch | No-op merge |
| CI already passing | Skips to merge |
| Freshness retry in progress | Loops back through CI gate, respects retry limit |
| State file already deleted | Exits cleanly |

---

## Best-Effort Behavior

| Scenario | Behavior |
|---|---|
| State file exists, Learn (Phase 4) complete | Normal merge and cleanup — no warnings |
| State file exists, Learn (Phase 4) incomplete | Warns, proceeds (confirms in manual mode) |
| State file missing | Warns, infers from git, proceeds (confirms in manual mode) |
| PR not open or merged | Hard block, does not proceed |

Every operation inside `complete-finalize` (Step 5) is best-effort — if
one fails, continue to the next.

---

## Gates

- PR must be open or already merged — hard block if closed
- Learn (Phase 4) complete is a warning, not a hard block
- Missing state file is a warning, not a hard block
- CI must pass before merge
- Confirmation in manual mode (the default); skipped in auto mode
- Mode is resolved from the state file on every skill entry, including
  `--continue-step` re-entries — neither the banner skip nor the SOFT-GATE
  skip bypasses it, so a resumed run cannot lose the configured mode
- Manual-mode merge requires a single-use confirmation marker — both merge
  surfaces consume it before the freshness check and structurally refuse the
  squash-merge without it, so a lost mode flag cannot merge unconfirmed
- Steps 1-4 run from the worktree; Step 5 (finalize) runs from the project root
