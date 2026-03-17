---
name: flow-reset
description: "Reset all FLOW artifacts. Closes PRs, removes worktrees, deletes branches, clears state files."
---

# FLOW Reset

Remove all FLOW artifacts from the current project. Maintainer-only — use when abandoned features leave orphaned worktrees, branches, state files, and PRs.

## Guard

Run:

```bash
git branch --show-current
```

If the current branch is NOT `main`, stop:

> "Must be on main branch to reset. Switch to main first."

## Step 1 — Inventory

Gather all FLOW artifacts. Display each category.

### Worktrees

Run:

```bash
git worktree list
```

List any worktrees besides the main working tree.

### State files

Use Glob to find all files in `.flow-states/` — JSON state files, logs, and the dev mode marker.

### Local branches

Run:

```bash
git branch
```

List any branches besides `main`.

### Remote branches

Run:

```bash
git branch -r
```

List any remote branches besides `origin/main` and `origin/HEAD`.

### Open PRs

Run:

```bash
gh pr list --state open --json number,headRefName
```

List any open PRs.

### Dev mode

Check if `.flow-states/.dev-mode` exists using the Read tool.

### Display inventory

Print the full inventory inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW Reset — Artifact Inventory
──────────────────────────────────────────────────

Worktrees: <count>
State files: <count>
Local branches: <count>
Remote branches: <count>
Open PRs: <count>
Dev mode: active / inactive
──────────────────────────────────────────────────
```
````

List each item under its category.

If nothing is found in any category, print:

> "No FLOW artifacts found. Nothing to reset."

And stop.

## Step 2 — Confirm

Use AskUserQuestion:

> "Destroy all listed artifacts? This cannot be undone."
>
> - **Yes, destroy everything**
> - **No, cancel**

If cancelled, stop.

## Step 3 — Execute

Process each category. Continue on failure — report errors at the end.

### Close open PRs

For each open PR, run `gh pr close <number>` where `<number>` is the PR number from the inventory.

### Remove worktrees

For each worktree (besides main), run `git worktree remove --force <path>` where `<path>` is the worktree path from the inventory.

### Delete remote branches

For each remote branch (besides `origin/main` and `origin/HEAD`), run `git push origin --delete <name>` where `<name>` is the branch name without the `origin/` prefix.

### Delete local branches

For each local branch (besides `main`), run `git branch -D <name>`.

### Delete state files and logs

For each file in `.flow-states/` (JSON, log, and any other files except `.dev-mode` which is handled below), run `rm .flow-states/<filename>`.

### Restore production marketplace (if dev mode was active)

If `.flow-states/.dev-mode` existed, run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin install flow@flow-marketplace
```

Then remove the marker:

```bash
rm .flow-states/.dev-mode
```

## Step 4 — Report

Print results inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW Reset — Complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

PRs closed: <count>
Worktrees removed: <count>
Remote branches deleted: <count>
Local branches deleted: <count>
State files deleted: <count>
Dev mode: restored / was inactive
Errors: <count or "none">
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If any step failed, list the error details below the summary.

## Step 5 — Verify

Run:

```bash
git worktree list
```

And:

```bash
git branch
```

Confirm only `main` remains. If stale artifacts persist, report them.
