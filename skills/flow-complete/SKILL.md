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

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

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

Carry any warnings forward to the confirmation step in Step 6.

Resolve the mode using the Mode Resolution rules above.

</SOFT-GATE>

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.0 — Phase 6: Complete — STARTING
──────────────────────────────────────────────────
```
````

## Logging

No logging for this phase. Complete deletes the log file as part of its
operation — writing log entries that are immediately deleted is pointless.

## Update State

Record phase entry in the state file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-complete --action enter --branch <branch>
```

Parse the JSON output and confirm `status` is `"ok"`.

---

## Resume Check

Read `complete_step` from the state file (default `0` if absent).

- If `complete_step` is `4`: skip to Step 4 (Run local CI gate).
- If `complete_step` is `5`: skip to Step 5 (Check GitHub CI status).
- If `complete_step` is `6`: skip to Step 6 (Confirm with user).
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

**If `MERGED`** — the PR is already merged. Skip directly to Step 7
(archive artifacts to PR). After Step 7, continue to Step 9 (close
issues), then Step 10 (remove labels), then continue through cleanup
(Steps 12-13) — skip Step 8 (merge) since the PR is already merged.

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
3. Set the resume step, continuation flag, and commit the resolution

Record the resume step before committing so the continuation context
needs only a single operation (self-invoke):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 4, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If the merge fails for any other reason** — stop and report the error.

### Step 4 — Run local CI gate

Run CI locally with the branch name simulated as "main" to catch
branch-dependent test failures before merge:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci --force --simulate-branch main
```

If it passes, continue to Step 5.

If it fails, the failure is likely a branch-dependent test that passes
on the feature branch but would fail on main. Launch the `ci-fixer`
sub-agent to diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix branch-dependent test failures"`

If fixed, record the resume step, set continuation flags, commit, and
self-invoke to re-check:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

Self-invoke `flow:flow-complete --continue-step` to re-run Step 4.
If mode was resolved to auto, pass `--auto` as well.

If not fixed after 3 attempts, stop and report.

### Step 5 — Check GitHub CI status

Check the CI status on the PR:

```bash
gh pr checks <pr_number>
```

Parse the output. Each check has a status: pass, fail, or pending.

**If all checks pass** — continue to Step 6.

**If any check is pending** — record the resume step so re-entry skips
straight to Step 5:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=5
```

Then invoke the `loop` skill via the Skill tool with args `15s /flow:flow-complete` and return. The loop will re-invoke the complete skill automatically until CI completes.

**If any check has failed** — launch the `ci-fixer` sub-agent to diagnose
and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures on PR branch"`

Provide the full `gh pr checks` output in the prompt so the sub-agent
knows what failed.

Wait for the sub-agent to return.

- **Fixed** — record the resume step and set continuation flags before
committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

To re-check CI, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

If still failing after 3 attempts, stop and report.

- **Not fixed** — stop and report to the user.

### Step 6 — Confirm with user (manual mode only)

Skip this step if mode is **auto** — proceed directly to Step 7.

<HARD-GATE>
If mode is **manual**, use AskUserQuestion. If the SOFT-GATE recorded
warnings, include them:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into main?
> <pr_url>"
> ⚠ <any warnings from the gate>

If no warnings:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into main?
> <pr_url>"

Options:

- **Yes, merge and clean up** — proceed to Step 7
- **No, not yet** — stop here
- **I have feedback on the code** — describe the issue

Do NOT proceed to Step 7, do NOT merge, do NOT take any action outside
this step until the user explicitly selects an option. Freeform text
that is not one of the listed options is feedback — treat it the same
as selecting "I have feedback on the code".

**If "Yes, merge and clean up"** — proceed to Step 7.

**If "No, not yet"** — stop here.

**If "I have feedback on the code"** (or freeform feedback):

Ask the user to describe the issue if they have not already. Fix the
code to address the feedback.

Set the continuation context and flag before committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set complete_step=4, then self-invoke flow:flow-complete --continue-step --manual."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

After the commit completes, record the resume step:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

To loop back through CI, invoke `flow:flow-complete --continue-step --manual`
using the Skill tool as your final action. Do not output anything else
after this invocation.

</HARD-GATE>

### Step 7 — Archive artifacts to PR

Record phase completion in the state file so Phase Timings includes
the Complete row:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-complete --action complete --next-phase flow-complete --branch <branch>
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

### Step 8 — Freshness check and merge PR

Verify the branch is up-to-date with main before merging. If main has
moved since the CI gate, merge it in and loop back through CI.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow check-freshness --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output and handle each status:

**If `"status": "max_retries"`** — stop and report to the user:
> "High contention: main has moved 3 times since the CI gate. Another
> engineer is merging frequently. Wait for a quieter window and
> re-invoke `/flow:flow-complete`."

**If `"status": "error"`** — stop and report the error to the user.

**If `"status": "up_to_date"`** — branch already contains the latest
main. Proceed to merge:

```bash
gh pr merge <pr_number> --squash
```

If the merge succeeds, report to the user:
> "PR #<pr_number> merged into main."

If the merge fails, check the error message:

