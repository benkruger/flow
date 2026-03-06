---
name: review
description: "Phase 5: Review — systematic code review against the plan, identified risks, and framework anti-patterns. Fixes issues found, runs bin/flow ci after any fix, then transitions to Security."
model: sonnet
---

# FLOW Review — Phase 5: Review

## Usage

```text
/flow:review
/flow:review --auto
/flow:review --manual
```

- `/flow:review` — uses configured mode from `.flow.json` (default: manual)
- `/flow:review --auto` — significant findings auto-fixed here (no user routing choice), auto-advance to Security
- `/flow:review --manual` — significant findings prompt user for fix/go-back choice

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
4. Check `phases.4.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 4: Simplify must be
     complete. Run /flow:simplify first."
</HARD-GATE>

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.review.commit` and `skills.review.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.16.4 — Phase 5: Review — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase 5 --action enter
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

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for the diff analysis sub-agent prompt and framework-specific
hard rules. Do not announce the framework — just follow the matching
section silently.

### If Rails

#### Diff Analysis Sub-Agent Prompt

Provide these instructions to the Step 1 sub-agent (fill in the details):

> You are analyzing a feature diff for the FLOW review phase.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for git commands
> (git diff, git log, git blame, git show). Never pipe git output through
> sed, grep, awk, or any other command — read the full output and extract
> what you need in your response. Never use Bash for any other purpose —
> no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Plan file:
> <paste the plan file contents — includes context, approach, risks, and tasks>
>
> First, get the full diff:
>
> ```bash
> git diff origin/main...HEAD
> ```
>
> Read every changed file completely. Then check:
>
> **Plan alignment:**
>
> - Does the implementation match the approach described in the plan?
> - Do schema changes match what was planned?
> - Do model, controller, worker, and route changes match the plan?
> - Are all planned tasks reflected in the diff?
> - Flag any deviation — minor drift or major mismatch.
>
> **Risk coverage:**
>
> - For each risk identified in the plan, confirm it was handled in the diff.
> - Flag any risk not addressed.
>
> **Rails anti-pattern check:**
>
> - Associations: every belongs_to/has_many has inverse_of:, dependent:,
>   class_name: explicit
> - Queries: no N+1, no DB queries in views, no .first/.last for defaults
> - Callbacks: Current attribute usage correct, no update_column
> - Models: self.table_name in namespaced Base, no STI
> - Soft deletes: .unscoped usage correct
> - Workers: halt! in pre_perform!, queue matches sidekiq.yml
> - Tests: create_*! helpers used, both branches tested, assertions present
> - RuboCop: scan diff for rubocop:disable comments, check .rubocop.yml changes
> - Code clarity: descriptive names, no inline comments, no over-engineering
>
> Return structured findings in three categories:
>
> 1. Plan alignment issues (with file:line references)
> 2. Uncovered risks (with which risk and why)
> 3. Anti-pattern violations (with file:line and what to fix)
>
> If a category has no findings, say so explicitly.

#### Rails-Specific Hard Rules

- Any `# rubocop:disable` comment in the diff is an automatic finding — remove it and fix the code
- Any modification to `.rubocop.yml` in the diff is an automatic finding — revert it and fix the code

### If Python

#### Diff Analysis Sub-Agent Prompt

Provide these instructions to the Step 1 sub-agent (fill in the details):

> You are analyzing a feature diff for the FLOW review phase.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for git commands
> (git diff, git log, git blame, git show). Never pipe git output through
> sed, grep, awk, or any other command — read the full output and extract
> what you need in your response. Never use Bash for any other purpose —
> no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Plan file:
> <paste the plan file contents — includes context, approach, risks, and tasks>
>
> First, get the full diff:
>
> ```bash
> git diff origin/main...HEAD
> ```
>
> Read every changed file completely. Then check:
>
> **Plan alignment:**
>
> - Does the implementation match the approach described in the plan?
> - Do module and script changes match what was planned?
> - Are all planned tasks reflected in the diff?
> - Flag any deviation — minor drift or major mismatch.
>
> **Risk coverage:**
>
> - For each risk identified in the plan, confirm it was handled in the diff.
> - Flag any risk not addressed.
>
> **Python anti-pattern check:**
>
> Imports: no circular imports, no wildcard imports (`from x import *`).
> Mutable defaults: no mutable default arguments (`def f(x=[])`).
> Error handling: no bare `except:`, no broad `except Exception`
> without re-raise.
> Type safety: consistent use of type hints if the project uses them.
> Tests: fixtures used where appropriate, both branches tested,
> assertions present.
> Lint: scan diff for noqa/type:ignore comments.
> Code clarity: descriptive names, no inline comments, no over-engineering.
>
> Return structured findings in three categories:
>
> 1. Plan alignment issues (with file:line references)
> 2. Uncovered risks (with which risk and why)
> 3. Anti-pattern violations (with file:line and what to fix)
>
> If a category has no findings, say so explicitly.

#### Python-Specific Hard Rules

- Any `# noqa` or `# type: ignore` comment in the diff is a finding — remove it and fix the code
- Any modification to lint configuration in the diff is a finding — revert it and fix the code

---

## Step 1 — Launch diff analyzer sub-agent

Read `plan_file` from the state file to get the plan file path. Use the
Read tool to read the plan file — it contains the approach, risks, and
planned tasks.

Then launch a mandatory sub-agent to analyze the full diff. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Review diff analysis"`

Provide the sub-agent with the **Diff Analysis Sub-Agent Prompt** from the
framework section above (fill in the feature name and plan file contents).

Wait for the sub-agent to return before proceeding.

---

## Step 2 — Review sub-agent findings

Read the sub-agent's structured findings. For each category:

**Plan alignment issues** — Confirm each finding against the plan file.
Minor drift is a note. Major drift means go back to Code.

**Uncovered risks** — Confirm each finding. An unaddressed risk
is a bug waiting to happen.

**Anti-pattern violations** — Confirm each finding against the actual code.
The sub-agent may have false positives — verify before flagging.

Compile the confirmed findings list for Step 3.

---

## Step 3 — Fixing Findings

For each finding:

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

After fixing any findings, if commit=auto use `/flow:commit --auto`, otherwise use `/flow:commit` for the Review fixes.

Then run `bin/flow ci --if-dirty` — required before any state transition.

<HARD-GATE>
`bin/flow ci` must be green before transitioning to Security.
Any fix made during Review requires `bin/flow ci` to run again.
</HARD-GATE>

---

## Step 4 — Present review summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 5: Review — SUMMARY
============================================

  Plan alignment    : ✓ matches approved plan
  Risk coverage     : ✓ all risks accounted for

  Findings fixed
  --------------
  - <description of fix and why>
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
`in_progress`, then invoke `flow:code`.

**Go back to Plan:** update Phases 5, 4, and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:plan`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase 5 --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.16.4 — Phase 5: Review — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`.

**If continue=auto**, skip the transition question and invoke `flow:security` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 5: Review is complete. Ready to begin Phase 6: Security?"
>
> - **Yes, start Phase 6 now** — invoke `flow:security`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 6 now" and "Not yet"

**If Yes** — invoke `flow:security` using the Skill tool.

**If Not yet**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Always run `bin/flow ci` after any fix made during Review
- Never transition to Security unless `bin/flow ci` is green
- Never skip the plan alignment check
- Never skip the risk coverage check
- Read the full diff before starting — no partial reviews
- Plus the **Framework-Specific Hard Rules** from the framework section above
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
