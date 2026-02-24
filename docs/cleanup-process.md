# Cleanup Process

Shared process used by both `/flow:cleanup` (Phase 8) and `/flow:abort` (escape hatch).
Each calling skill handles its own announce, state reading, and confirmation, then follows these steps.

## Step 1 — Navigate to project root

Use `git worktree list --porcelain` to find the project root. All cleanup commands
run from the project root, not from inside the worktree.

```bash
cd <project_root>
```

If navigation fails, tell the user and stop.

## Step 2 — Remove the worktree

```bash
git worktree remove .worktrees/<feature-name> --force
```

If this fails (already removed, doesn't exist, path mismatch), note it and continue.

## Step 3 — Delete the state file and log

Delete `.claude/flow-states/<branch>.json` and `.claude/flow-states/<branch>.log`.

If either doesn't exist, note it and continue.

## Step 4 — Report results

Tell the user what was cleaned, what was already gone, and what failed.
The calling skill adds its own banner after the report.

## Hard Rules

- Never run from inside the worktree — always navigate to project root first
- Every step is best-effort — if one fails, continue to the next