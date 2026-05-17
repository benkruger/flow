---
title: /flow-reset
nav_order: 18
parent: Skills
---

# /flow-reset

**Phase:** Any (no phase gate)

**Usage:** `/flow-reset`

Wipes `.flow-states/` on this machine in one pass. PRs, worktrees, and
branches are NOT touched — those require per-flow `/flow:flow-abort`
invoked against each branch separately before reset.

Must be run from the repository's integration branch (whatever
`bin/flow base-branch` resolves to — `main`, `staging`, `develop`, etc.).
Inventories everything before acting and requires explicit confirmation.

---

## What It Does

1. Checks that the current branch matches `bin/flow base-branch`
2. Runs `bin/flow cleanup . --all --dry-run` to build a categorized
   inventory of `.flow-states/` contents: flows with `state.json`,
   state-less orphan directories, top-level files (including stray
   symlinks), machine-level singletons (`orchestrate.json`), and the
   base-branch CI sentinel directory at `.flow-states/<base-branch>/`
3. Renders the inventory as a five-row table and asks for confirmation
4. Runs `bin/flow cleanup . --all` to remove the entire `.flow-states/`
   directory via `fs::remove_dir_all`. The directory shell is
   recreated on demand by subsequent flow-start invocations. On a
   filesystem failure (permissions, busy file, etc.), the partial
   state of the directory is left on disk and the `flow_states_dir`
   field reports `"failed: <reason>"` so the user can inspect.
5. Reads `flow_states_dir` from the JSON output to confirm the
   outcome (`"deleted"` on success, `"skipped"` if the directory was
   already absent, `"failed: <reason>"` on filesystem error).

---

## What It Does NOT Do

- Does not close PRs (those stay open on GitHub)
- Does not remove worktrees (those persist on disk under `.worktrees/`)
- Does not delete branches (local or remote)
- Does not run any subprocess — no `gh`, no `git worktree`, no
  `git branch`. The operation is pure filesystem.

For per-flow GitHub cleanup, run `/flow:flow-abort <branch>`
separately for each flow before invoking `/flow:flow-reset`.

---

## When to Use It

- The `.flow-states/` directory has accumulated orphan state from
  killed Complete passes, manually-cleaned-up flows, or interrupted
  flow-start operations
- You want to wipe local FLOW state without affecting GitHub state
  (PRs stay open) or git state (worktrees and branches stay)
- You are starting fresh on a machine after experimenting with FLOW

---

## vs /flow-abort

| | `/flow-abort` | `/flow-reset` |
|---|---|---|
| **Scope** | Single feature | All local FLOW state |
| **GitHub side** | Closes PR, deletes remote branch | Untouched |
| **Worktree** | Removes via `git worktree remove` | Untouched |
| **Local branch** | Deletes via `git branch -D` | Untouched |
| **State file(s)** | Removes one branch's directory | Wipes entire `.flow-states/` |
| **Prerequisite** | Active FLOW feature | Must be on the integration branch |

Use `/flow:flow-abort` to dispose of a single feature's GitHub state
and worktree. Use `/flow:flow-reset` to wipe local state across every
flow on the machine in one pass — typically AFTER per-flow aborts.

---

## Gates

- Must be on the integration branch (whatever `bin/flow base-branch` returns)
- Requires explicit user confirmation before the wipe runs
- The operation is irreversible (local state is gone after `flow_states_dir == "deleted"`)
