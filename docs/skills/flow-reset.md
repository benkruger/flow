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

Runs from any cwd in the repo tree, including linked worktrees. The
skill is a thin wrapper around the `bin/flow reset` subcommand,
which exec's a shell script that resolves the main repo root via
`git rev-parse --git-common-dir` and removes `.flow-states/` via
`rm -rf`. Requires explicit user confirmation before the wipe runs.

---

## What It Does

1. Asks for confirmation via `AskUserQuestion` — the user must
   explicitly approve the wipe.
2. Invokes `${CLAUDE_PLUGIN_ROOT}/bin/flow reset`. The Rust shim
   exec's the existing shell script, which resolves the main repo
   root (works from any cwd including worktrees), refuses to operate
   if path resolution returns "/" or empty as a safety check, and
   removes the entire `.flow-states/` directory via `rm -rf`. The
   directory shell is recreated on demand by subsequent flow-start
   invocations.
3. On non-zero exit, surfaces stderr so the user can investigate
   (typical causes: filesystem permissions, busy file, the safety
   check tripping when path resolution fails).

---

## What It Does NOT Do

- Does not close PRs (those stay open on GitHub)
- Does not remove worktrees (those persist on disk under `.worktrees/`)
- Does not delete branches (local or remote)
- Does not run any GitHub or git subprocess — no `gh`, no
  `git worktree`, no `git branch`. The operation is pure filesystem.

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
| **Prerequisite** | Active FLOW feature | None — runs from any cwd in the repo tree |

Use `/flow:flow-abort` to dispose of a single feature's GitHub state
and worktree. Use `/flow:flow-reset` to wipe local state across every
flow on the machine in one pass — typically AFTER per-flow aborts.

---

## Gates

- Requires explicit user confirmation before the wipe runs
- The operation is irreversible (local state is gone after the script returns 0)
