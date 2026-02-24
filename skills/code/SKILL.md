---
name: code
description: "Phase 5: Code — execute plan tasks one at a time with TDD. Review diff before each commit. bin/ci must pass before moving to the next task. All Rails architecture standards enforced."
---

# FLOW Code — Phase 5: Code

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.claude/flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
4. Check `phases.4.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 4: Plan must be
     complete. Run /flow:plan first."
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````
```
============================================
  FLOW — Phase 5: Code — STARTING
  Recommended model: Opus
============================================
```
````

## Update State

Read `.claude/flow-states/<branch>.json`. cd into the worktree.

Update Phase 5:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `5`

## Logging

Wrap every Bash command (except the HARD-GATE) with timestamps in the
**same Bash call** — no separate calls for logging:

```bash
date -u +"%Y-%m-%dT%H:%M:%SZ [Phase 5] Step X — desc — START" >> /tmp/flow-<branch>.log; COMMAND; EC=$?; date -u +"%Y-%m-%dT%H:%M:%SZ [Phase 5] Step X — desc — DONE (exit $EC)" >> /tmp/flow-<branch>.log; exit $EC
```

Get `<branch>` from the state file. The gap between DONE and the next
START = Claude's processing time.

---

## Resuming Mid-Code

If any tasks in `state["plan"]["tasks"]` have `status: "in_progress"`,
this is a resume. Print inside a fenced code block:

````
```
============================================
  FLOW — Resuming Code
============================================
  Resuming at Task <id>: <description>
  Tasks complete: <n> of <total>
============================================
```
````

Continue from the first task with `status: "in_progress"` or `"pending"`.

---

## Task Loop

Work through `state["plan"]["tasks"]` in order. For each task:

### Before Starting a Task

Update the task in state: `status → in_progress`, `started_at → now`.

Print inside a fenced code block:

````
```
============================================
  Task <id> of <total> — <type>
  <description>
  Files: <files>
============================================
```
````

### Architecture Check

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

---

### TDD Cycle

**For every implementation task, there is a paired test task that runs first.**

**Step A — Write the failing test**

Write the test file. Follow the test task description exactly.

Run the specific test file to confirm it fails:

```bash
bin/rails test <test/path/to/file_test.rb>
```

The test MUST fail before proceeding. If it passes immediately, the test
is not testing the right thing — rewrite it until it fails for the right reason.

**Step B — Write minimal implementation**

Write only what is needed to make the test pass. No over-engineering.

Run the test again to confirm it passes:

```bash
bin/rails test <test/path/to/file_test.rb>
```

**Step C — Refactor**

Clean up without changing behaviour. Run the test again to confirm it
still passes.

---

### Review

After the TDD cycle passes, show the diff for this task and ask for
review before touching `bin/ci` or committing.

Run `git status` and `git diff HEAD` as two separate commands, then
render the output inline:

**Status**
```
modified:   app/models/payment/base.rb
new file:   test/models/payment/base_test.rb
```

**Diff**
```diff
+ added lines
- removed lines
```

Then use AskUserQuestion:

> "Task <id>: <description> — does this look right?"
> - **Yes, run bin/ci and commit**
> - **Needs changes** — describe what to fix

**If "Needs changes"** — fix the issue, re-run the test, show the diff
again. Loop until approved.

---

### bin/ci Gate

Run `bin/ci`. This must be green before committing.

**If bin/ci fails:**

- Read the output carefully
- Fix each failure — follow the same approach as flow:start gem breakage:
  - RuboCop violations → `rubocop -A` first, then manual fixes
  - Test failures → understand the root cause, fix the code not the test
  - Coverage gaps → write the missing test
- Re-run `bin/ci` after each fix
- Max 3 attempts — if still failing after 3, stop and report exactly what is failing

<HARD-GATE>
Do NOT commit and do NOT move to the next task until bin/ci is green.
</HARD-GATE>

---

### Commit

Use `/flow:commit` to review and commit this task's changes.

The commit message subject should reference the task:
```
Add <what was built> — Task <id> of <total>
```

---

### Complete the Task

Update the task in state:
- `status → complete`
- `completed_at → now`

Print inside a fenced code block:

````
```
Task <id> complete. <n> of <total> done.
```
````

Then move to the next task. Loop.

---

## Back Navigation

At any point during the task loop, if something fundamental is wrong:

Use AskUserQuestion:
> - **Go back to Plan** — task description is wrong or missing tasks
> - **Go back to Design** — the approach itself needs rethinking
> - **Go back to Research** — something was missed that changes everything

**Go back to Plan:** update Phase 5 to `pending`, Phase 4 to
`in_progress`, then invoke `flow:plan`.

**Go back to Design:** update Phases 5 and 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:design`.

**Go back to Research:** update Phases 5, 4, and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:research`.

---

## All Tasks Complete

Once every task in `state["plan"]["tasks"]` is `complete`:

**Final bin/ci sweep:**

```bash
bin/ci
```

Then check coverage:

```bash
cat coverage/uncovered.txt
```

If there are uncovered lines:
- Write tests for each uncovered line
- Run `bin/ci` again
- Repeat until `coverage/uncovered.txt` is empty

<HARD-GATE>
Do NOT transition to Review until bin/ci is green AND coverage/uncovered.txt
is empty. 100% coverage is mandatory.
</HARD-GATE>

## Done — Update state and complete phase

Update Phase 5 in state:
1. `cumulative_seconds` += `current_time - session_started_at`
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `6`

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 5: Code is complete. Ready to begin Phase 6: Review?"
> - **Yes, start Phase 6 now** — invoke `flow:review`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 6 now" and "Not yet"

**If Yes**, print inside a fenced code block:

````
```
============================================
  FLOW — Phase 5: Code — COMPLETE
============================================
```
````

**If Not yet**, print inside a fenced code block:

````
```
============================================
  FLOW — Paused
  Run /flow:resume when ready to continue.
============================================
```
````

---

## Hard Rules

- **Never skip the TDD cycle** — test must fail before code is written
- **Never skip the review** — user approves every task before bin/ci runs
- **Never skip bin/ci** — must be green before every commit
- **Never move to the next task** until the current task is committed
- **Never use `Model::Create.create!`** in tests — always `create_*!` helpers
- **Never use `update_column`** — always `update!`
- **Never rebase** — always merge
- **Always read full class hierarchy** before touching any model
- **Never disable a RuboCop cop** — fix the code, not the cop. No `# rubocop:disable` without direct user approval. Stop and ask if you believe it is genuinely necessary.
- **Never modify `.rubocop.yml`** — fix the code, not the configuration. Ask the user explicitly before touching this file.