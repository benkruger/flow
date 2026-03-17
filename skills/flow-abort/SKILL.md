---
name: flow-abort
description: "Abort the current FLOW feature. Closes the PR, deletes the remote branch, removes the worktree, and deletes the state file. Available from any phase. Use --manual for confirmation prompt."
---

# FLOW Abort

Abandon the current feature completely. This is the escape hatch — available
from any phase, no prerequisites.

## Usage

```text
/flow:flow-abort
/flow:flow-abort --auto
/flow:flow-abort --manual
```

- `/flow:flow-abort` — uses configured mode from the state file (default: auto)
- `/flow:flow-abort --auto` — skips confirmation and proceeds directly to cleanup
- `/flow:flow-abort --manual` — prompts for user confirmation before any destructive action

## Mode Resolution

1. If `--auto` was passed → mode is **auto**
2. If `--manual` was passed → mode is **manual**
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-abort` value.
4. If the state file has no `skills` key → use built-in default: **auto**

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

Resolve the mode using the Mode Resolution rules above.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v0.31.4 — Abort — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Confirm with user (manual mode only)

Skip this step if mode is **auto** — proceed directly to Steps 2–7.

If mode is **manual**, this is destructive and irreversible. Use AskUserQuestion.

If the entry check printed warnings, include them in the confirmation:

> "Abort feature '<feature>'?
> ⚠ <any warnings from the entry check>
> This will close the PR, delete the remote branch, remove the worktree, and delete the state file and log. All uncommitted work in the worktree will be lost."

- **Yes, abort everything** — proceed
- **No, keep going** — stop here

### Steps 2–7 — Run cleanup script

Run the cleanup script from the project root with abort flags:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path> --pr <pr_number>
```

If `pr_number` is unknown, omit `--pr`. The cleanup script always deletes remote and local branches.

The script outputs JSON with a `steps` dict showing what happened to each resource (pr\_close, worktree, remote\_branch, local\_branch, state\_file, log\_file, ci\_sentinel). Each step reports "closed"/"removed"/"deleted", "skipped", or "failed: reason".

### Done

Tell the user what was cleaned, what was already gone, and what failed.

Then output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.31.4 — Abort — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Feature '<feature>' has been abandoned.
  PR closed, remote branch deleted,
  worktree removed, state file and log deleted.
```
````

Report which steps succeeded and which were already cleaned up.

## Rules

- Available from ANY phase — no phase gate
- Never run from inside the worktree — always navigate to project root first
- Confirm with the user only when mode is **manual**
- Every step after confirmation is best-effort — if one fails, continue to the next
- Never rebase, never force push — just close and delete
