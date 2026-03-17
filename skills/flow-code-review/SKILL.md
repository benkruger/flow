---
name: flow-code-review
description: "Phase 4: Code Review — four lenses on the same diff: clarity, correctness, safety, and CLAUDE.md compliance. Invokes /simplify, /review, /security-review, and code-review:code-review with a commit after each step."
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

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-code-review.commit` and `skills.flow-code-review.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=manual, continue=manual

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
  FLOW v0.31.4 — Phase 4: Code Review — STARTING
──────────────────────────────────────────────────
```
````

## Update State

Update state for phase entry:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-code-review --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 4] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

## Resume Check

Read `code_review_step` from the state file (default `0` if absent).

- If `1` — Step 1 is done. Skip to Step 2.
- If `2` — Steps 1-2 are done. Skip to Step 3.
- If `3` — Steps 1-3 are done. Skip to Step 4.
- If `4` — All steps are done. Skip to Done.

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The
first three review steps use Claude's built-in commands which apply
language-aware checks automatically. The fourth step uses the
code-review plugin for multi-agent validation. The CLAUDE.md conventions
inform fix decisions.

---

## Step 1 — Simplify

Set the continuation flag before invoking the child skill:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=simplify
```

Invoke Claude Code's built-in `/simplify` skill using the Skill tool.
`/simplify` refactors code for clarity, reduces complexity, and improves
naming while preserving exact functionality. It is safe to run here
because Phase 3 (Code) tests already verified all behavior.

Wait for `/simplify` to complete and report its changes.

### Out-of-scope findings

Review `/simplify`'s output for any findings it identified but did not fix
because they are pre-existing (not introduced by the current PR). For each
out-of-scope finding, classify as Tech Debt and file an issue.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Tech Debt" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-code-review"
```

Repeat for each out-of-scope finding. Then continue to the diff review below.

### Diff review

Show the user what `/simplify` changed:

```bash
git diff HEAD
```

Render the diff inline in your response.

**If there are no changes** (empty diff), skip the commit and proceed
directly to Step 2.

**If there are changes and commit=auto**, skip the AskUserQuestion and
proceed directly to commit. The diff is still shown for visibility.

**If there are changes and commit=manual**, use AskUserQuestion:

> "Accept /simplify refactoring?"
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

Repeat for each changed file. Never use `git restore .` — it discards
all changes without review. After restoring, skip the commit and proceed
to Step 2.

**If "Go back to Code"**: Restore each changed file individually (same
process as "No, revert" above), then follow the back-navigation
instructions below.

**Commit**: Run `bin/flow ci` first. If green, set the continuation flag:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, use `/flow:flow-commit --auto`; otherwise use `/flow:flow-commit`.

After the commit completes, clear the continuation flag:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

Record step completion:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=1
```

Clear the continuation flag before self-invoking:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

To continue to Step 2, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 2 — Review

Read `pr_number` from the state file. Read `plan_file` from the state
file to get the plan file path. Use the Read tool to read the plan file.

Set the continuation flag before invoking the child skill:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=review
```

Invoke Claude's built-in review command on the PR:

```text
/review <pr_number>
```

This analyzes the full diff for code quality, correctness, and test
coverage using Claude's language-aware analysis.

If `/review` reports no findings, show the Review summary with zero
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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Tech Debt" --title "<issue_title>" --body-file .flow-issue-body
```

Or for documentation:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Documentation Drift" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-code-review"
```

After fixing in-scope findings, run:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

<HARD-GATE>
`bin/flow ci` must be green before proceeding to Step 3.
Any fix made during Review requires `bin/flow ci` to run again.
</HARD-GATE>

If fixes were made, set the continuation flag before committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto use `/flow:flow-commit --auto`,
otherwise use `/flow:flow-commit` for the Review fixes.

After the commit completes, clear the continuation flag:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=2
```

Clear the continuation flag before self-invoking:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

To continue to Step 3, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 3 — Security

Set the continuation flag before invoking the child skill:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=security-review
```

Invoke Claude's built-in security review command:

```text
/security-review
```

This analyzes the branch diff for security vulnerabilities using Claude's
language-aware security analysis.

### Fix every finding

For each finding from the security review:

1. Fix the issue in code
2. Run `bin/flow ci`
3. Set `bin/flow set-timestamp --set _continue_pending=commit`
4. If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise invoke `/flow:flow-commit`.
5. After the commit completes, clear with `bin/flow set-timestamp --set _continue_pending=`
6. Move to the next finding

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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=3
```

Clear the continuation flag before self-invoking:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

To continue to Step 4, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 4 — Code Review Plugin

Set the continuation flag before invoking the child skill:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=code-review:code-review
```

Invoke the `code-review:code-review` plugin using the Skill tool with no
flags or arguments.

This runs a multi-agent review: 4 parallel agents (2x CLAUDE.md
compliance, 1x bug scan, 1x security/logic scan) with a validation layer
that re-validates each finding at 80+ confidence. It produces high-signal
findings only.

If the plugin returns early (pre-flight skip, e.g. "no review needed" or
"already reviewed"), treat this as no findings.

If the plugin reports no findings, skip the commit. Show the Code Review
Plugin summary with zero findings, then without pausing continue to Done.

### Fix every finding

For each finding from the code-review plugin:

1. Fix the issue in code
2. Run `bin/flow ci`
3. Set `bin/flow set-timestamp --set _continue_pending=commit`
4. If commit=auto, invoke `/flow:flow-commit --auto` for the fix. Otherwise invoke `/flow:flow-commit`.
5. After the commit completes, clear with `bin/flow set-timestamp --set _continue_pending=`
6. Move to the next finding

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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=4
```

Clear the continuation flag before self-invoking:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-code-review --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.31.4 — Phase 4: Code Review — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-learn` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 4: Code Review is complete. Ready to begin Phase 5: Learn?"
>
> - **Yes, start Phase 5 now** — invoke `flow:flow-learn`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 5 now" and "Not yet"

**If Yes** — invoke `flow:flow-learn` using the Skill tool.

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
- Fix every finding from `/review`, `/security-review`, and `code-review:code-review` — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- Each step (Simplify, Review, Security, Code Review Plugin) gets its own commit when changes are made
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- After each step (Simplify, Review, Security, Code Review Plugin) completes, advance to the next step via self-invocation — never pause or wait for user input between steps
