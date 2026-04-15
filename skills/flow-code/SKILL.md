---
name: flow-code
description: "Phase 3: Code — execute plan tasks one at a time with TDD. Review diff before each commit. bin/flow ci must pass before moving to the next task. Project architecture standards enforced."
---

# FLOW Code — Phase 3: Code

## Usage

```text
/flow:flow-code
/flow:flow-code --auto
/flow:flow-code --manual
/flow:flow-code --continue-step
/flow:flow-code --continue-step --auto
/flow:flow-code --continue-step --manual
```

- `/flow:flow-code` — uses configured mode from the state file (default: manual)
- `/flow:flow-code --auto` — streamline mode active from task 1 (skip per-task approval, still show diffs), auto-advance to Code Review
- `/flow:flow-code --manual` — requires explicit approval for each task
- `/flow:flow-code --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next task via Resume Check

<HARD-GATE>
Run `phase-enter` as your very first action. If it returns an error, stop
immediately and show the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter --phase flow-code
```

Parse the JSON output. If `"status": "error"`, STOP and show the error.

If `"status": "ok"`, capture the returned fields:
`project_root`, `branch`, `worktree_path`, `pr_number`, `pr_url`,
`feature`, `slack_thread_ts`, `plan_file`, and `mode` (commit + continue).

</HARD-GATE>

Use the returned fields for all downstream references. Do not re-read
the state file or re-run git commands to gather the same information.
Do not `cd` to the project root — `bin/flow` commands find paths
internally.

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
3. Otherwise, use `mode.commit` and `mode.continue` from the `phase-enter` response.
4. If `phase-enter` was skipped (self-invocation), use the mode from the flag that was passed.

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous task's commit. Skip the Announce banner and the `phase-enter`
call (do not enter the phase again). Proceed directly to the Resume
Check section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — Phase 3: Code — STARTING
──────────────────────────────────────────────────
```
````

## Project Conventions

Read the project's CLAUDE.md for project-specific conventions. Each
project owns its own toolchain via `bin/{format,lint,build,test}` and
documents its conventions in CLAUDE.md. Follow those conventions for:

- **Architecture checks** — what to read before writing code
- **Test patterns** — existing fixtures, helpers, and test conventions
- **Targeted test command** — how to run a single test file (typically `bin/test --file <path>`)
- **CI failure fix order** — how to diagnose and fix CI failures
- **Hard rules** — project-specific constraints

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 3] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

---

## Resume Check

Read `files.plan` from the state file to get the plan file path (fall back
to `plan_file` for old state files). Use the Read tool to read the plan file. Identify the Tasks section — this is the
ordered list of implementation tasks to execute.

Read `code_task` from the state file (default `0` if absent).

- If `code_task` > 0 and `code_task` < total tasks: skip to task
  `code_task + 1`. Output in your response (not via Bash) inside a
  fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW — Resuming Code
──────────────────────────────────────────────────
  Resuming at: <task description>
  Tasks complete: <code_task> of <total>
──────────────────────────────────────────────────
```
````

- If `code_task` >= total tasks: skip to All Tasks Complete.

- If `code_task` is 0 and this is a resume (re-entering the phase after
  a session restart), determine progress by comparing the plan to
  committed work:

```bash
git log --oneline origin/main..HEAD
```

Compare commit messages to the tasks in the plan file. Continue from the
first task that doesn't have a matching commit.

---

## Execute Next Task

From the plan file, identify the next task to work on (task number
`code_task + 1`). Execute only this single task — do not look ahead
to subsequent tasks. After committing, self-invoke to handle the next
task in a fresh skill invocation.

### Atomic Task Group

When the plan marks a set of tasks as an **atomic group** (typically
because they form a circular CI dependency — no intermediate state can
pass `bin/flow ci` independently), handle them as a single commit
boundary.

