---
name: cleanup
description: "Phase 8: Cleanup — remove the worktree and delete the state file. Final phase. Requires Phase 7: Reflect to be complete."
model: haiku
---

# FLOW Cleanup — Phase 8: Cleanup

<SOFT-GATE>
Run this phase entry check as your very first action. This gate never
blocks — it records warnings for the confirmation step.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: record warning "No state file found for
     branch '<branch>'."
4. If the file exists, check `phases.7.status` in the JSON.
   - If not `"complete"`: record warning "Phase 7 not complete
     (status: <actual status>)."

Carry any warnings forward to the confirmation step in Step 2.
</SOFT-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.3 — Phase 8: Cleanup — STARTING
============================================
```
````

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command with exit code capture:

```bash
COMMAND; EC=$?; exit $EC
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 8] Step X — desc (exit EC)
```

Get `<branch>` from the state file or `git branch --show-current`.

---

## Steps

### Step 1 — Read state (handle missing)

If the state file exists, read `.flow-states/<branch>.json` from
the project root. Note the `worktree` and `feature` values.

If the state file is missing, infer what you can:
- `branch` from `git branch --show-current`
- Detect worktree path from `git worktree list`
- Use the branch name as the feature name

Tell the user what was inferred:
> "No state file found. Inferring from git: branch '<branch>',
> worktree '<path>'."

### Step 2 — Confirm with user

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

### Steps 3–6 — Run cleanup script

Run the cleanup script from the project root:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path>
```

The script outputs JSON with a `steps` dict showing what happened to each resource (worktree, state\_file, log\_file). Each step reports "removed"/"deleted", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone, and what failed.

### Done — Print banner

Format the total `cumulative_seconds` (from the state file read in Step 1) as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.8.3 — Phase 8: Cleanup — COMPLETE (<formatted_time>)
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