- If the error contains "base branch policy prohibits the merge" — GitHub
  CI has not finished on the latest commits. Set the resume step and
  self-invoke to wait for CI:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=5
```

  Invoke `flow:flow-complete --continue-step` using the Skill tool as
  your final action. If mode was resolved to auto, pass `--auto` as
  well. Do not output anything else after this invocation.

- For any other error — stop and report the error to the user. Do not
  retry the merge command with any additional flags or elevated
  privileges.

**If `"status": "merged"`** — main had new commits that were merged
into the branch without conflicts. Push the merge commit and loop back
to re-run CI on the combined code:

```bash
git push
```

Record the resume step and self-invoke to re-run CI:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

To re-run CI, invoke `flow:flow-complete --continue-step` using the
Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If `"status": "conflict"`** — main had new commits that conflict with
the branch. The `files` array lists the conflicted files.

1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of
   the feature from this session
3. Record the resume step, set continuation flags, and commit

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=4
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 4, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

### Step 9 — Close referenced issues

Close any GitHub issues referenced in the start prompt. This is best-effort —
continue to remove-labels even if closing fails.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow close-issues --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output. Report which issues were closed and which failed.
If no issues were referenced, proceed silently.

If any issues were closed (the `closed` array is non-empty), write the
`closed` array to `.flow-states/<branch>-closed-issues.json` using the
Write tool. Each item in the array is a dict with `number` and `url` keys.

**Generate the summary** while the state file still exists:

If closed issues were written to a file, include the file path:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow format-complete-summary --state-file <project_root>/.flow-states/<branch>.json --closed-issues-file <project_root>/.flow-states/<branch>-closed-issues.json
```

If no issues were closed, omit the closed-issues-file arg:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow format-complete-summary --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output. Keep the `summary` field — use it in the Done
banner below.

### Step 10 — Remove In-Progress labels

Remove the "Flow In-Progress" label from any issues referenced in the start
prompt. This is best-effort — continue to cleanup even if removal fails.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow label-issues --state-file <project_root>/.flow-states/<branch>.json --remove
```

### Step 11 — Auto-close parent issues and milestones

For each closed issue from Step 9, check if its parent epic or milestone
should be auto-closed. Best-effort — report closures in the Done banner,
continue silently on failure.

If Step 9 closed any issues (the `closed` array was non-empty), run for
each closed issue number:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow auto-close-parent --repo <repo> --issue-number <N>
```

Parse the JSON output. If `parent_closed` or `milestone_closed` is true,
note it for the Done banner. If the command fails, continue to the next
issue.

### Slack Notification

Read `slack_thread_ts` from the state file. If present, post the final thread reply with end-to-end timeline before cleanup deletes the state file. Best-effort — skip silently on failure.

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow notify-slack --phase flow-complete --message "<message_text>" --thread-ts <thread_ts>
```

If `"status": "ok"`, record the notification:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-notification --phase flow-complete --ts <ts> --thread-ts <thread_ts> --message "<message_text>"
```

If `"status": "skipped"` or `"status": "error"`, continue without error.

### Navigate to project root

The worktree is about to be removed — you cannot be inside it when that
happens. Navigate to the project root now. All subsequent steps (cleanup
and pull) run from the project root on main.

```bash
cd <project_root>
```

### Step 12 — Run cleanup script

Run the cleanup script from the project root:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path>
```

The script outputs JSON with a `steps` dict showing what happened to each
resource (worktree, state\_file, log\_file, ci\_sentinel). Each step reports
"removed"/"deleted", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone,
and what failed.

### Step 13 — Pull merged changes

The worktree is removed and you are on main. Pull to get the merged
feature code:

```bash
git pull origin main
```

If the pull fails, warn the user but do not block — cleanup succeeded.

### Done — Print banner

Output the COMPLETE banner line, the summary from Step 7, and cleanup
status in your response (not via Bash) inside a single fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.0 — Phase 6: Complete — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

<summary text from format-complete-summary>

  ✓ Worktree removed
  ✓ state file and log deleted
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

The summary already includes the feature name, prompt, PR: <pr_url>,
per-phase timeline (Start:, Plan:, Code:, Code Review:, Learn:,
Complete:, Total:), and artifact counts (issues filed, notes captured).
Do not add a separate PR line — it is part of the summary.

After the banner, write a brief session summary in natural prose (2-3
sentences). Describe what was built or fixed, the approach taken, and the
outcome. Use your conversation context — do not fetch additional data or
run any commands. This is a narrative recap, not a structured template.

## Rules

- Steps 1-11 run from the worktree (feature branch); Steps 12-13 run from the project root after an explicit cd before Step 12
- If the merge fails, never retry with additional flags or elevated privileges — report to the user and stop
- Confirm with the user only when mode is **manual**
- State file deletion is what resets the session hook — do not skip it
- Every step after the merge (Steps 9-12) is best-effort — if one fails, continue to the next
- The skill is idempotent: safe to re-invoke via `/loop` after a "pending CI" stop
- Never use `general-purpose` sub-agents — use `"flow:ci-fixer"` for CI failures
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
