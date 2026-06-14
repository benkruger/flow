---
title: /flow-abort
nav_order: 17
parent: Skills
---

# /flow-abort

**Phase:** Any (no phase gate)

**Usage:** `/flow-abort`

The escape hatch. Abandons the current feature completely — closes the PR,
deletes the remote branch, removes the worktree, and deletes the state file.

Available from any phase, no prerequisites. Best-effort — warns if the state
file is missing.

The abort-confirmation mode is resolved from the state file's
`skills.flow-abort` config — the single source of truth for skill
autonomy (default: manual, prompts before any destructive action; auto
skips the confirmation). There are no `--auto`/`--manual` flags.

---

## What It Does

1. Reads `.flow-states/<branch>/state.json` for feature details
   (or infers from git state if the file is missing)
2. Removes the "Flow In-Progress" label from any issues referenced in the prompt (if state file exists)
3. Confirms with the user before any destructive action, including any
   warnings from the entry check
4. Runs `bin/flow cleanup <project_root> --branch <branch> --worktree <worktree> --pr <pr>`,
   which performs every cleanup step under one Rust subcommand:
   `pr_close` (`gh pr close`), `worktree`
   (`git worktree remove --force`), `remote_branch`
   (`git push origin --delete`), `local_branch` (`git branch -D`),
   `branch_dir` (recursive remove of `.flow-states/<branch>/`
   covering state file, log, plan, DAG, frozen phases, CI sentinel,
   timings, closed-issues record, issues summary, scratch rule
   content, commit message, and start prompt), and `queue_entry`
   (the matching start-lock entry under
   `.flow-states/start-queue/<branch>`). Each step reports
   `"closed"`/`"removed"`/`"deleted"`, `"skipped"`, or
   `"failed: <reason>"` in the JSON output

Every step after confirmation is best-effort — if one fails (e.g., PR
already closed, worktree already removed), it continues to the next.

---

## When to Use It

- You started a feature and decided not to pursue it
- The approach is fundamentally wrong and you want a clean slate
- You want to abandon work without going through Review and Complete

---

## vs /flow-complete

| | `/flow-complete` | `/flow-abort` |
|---|---|---|
| **When** | After Review (Phase 4: Complete) | Any phase |
| **PR** | Squash-merged into main | Closed |
| **Remote branch** | Auto-deleted by GitHub | Deleted (via cleanup) |
| **Worktree** | Removed | Removed |
| **State file** | Deleted | Deleted |
| **Missing state** | Warns, proceeds | Warns, proceeds |

Use `/flow-complete` for the happy path after a completed feature.
Use `/flow-abort` to walk away from a feature entirely.

---

## Gates

- No phase gate — available from any phase
- State file not required — warns if missing, infers from git state
- Requires user confirmation when mode is manual (via `.flow.json`)
- Must run from the project root — never from inside the worktree
- All operations are irreversible
