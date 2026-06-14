---
name: flow-complete
description: "Phase 4: Complete — merge the PR, remove the worktree, and delete the state file. Final phase."
---

# FLOW Complete — Phase 4: Complete

## Usage

```text
/flow:flow-complete
/flow:flow-complete --continue-step
```

- `/flow:flow-complete` — uses the configured mode from the state file's `skills.flow-complete` config
- `/flow:flow-complete --continue-step` — self-invocation: skip Announce and SOFT-GATE, dispatch to the next step via Resume Check

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

Resolve the mode as the first action on **every** entry — both a
fresh invocation and a `--continue-step` self-invocation. Resolving
on every entry means a resumed run re-derives the mode from the state
file rather than depending on a flag threaded through the
self-invocation, so the configured `skills.flow-complete` mode stays
authoritative across the whole Complete phase.

There are no `--auto`/`--manual` flags — the state file's
`skills.flow-complete` config is the single source of truth for
skill autonomy.

1. Resolve the current branch: run `git worktree list --porcelain`,
   note the project root (the path on the first `worktree` line),
   find the `worktree` entry whose path matches the current working
   directory, and take the `branch refs/heads/<name>` line from that
   entry (strip the `refs/heads/` prefix). Call this `<branch>`.
2. Run the resolver below and use the `continue` field from its JSON
   output as the Complete-phase mode. It reads the
   `skills.flow-complete` entry in the state file, tolerating every
   object config shape, and falls back to **manual** when the config
   is missing or unparseable:

   ```bash
   ${CLAUDE_PLUGIN_ROOT}/bin/flow resolve-skill-mode --skill flow-complete --branch <branch>
   ```

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step's commit. `## Mode Resolution` above has already run —
it runs on every entry, so the resumed run re-resolves the mode from
the state file. Skip the Announce banner and the SOFT-GATE, and
proceed directly to the Resume Check section.

Use the Read tool to read `<project_root>/.flow-states/<branch>/state.json`
to get the state data (`feature`, `branch`, `worktree`, `pr_number`,
`pr_url`). Proceed directly to the Resume Check section.

<SOFT-GATE>
Run this entry check on a fresh invocation, immediately after Mode
Resolution. This gate never blocks — it records warnings for the
confirmation step.

1. Run `git worktree list --porcelain`. Note the path on the first
   `worktree` line (this is the project root). Find the `worktree` entry
   whose path matches your current working directory — the
   `branch refs/heads/<name>` line in that entry is the current branch
   (strip the `refs/heads/` prefix).
2. Use the Read tool to read `<project_root>/.flow-states/<branch>/state.json`.
   - If the file exists: extract `feature`, `branch`, `worktree`, `pr_number`,
     `pr_url`, and `cumulative_seconds`. Check `phases.flow-review.status` — if
     not `"complete"`, record warning "Phase 3 not complete (status: <actual status>)."
   - If the file does not exist: record warning "No state file found for
     branch '<branch>'."

Use these values for all subsequent steps — do not re-read the state file
or re-run git commands to gather the same information.

Carry any warnings forward to the confirmation step in Step 3.

The mode is already resolved by `## Mode Resolution` above — this
gate records warnings only and does not re-resolve the mode.

</SOFT-GATE>

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.6.1 — Phase 4: Complete — STARTING
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
- If `complete_step` is `3`: skip to Step 3 (Confirm with user).
- If `complete_step` is `4`: skip to Step 4 (Merge PR).
- If `complete_step` is `5`: skip to Step 5 (Finalize).
- If `complete_step` is `0`, `1`, or absent: proceed normally to
  Step 1.

---

## Steps

### Step 1 — Run complete-fast

Run the consolidated fast-path command. It handles phase entry, state
detection, PR status check, merge main, local CI dirty check (without
simulate-branch), and squash merge — all in a single call. It does not
make its own GitHub-CI determination; `gh pr merge --squash` is the
authority and surfaces a `not_mergeable` path when a required GitHub
check is failing or pending.

