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
   - If the file exists: extract `feature`, `branch`, `worktree`,
     `pr_number`, and `pr_url`. Print the feature name, branch, PR URL,
     and current phase.
   - If the file does not exist: infer what you can from git state:
     - `branch` from `git branch --show-current` (already known)
     - Detect worktree path from `git worktree list`
     - Use the branch name as the feature name
     - `pr_number` unknown — skip PR close step later
     - Print "WARNING: No state file found for branch '<branch>'. Will
       attempt best-effort cleanup using git state." and tell the user
       what was inferred. Continue — do not stop.

If the Read tool fails for any other reason, stop and show the error.

Use these values for all subsequent steps — do not re-read the state file
or re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.11.0 — Abort — STARTING
============================================
```
````

## Steps

### Step 1 — Confirm with user

This is destructive and irreversible. Use AskUserQuestion.

If the entry check printed warnings, include them in the confirmation:

> "Abort feature '<feature>'?
> ⚠ <any warnings from the entry check>
> This will close the PR, delete the remote branch, remove the worktree, and delete the state file and log. All uncommitted work in the worktree will be lost."

- **Yes, abort everything** — proceed
- **No, keep going** — stop here

### Steps 2–7 — Run cleanup script

Run the cleanup script from the project root with abort flags:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path> --pr <pr_number> --delete-remote
```

If `pr_number` is unknown, omit `--pr`. The `--delete-remote` flag tells the script to also delete the remote branch and local branch.

The script outputs JSON with a `steps` dict showing what happened to each resource (pr\_close, worktree, remote\_branch, local\_branch, state\_file, log\_file). Each step reports "closed"/"removed"/"deleted", "skipped", or "failed: reason".

### Done

Tell the user what was cleaned, what was already gone, and what failed.

Then print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.11.0 — Abort — COMPLETE
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
