---
name: flow-code
description: "Phase 3: Code — execute plan tasks one at a time with TDD. Review diff before each commit. bin/ci must pass before moving to the next task. Framework architecture standards enforced."
model: opus
---

# FLOW Code — Phase 3: Code

## Usage

```text
/flow:flow-code
/flow:flow-code --auto
/flow:flow-code --manual
```

- `/flow:flow-code` — uses configured mode from the state file (default: manual)
- `/flow:flow-code --auto` — streamline mode active from task 1 (skip per-task approval, still show diffs), auto-advance to Code Review
- `/flow:flow-code --manual` — requires explicit approval for each task

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
3. Check `phases.flow-plan.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 2: Plan must be
     complete. Run /flow:flow-plan first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-code.commit` and `skills.flow-code.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.6 — Phase 3: Code — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-code --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions. The CLAUDE.md
is primed with architecture patterns, test conventions, CI failure fix order,
and hard rules during `/flow:flow-prime`. Follow those conventions for:

- **Architecture checks** — what to read before writing code
- **Test patterns** — existing fixtures, helpers, and test conventions
- **Targeted test command** — how to run a single test file
- **CI failure fix order** — how to diagnose and fix CI failures
- **Hard rules** — framework-specific constraints

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 3] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

---

## Reading the Plan

Read `plan_file` from the state file to get the plan file path. Use the
Read tool to read the plan file. Identify the Tasks section — this is the
ordered list of implementation tasks to execute.

---

## Resuming Mid-Code

If this is a resume (re-entering the phase), determine progress by
comparing the plan to committed work:

```bash
git log --oneline origin/main..HEAD
```

Compare commit messages to the tasks in the plan file. Continue from the
first task that doesn't have a matching commit. Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW — Resuming Code
============================================
  Resuming at: <task description>
  Tasks complete: <n> of <total>
============================================
```
````

---

## Task Loop

Work through each task from the plan file in order. For each task:

### Before Starting a Task

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  Task <n> of <total>
  <description>
  Files: <files>
============================================
```
````

### Architecture Check

Follow the **Architecture Check** from the project CLAUDE.md. Check based
on task type as described there before writing any code.

---

### TDD Cycle

**For every implementation task, there is a paired test task that runs first.**

**Step A — Write the failing test**

Write the test file. Follow the test task description exactly.

Run the **Targeted Test Command** from the project CLAUDE.md to confirm
it fails.

The test MUST fail before proceeding. If it passes immediately, the test
is not testing the right thing — rewrite it until it fails for the right reason.

**Step B — Write minimal implementation**

Write only what is needed to make the test pass. No over-engineering.

Run the **Targeted Test Command** again to confirm it passes.

**Step C — Refactor**

Clean up without changing behaviour. Run the test again to confirm it
still passes.

---

### Review

After the TDD cycle passes, show the diff for this task and ask for
review before running `bin/flow ci` or committing.

Run `git status` and `git diff HEAD` as two separate commands, then
render the output inline:

**Status**

```text
modified:   <path/to/implementation_file>
new file:   <path/to/test_file>
```

**Diff**

```diff
+ added lines
- removed lines
```

**If commit=auto**, streamline is active from task 1 — skip the
AskUserQuestion and proceed directly to `bin/flow ci`.

**If streamline mode is active** (opted in during a previous task),
skip the AskUserQuestion and proceed directly to `bin/flow ci`.

Otherwise, use AskUserQuestion:

> "Task <n>: <description> — does this look right?"
>
> - **Yes, run bin/flow ci and commit**
> - **Needs changes** — describe what to fix
> - **Streamline remaining tasks** — (only shown from the second task onward)

**If "Needs changes"** — fix the issue, re-run the test, show the diff
again. Loop until approved.

**If "Streamline remaining tasks"** — set a session-only flag (not
persisted to state). For all remaining tasks, still show the diff for
user visibility, but skip the AskUserQuestion and proceed directly to
`bin/flow ci` and commit.

---

### bin/flow ci Gate

Run `bin/flow ci`. This must be green before committing.

**If `bin/flow ci` fails:**

- Read the output carefully
- Fix each failure following the **CI Failure Fix Order** from the project CLAUDE.md
- Re-run `bin/flow ci` after each fix
- Max 3 attempts — if still failing after 3, stop and report exactly what is failing

<HARD-GATE>
Do NOT commit and do NOT move to the next task until `bin/flow ci` is green.
</HARD-GATE>

---

### Commit

Set the continuation flag before committing:

```bash
bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, use `/flow:flow-commit --auto`. Otherwise, use `/flow:flow-commit`.

The commit message subject should reference the task:

```text
Add <what was built> — Task <n> of <total>
```

After the commit completes, clear the continuation flag:

```bash
bin/flow set-timestamp --set _continue_pending=
```

---

### Continue to Next Task

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
Task <n> complete. <completed> of <total> done.
```
````

Without pausing or asking for confirmation, move to the next task
from the plan file. Only stop looping when all tasks are complete.

---

## Back Navigation

At any point during the task loop, if something fundamental is wrong:

Use AskUserQuestion:
> - **Go back to Plan** — task description is wrong or missing tasks

**Go back to Plan:** update Phase 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

---

## All Tasks Complete

Once every task from the plan file is complete:

**Final `bin/flow ci --if-dirty` sweep:**

```bash
bin/flow ci --if-dirty
```

Then check coverage — Read `coverage/uncovered.txt`.

If there are uncovered lines:
- Write tests for each uncovered line
- Run `bin/flow ci` again
- Repeat until `coverage/uncovered.txt` is empty

<HARD-GATE>
Do NOT transition to Code Review until `bin/flow ci` is green AND coverage/uncovered.txt
is empty. 100% coverage is mandatory.
</HARD-GATE>

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-code --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.28.6 — Phase 3: Code — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-code-review` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 3: Code is complete. Ready to begin Phase 4: Code Review?"
>
> - **Yes, start Phase 4 now** — invoke `flow:flow-code-review`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 4 now" and "Not yet"

**If Yes** — invoke `flow:flow-code-review` using the Skill tool.

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

- **Never skip the TDD cycle** — test must fail before code is written
- **Always show the diff for every task** — when commit=manual, the first task requires explicit approval; when commit=auto, streamline is active from task 1
- **Never skip `bin/flow ci`** — must be green before every commit
- **Never move to the next task** until the current task is committed
- **Never rebase** — always merge
- Plus the **Framework-Specific Hard Rules** from the project CLAUDE.md
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
