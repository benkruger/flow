---
title: /flow-reset
nav_order: 18
parent: Skills
---

# /flow-reset

**Phase:** Any (no phase gate)

**Usage:** `/flow-reset`

The nuclear option. Removes all FLOW artifacts from the current project in one
sweep — worktrees, state files, local and remote branches, and open PRs.

Must be run from the `main` branch. Inventories everything before acting and
requires explicit confirmation.

---

## What It Does

1. Checks that the current branch is `main`
2. Runs `bin/flow cleanup . --all --dry-run` to print the inventory
   of artifacts that would be removed: every flow's per-branch
   directory under `.flow-states/<branch>/`, the orchestration queue
   singleton (`orchestrate.json`), the base-branch CI sentinel
   directory (`.flow-states/main/`), and any residual start-lock
   entries
3. Displays the JSON inventory and asks for confirmation
4. Runs `bin/flow cleanup . --all` to perform the live cleanup. Each
   per-flow cleanup closes the PR, removes the worktree, deletes
   local and remote branches, removes the branch directory, and
   sweeps the matching start-queue entry. The walk continues when
   individual per-flow steps fail.
5. Verifies via `git worktree list` and `git branch --list` that
   only `main` remains

---

## When to Use It

- Multiple abandoned features have left orphaned artifacts
- You want a completely clean slate (no worktrees, no state files, no branches)
- You are starting fresh after experimenting with FLOW

---

## vs /flow-abort

| | `/flow-abort` | `/flow-reset` |
|---|---|---|
| **Scope** | Single feature | All features |
| **When** | Abandon one feature | Clean everything |
| **State file** | Required (warns if missing) | Not required |
| **Prerequisite** | Active FLOW feature | Must be on `main` |

Use `/flow-abort` to walk away from one feature.
Use `/flow-reset` to start completely fresh.

---

## Gates

- Must be on `main` branch
- Requires explicit user confirmation before any destructive action
- All operations are irreversible
