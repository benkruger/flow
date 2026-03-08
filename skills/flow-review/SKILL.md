---
name: flow-review
description: "Phase 5: Review — systematic code review against the plan, identified risks, and framework anti-patterns. Fixes issues found, runs bin/flow ci after any fix, then transitions to Security."
model: sonnet
---

# FLOW Review — Phase 5: Review

## Usage

```text
/flow:flow-review
/flow:flow-review --auto
/flow:flow-review --manual
```

- `/flow:flow-review` — uses configured mode from `.flow.json` (default: manual)
- `/flow:flow-review --auto` — significant findings auto-fixed here (no user routing choice), auto-advance to Security
- `/flow:flow-review --manual` — significant findings prompt user for fix/go-back choice

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
4. Check `phases.flow-simplify.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 4: Simplify must be
     complete. Run /flow:flow-simplify first."
</HARD-GATE>

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.flow-review.commit` and `skills.flow-review.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 5: Review — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-review --action enter
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
YYYY-MM-DDTHH:MM:SSZ [Phase 5] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The review
uses Claude's built-in `/review` command which applies language-aware checks
automatically. The CLAUDE.md conventions inform fix decisions.

---

## Step 1 — Run code review

Read `pr_number` from the state file. Read `plan_file` from the state file
to get the plan file path. Use the Read tool to read the plan file.

Invoke Claude's built-in review command on the PR:

```text
/review <pr_number>
```

This analyzes the full diff for code quality, correctness, security issues,
and test coverage using Claude's language-aware analysis.

---

## Step 2 — Fix every finding

For each finding from the review:

**Minor finding** (style, missing option, small oversight):

- Fix it directly
- Describe what was fixed and why

**Significant finding** (logic error, missing risk coverage, plan mismatch):

If commit=auto, fix it directly here in Review without asking.

If commit=manual, use AskUserQuestion:

> "Found a significant issue: <description>. How would you like to proceed?"
>
> - **Fix it here in Review**
> - **Go back to Code**
> - **Go back to Plan**

After fixing findings, run:

```bash
bin/flow ci
```

<HARD-GATE>
`bin/flow ci` must be green before transitioning to Security.
Any fix made during Review requires `bin/flow ci` to run again.
</HARD-GATE>

If fixes were made, if commit=auto use `/flow:flow-commit --auto`, otherwise
use `/flow:flow-commit` for the Review fixes.

---

## Step 3 — Present review summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 5: Review — SUMMARY
============================================

  Findings fixed
  --------------
  - <description of fix and why>
  - <description of fix and why>

  bin/flow ci       : ✓ green

============================================
```
````

---

## Back Navigation

Use AskUserQuestion if a finding is too significant to fix in Review:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something

**Go back to Code:** update Phases 5 and 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:flow-code`.

**Go back to Plan:** update Phases 5, 4, and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-review --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.19.1 — Phase 5: Review — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-security` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 5: Review is complete. Ready to begin Phase 6: Security?"
>
> - **Yes, start Phase 6 now** — invoke `flow:flow-security`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 6 now" and "Not yet"

**If Yes** — invoke `flow:flow-security` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Always run `bin/flow ci` after any fix made during Review
- Never transition to Security unless `bin/flow ci` is green
- Fix every finding from `/review` — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
