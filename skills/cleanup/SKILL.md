---
name: cleanup
description: "Phase 7: Cleanup — remove the worktree and delete the state file. Final phase. Requires Phase 6: Reflect to be complete."
model: haiku
---

# FLOW Cleanup — Phase 7: Cleanup

<SOFT-GATE>
Run this entry check as your very first action. This gate never
blocks — it records warnings for the confirmation step.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file exists: extract `feature`, `worktree`, and
     `cumulative_seconds`. Check `phases.6.status` — if not `"complete"`,
     record warning "Phase 6 not complete (status: <actual status>)."
   - If the file does not exist: record warning "No state file found for
     branch '<branch>'."

Use these values for all subsequent steps — do not re-read the state file
or re-run git commands to gather the same information.

Carry any warnings forward to the confirmation step in Step 2.
</SOFT-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 7: Cleanup — STARTING
============================================
```
````

## Logging

No logging for this phase. Cleanup deletes the log file as part of its
operation — writing log entries that are immediately deleted is pointless.

---

## Steps

### Step 1 — Handle missing state file

This step only runs if the SOFT-GATE found no state file. If the state
file existed, the SOFT-GATE already extracted all needed values — skip
to Step 2.

Infer what you can:
- `branch` from `git branch --show-current` (already known from the gate)
- Detect worktree path from `git worktree list`
- Use the branch name as the feature name

Tell the user what was inferred:
> "No state file found. Inferring from git: branch '<branch>',
> worktree '<path>'."

### Step 2 — Check PR merge status

Verify the PR has been merged before allowing cleanup.

If the state file had a `pr_number`, run:

```bash
gh pr view <pr_number> --json state --jq .state
```

If the state file had no `pr_number` (or no state file was found), try the branch name:

```bash
gh pr view <branch> --json state --jq .state
```

If the result is `MERGED`, continue to Step 3.

If the result is anything else (e.g., `OPEN`, `CLOSED`), stop:

> "Your PR must be merged before cleanup. Current PR status: **<state>**.
> Merge the PR first, then run `/flow:cleanup` again."

If both commands fail (no PR found), stop:

> "Could not find a PR for this branch. Merge your PR first, then run
> `/flow:cleanup` again."

### Step 3 — Confirm with user

This phase is destructive and irreversible. Use AskUserQuestion.

If the SOFT-GATE printed warnings, include them in the confirmation so
the user knows what's off before confirming:

> "Ready to clean up feature '<feature>'?
> ⚠ <any warnings from the gate>
> This will remove the worktree and delete the state file and log permanently."

- **Yes, clean up** — proceed
- **No, not yet** — stop here

If there were no warnings:

> "Ready to clean up feature '<feature>'?
> This will remove the worktree and delete the state file and log permanently."

- **Yes, clean up** — proceed
- **No, not yet** — stop here

### Step 4 — Run cleanup script

Run the cleanup script from the project root:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path>
```

The script outputs JSON with a `steps` dict showing what happened to each resource (worktree, state\_file, log\_file). Each step reports "removed"/"deleted", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone, and what failed.

### Done — Print banner

For the banner below, compute `<formatted_time>` from the integer `cumulative_seconds` read in Step 1: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60. Do not write the formatted string back to the state file.

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 7: Cleanup — COMPLETE (<formatted_time>)
  Feature '<feature>' is fully done.
  Worktree removed, state file and log deleted.
============================================
```
````

## Rules

- Never run from inside the worktree — always navigate to project root first
- Always confirm with the user before cleanup — removal is irreversible
- State file deletion is what resets the session hook — do not skip it
- Every step after confirmation is best-effort — if one fails, continue to the next
