---
name: code
description: "Phase 3: Code — execute plan tasks one at a time with TDD. Review diff before each commit. bin/ci must pass before moving to the next task. Framework architecture standards enforced."
model: opus
---

# FLOW Code — Phase 3: Code

## Usage

```text
/flow:code
/flow:code --auto
/flow:code --manual
```

- `/flow:code` — uses configured mode from `.flow.json` (default: manual)
- `/flow:code --auto` — streamline mode active from task 1 (skip per-task approval, still show diffs), auto-advance to Simplify
- `/flow:code --manual` — requires explicit approval for each task

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.2.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 2: Plan must be
     complete. Run /flow:plan first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.code.commit` and `skills.code.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=manual, continue=manual

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.16.4 — Phase 3: Code — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase 3 --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for architecture checks, targeted test command, CI failure fix
order, and framework-specific hard rules. Do not announce the framework —
just follow the matching section silently.

### If Rails

#### Architecture Check

Before writing a single line, check based on task type:

**Model task:**

- Read the full class hierarchy: the model file, its parent class, and ApplicationRecord
- Look for `before_save`, `after_create`, `before_destroy` and all other callbacks
- Check for `default_scope` (soft deletes — use `.unscoped` where needed)
- Note the Base/Create split — never skip reading both
- If `update!` or `save` will be called, check if callbacks will overwrite your values — set `Current` attributes instead of passing directly

**Test task:**

- Search `test/support/` for existing `create_*!` helpers for affected models
- If a helper exists → use it. Never `Model::Create.create!` directly.
- If a helper is missing and multiple tests need it → create it in `test/support/`
- Never `update_column` — always `update!`
- Read the mailer template if testing a mailer — all fields it references must be populated

**Worker task:**

- Check `config/sidekiq.yml` for the correct queue name before writing the worker
- Structure: `pre_perform!` (load/validate, call `halt!` to stop), `perform!` (main work), `post_perform!` (cleanup/notifications)
- Test via `worker.perform(...)`, check `worker.halted?`

**Controller task:**

- Params via `options` (OpenStruct): `options.record_id`
- Responses: `render_ok`, `render_error`, `render_unauthorized`, `render_not_found`
- Check which subdomain's BaseController to inherit from

**Route task:**

- Always use `scope` with `module:`, `as:`, `controller:`, `action:` explicitly
- Never raw paths — always named route helpers
- Check `config/routes/` for the correct file for this subdomain

#### Targeted Test Command

Run the specific test file to confirm it fails/passes:

```bash
bin/rails test <test/path/to/file_test.rb>
```

#### CI Failure Fix Order

If `bin/flow ci` fails:

- RuboCop violations → `rubocop -A` first, then manual fixes
- Test failures → understand the root cause, fix the code not the test
- Coverage gaps → write the missing test

#### Rails-Specific Hard Rules

- **Never use `Model::Create.create!`** in tests — always `create_*!` helpers
- **Never use `update_column`** — always `update!`
- **Always read full class hierarchy** before touching any model
- **Never disable a RuboCop cop** — fix the code, not the cop. No `# rubocop:disable` without direct user approval. Stop and ask if you believe it is genuinely necessary.
- **Never modify `.rubocop.yml`** — fix the code, not the configuration. Ask the user explicitly before touching this file.

### If Python

#### Architecture Check

Before writing a single line, check based on task type:

**Module task:**

- Read the full module and its imports
- Check for circular import risks
- Note any module-level state or initialization
- If modifying a function signature, grep for all callers

**Test task:**

- Check `conftest.py` for existing fixtures for affected modules
- If a fixture exists → use it. Never duplicate fixture logic.
- If a fixture is missing and multiple tests need it → create it in `conftest.py`
- Follow existing test patterns in the project

**Script task:**

- Read the argument parsing and main flow
- Check for error handling and exit codes
- Verify the script is registered in any entry points or bin/ wrappers

#### Targeted Test Command

Run the specific test file to confirm it fails/passes:

```bash
bin/test <tests/path/to/test_file.py>
```

#### CI Failure Fix Order

If `bin/flow ci` fails:

- Lint violations → read the lint output carefully, fix the code
- Test failures → understand the root cause, fix the code not the test
- Coverage gaps → write the missing test

#### Python-Specific Hard Rules

- **Always read module imports** before modifying any module
- **Always check `conftest.py`** for existing fixtures before creating new ones
- **Never add lint exclusions** — fix the code, not the linter configuration

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
first task that doesn't have a matching commit. Print inside a fenced
code block:

````text
```
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

Print inside a fenced code block:

````text
```
============================================
  Task <n> of <total>
  <description>
  Files: <files>
============================================
```
````

### Architecture Check

Follow the **Architecture Check** from the framework section above. Check based
on task type as described there before writing any code.

---

### TDD Cycle

**For every implementation task, there is a paired test task that runs first.**

**Step A — Write the failing test**

Write the test file. Follow the test task description exactly.

Run the **Targeted Test Command** from the framework section above to confirm
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
- Fix each failure following the **CI Failure Fix Order** from the framework section above
- Re-run `bin/flow ci` after each fix
- Max 3 attempts — if still failing after 3, stop and report exactly what is failing

<HARD-GATE>
Do NOT commit and do NOT move to the next task until `bin/flow ci` is green.
</HARD-GATE>

---

### Commit

If commit=auto, use `/flow:commit --auto`. Otherwise, use `/flow:commit`.

The commit message subject should reference the task:

```text
Add <what was built> — Task <n> of <total>
```

---

### Continue to Next Task

Print inside a fenced code block:

````text
```
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
`in_progress`, then invoke `flow:plan`.

---

## All Tasks Complete

Once every task from the plan file is complete:

**Final `bin/flow ci --if-dirty` sweep:**

```bash
bin/flow ci --if-dirty
```

Then check coverage:

```bash
cat coverage/uncovered.txt
```

If there are uncovered lines:
- Write tests for each uncovered line
- Run `bin/flow ci` again
- Repeat until `coverage/uncovered.txt` is empty

<HARD-GATE>
Do NOT transition to Review until `bin/flow ci` is green AND coverage/uncovered.txt
is empty. 100% coverage is mandatory.
</HARD-GATE>

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase 3 --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Print inside a fenced code block:

````text
```
============================================
  FLOW v0.16.4 — Phase 3: Code — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`.

**If continue=auto**, skip the transition question and invoke `flow:simplify` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 3: Code is complete. Ready to begin Phase 4: Simplify?"
>
> - **Yes, start Phase 4 now** — invoke `flow:simplify`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 4 now" and "Not yet"

**If Yes** — invoke `flow:simplify` using the Skill tool.

**If Not yet**, print inside a fenced code block:

````text
```
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
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
- Plus the **Framework-Specific Hard Rules** from the framework section above
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
