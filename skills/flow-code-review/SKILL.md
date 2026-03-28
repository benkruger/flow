---
name: flow-code-review
description: "Phase 4: Code Review — three review lenses (clarity via inline review, correctness including rule compliance via inline review, safety via inline security review) plus an optional fourth (CLAUDE.md compliance via code-review:code-review plugin, configurable). Commits after each step."
---

# FLOW Code Review — Phase 4: Code Review

## Usage

```text
/flow:flow-code-review
/flow:flow-code-review --auto
/flow:flow-code-review --manual
/flow:flow-code-review --continue-step
/flow:flow-code-review --continue-step --auto
/flow:flow-code-review --continue-step --manual
```

- `/flow:flow-code-review` — uses configured mode from the state file (default: manual)
- `/flow:flow-code-review --auto` — auto-fix and auto-commit all findings, auto-advance to Learn
- `/flow:flow-code-review --manual` — requires explicit approval of changes and routing decisions
- `/flow:flow-code-review --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next step via Resume Check

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
3. Check `phases.flow-code.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 3: Code must be
     complete. Run /flow:flow-code first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-code-review.commit` and `skills.flow-code-review.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Code Review Plugin Mode Resolution

1. Read `skills.flow-code-review.code_review_plugin` from the state file at `<project_root>/.flow-states/<branch>.json`.
2. Valid values: `"always"` (default), `"auto"`, `"never"`.
3. If the key does not exist → use built-in default: `"always"`.

When `code_review_plugin` is `"never"`, Step 4 (the code-review:code-review plugin) is
skipped entirely and the phase completes after Step 3.

When `code_review_plugin` is `"auto"` or `"always"`, Step 4 runs as normal.

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step. Skip the Announce banner and the Update State section
(do not call `phase-transition --action enter` again). Proceed directly
to the Resume Check section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — Phase 4: Code Review — STARTING
──────────────────────────────────────────────────
```
````

## Update State

Update state for phase entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-code-review --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 4] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

## Resume Check

Read `code_review_step` from the state file (default `0` if absent).

- If `1` — Step 1 is done. Skip to Step 2.
- If `2` — Steps 1-2 are done. Skip to Step 3.
- If `3` — Steps 1-3 are done. Check Code Review Plugin Mode Resolution:
  if `code_review_plugin` is `"never"`, skip to Done.
  Otherwise, skip to Step 4.
- If `4` — All steps are done. Skip to Done.

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The
first three review steps perform inline review passes against the branch
diff. When enabled via Code Review Plugin Mode Resolution, a fourth step
uses the code-review plugin for multi-agent validation. The CLAUDE.md
conventions inform fix decisions.

---

## Step 1 — Simplify

Get the full branch diff to use as review context:

```bash
git diff origin/main..HEAD
```

Perform three review passes on the diff output. Execute each pass
sequentially, aggregating findings as you go.

**Pass 1 — Code Reuse:** Review the diff for duplicated logic, missed
abstractions, and opportunities to consolidate. Identify patterns that
appear in multiple locations and suggest how to share them.

**Pass 2 — Code Quality:** Review the diff for naming clarity,
structural simplicity, readability improvements, and unnecessary
complexity. Identify conditionals that could be simplified, names that
could be clearer, and abstractions that add complexity without value.

**Pass 3 — Efficiency:** Review the diff for unnecessary allocations,
redundant operations, and performance patterns. Identify operations that
could be avoided or simplified without changing behavior.

After all three passes, aggregate the findings. Apply fixes
for any valid findings that improve the code without changing behavior.
It is safe to refactor here because Phase 3 (Code) tests already
verified all behavior.

### Out-of-scope findings

Review the findings for any that are pre-existing
(not introduced by the current PR). For each out-of-scope finding,
classify as Tech Debt and file an issue.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Tech Debt" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-code-review"
```

Repeat for each out-of-scope finding. Then continue to the diff review below.

### Diff review

Show the user what the review passes changed:

```bash
git diff HEAD
```

Render the diff inline in your response.

**If there are no changes** (empty diff), skip the commit and proceed
directly to Step 2.

**If there are changes and commit=auto**, skip the AskUserQuestion and
proceed directly to commit. The diff is still shown for visibility.

**If there are changes and commit=manual**, use AskUserQuestion:

> "Accept Simplify refactoring?"
>
> - **Yes, commit these changes** — accept and proceed to commit
> - **No, revert** — undo the simplifications
> - **Edit manually** — make specific changes before committing
> - **Go back to Code** — revert changes and return to Code phase

**If "Edit manually"**: The user will describe changes. After editing,
run `git diff HEAD` again to show the revised diff. Then ask again:
"Ready to commit?" with the two options: **Yes, commit** or **No, revert**.

**If "No, revert"**: Run `git diff --stat` to list changed files, then
restore each file individually:

