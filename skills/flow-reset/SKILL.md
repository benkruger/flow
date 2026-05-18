---
name: flow-reset
description: "Wipe `.flow-states/` on this machine in one pass. PRs, worktrees, and branches are NOT touched — those require per-flow `/flow:flow-abort`."
---

# FLOW Reset

Wipe `.flow-states/` on this machine in one pass. Use when abandoned
features have left orphaned state directories that the per-feature
`/flow:flow-abort` cannot reach. PRs, worktrees, and branches are
NOT touched — those require per-flow `/flow:flow-abort` invoked
against each branch separately before reset.

The skill is a thin wrapper around the `bin/flow reset` subcommand,
which exec's a shell script that resolves the main repo root via
`git rev-parse --git-common-dir` so it works from any cwd in the
repo tree (including linked worktrees) and removes `.flow-states/`
via `rm -rf`.

## Step 1 — Confirm

This is destructive and irreversible — for `.flow-states/` only. PRs,
worktrees, and branches stay untouched. Use AskUserQuestion:

> "Wipe `.flow-states/`? This removes local FLOW state for every flow on this
> machine. PRs, worktrees, and branches are NOT touched. For per-flow GitHub
> cleanup, run `/flow:flow-abort <branch>` separately first."
>
> - **Yes, wipe `.flow-states/`**
> - **No, cancel**

If cancelled, stop.

## Step 2 — Execute

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow reset
```

If the script exits 0, print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW Reset — Complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If the script exits non-zero, surface the stderr to the user so they can
investigate. The script's only failure modes are filesystem errors
(permissions, busy file) and the safety check that refuses to operate at
the filesystem root.

## Rules

- Reset wipes local state only. For each open PR or worktree, run
  `/flow:flow-abort <branch>` separately before reset.
- Never rebase, never force push — reset never touches GitHub state
