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
previous step's commit. Skip the Announce banner and proceed directly
to the Resume Check section.

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

Carry any warnings forward to the confirmation step in Step 4.

Resolve the mode using the Mode Resolution rules above.

</SOFT-GATE>

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — Phase 6: Complete — STARTING
──────────────────────────────────────────────────
```
````

## Logging

No logging for this phase. Complete deletes the log file as part of its
operation — writing log entries that are immediately deleted is pointless.

---

## Resume Check

Read `complete_step` from the state file (default `0` if absent).

- If `complete_step` is `2`: skip to Step 2 (Run local CI gate).
- If `complete_step` is `3`: skip to Step 3 (Check GitHub CI status).
- If `complete_step` is `4`: skip to Step 4 (Confirm with user).
- If `complete_step` is `5`: skip to Step 5 (Merge PR).
- If `complete_step` is `0` or absent: proceed normally to Step 1.

---

## Steps

### Step 1 — Preflight

Run the consolidated preflight script. It handles state detection, PR
status check, phase transition entry, mode resolution, Learn phase
warning, and merging main into the branch — all in a single call.

Pass the mode flag resolved from Mode Resolution:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-preflight --branch <branch> --auto
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-preflight --branch <branch> --manual
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-preflight --branch <branch>
```

Use the first form when mode is **auto**, the second when **manual**,
the third when no flag was resolved (lets the script decide from the
state file).

Parse the JSON output and handle each status:

**If `"status": "ok"` and `"pr_state": "MERGED"`** — the PR is already
merged. Skip directly to Step 6 (post-merge) to archive artifacts,
then continue through Step 7 (cleanup). Skip Steps 2–5.

**If `"status": "ok"` and `"merge": "clean"` or `"merge": "merged"`** —
continue to Step 2.

**If `"status": "conflict"`** — merge conflicts detected. The
`conflict_files` array lists the conflicted files.

1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of the
   feature from this session
3. Set the resume step, continuation flag, and commit the resolution

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 2, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If `"status": "error"`** — stop and report the error to the user.

Check the `warnings` array from the output. Carry any warnings forward
to the confirmation step in Step 4.

### Step 2 — Run local CI gate

Run CI locally with the branch name simulated as "main" to catch
branch-dependent test failures before merge:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci --simulate-branch main
```

If it passes, continue to Step 3.

If it fails, the failure is likely a branch-dependent test that passes
on the feature branch but would fail on main. Launch the `ci-fixer`
sub-agent to diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix branch-dependent test failures"`

If fixed, record the resume step, set continuation flags, commit, and
self-invoke to re-check:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

Self-invoke `flow:flow-complete --continue-step` to re-run Step 2.
If mode was resolved to auto, pass `--auto` as well.

If not fixed after 3 attempts, stop and report.

### Step 3 — Check GitHub CI status

Check the CI status on the PR:

```bash
gh pr checks <pr_number>
```

Parse the output. Each check has a status: pass, fail, or pending.

**If all checks pass** — continue to Step 4.

**If any check is pending** — record the resume step so re-entry skips
straight to Step 3:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=3
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
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

To re-check CI, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

If still failing after 3 attempts, stop and report.

- **Not fixed** — stop and report to the user.

### Step 4 — Confirm with user (manual mode only)

Skip this step if mode is **auto** — proceed directly to Step 5.

<HARD-GATE>
If mode is **manual**, use AskUserQuestion. If the preflight recorded
warnings, include them:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into main?
> <pr_url>"
> ⚠ <any warnings from the preflight>

If no warnings:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into main?
> <pr_url>"

Options:

- **Yes, merge and clean up** — proceed to Step 5
- **No, not yet** — stop here
- **I have feedback on the code** — describe the issue

Do NOT proceed to Step 5, do NOT merge, do NOT take any action outside
this step until the user explicitly selects an option. Freeform text
that is not one of the listed options is feedback — treat it the same
as selecting "I have feedback on the code".

**If "Yes, merge and clean up"** — proceed to Step 5.

**If "No, not yet"** — stop here.

**If "I have feedback on the code"** (or freeform feedback):

Ask the user to describe the issue if they have not already. Fix the
code to address the feedback.

Set the continuation context and flag before committing:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set complete_step=2, then self-invoke flow:flow-complete --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

After the commit completes, record the resume step:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

To loop back through CI, invoke `flow:flow-complete --continue-step --manual`
using the Skill tool as your final action. Do not output anything else
after this invocation.

</HARD-GATE>

### Step 5 — Merge PR