```bash
git restore <file>
```

Repeat for each changed file. Never use `git restore .`, `git reset HEAD`,
or `git clean` — these discard changes without review. Restore files one
at a time so each revert is deliberate. After restoring, skip the commit
and proceed to Step 2.

**If "Go back to Code"**: Restore each changed file individually (same
process as "No, revert" above), then follow the back-navigation
instructions below.

**Commit**: Run CI first:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

If green, set the continuation context and flag.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set code_review_step=1, then self-invoke flow:flow-code-review --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set code_review_step=1, then self-invoke flow:flow-code-review --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, use `/flow:flow-commit --auto`; otherwise use `/flow:flow-commit`.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=1
```

To continue to Step 2, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 2 — Review

Read `files.plan` from the state file to get the plan file path. Use the
Read tool to read the plan file.

Get the full branch diff to use as review context:

```bash
git diff origin/main..HEAD
```

Perform five correctness review passes on the diff output, using the plan
file as context. Execute each pass sequentially, aggregating findings as
you go.

**Pass 1 — Plan Alignment:** Review the diff against the plan. Does the
implementation match the plan's intent? Identify missing tasks, extra
scope beyond the plan, and deviations from the planned approach.

**Pass 2 — Logic Correctness:** Review the diff for logic errors. Identify
edge cases, off-by-one errors, null handling gaps, incorrect error
propagation, and race conditions.

**Pass 3 — Test Coverage:** Review the diff for untested code paths.
Identify missing assertions, untested error paths, boundary conditions
without tests, and tests that do not verify what they claim.

**Pass 4 — API Contracts:** Review the diff for interface mismatches.
Identify function signatures that do not match their callers, inconsistent
return types, and interfaces that do not match their documentation.

**Pass 5 — Rule Compliance:** Use the Glob tool to find all
`.claude/rules/*.md` files in the working directory. If no files are
found, skip this pass. Otherwise, use the Read tool to read each file.
Treat each rule as a checklist item. Review the diff for violations of
any accumulated project rule. Identify code that contradicts explicit
guidance from the rules files.

After all five passes, aggregate the findings.

If no findings were identified, show the Review summary with zero
findings listed, then without pausing continue to Step 3.

### Fix every finding

For each finding from the review, classify it:

**Minor finding** (style, missing option, small oversight):

- Fix it directly
- Describe what was fixed and why

**Significant finding** (logic error, missing risk coverage, plan mismatch):

If commit=auto, fix it directly without asking.

If commit=manual, use AskUserQuestion:

> "Found a significant issue: &lt;description&gt;. How would you like to proceed?"
>
> - **Fix it here in Code Review**
> - **Go back to Code**
> - **Go back to Plan**

**Out-of-scope finding** (pre-existing, unrelated to the feature):

Classify as one of:

- **Tech Debt** — working but fragile, duplicated, or convention-violating code
- **Documentation Drift** — docs out of sync with actual behavior

File an issue and move on — do not fix out-of-scope findings.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Tech Debt" --title "<issue_title>" --body-file .flow-issue-body
```

Or for documentation:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Documentation Drift" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-code-review"
```

After fixing in-scope findings, run:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

<HARD-GATE>
`bin/flow ci` must be green before proceeding to Step 3.
Any fix made during Review requires `bin/flow ci` to run again.
</HARD-GATE>

If fixes were made, set the continuation context and flag before committing.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Show review summary, set code_review_step=2, then self-invoke flow:flow-code-review --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Show review summary, set code_review_step=2, then self-invoke flow:flow-code-review --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto use `/flow:flow-commit --auto`,
otherwise use `/flow:flow-commit` for the Review fixes.

### Review summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  FLOW — Code Review — Step 2: Review — SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Findings fixed
  --------------
  - <description of fix and why>
  - <description of fix and why>

  bin/flow ci       : ✓ green

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=2
```

To continue to Step 3, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 3 — Security

Get the full branch diff to use as security review context:

```bash
git diff origin/main..HEAD
```

Perform three security review passes on the diff output. Execute each pass
sequentially, aggregating findings as you go.

**Pass 1 — Input Validation:** Review the diff for injection vulnerabilities,
unsanitized user input, command injection, path traversal, and unsafe
deserialization. Identify any place where external input flows into sensitive
operations without validation or escaping.

**Pass 2 — Authentication & Authorization:** Review the diff for
authentication bypasses, missing access controls, insecure session handling,
and privilege escalation. Identify any place where identity or permissions
are checked incorrectly or not at all.

**Pass 3 — Data Exposure:** Review the diff for sensitive data leaks,
hardcoded secrets, insecure storage, weak cryptography, and information
disclosure. Identify any place where confidential data could be exposed
to unauthorized parties.

After all three passes, aggregate the findings.

### Fix every finding

For each finding from the security review, fix the issue in code, then
run CI:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Set the continuation context and flag:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Continue fixing remaining security findings."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise
invoke `/flow:flow-commit`.

Move to the next finding.

<HARD-GATE>
`bin/flow ci` must be green after every fix. Do not move to the next
finding until the current fix passes `bin/flow ci` and is committed.
</HARD-GATE>

Repeat until all findings are fixed.

If no findings, skip the commit. Show the Security summary with zero
findings, then without pausing continue to Step 4.

### Security summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  FLOW — Code Review — Step 3: Security — SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Findings         : N
  Fixed            : N

  Findings
  --------
  - [FIXED] <description of finding>
  - [FIXED] <description of finding>

  bin/flow ci      : ✓ green

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=3
```

Check Code Review Plugin Mode Resolution:

- If `code_review_plugin` is `"never"` — the plugin is skipped. Invoke
  `flow:flow-code-review --continue-step` using the Skill tool as your
  final action. The Resume Check will route to Done.
- If `code_review_plugin` is `"always"` or `"auto"` — invoke
  `flow:flow-code-review --continue-step` using the Skill tool as your
  final action. The Resume Check will route to Step 4.

If commit=auto was resolved, pass `--auto` as well. Do not output
anything else after this invocation.

---

## Step 4 — Code Review Plugin

Set the continuation context and flag before invoking the child skill:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Wait for all pending background agents to complete. Then process code-review findings, fix issues, run bin/flow ci, then commit if fixes were made."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=code-review:code-review
```

Invoke the `code-review:code-review` plugin using the Skill tool with no
flags or arguments.

This runs a multi-agent review: 4 parallel agents (2x CLAUDE.md
compliance, 1x bug scan, 1x security/logic scan) with a validation layer
that re-validates each finding at 80+ confidence. It produces high-signal
findings only.

If the plugin returns early (pre-flight skip, e.g. "no review needed" or
"already reviewed"), treat this as no findings.

### Background agent check

Plugins may launch background review agents that run asynchronously.
After the child skill returns and the stop-continue hook resumes you,
check for any pending background agent notifications. Wait for ALL
background agents to complete before proceeding. Do not evaluate "no
findings" until every agent has reported. Treat agent findings the same
as direct findings from the child skill.

If the plugin reports no findings, skip the commit. Show the Code Review
Plugin summary with zero findings, then without pausing continue to Done.

### Fix every finding

For each finding from the code-review plugin, fix the issue in code, then
run CI:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Set the continuation context and flag:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Continue fixing remaining code-review findings."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise
invoke `/flow:flow-commit`.

Move to the next finding.

<HARD-GATE>
`bin/flow ci` must be green after every fix. Do not move to the next
finding until the current fix passes `bin/flow ci` and is committed.
</HARD-GATE>

Repeat until all findings are fixed.

### Code Review Plugin summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  FLOW — Code Review — Step 4: Code Review Plugin — SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Findings         : N
  Fixed            : N

  Findings
  --------
  - [FIXED] <description of finding>
  - [FIXED] <description of finding>

  bin/flow ci      : ✓ green

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=4
```

To continue to Done, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Back Navigation

Use AskUserQuestion if a finding is too significant to fix in Code Review:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something

**Go back to Code:** update Phase 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:flow-code`.

**Go back to Plan:** update Phases 4 and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-code-review --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — Phase 4: Code Review — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

### Slack Notification

Read `slack_thread_ts` from the state file. If present, post a thread reply. Best-effort — skip silently on failure.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow notify-slack --phase flow-code-review --message "<message_text>" --thread-ts <thread_ts>
```

If `"status": "ok"`, record the notification:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-notification --phase flow-code-review --ts <ts> --thread-ts <thread_ts> --message "<message_text>"
```

If `"status": "skipped"` or `"status": "error"`, continue without error.

<HARD-GATE>
STOP. Re-read `skills.flow-code-review.continue` from the state file at
`<project_root>/.flow-states/<branch>.json` before advancing.
The previous phase's continue mode does NOT carry over — each phase
has its own mode.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use the value from the state file. If absent → default to manual.
2. If continue=auto → invoke `flow:flow-learn` directly.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 4: Code Review is complete. Ready to begin Phase 5: Learn?"
      Options: "Yes, start Phase 5 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 5 now" and "Not yet"
   d. If Yes → invoke `flow:flow-learn` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-learn` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-continue when ready.
══════════════════════════════════════════════════
```
````

---

## Hard Rules

- Always run `bin/flow ci` after any fix made during Code Review
- Never transition to Learn unless `bin/flow ci` is green
- Fix every finding from inline correctness review, inline security review, and (when enabled) `code-review:code-review` — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- Each active step (Simplify, Review, Security, and Code Review Plugin when enabled) gets its own commit when changes are made
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- After each active step completes, advance to the next step via self-invocation — never pause or wait for user input between steps
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
