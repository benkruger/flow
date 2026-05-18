---
name: flow-code
description: "Phase 2: Code — execute plan tasks one at a time with TDD. Review diff before each commit. bin/flow ci must pass before moving to the next task. Project architecture standards enforced."
---

# FLOW Code — Phase 2: Code

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
- `/flow:flow-code --auto` — streamline mode active from task 1 (skip per-task approval, still show diffs), auto-advance to Review
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
`project_root`, `branch`, `worktree_path`, `worktree_cwd`,
`relative_cwd`, `pr_number`, `pr_url`, `feature`, `slack_thread_ts`,
`plan_file`, and `mode` (commit + continue).

</HARD-GATE>

Use the returned fields for all downstream references. Do not re-read
the state file or re-run git commands to gather the same information.
Do not `cd` to the project root — `bin/flow` commands find paths
internally.

## Re-anchor cwd

Mono-repo flows started inside a subdirectory (e.g. `api/`) capture
that path as `relative_cwd` and rely on cwd staying at
`<worktree>/<relative_cwd>` so subsequent `bin/flow` calls pass the
cwd-drift guard. Context loss between skill invocations can reset cwd
to the main repo root; the bash block below re-anchors regardless of
how the session got here. Substitute the `worktree_cwd` value from the
phase-enter response — a no-op for root-level flows (where it equals
`worktree_path`) and a real re-anchor for mono-repo flows.

```bash
cd "<worktree_cwd>"
```

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
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
  FLOW v2.2.0 — Phase 2: Code — STARTING
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

After every Bash command completes, log it to `.flow-states/<branch>/log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 2] Step X — desc (exit EC)"
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
  committed work.

**Resolve the integration branch.** Run the `bin/flow base-branch`
command shown below (it uses the plugin root prefix) to retrieve
the base branch the flow coordinates against. Capture its stdout —
call the value `<base_branch>` — and substitute it into the
`git log` command below. A repo whose default branch is `staging`
produces `<base_branch> = staging`; a standard repo produces
`<base_branch> = main`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

**Get the commit log.** Substitute `<base_branch>` with the value
you just captured.

```bash
git log --oneline origin/<base_branch>..HEAD
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
task's TDD cycle, record it and persist the task name for TUI display.
Both updates can go in a single call:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task=<n> --set code_task_name="<description>"
```

For atomic groups with multiple tasks, batch all counter advances in
one call. `apply_updates` processes `--set` arguments sequentially
against mutating in-memory state, so each +1 step is validated in
order. Example for tasks 3-5:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_task=3 --set code_task=4 --set code_task=5 --set code_task_name="<description>"
```

Do NOT run `bin/flow ci` and do NOT commit after intermediate tasks.
Proceed directly to the next task in the group.

**Combined review after the last task.** Show the combined diff
covering all tasks in the group. Run `git status` and `git diff HEAD`
as two separate commands, then render the output inline following the
same format as the ### Review section. If commit=auto, skip the
AskUserQuestion and proceed to Plan Test Verification. Otherwise,
ask for review of the combined diff.

**CI gate routes through `/flow:flow-commit`.** As with single-task
commits, the group's CI gate runs inside `finalize-commit`
(`ci::run_impl()` Rust call). A failed internal CI returns the
error to the model, which fixes and retries. Do NOT invoke
`bin/flow ci` directly between tasks in the group — Layer 11 of
`validate-pretool` blocks it.

**Plan Test Verification.** Run the Plan Test Verification check
(see the section below) for ALL tasks in the group — not just the
last one. Verify that every test function named in any task's
plan description exists in the codebase.

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

Skip the TDD Cycle (there is no test or implementation to write
for a task that produces no file changes) and perform the task's
measurement action as the task body — for example, `bin/flow log`
to record a TOTAL into the session log. Do NOT invoke
`bin/flow ci` directly: Layer 11 of `validate-pretool` blocks it
during Code phase (the `--clean` carve-out is the only allowed
form). Then follow the Commit section exactly as a file-changing
task would: advance `code_task` via `set-timestamp`, set
`_continue_context` and `_continue_pending=commit`, and invoke
`/flow:flow-commit`. The commit skill stages all changes via
`git add -A` in Round 3, then runs `git diff --cached` in Round 4;
when the staged diff is empty it prints "Nothing to commit",
prints its COMPLETE banner, and returns to the caller without
calling `finalize-commit`. The self-invocation at the end of the
Commit section then fires unchanged — it runs after
`/flow:flow-commit` returns, independent of whether a commit was
actually produced. Do not skip `/flow:flow-commit` even when you
already know the diff will be empty.

### Discovery output handling

Some discovery commands during Code phase produce output longer than
the Bash tool's display buffer can return inline. `bin/flow ci --lint`
after a wide ignore-list removal can emit ~50 violation lines; a
plain `git grep` over a deletion target can return hundreds of
matches. When the buffer is exceeded the tool truncates the middle,
so any violation enumeration based on the inline output silently
misses entries. Bash redirection to a temp file would be the natural
workaround, but `validate-pretool` Layer 2 blocks bare `>` redirects
under the FLOW permission model, so the discipline below uses
existing artifacts and dedicated tools instead.

