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

The skill is a thin wrapper around `bin/flow cleanup --all`. The
Rust primitive walks `.flow-states/` once, builds a categorized
inventory of what is inside (flows with `state.json`, state-less
orphan directories, top-level files, machine-level singletons, and
the base-branch CI sentinel directory), then removes the entire
`.flow-states/` directory via `fs::remove_dir_all`. The directory
shell is recreated on demand by subsequent flow-start invocations.

## Guard

Reset must run from the project root with the repository's integration branch
checked out. Running from a worktree would attempt to remove the worktree
mid-execution. The integration branch is whatever `origin/HEAD` resolves to —
`main` for most repos, but `staging`, `develop`, `master`, etc. for others —
and `bin/flow base-branch` prints the resolved name.

Run both commands and compare the outputs:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

```bash
git branch --show-current
```

If the current branch is NOT the resolved base branch, stop and substitute the
resolved name into the rejection message:

> "Must be on `<base_branch>` branch to reset. Switch to `<base_branch>` first."

## Step 1 — Inventory

Print the inventory of `.flow-states/` contents without modifying disk. The
JSON output's `inventory` object carries five arrays naming the entries the
walker found: `flows_with_state`, `orphan_dirs`, `top_level_files`,
`singletons`, and `sentinel_dirs`. `flow_states_dir` reports the dry-run
outcome (`"would_remove"` when the directory exists, `"skipped"` when it is
already missing).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all --dry-run
```

Render the inventory as a five-row table inline inside a fenced code block so
the user can review it before approving the destructive run. The Category
column names the bucket; the Count column reports `inventory.<bucket>.len()`;
the Examples column lists up to five names from each bucket.

````text
.flow-states/ contents:

Category                Count  Examples
----------------------  -----  ----------------------------------------------
Flows with state.json       N  name1, name2, ...
Orphan dirs (no state)      N  name1, name2, ...
Top-level files             N  name1, name2, ...
Machine singletons          N  orchestrate.json
Sentinel dirs               N  main/
````

If all five inventory arrays are empty AND `flow_states_dir` is `"skipped"`,
print:

> "Nothing to reset."

And stop.

## Step 2 — Confirm

This is destructive and irreversible — for `.flow-states/` only. PRs,
worktrees, and branches stay untouched. Use AskUserQuestion:

> "Wipe `.flow-states/`? This removes local FLOW state for every flow on this
> machine. PRs, worktrees, and branches are NOT touched. For per-flow GitHub
> cleanup, run `/flow:flow-abort <branch>` separately first."
>
> - **Yes, wipe `.flow-states/`**
> - **No, cancel**

If cancelled, stop.

## Step 3 — Execute

Run the live cleanup. The Rust primitive recursively removes `.flow-states/`
via `fs::remove_dir_all`. On success, `flow_states_dir` reports `"deleted"`;
on filesystem failure (permissions, busy file, etc.), it reports
`"failed: <reason>"` and the partial state of the directory is left on disk
for the user to inspect.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all
```

Render the inventory table inline (same shape as Step 1) so the user sees
what was on disk at decision time, followed by the `flow_states_dir`
outcome. A `"failed: <reason>"` outcome surfaces to the user directly.

## Step 4 — Verify

Confirm the result of Step 3 by parsing the `flow_states_dir` field from the
JSON output. `"deleted"` indicates the directory was removed successfully;
`"skipped"` indicates the directory was already absent before the run; any
`"failed: <reason>"` value indicates the recursive removal returned an
`io::Error` and the partial state of the directory is still on disk for the
user to inspect.

If the outcome was anything other than `"deleted"` or `"skipped"`, surface
the `flow_states_dir` value to the user so they can investigate. Otherwise
print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW Reset — Complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Rules

- Available from the integration branch only — running from a worktree is unsafe
- Reset wipes local state only. For each open PR or worktree, run
  `/flow:flow-abort <branch>` separately before reset.
- Never rebase, never force push — reset never touches GitHub state