---
name: flow-complete
description: "Phase 6: Complete — merge the PR, remove the worktree, and delete the state file. Final phase."
model: haiku
---

# FLOW Complete — Phase 6: Complete

## Usage

```text
/flow:flow-complete
/flow:flow-complete --auto
/flow:flow-complete --manual
```

- `/flow:flow-complete` — uses configured mode from the state file (default: auto)
- `/flow:flow-complete --auto` — skips confirmation and proceeds directly
- `/flow:flow-complete --manual` — prompts for user confirmation before merge

## Mode Resolution

1. If `--auto` was passed → mode is **auto**
2. If `--manual` was passed → mode is **manual**
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-complete` value.
4. If the state file has no `skills` key → use built-in default: **auto**

<SOFT-GATE>
Run this entry check as your very first action. This gate never
blocks — it records warnings for the confirmation step.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file exists: extract `feature`, `branch`, `worktree`, `pr_number`,
     `pr_url`, and `cumulative_seconds`. Check `phases.flow-learn.status` — if
     not `"complete"`, record warning "Phase 5 not complete (status: <actual status>)."
   - If the file does not exist: record warning "No state file found for
     branch '<branch>'."

Use these values for all subsequent steps — do not re-read the state file
or re-run git commands to gather the same information.

Carry any warnings forward to the confirmation step in Step 5.

Resolve the mode using the Mode Resolution rules above.

Navigate to the project root now — all subsequent steps must run from
the project root, not from inside the worktree:

```bash
cd <project_root>
```

</SOFT-GATE>

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.8 — Phase 6: Complete — STARTING
============================================
```
````

## Logging

No logging for this phase. Complete deletes the log file as part of its
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

### Step 2 — Check PR status

Check the current PR status:

If the state file had a `pr_number`, run:

```bash
gh pr view <pr_number> --json state --jq .state
```

If the state file had no `pr_number` (or no state file was found), try the branch name:

```bash
gh pr view <branch> --json state --jq .state
```

**If `MERGED`** — the PR is already merged. Skip directly to Step 8 (close issues).

**If `OPEN`** — continue to Step 3 to merge.

**If `CLOSED`** — stop with error:
> "PR is closed but not merged. Reopen or create a new PR first."

**If no PR found** — stop with error:
> "Could not find a PR for this branch."

### Step 3 — Merge main into branch

Fetch the latest main and merge it into the feature branch:

```bash
git fetch origin main
```

```bash
git merge origin/main
```

**If the merge succeeds with no conflicts:**
- If there are new commits from the merge, push them:

```bash
git push
```

- Continue to Step 4.

**If the merge has conflicts:**
1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of the
   feature from this session
3. Commit the resolution via `/flow:flow-commit` — the commit skill handles
   staging, diff review, and push
4. Continue to Step 4.

**If the merge fails for any other reason** — stop and report the error.

### Step 4 — Check CI status

Check the CI status on the PR:

```bash
gh pr checks <pr_number>
```

Parse the output. Each check has a status: pass, fail, or pending.

**If all checks pass** — continue to Step 5.

**If any check is pending** — stop and suggest polling:

> "CI checks are still running. Re-run `/flow:flow-complete` when done,
> or use `/loop 15s /flow:flow-complete` to auto-retry."

**If any check has failed** — launch the `ci-fixer` sub-agent to diagnose
and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures on PR branch"`

Provide the full `gh pr checks` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — commit the fixes via `/flow:flow-commit`, then re-check CI
  by running `gh pr checks <pr_number>` again. If still failing after 3
  attempts, stop and report.
- **Not fixed** — stop and report to the user.

### Step 5 — Confirm with user (manual mode only)

Skip this step if mode is **auto** — proceed directly to Step 6.

If mode is **manual**, use AskUserQuestion. If the SOFT-GATE recorded
warnings, include them:

> "PR is green and ready to merge. Squash-merge '<feature>' into main?"
> ⚠ <any warnings from the gate>

- **Yes, merge and clean up** — proceed
- **No, not yet** — stop here

If no warnings:

> "PR is green and ready to merge. Squash-merge '<feature>' into main?"

- **Yes, merge and clean up** — proceed
- **No, not yet** — stop here

### Step 6 — Archive artifacts to PR

Archive key artifacts to the PR body before merging. These files are
deleted during cleanup, so this is the last chance to preserve them.

**Session log artifact:** If `transcript_path` from the state file is
not null, add a session log link to the PR artifacts:

```bash
bin/flow update-pr-body --pr <pr_number> --add-artifact --label "Session log" --value <transcript_path>
```

If `transcript_path` is null, skip this command.

**Phase Timings:** Generate the phase timings table and append it to
the PR body as a non-collapsible section:

```bash
bin/flow format-pr-timings --state-file <project_root>/.flow-states/<branch>.json --output <project_root>/.flow-states/<branch>-timings.md
```

```bash
bin/flow update-pr-body --pr <pr_number> --append-section --heading "Phase Timings" --content-file <project_root>/.flow-states/<branch>-timings.md --no-collapse
```

**State file and session log:** Use the Read tool to read
`<project_root>/.flow-states/<branch>.json` and
`<project_root>/.flow-states/<branch>.log`.

Append the state file to the PR body:

```bash
bin/flow update-pr-body --pr <pr_number> --append-section --heading "State File" --summary ".flow-states/<branch>.json" --content-file <project_root>/.flow-states/<branch>.json --format json
```

Append the session log to the PR body:

```bash
bin/flow update-pr-body --pr <pr_number> --append-section --heading "Session Log" --summary ".flow-states/<branch>.log" --content-file <project_root>/.flow-states/<branch>.log --format text
```

If any file does not exist, skip that step — do not fail.

### Step 7 — Merge PR

Merge the PR via squash merge:

```bash
gh pr merge <pr_number> --squash
```

If the merge succeeds, report to the user:
> "PR #<pr_number> merged into main."

If the merge fails, stop and report the error to the user. Do not retry
the merge command with any additional flags or elevated privileges.

### Step 8 — Close referenced issues

Close any GitHub issues referenced in the start prompt. This is best-effort —
continue to cleanup even if closing fails.

```bash
bin/flow close-issues --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output. Report which issues were closed and which failed.
If no issues were referenced, proceed silently.

### Step 9 — Run cleanup script

Run the cleanup script from the project root:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path>
```

The script outputs JSON with a `steps` dict showing what happened to each
resource (worktree, state\_file, log\_file, ci\_sentinel). Each step reports
"removed"/"deleted", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone,
and what failed.

### Step 10 — Pull merged changes

The worktree is removed and you are on main. Pull to get the merged
feature code:

```bash
git pull origin main
```

If the pull fails, warn the user but do not block — cleanup succeeded.

### Done — Print banner

For the banner below, compute `<formatted_time>` from the integer
`cumulative_seconds` read in the SOFT-GATE: `Xh Ym` if >= 3600,
`Xm` if >= 60, `<1m` if < 60. Do not write the formatted string back
to the state file.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.8 — Phase 6: Complete — COMPLETE (<formatted_time>)
  Feature '<feature>' is fully done.
  Worktree removed, state file and log deleted.
============================================
```
````

## Rules

- Never run from inside the worktree — the SOFT-GATE navigates to project root
- If the merge fails, never retry with additional flags or elevated privileges — report to the user and stop
- Confirm with the user only when mode is **manual**
- State file deletion is what resets the session hook — do not skip it
- Every step after the merge (Steps 8-10) is best-effort — if one fails, continue to the next
- The skill is idempotent: safe to re-invoke via `/loop` after a "pending CI" stop
- Never use `general-purpose` sub-agents — use `"flow:ci-fixer"` for CI failures
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