Apply this when a discovery command's output is enumerable (lint
violations, grep hits, file lists) AND the rough output size could
exceed a screen of text. Skip it for short queries (one-line counts,
single-file reads) where inline output is sufficient.

**For `bin/flow ci` and its single-phase variants** (`--format`,
`--lint`, `--build`, `--test`): the runner already writes the
full unabridged stdout-plus-stderr stream to a log file. The path
appears in the runner's footer (`Full log: …-ci-last.log`) and is
also predictable: `<project_root>/.flow-states/<branch>-ci-last.log`.
Use the Read tool on that path to see every violation line — the
Read tool is unaffected by the Bash tool's display-buffer cap.

**For grep-style enumeration** (line-by-line file searches): use
the Grep tool directly. Grep returns matches in a structured form
that bypasses the Bash display buffer; pass `output_mode: "content"`
with `-n` for line-numbered hits, or `output_mode: "files_with_matches"`
when only the file list matters.

**For arbitrary commands without a built-in log file**: the FLOW
permission model does not allow ad-hoc shell redirection to `/tmp/`,
so enumeration must come from a tool that returns structured output.
Glob and Grep cover most cases; for richer output, prefer running
the command in narrower passes (e.g., one directory at a time) so
each pass fits inline.

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
review before committing.

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
AskUserQuestion and proceed directly to Plan Test Verification.

**If streamline mode is active** (opted in during a previous task),
skip the AskUserQuestion and proceed directly to Plan Test Verification.

Otherwise, use AskUserQuestion:

> "Task <n>: <description> — does this look right?"
>
> - **Yes, commit**
> - **Needs changes** — describe what to fix
> - **Streamline remaining tasks** — (only shown from the second task onward)

**If "Needs changes"** — fix the issue, re-run the test, show the diff
again. Loop until approved.

**If "Streamline remaining tasks"** — set a session-only flag (not
persisted to state). For all remaining tasks, still show the diff for
user visibility, but skip the AskUserQuestion and proceed directly to
Plan Test Verification and commit.

---

### CI gate routes through `/flow:flow-commit`

The per-task CI gate runs inside `/flow:flow-commit`'s
`finalize-commit` call as a Rust function (`ci::run_impl()`), not
as a separate Bash invocation. Each TDD task's per-file test
verifies the file under change; cross-file regressions are caught
when the commit's internal CI runs. A failed internal CI returns
the error to the model, which fixes and retries. Do NOT invoke
`bin/flow ci` directly during Code phase — Layer 11 of
`validate-pretool` blocks it per
`.claude/rules/per-file-coverage-iteration.md` "Enforcement".

The `--clean` carve-out is available when phantom-misses
symptoms appear (see the rule). Reach for it only when the
diagnostic signals match.

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

## All Tasks Complete

Once every task from the plan file is complete, proceed directly to
the Done section below.

Every commit during Code phase routed through `/flow:flow-commit` →
`finalize-commit`, which runs the full CI gate (`ci::run_impl()`) and
enforces the 100/100/100 coverage threshold before the commit lands.
The last task's commit verified the same invariants the final sweep
would have measured, so no explicit `bin/flow ci` is needed before
transitioning to Review. Layer 11 of `validate-pretool` blocks
`bin/flow ci` during Code phase to protect against the
single-file-iteration misuse pattern; the per-task per-file gate
(`bin/test tests/<name>.rs`) plus the commit-time internal CI cover
the same surface at lower cost.

<HARD-GATE>
Do NOT transition to Review until every plan task is committed.
100% coverage is enforced by each commit's internal CI gate;
post-commit re-verification is not required.
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
  ✓ FLOW v2.2.0 — Phase 2: Code — COMPLETE (<formatted_time>)
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
2. If continue=auto → invoke `flow:flow-review` directly using the Skill tool.
   Do NOT run `bin/flow status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Run `bin/flow status` via Bash and print its stdout in your
      response inside a fenced code block:

      ```bash
      ${CLAUDE_PLUGIN_ROOT}/bin/flow status
      ```

   b. Use AskUserQuestion:
      "Phase 2: Code is complete. Ready to begin Phase 3: Review?"
      Options: "Yes, start Phase 3 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 3 now" and "Not yet"
   d. If Yes → invoke `flow:flow-review` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-review` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-review when ready.
══════════════════════════════════════════════════
```
````

---

## Hard Rules

- **Never skip the TDD cycle** — test must fail before code is written
- **Always show the diff for every task** — when commit=manual, the first task requires explicit approval; when commit=auto, streamline is active from task 1
- **Never invoke `bin/flow ci` directly during Code phase** — Layer 11 of `validate-pretool` blocks it. The per-task targeted test command (see the project CLAUDE.md) plus `/flow:flow-commit`'s internal `ci::run_impl()` cover the same surface. The `--clean` carve-out is for the documented phantom-misses recovery path only.
- **Never move to the next task** until the current task is committed
- **Never rebase** — always merge
- Plus the **Project-Specific Hard Rules** from the project CLAUDE.md
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- When in autonomous mode, classify tool failures per `.claude/rules/autonomous-flow-self-recovery.md` — mechanical fixes are in-flow, substantive failures prompt the user