Run the consolidated merge script. It handles the freshness check and
squash merge in a single call:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-merge --pr <pr_number> --state-file <project_root>/.flow-states/<branch>.json
```

Parse the JSON output and handle each status:

**If `"status": "merged"`** — the PR is merged. Report to the user:
> "PR #<pr_number> merged into main."
Continue to Step 6.

**If `"status": "ci_rerun"`** — main had new commits that were merged
into the branch without conflicts. The branch was pushed. Loop back
to re-run CI:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

To re-run CI, invoke `flow:flow-complete --continue-step` using the
Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If `"status": "ci_pending"`** — GitHub CI has not finished on the
latest commits. Set the resume step and self-invoke to wait for CI:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=3
```

Invoke `flow:flow-complete --continue-step` using the Skill tool as
your final action. If mode was resolved to auto, pass `--auto` as
well. Do not output anything else after this invocation.

**If `"status": "conflict"`** — the `conflict_files` array lists the
conflicted files.

1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of
   the feature from this session
3. Record the resume step, set continuation flags, and commit

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

If mode is **auto**, use the first form. If mode is **manual**, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 2, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. If mode was resolved to auto, pass
`--auto` as well. Do not output anything else after this invocation.

**If `"status": "max_retries"`** — stop and report to the user:
> "High contention: main has moved 3 times since the CI gate. Another
> engineer is merging frequently. Wait for a quieter window and
> re-invoke `/flow:flow-complete`."

**If `"status": "error"`** — stop and report the error to the user.
Do not retry the merge command with any additional flags or elevated
privileges.

### Step 6 — Post-merge operations

Run the consolidated post-merge script. It handles phase-transition
complete, render-pr-body, format-issues-summary, close-issues,
format-complete-summary, label-issues --remove, auto-close-parent,
and notify-slack — all best-effort in a single call.

The script produces the PR body with all sections — What, Artifacts,
Plan, DAG Analysis, Phase Timings, State File, Session Log, and
Issues Filed — from the state file and available artifact files.
Sections with missing data are omitted automatically.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-post-merge --pr <pr_number> --state-file <project_root>/.flow-states/<branch>.json --branch <branch>
```

Parse the JSON output. Keep `formatted_time`, `cumulative_seconds`,
`summary`, `issues_links`, and `banner_line` for the Done banner.

If the output has a non-empty `failures` dict, note the failures but
continue — all post-merge operations are best-effort.

### Navigate to project root

The worktree is about to be removed — you cannot be inside it when that
happens. Navigate to the project root now. All subsequent steps (cleanup)
run from the project root on main.

```bash
cd <project_root>
```

### Step 7 — Run cleanup script

Run the cleanup script with `--pull` to pull merged changes after
worktree removal:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup <project_root> --branch <branch> --worktree <worktree_path> --pull
```

The script outputs JSON with a `steps` dict showing what happened to each
resource (worktree, state\_file, log\_file, ci\_sentinel, git\_pull). Each
step reports "removed"/"deleted"/"pulled", "skipped", or "failed: reason".

Report the results to the user: what was cleaned, what was already gone,
and what failed.

### Done — Print banner

Output the COMPLETE banner line, the summary from Step 6, and cleanup
status in your response (not via Bash) inside a single fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 6: Complete — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

<summary text from format-complete-summary>

  ✓ Worktree removed
  ✓ state file and log deleted
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

The summary already includes the feature name, prompt, PR: <pr_url>,
per-phase timeline (Start:, Plan:, Code:, Code Review:, Learn:,
Complete:, Total:), and artifact counts (issues filed count, notes
captured count). Do not add a separate PR line — it is part of the
summary.

If the `complete-post-merge` JSON output has a non-empty
`issues_links` field, render it as regular text (not inside a code
block) immediately after the banner code block. This makes the issue
URLs clickable — URLs inside code blocks are not rendered as links.

After the banner (and issue links if any), write a brief
session summary in natural prose (2-3 sentences). Describe what was
built or fixed, the approach taken, and the outcome. Use your
conversation context — do not fetch additional data or run any
commands. This is a narrative recap, not a structured template.

## Rules

- Steps 1-6 run from the worktree (feature branch); Step 7 runs from the project root after an explicit cd before Step 7
- If the merge fails, never retry with additional flags or elevated privileges — report to the user and stop
- Confirm with the user only when mode is **manual**
- State file deletion is what resets the session hook — do not skip it
- Every step after the merge (Step 6) is best-effort — if it fails, continue to the next
- The skill is idempotent: safe to re-invoke via `/loop` after a "pending CI" stop
- Never use `general-purpose` sub-agents — use `"flow:ci-fixer"` for CI failures
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
