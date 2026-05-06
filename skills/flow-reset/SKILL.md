---
name: flow-reset
description: "Reset all FLOW artifacts. Closes PRs, removes worktrees, deletes branches, clears state files."
---

# FLOW Reset

Remove every FLOW artifact from the current project in one pass: every PR, every
worktree, every per-branch state directory, every residual start-lock entry, the
orchestration queue singleton, and the base-branch CI sentinel directory. Use
when abandoned features have left orphaned worktrees, branches, state files,
and PRs that the per-feature `/flow:flow-abort` cannot reach.

The skill is a thin wrapper around `bin/flow cleanup --all`. The Rust primitive
walks `.flow-states/` for every flow with a `state.json`, runs the per-branch
cleanup against each, then runs the three machine-level tail steps
(`orchestrate.json`, `.flow-states/main/`, `start-queue/` sweep). The directory
shells (`.flow-states/`, `.flow-states/start-queue/`) survive so subsequent
flow-starts do not need to recreate them.

## Guard

Reset must run from the project root with `main` checked out. Running from a
worktree would attempt to remove the worktree mid-execution.

```bash
git branch --show-current
```

If the current branch is NOT `main`, stop:

> "Must be on main branch to reset. Switch to main first."

## Step 1 — Inventory

Print the inventory of what `--all` would remove without modifying disk. The
JSON output's `flows[]`, `orchestrate_json`, `main_dir`, and `queue_sweep`
fields describe every artifact that the live run would touch.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all --dry-run
```

Render the JSON output inline inside a fenced code block so the user can
review it before approving the destructive run.

If `flows[]` is empty AND `orchestrate_json` is `"skipped"` AND `main_dir` is
`"skipped"` AND `queue_sweep` is `"skipped"`, print:

> "No FLOW artifacts found. Nothing to reset."

And stop.

## Step 2 — Confirm

This is destructive and irreversible. Use AskUserQuestion:

> "Destroy all listed artifacts? This cannot be undone."
>
> - **Yes, destroy everything**
> - **No, cancel**

If cancelled, stop.

## Step 3 — Execute

Run the live cleanup. Each per-branch cleanup may report `"failed: <reason>"`
for individual steps (a missing worktree, an already-deleted remote branch);
the walk continues to the next flow regardless.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all
```

Render the JSON output inline so the user can see the per-flow `steps` map and
the tail-step results.

## Step 4 — Verify

Confirm only `main` remains.

```bash
git worktree list
```

```bash
git branch --list
```

If any worktree besides the main working tree appears, or any local branch
besides `main`, list the survivors so the user can investigate. Otherwise
print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW Reset — Complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Rules

- Available from `main` only — running from a worktree is unsafe
- Never rebase, never force push — the cleanup primitive only invokes the
  destructive surfaces the per-feature `/flow:flow-abort` already uses
- Every step after confirmation is best-effort — if one per-flow step fails,
  the next flow still processes