`complete-fast` resolves the autonomy mode itself from the state
file's `skills.flow-complete` config — the same source Mode
Resolution reads — so it takes no mode flag. It dispatches to
`ci::run_impl()` on a sentinel miss, so the invocation needs a
10-minute Bash tool timeout so CI's 3–4 minute duration does not
trip the default 2-minute Bash tool timeout and background the
process, defeating the gate (per `.claude/rules/ci-is-a-gate.md`).

Use a 10-minute Bash tool timeout (`timeout: 600000`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-fast --branch <branch>
```

Parse the JSON output and dispatch on the `path` field:

**If `"path": "merged"`** — the PR is merged (auto mode happy path).
Skip directly to Step 5 (finalize).

**If `"path": "already_merged"`** — the PR was already merged before
this invocation. Skip directly to Step 5 (finalize).

**If `"path": "confirm"`** — manual mode. All CI checks passed. Skip
to Step 3 (confirm with user).

**If `"path": "ci_stale"`** — main was merged into the branch and the
tree changed. Set the resume step and self-invoke to run CI:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

To continue, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. Do not output anything else after this invocation.

**If `"path": "ci_failed"`** — local CI failed. Launch the
`ci-fixer` sub-agent to diagnose and fix. Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures before merge"`

Provide the `output` field from the JSON in the prompt so the sub-agent
knows what failed.

If fixed, set the resume step, continuation flags, and commit:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

To re-check CI, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. Do not output anything else after this invocation.

If not fixed after 3 attempts, stop and report.

**If `"path": "not_mergeable"`** — `gh pr merge --squash` refused the
merge: a required GitHub check is failing or still pending. The
`reason` field carries the verbatim `gh` stderr. Stop and report to
the user:

> "GitHub blocked the merge: `<reason>`. A required check is failing
> or pending on GitHub. Inspect with `gh pr checks <pr_number>` /
> `gh pr view <pr_number>`, resolve on GitHub, then re-invoke
> `/flow:flow-complete`."

Do not loop or retry — `gh pr merge` is the authority on whether the
PR can merge, and FLOW does not poll GitHub CI on its behalf.

**If `"path": "conflict"`** — merge conflicts detected. The
`conflict_files` array lists the conflicted files.

1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of the
   feature from this session
3. Set the resume step, continuation flag, and commit the resolution

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 2, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. Do not output anything else after this invocation.

**If `"path": "max_retries"`** — stop and report to the user:
> "High contention: main has moved 3 times since the CI gate. Another
> engineer is merging frequently. Wait for a quieter window and
> re-invoke `/flow:flow-complete`."

**If `"status": "error"`** — stop and report the error to the user.
Do not retry the command with any additional flags or elevated privileges.

Check the `warnings` array from the output. Carry any warnings forward
to the confirmation step in Step 3.

### Step 2 — Run local CI gate

Run CI locally. Use a 10-minute Bash tool timeout (`timeout: 600000`)
— CI runs can take 3–4 minutes and the default 2-minute timeout
would background the process, defeating the gate (per
`.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

If it passes, continue to Step 3.

If it fails, launch the `ci-fixer` sub-agent to diagnose and fix.
Use the Agent tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures before merge"`

If fixed, record the resume step, set continuation flags, commit, and
self-invoke to re-check:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

Self-invoke `flow:flow-complete --continue-step` to re-run Step 2.

If not fixed after 3 attempts, stop and report.

### Step 3 — Confirm with user (manual mode only)

Skip this step if mode is **auto** — proceed directly to Step 4.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=3
```

**Resolve the integration branch.** Before composing the prompt,
run `bin/flow base-branch` to retrieve the integration branch the
flow coordinates against. Capture its stdout — call the value
`<base_branch>` — and substitute it into the prompt below. A repo
whose default branch is `staging` produces `<base_branch> =
staging`; a standard repo produces `<base_branch> = main`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

<HARD-GATE>
If mode is **manual**, use AskUserQuestion. If the preflight recorded
warnings, include them:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into <base_branch>?
> <pr_url>"
> ⚠ <any warnings from the preflight>

If no warnings:

> "PR #<pr_number> is green and ready to merge. Squash-merge '<feature>' into <base_branch>?
> <pr_url>"

Options:

- **Yes, merge and clean up** — proceed to Step 4
- **No, not yet** — stop here
- **I have feedback on the code** — describe the issue

Do NOT proceed to Step 4, do NOT merge, do NOT take any action outside
this step until the user explicitly selects an option. Freeform text
that is not one of the listed options is feedback — treat it the same
as selecting "I have feedback on the code".

**If "Yes, merge and clean up"** — record the user's confirmation by
writing the merge-approval marker, then proceed to Step 4. The marker
is the "proceed" half of the Complete-phase merge gate: when
`flow-complete` is configured manual, Step 4's `complete-merge` (and
`complete-fast` in the auto path) require and consume it before the
squash-merge.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow confirm-merge --branch <branch>
```

**If "No, not yet"** — stop here.

**If "I have feedback on the code"** (or freeform feedback):

Ask the user to describe the issue if they have not already. Fix the
code to address the feedback.

Set the continuation context and flag before committing:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set complete_step=2, then self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

After the commit completes, record the resume step:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

To loop back through CI, invoke `flow:flow-complete --continue-step`
using the Skill tool as your final action. Do not output anything else
after this invocation.

</HARD-GATE>

### Step 4 — Merge PR

Skip this step if the PR was already merged in Step 1 (complete-fast
returned `"merged"` or `"already_merged"`).

**Resolve the integration branch.** Before running the merge command,
run `bin/flow base-branch` to retrieve the integration branch the
flow coordinates against. Capture its stdout — call the value
`<base_branch>` — and use it in the success message below. A repo
whose default branch is `staging` produces `<base_branch> =
staging`; a standard repo produces `<base_branch> = main`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

For manual mode (after Step 3 confirmation), run the merge command.
When main has moved, `complete-merge` runs sentinel-gated CI on the
freshly-merged tree (`ci::run_impl()`), so use a 10-minute Bash tool
timeout (`timeout: 600000`) — CI runs can take 3–4 minutes and the
default 2-minute timeout would background the process, defeating the
gate (per `.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-merge --pr <pr_number> --state-file <project_root>/.flow-states/<branch>/state.json
```

Parse the JSON output and handle each status:

**If `"status": "merged"`** — the PR is merged. Report to the user
using the `<base_branch>` value resolved at the top of this step:

> "PR #<pr_number> merged into <base_branch>."

Continue to Step 5.

**If `"status": "ci_failed"`** — main had new commits that were merged
into the branch without conflicts and pushed, but the inline CI on the
freshly-merged tree failed. The `output` field carries the CI output.
Launch the `ci-fixer` sub-agent to diagnose and fix. Use the Agent
tool:

- `subagent_type`: `"flow:ci-fixer"`
- `description`: `"Fix CI failures before merge"`

Provide the `output` field from the JSON in the prompt so the sub-agent
knows what failed.

If fixed, record the resume step, set continuation flags, and commit:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the fixes via `/flow:flow-commit`.

To re-run CI and re-attempt the merge, invoke
`flow:flow-complete --continue-step` using the Skill tool as your
final action. Do not output anything else after this invocation.

If not fixed after 3 attempts, stop and report.

**If `"status": "ci_rerun"`** — main had new commits that were merged
into the branch without conflicts. The branch was pushed and the
inline CI on the merged tree passed. Loop back to re-run CI (it skips
on the sentinel match) and re-attempt the merge:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

To re-run CI, invoke `flow:flow-complete --continue-step` using the
Skill tool as your final action. Do not output anything else after this invocation.

**If `"status": "not_mergeable"`** — `gh pr merge --squash` refused the
merge: a required GitHub check is failing or still pending. The
`message` field carries the verbatim `gh` stderr. Stop and report to
the user:

> "GitHub blocked the merge: `<message>`. A required check is failing
> or pending on GitHub. Inspect with `gh pr checks <pr_number>` /
> `gh pr view <pr_number>`, resolve on GitHub, then re-invoke
> `/flow:flow-complete`."

Do not loop or retry — `gh pr merge` is the authority on whether the
PR can merge.

**If `"status": "conflict"`** — the `conflict_files` array lists the
conflicted files.

1. Read each conflicted file using the Read tool
2. Resolve the conflicts using the Edit tool — you have full context of
   the feature from this session
3. Record the resume step, set continuation flags, and commit

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=2
```

Set the continuation context. The self-invocation carries no mode
flag — Mode Resolution re-resolves the mode from
the state file on the resumed entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-complete --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Commit the resolution via `/flow:flow-commit` — the commit skill handles
staging, diff review, and push.

To continue to Step 2, invoke `flow:flow-complete --continue-step` using
the Skill tool as your final action. Do not output anything else after this invocation.

**If `"status": "max_retries"`** — stop and report to the user:
> "High contention: main has moved 3 times since the CI gate. Another
> engineer is merging frequently. Wait for a quieter window and
> re-invoke `/flow:flow-complete`."

**If `"status": "error"` and `"reason": "merge_not_confirmed"`** — the
merge-approval gate fired: `flow-complete` is configured manual and no
unconsumed confirmation marker was present. The marker is single-use,
so a prior merge attempt that looped back through this step already
consumed it. Return to Step 3 to re-confirm with the user — a fresh
confirmation writes a new marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set complete_step=3
```

To re-confirm, invoke `flow:flow-complete --continue-step` using the
Skill tool as your final action. Do not output anything else after
this invocation.

**If `"status": "error"`** — stop and report the error to the user.
Do not retry the merge command with any additional flags or elevated
privileges.

### Step 5 — Finalize: post-merge + cleanup

The next step removes the worktree. Navigate to the project root first
so the shell does not end up stranded inside a deleted directory.
`complete-finalize` self-gates against this — when its canonicalized
cwd equals or sits beneath the canonicalized `--worktree`, it returns
`{"status":"error","reason":"cwd_inside_worktree"}` before any side
effect, so a missed `cd` produces a clean error rather than shell
corruption. Run the `cd` anyway — it is the simpler path:

```bash
cd <project_root>
```

Run the consolidated finalize command. It handles phase-transition
complete, render-pr-body, format-issues-summary, close-issues,
format-complete-summary, label-issues with --remove, auto-close-parent,
notify-slack, worktree removal, state file deletion, and git pull —
all best-effort in a single call.

The render-pr-body step produces the PR body with all sections —
What, Artifacts, Plan, Phase Timings, Token Cost,
Review Findings, Learn Findings, State File, Session Log, and
Issues Filed — from the state file and available artifact files.
Sections with missing data are omitted automatically.

After a clean `--pull`, `complete-finalize` runs sentinel-gated CI on
the integration branch (`ci::run_impl()`), so use a 10-minute Bash
tool timeout (`timeout: 600000`) — CI runs can take 3–4 minutes and
the default 2-minute timeout would background the process, defeating
the gate (per `.claude/rules/ci-is-a-gate.md`).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow complete-finalize --pr <pr_number> --state-file <project_root>/.flow-states/<branch>/state.json --branch <branch> --worktree <worktree_path> --pull
```

Parse the JSON output.

**If `"status": "error"` and `"reason": "cwd_inside_worktree"`** —
the self-gate fired because the `cd <project_root>` above was missed
(or the cwd drifted back into the worktree). The worktree was NOT
removed. Re-run the `cd <project_root>` command above and re-invoke
`complete-finalize` once. If the second invocation also returns
`cwd_inside_worktree`, stop and report the error to the user — the
project root path is unresolvable in this session and manual
intervention is needed.

**On success** — keep `formatted_time`, `cumulative_seconds`,
`summary`, `issues_links`, and `banner_line` for the Done banner.

The `cleanup` field contains the results of Step 6 (cleanup operations):
worktree removal, state file and log deletion, local branch cleanup, and
git pull. Report the results to the user: what was cleaned, what was
already gone, and what failed.

If the output has a non-empty `post_merge_failures` dict, note the
failures but continue — all post-merge operations are best-effort.

If the output has a `base_ci` field (its `status` is `"failed"`),
the sentinel-gated CI on the integration branch failed after the
clean `--pull`. The squash merge already landed, so this is a
warning, NOT a rollback — do not attempt to undo the merge. Report
to the user that the integration branch needs attention:

> "⚠ Integration-branch CI failed after merge. The PR is merged, but
> the local <base_branch> tree did not pass CI. Inspect with
> `bin/flow ci` on <base_branch> and repair before the next flow
> starts. (`base_ci.ci` carries the CI output.)"

The next `/flow:flow-start` re-runs CI on the base branch under the
start lock and surfaces the same failure to `ci-fixer`.

### Step 6 — Cleanup results

The cleanup operations were performed as part of the complete-finalize
call in Step 5. The `cleanup` field in the JSON output shows what
happened to each resource (pr\_close, worktree\_tmp, worktree,
remote\_branch, local\_branch, state\_file, plan\_file,
log\_file, frozen\_phases, ci\_sentinel, timings\_file,
closed\_issues\_file, issues\_file, adversarial\_test — plus
git\_pull when the Complete path runs with `--pull`).
Each step reports "closed"/"removed"/"deleted"/"pulled", "skipped", or
"failed: reason". The `adversarial_test` step matches
`.flow-states/<branch>/adversarial_test.*` so the Phase 3 adversarial
agent's temp file is removed regardless of the runtime-chosen extension.

Report the results to the user: what was cleaned, what was already gone,
and what failed.

### Done — Print banner

Output the COMPLETE banner line, the summary from Step 5, and cleanup
status in your response (not via Bash) inside a single fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.6.1 — Phase 4: Complete — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

<summary text from format-complete-summary>

  ✓ Worktree removed
  ✓ state file and log deleted
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

The summary already includes the feature name, prompt, PR: <pr_url>,
per-phase timeline (Start:, Plan:, Code:, Review:, Learn:,
Complete:, Total:), Review Findings and Learn Findings sections
(each finding with its outcome marker and reasoning), the Token Cost
section (per-phase token totals + cost in USD, plus a Total row; an
optional By Model breakdown when 2+ models contributed; an optional
"↻" marker and footer note when a rate-limit window reset was
observed mid-flow — the section renders one row per non-pending
phase and is omitted entirely only when every phase is still
"pending"), and artifact counts (issues filed count, notes
captured count). Do not add a separate PR line — it is part of
the summary.

The Token Cost section uses these conditional output markers when
snapshot data is incomplete:

- **`—` (em-dash)** in the cost column means the per-phase or
  Total cost is unknown for the row. Cost is token-derived:
  `window_deltas::pair_delta` prices the per-phase `by_model`
  token delta through `pricing::cost_for`. A phase shows `—` when
  that price cannot be computed — no enter snapshot, an empty
  per-phase `by_model` delta, or a delta that references an
  unpriced model family; the Total row shows `—` when no phase
  contributed a complete cost pair.
- **`*` (asterisk suffix)** on a cost value (e.g. `$0.450*`) marks
  the row as partial: the phase ran but the snapshot data was not
  fully recoverable, so the displayed value is the best-effort
  computation from available endpoints. The Total cost is
  suffixed with `*` when any phase contributed `None` cost into
  the aggregate.
- **`* cost partial — some phases had no cost data`** appears as
  a footnote line below the Total when the Total carries the `*`
  marker, naming the cause so the reader does not have to
  reconstruct it from per-phase rows.

If the `complete-finalize` JSON output has a non-empty
`issues_links` field, render it as regular text (not inside a code
block) immediately after the banner code block. This makes the issue
URLs clickable — URLs inside code blocks are not rendered as links.

After the banner (and issue links if any), write a brief
session summary in natural prose (2-3 sentences). Describe what was
built or fixed, the approach taken, and the outcome. Use your
conversation context — do not fetch additional data or run any
commands. This is a narrative recap, not a structured template.

## Rules

- Steps 1-4 run from the worktree (feature branch); Step 5 (finalize) runs from the project root
- If the merge fails, never retry with additional flags or elevated privileges — report to the user and stop
- Confirm with the user only when mode is **manual**
- State file deletion is what resets the session hook — do not skip it
- Every operation inside `complete-finalize` (Step 5) is best-effort — if one fails, continue to the next
- Never use `general-purpose` sub-agents — use `"flow:ci-fixer"` for CI failures
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
- When in autonomous mode, classify tool failures per `.claude/rules/autonomous-flow-self-recovery.md` — mechanical fixes are in-flow, substantive failures prompt the user
