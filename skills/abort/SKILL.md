---
name: abort
description: "Abort the current FLOW feature. Closes the PR, deletes the remote branch, removes the worktree, and deletes the state file. Available from any phase."
---

# FLOW Abort

Abandon the current feature completely. This is the escape hatch — available
from any phase, no prerequisites.

## Entry Check

Run this entry check as your very first action.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: print "WARNING: No state file found for
     branch '<branch>'. Will attempt best-effort cleanup using git state."
     Continue — do not stop.
   - If the file exists: print the feature name, branch, PR URL, and
     current phase from the JSON.

If the Read tool fails for any other reason, stop and show the error.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.0 — Abort — STARTING
============================================
```
````

## Steps

### Step 1 — Read state

If the state file exists, read `.flow-states/<branch>.json` from
the project root. Note `feature`, `branch`, `worktree`, `pr_number`,
and `pr_url`.

If the state file is missing, infer what you can:
- `branch` from `git branch --show-current`
- Detect worktree path from `git worktree list`
- Use the branch name as the feature name
- `pr_number` unknown — skip PR close step

Tell the user what was inferred.

### Step 2 — Confirm with user

This is destructive and irreversible. Use AskUserQuestion.

If the entry check printed warnings, include them in the confirmation:

> "Abort feature '<feature>'?
> ⚠ <any warnings from the entry check>
> This will close the PR, delete the remote branch, remove the worktree, and delete the state file and log. All uncommitted work in the worktree will be lost."

- **Yes, abort everything** — proceed
- **No, keep going** — stop here

### Step 3 — Navigate to project root

Use `git worktree list --porcelain` to find the project root. All cleanup commands
run from the project root, not from inside the worktree.

```bash
cd <project_root>
```

If navigation fails, tell the user and stop.

### Step 4 — Close the PR

If `pr_number` exists (from state or inferred):

```bash
gh pr close <pr_number> --comment "Aborted via /flow:abort"
```

If this fails (PR already closed/merged) or `pr_number` is unknown,
note it and continue — do not stop.

### Step 5 — Remove the worktree

```bash
git worktree remove .worktrees/<feature-name> --force
```

If this fails (already removed, doesn't exist, path mismatch), note it and continue.

### Step 6 — Delete the remote branch

```bash
git push origin --delete <branch-name>
```

If this fails (branch already deleted), note it and continue.

### Step 7 — Delete the local branch

From the project root (which is on main):

```bash
git branch -D <branch-name>
```

If this fails (branch already deleted), note it and continue.

### Step 8 — Delete the state file and log

Delete `.flow-states/<branch>.json` and `.flow-states/<branch>.log`.

If either doesn't exist, note it and continue.

### Done

Tell the user what was cleaned, what was already gone, and what failed.

Then print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.0 — Abort — COMPLETE
  Feature '<feature>' has been abandoned.
  PR closed, remote branch deleted,
  worktree removed, state file and log deleted.
============================================
```
````

Report which steps succeeded and which were already cleaned up.

## Rules

- Available from ANY phase — no phase gate
- Never run from inside the worktree — always navigate to project root first
- Always confirm with the user — this is irreversible
- Every step after confirmation is best-effort — if one fails, continue to the next
- Never rebase, never force push — just close and delete
