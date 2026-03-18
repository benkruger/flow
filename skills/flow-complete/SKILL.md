---
name: flow-complete
description: "Phase 6: Complete — merge the PR, remove the worktree, and delete the state file. Final phase."
---

# FLOW Complete — Phase 6: Complete

## Usage

```text
/flow:flow-complete
/flow:flow-complete --auto
/flow:flow-complete --manual
/flow:flow-complete --continue-step
/flow:flow-complete --continue-step --auto
/flow:flow-complete --continue-step --manual
```

- `/flow:flow-complete` — uses configured mode from the state file (default: auto)
- `/flow:flow-complete --auto` — skips confirmation and proceeds directly
- `/flow:flow-complete --manual` — prompts for user confirmation before merge
- `/flow:flow-complete --continue-step` — self-invocation: skip Announce and SOFT-GATE, dispatch to the next step via Resume Check

## Mode Resolution

1. If `--auto` was passed → mode is **auto**
2. If `--manual` was passed → mode is **manual**
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-complete` value.
4. If the state file has no `skills` key → use built-in default: **auto**

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step's commit. Skip the Announce banner, the SOFT-GATE,
and the Update State section (do not call `phase-transition` again).

Run `git worktree list --porcelain` to find the project root (first
`worktree` line) and `git branch --show-current` for the current branch.
Navigate to the project root:

```bash
cd <project_root>
```

Use the Read tool to read `<project_root>/.flow-states/<branch>.json`
to get the state data (`feature`, `branch`, `worktree`, `pr_number`,
`pr_url`). Proceed directly to the Resume Check section.

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
──────────────────────────────────────────────────
  FLOW v0.32.3 — Phase 6: Complete — STARTING
──────────────────────────────────────────────────
```
````

## Logging

No logging for this phase. Complete deletes the log file as part of its
operation — writing log entries that are immediately deleted is pointless.

## Update State

Record phase entry in the state file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-complete --action enter
```

Parse the JSON output and confirm `status` is `"ok"`.

---

## Resume Check

Read `complete_step` from the state file (default `0` if absent).

- If `complete_step` is `4`: skip to Step 4 (Check CI status).
- If `complete_step` is `0` or absent: proceed normally to Step 1.

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

**If `MERGED`** — the PR is already merged. Skip directly to Step 6
(archive artifacts to PR). After Step 6, continue to Step 8 (remove
labels), then Step 9 (close issues) — skip Step 7 (merge) since the
PR is already merged.

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
3. Set the continuation flag and commit the resolution

Set the continuation context and flag before committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set complete_step=4, then self-invoke flow:flow-complete --continue-step."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

After the commit completes, clear the continuation flag and record the
resume step:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

To continue to Step 4, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If the merge fails for any other reason** — stop and report the error.

### Step 4 — Check CI status

Check the CI status on the PR:

```bash
gh pr checks <pr_number>
```

Parse the output. Each check has a status: pass, fail, or pending.

**If all checks pass** — continue to Step 5.

**If any check is pending** — invoke the `loop` skill via the Skill tool with args `15s /flow:flow-complete` and return. The loop will re-invoke the complete skill automatically until CI completes.

**If any check has failed** — launch the `ci-fixer` sub-agent to diagnose
and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures on PR branch"`

Provide the full `gh pr checks` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — set the continuation context and flag before committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set complete_step=4, then self-invoke flow:flow-complete --continue-step."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

After the commit completes, clear the continuation flag and record the
resume step:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

To re-check CI, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

If still failing after 3 attempts, stop and report.

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

Record phase completion in the state file so Phase Timings includes
the Complete row:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-complete --action complete --next-phase flow-complete
```

Parse the JSON output. Keep `formatted_time` and `cumulative_seconds`
from this output — use them for the Complete row and total in the Done
banner below.

Render the complete PR body. This single call generates all sections
(What, Artifacts, Plan, DAG Analysis, Phase Timings, State File,
Session Log, Issues Filed) from the state file and available artifact
files. Sections with missing data are omitted automatically.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow render-pr-body --pr <pr_number>
```

**Issues banner line:** Format the issues summary to get the banner
line for the Done banner:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow format-issues-summary --state-file <project_root>/.flow-states/<branch>.json --output <project_root>/.flow-states/<branch>-issues.md
```

Parse the JSON output. Keep the `banner_line` — use it in the Done
banner below. If `has_issues` is `false`, there is no banner line.

### Step 7 — Merge PR

Merge the PR via squash merge:

```bash
gh pr merge <pr_number> --squash
```

If the merge succeeds, report to the user:
> "PR #<pr_number> merged into main."

If the merge fails, stop and report the error to the user. Do not retry
the merge command with any additional flags or elevated privileges.

### Step 8 — Remove In-Progress labels

Remove the "Flow In-Progress" label from any issues referenced in the start
prompt. This is best-effort — continue to close-issues even if removal fails.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow label-issues --state-file <project_root>/.flow-states/<branch>.json --remove
```

### Step 9 — Close referenced issues

Close any GitHub issues referenced in the start prompt. This is best-effort —
continue to cleanup even if closing fails.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow close-issues --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output. Report which issues were closed and which failed.
If no issues were referenced, proceed silently.

### Step 10 — Run cleanup script

Run the cleanup script from the project root:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path>
```

The script outputs JSON with a `steps` dict showing what happened to each
resource (worktree, state\_file, log\_file, ci\_sentinel). Each step reports
"removed"/"deleted", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone,
and what failed.

### Step 11 — Pull merged changes

The worktree is removed and you are on main. Pull to get the merged
feature code:

```bash
git pull origin main
```

If the pull fails, warn the user but do not block — cleanup succeeded.

### Done — Print banner

For each phase row, format its `cumulative_seconds` (from the SOFT-GATE
data) as: `Xh Ym` if >= 3600, `Xm` if >= 60, `<1m` if < 60. For the
Complete row, use `formatted_time` from the `phase-transition --complete`
output in Step 6 (the SOFT-GATE data predates Complete's timing).

Compute the total by summing all phase `cumulative_seconds` values
(including Complete's `cumulative_seconds` from the Step 6 output)
and formatting the result the same way.

Use `<feature>` and `<branch>` from the SOFT-GATE data.

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.32.3 — Phase 6: Complete — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Feature:      <feature>
  Branch:       <branch>
  PR:           <pr_url>

  ┌─────────────────────────┐
  │ Start:         <time>   │
  │ Plan:          <time>   │
  │ Code:          <time>   │
  │ Code Review:   <time>   │
  │ Learn:         <time>   │
  │ Complete:      <time>   │
  ├─────────────────────────┤
  │ Total:         <time>   │
  └─────────────────────────┘

  ✓ Worktree removed
  ✓ state file and log deleted
  <banner_line>
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Only include the `<banner_line>` line if `has_issues` was `true` in the
`format-issues-summary` output from Step 6. Use the `banner_line` value
exactly as returned — do not recompute it.

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