**Detect the group.** When you reach a task that the plan marks as
part of an atomic group, switch to the atomic flow below instead of
the standard single-task flow. The plan will annotate which tasks
belong to the group and explain why they cannot be committed
individually (e.g., "Tasks 3-6 form an atomic group — adding a CI
check requires fixing violations in the same commit").

**Show a group banner.** Output in your response (not via Bash):

````markdown
```text
  ── Atomic Group: Tasks <first>-<last> of <total> ──
  Reason: <why these tasks cannot pass CI independently>
```
````

**Execute all tasks in the group sequentially.** Run the full TDD
cycle (write failing test, implement, refactor, run targeted tests)
and the Architecture Check independently. After completing each
task's TDD cycle, record it and persist the task name for TUI display:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task=<n>
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task_name="<description>"
```

Do NOT run `bin/flow ci` and do NOT commit after intermediate tasks.
Proceed directly to the next task in the group.

**Combined review after the last task.** Show the combined diff
covering all tasks in the group. Run `git status` and `git diff HEAD`
as two separate commands, then render the output inline following the
same format as the ### Review section. If commit=auto, skip the
AskUserQuestion and proceed to CI. Otherwise, ask for review of the
combined diff.

**Single CI gate.** Run `bin/flow ci` once for the entire group.
If it fails, fix and retry following the standard CI failure process.

**Plan Test Verification.** After CI passes, run the Plan Test
Verification check (see the section below) for ALL tasks in the
group — not just the last one. Verify that every test function
named in any task's plan description exists in the codebase.

**Single commit.** Use the standard Commit section flow: set the
continuation context and `_continue_pending`, then invoke
`/flow:flow-commit`. The commit message should reference the group:

```text
Add <what the group accomplished> — Tasks <first>-<last> of <total>
```

**Self-invoke** as usual after the group commit to continue with
the next task after the group.

### Measurement-Only Tasks

Some plan tasks produce no file changes — a final `coverage TOTAL`
capture for the PR body, a `threshold verification` re-run, or a
`final regression re-run` that the plan explicitly names as a task.
These tasks still route through the standard Commit flow below so
every task honors CLAUDE.md's "All commits via `/flow:flow-commit`"
convention and never invents a shortcut.

Execute the task (run `bin/flow ci`, log the measurement, or do
whatever the task description specifies), then follow the Commit
section exactly as a file-changing task would: advance `code_task`
via `set-timestamp`, set `_continue_context` and `_continue_pending=commit`,
and invoke `/flow:flow-commit`. The commit skill's Round 4 runs
`git add -A` followed by `git diff --cached`; when the diff is
empty it prints "Nothing to commit", prints its COMPLETE banner,
and returns to the caller without calling `finalize-commit`. The
self-invocation at the end of the Commit section then fires
unchanged — it runs after `/flow:flow-commit` returns, independent
of whether a commit was actually produced. Do not skip
`/flow:flow-commit` even when you already know the diff will be
empty.

### Before Starting a Task

Persist the task name to the state file for TUI display:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task_name="<description>"
```

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Task <n> of <total> ───────────────────────
  <description>
  Files: <files>
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

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

This must be green before committing.

**If `bin/flow ci` fails:**

- Read the output carefully
- Fix each failure following the **CI Failure Fix Order** from the project CLAUDE.md
- Re-run CI after each fix:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

- Max 3 attempts — if still failing after 3, stop and report exactly what is failing

**Flaky test detection:** If a test fails on one attempt but passes on a
subsequent attempt without any code changes, it is flaky. File a
"Flaky Test" issue with reproduction data and continue.

The issue body must include: the test name, the failure message, how many
attempts it took to pass, and the task being worked on.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Flaky Test" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flaky Test" --title "<issue_title>" --url "<issue_url>" --phase "flow-code"
```

<HARD-GATE>
Do NOT commit and do NOT move to the next task until `bin/flow ci` is green.
</HARD-GATE>

---

### Plan Test Verification

The plan decomposes work into specific test functions. This step verifies
that every test the plan promised was actually written during the TDD
cycle — CI proves tests pass, but cannot prove missing tests exist.

After CI passes and before committing, verify that every test function
the plan explicitly names for this task exists in the codebase.

Re-read the current task's description from the plan file. Look for
explicitly named test functions. These appear as comma-separated lists,
under headings like "Rust tests:" or "Tests:", or inline in the task
description. Test naming conventions vary by language:

- **Rust** — `test_` prefix (e.g., `test_parser_handles_empty_input`)
- **Python** — `test_` prefix (e.g., `test_login_timeout`)
- **Go** — `Test` prefix with capital T (e.g., `TestParseConfig`)
- **Swift** — `test` prefix in camelCase (e.g., `testLoginTimeout`)
- **Ruby** — `test_` prefix for minitest; `it` or `describe` blocks for RSpec

If the task description names specific test functions, use the Grep
tool to verify each one exists as a function or method definition in
the codebase. Match the language convention: `fn <name>` for Rust,
`def <name>` for Python/Ruby, `func <name>` for Go/Swift.

- If all named tests are found → proceed to Commit
- If the task does not name specific test functions → proceed to Commit
- If any named test is missing → list the missing tests, write them,
  re-run CI, and only proceed to Commit once all named tests exist
  and CI is green

---

### Commit

Record the completed task number:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task=<n>
```

Set the continuation context and flag before committing.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-code --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Self-invoke flow:flow-code --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Invoke `/flow:flow-commit`.

The commit message subject should reference the task:

```text
Add <what was built> — Task <n> of <total>
```

To continue to the next task, invoke `flow:flow-code --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this
invocation.

---

## Back Navigation

At any point during task execution, if something fundamental is wrong:

Use AskUserQuestion:
> - **Go back to Plan** — task description is wrong or missing tasks

**Go back to Plan:** update Phase 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

---

## All Tasks Complete

Once every task from the plan file is complete:

**Final `bin/flow ci` sweep:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Then check coverage — Read `coverage/uncovered.txt`.

If there are uncovered lines, write tests for each uncovered line, then
run CI again:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

Repeat until `coverage/uncovered.txt` is empty.

<HARD-GATE>
Do NOT transition to Code Review until `bin/flow ci` is green AND coverage/uncovered.txt
is empty. 100% coverage is mandatory.
</HARD-GATE>

## Done — Update state and complete phase

Finalize the phase (complete + Slack notification in one call):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-finalize --phase flow-code --branch <branch> --thread-ts <slack_thread_ts>
```

Omit `--thread-ts` if `slack_thread_ts` was not returned by `phase-enter`.

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 3: Code — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

<HARD-GATE>
STOP. Parse `continue_action` from the `phase-finalize` output above
to determine how to advance.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use `continue_action` from the `phase-finalize` output.
   If `continue_action` is `"invoke"` → continue=auto.
   If `continue_action` is `"ask"` → continue=manual.
2. If continue=auto → invoke `flow:flow-code-review` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 3: Code is complete. Ready to begin Phase 4: Code Review?"
      Options: "Yes, start Phase 4 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 4 now" and "Not yet"
   d. If Yes → invoke `flow:flow-code-review` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-code-review` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-code-review when ready.
══════════════════════════════════════════════════
```
````

---

## Hard Rules

- **Never skip the TDD cycle** — test must fail before code is written
- **Always show the diff for every task** — when commit=manual, the first task requires explicit approval; when commit=auto, streamline is active from task 1
- **Never skip `bin/flow ci`** — must be green before every commit
- **Never move to the next task** until the current task is committed
- **Never rebase** — always merge
- Plus the **Project-Specific Hard Rules** from the project CLAUDE.md
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
