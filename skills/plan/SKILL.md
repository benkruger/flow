---
name: plan
description: "Phase 4: Plan — break the approved design into ordered, executable tasks section by section. Each section is approved individually. Supports back navigation within the plan and to Design or Research."
model: sonnet
---

# FLOW Plan — Phase 4: Plan

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.3.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 3: Design must be
     complete. Run /flow:design first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW v0.13.1 — Phase 4: Plan — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 4:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `4`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for plan section initialization, section verification sub-agent
prompt, section definitions, and plan save schema.

Initialise `state["plan"]` using the **Plan Sections Initialization** from
the matching framework section below.

### If Rails

#### Plan Sections Initialization

Initialise `state["plan"]` if it does not exist:

```json
{
  "sections": {
    "schema":      { "status": "pending", "tasks": [] },
    "models":      { "status": "pending", "tasks": [] },
    "workers":     { "status": "pending", "tasks": [] },
    "controllers": { "status": "pending", "tasks": [] },
    "integration": { "status": "pending", "tasks": [] }
  },
  "current_section": null,
  "tasks": [],
  "approved_at": null
}
```

#### Section Verification Sub-Agent Prompt

Provide these instructions to the section verification sub-agent
(fill in the details):

> You are verifying plan tasks for the FLOW plan phase.
> Feature: <feature name from state>
> Section: <current section name>
>
> Design decisions: <paste relevant state["design"] fields>
> Research findings: <paste relevant state["research"] fields>
>
> Tasks to verify:
> <paste the draft tasks for this section>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> For each task, check the codebase:
>
> 1. **File paths** — Use Glob to verify files exist. For new files,
>    use Glob on the parent directory to confirm it exists.
> 2. **Test helpers** — Use Grep to search test/support/ for create_*!
>    helpers. If not found, flag that a helper creation task is needed.
> 3. **Route context** — Use Read to check route files. What routes already
>    exist in the target file? What patterns are used?
> 4. **Schema context** — Use Read to check data/release.sql for related tables.
>
> Return per-task:
>
> - File paths: verified / corrected
> - Available helpers (if test task)
> - Route context (if route/controller task)
> - Schema context (if schema task)
> - Any corrections needed

#### Section Definitions

Work through each section below in order:

##### Section 1 — Schema

*Skip if `design["schema_changes"]` is empty.*

Generate tasks for all `data/release.sql` changes:

```text
Task 1 — Schema
  Add <table_name> table to data/release.sql
  Files: data/release.sql
  Note: Column types, constraints, indexes, foreign keys
```

One task per table or significant column change. Be specific — include
column names, types, and any constraints.

**No back navigation on first section.**

##### Section 2 — Models and Tests

Generate tasks following strict TDD order — test before implementation:

```text
Task N — Test (failing)
  Write failing test for <Model>::Base — <what it tests>
  Files: test/models/<model>/base_test.rb
  Helper: test/support/<model>_helpers.rb (create_<model>! if needed)
  TDD: write test first, run it, confirm it fails

Task N+1 — Implementation
  Implement <Model>::Base
  Files: app/models/<model>/base.rb
  Note: table_name, soft delete, associations, callbacks

Task N+2 — Test (failing)
  Write failing test for <Model>::Create
  Files: test/models/<model>/create_test.rb

Task N+3 — Implementation
  Implement <Model>::Create
  Files: app/models/<model>/create.rb
```

Pair every implementation task with a test task that comes before it.
Check `test/support/` — if a `create_*!` helper is missing, add a task
to create it before the test task that needs it.

##### Section 3 — Workers

*Skip if `design["worker_changes"]` is empty.*

Generate tasks following TDD order:

```text
Task N — Test (failing)
  Write failing test for <Worker>
  Files: test/workers/<worker>_test.rb
  Note: Test pre_perform!, perform!, post_perform! separately
        Test halt! conditions

Task N+1 — Implementation
  Implement <Worker>
  Files: app/workers/<worker>.rb
  Note: Queue from config/sidekiq.yml, pre_perform!/perform!/post_perform! structure
```

##### Section 4 — Controllers and Routes

Generate tasks following TDD order:

```text
Task N — Route
  Add route to config/routes/<file>.rb
  Files: config/routes/<file>.rb
  Note: scope with module:, as:, controller:, action: explicitly

Task N+1 — Test (failing)
  Write failing controller test
  Files: test/controllers/<path>_test.rb
  Note: authenticate_admin! or authenticate_user! as needed

Task N+2 — Implementation
  Implement <Controller>#<action>
  Files: app/controllers/<path>_controller.rb
  Note: options OpenStruct params, render_ok/render_error responses
```

##### Section 5 — Integration Tests

Generate tasks for any cross-cutting test coverage:

```text
Task N — Integration test
  Write integration test for <end-to-end flow>
  Files: test/integration/<name>_test.rb
  Note: Full lifecycle — create, read, edge cases
```

#### Plan Save Schema

Write to `.flow-states/<branch>.json` under `plan`:

```json
{
  "sections": {
    "schema":      { "status": "approved", "tasks": [1] },
    "models":      { "status": "approved", "tasks": [2,3,4,5] },
    "workers":     { "status": "approved", "tasks": [6,7] },
    "controllers": { "status": "approved", "tasks": [8,9,10] },
    "integration": { "status": "approved", "tasks": [11,12] }
  },
  "current_section": null,
  "tasks": [
    {
      "id": 1,
      "section": "schema",
      "type": "schema",
      "description": "Add payments table to data/release.sql",
      "files": ["data/release.sql"],
      "tdd": false,
      "status": "pending"
    }
  ],
  "approved_at": "<current UTC timestamp>"
}
```

### If Python

#### Plan Sections Initialization

Initialise `state["plan"]` if it does not exist:

```json
{
  "sections": {
    "modules": { "status": "pending", "tasks": [] },
    "scripts": { "status": "pending", "tasks": [] },
    "tests":   { "status": "pending", "tasks": [] }
  },
  "current_section": null,
  "tasks": [],
  "approved_at": null
}
```

#### Section Verification Sub-Agent Prompt

Provide these instructions to the section verification sub-agent
(fill in the details):

> You are verifying plan tasks for the FLOW plan phase.
> Feature: <feature name from state>
> Section: <current section name>
>
> Design decisions: <paste relevant state["design"] fields>
> Research findings: <paste relevant state["research"] fields>
>
> Tasks to verify:
> <paste the draft tasks for this section>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> For each task, check the codebase:
>
> 1. **File paths** — Use Glob to verify files exist. For new files,
>    use Glob on the parent directory to confirm it exists.
> 2. **Test fixtures** — Use Grep to search conftest.py for existing
>    fixtures. If not found, flag that a fixture creation task is needed.
> 3. **Module context** — Use Read to check imports and dependencies.
>    What modules already exist? What patterns are used?
>
> Return per-task:
>
> - File paths: verified / corrected
> - Available fixtures (if test task)
> - Module context (if module task)
> - Any corrections needed

#### Section Definitions

Work through each section below in order:

##### Section 1 — Modules

Generate tasks following strict TDD order — test before implementation:

```text
Task N — Test (failing)
  Write failing test for <module>
  Files: tests/test_<module>.py
  Fixture: conftest.py (add fixture if needed)
  TDD: write test first, run it, confirm it fails

Task N+1 — Implementation
  Implement <module>
  Files: lib/<module>.py (or appropriate path)
  Note: imports, function signatures, error handling
```

Pair every implementation task with a test task that comes before it.
Check `conftest.py` — if a fixture is missing, add a task to create it
before the test task that needs it.

**No back navigation on first section.**

##### Section 2 — Scripts

*Skip if `design["script_changes"]` is empty.*

Generate tasks following TDD order:

```text
Task N — Test (failing)
  Write failing test for <script>
  Files: tests/test_<script>.py
  Note: Test argument parsing, main flow, error handling

Task N+1 — Implementation
  Implement <script>
  Files: bin/<script> or lib/<script>.py
  Note: argparse, main flow, exit codes
```

##### Section 3 — Integration Tests

Generate tasks for any cross-cutting test coverage:

```text
Task N — Integration test
  Write integration test for <end-to-end flow>
  Files: tests/test_<name>_integration.py
  Note: Full lifecycle — setup, execute, verify
```

#### Plan Save Schema

Write to `.flow-states/<branch>.json` under `plan`:

```json
{
  "sections": {
    "modules": { "status": "approved", "tasks": [1,2,3,4] },
    "scripts": { "status": "approved", "tasks": [5,6] },
    "tests":   { "status": "approved", "tasks": [7] }
  },
  "current_section": null,
  "tasks": [
    {
      "id": 1,
      "section": "modules",
      "type": "test",
      "description": "Write failing test for flow_utils",
      "files": ["tests/test_flow_utils.py"],
      "tdd": true,
      "status": "pending"
    }
  ],
  "approved_at": "<current UTC timestamp>"
}
```

## Logging

No logging for this phase. Plan runs no Bash commands beyond the entry
gate — the work is AskUserQuestion calls, sub-agent verifications, and
state file writes.

---

## Resuming Mid-Plan

If `state["plan"]["current_section"]` is already set, this is a resume.

Show what is already approved. Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW — Plan in progress
============================================

  Approved sections:
  [x] Schema
  [x] Models

  Resuming at: Workers

============================================
```
````

Then use AskUserQuestion:

> "Ready to continue from the Workers section?"
>
> - **Yes, continue** — jump to that section
> - **Go back to an approved section** — show picker of approved sections

---

## Step 1 — Quick check-in

Use AskUserQuestion:

> "Ready to plan? Anything to add before we start?"
>
> - **Ready — generate tasks from the design**
> - **I want to add something first** — describe it in Other

If the user adds something, note it and incorporate it into the plan.

---

## Step 2 — Review the design

Review the design data already in context from the gate:
- `feature_description`
- All change category fields (framework-defined — see the design object in the state file)
- `risks`

Skip sections with no changes (e.g., if a change category is empty,
skip that section and note it was skipped).

---

## Section Structure

Work through each section below in order. For each section:

1. Generate tasks for that section
2. Verify tasks via mandatory sub-agent
3. Adjust tasks based on sub-agent findings
4. Present them clearly
5. Ask the section approval question
6. Handle the response

### Section Verification via Sub-Agent

After generating tasks for a section, launch a mandatory sub-agent to verify
them. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Plan task verification — <section name>"`

Provide the sub-agent with the **Section Verification Sub-Agent Prompt**
from the framework section above (fill in the feature name, section, design
decisions, research findings, and tasks).

Adjust tasks based on the sub-agent's findings before presenting the
section to the user.

### Section Approval Question

At the end of every section, use AskUserQuestion:

> "Does the [Section Name] plan look right?"
>
> - **Yes, looks good** — mark section approved, move to next
> - **Needs changes** — describe in Other, revise and re-present
> - **Go back to [previous section]** — only shown if one section back
> - **Go back further** — only shown if two or more sections back

**"Go back further"** triggers a second AskUserQuestion listing all
approved sections as options. User picks one. Mark that section and
all sections after it as `pending`, re-open the chosen section.

When going back: explain clearly which sections were invalidated and
why (because earlier decisions affect later ones).

---

Follow the **Section Definitions** from the framework section above. Work through
each section in order. Skip sections with no corresponding design changes.

---

## Step 3 — Final Full Plan Review

Once all sections are approved, show the complete ordered task list. Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
============================================
  FLOW — Phase 4: Plan — FULL PLAN
============================================

  Feature: <feature>

  [ ] Task 1  — <section>: <description>
  [ ] Task 2  — Test:      Failing test for <component>
  [ ] Task 3  — <section>: Implement <component>
  [ ] Task 4  — Test:      Failing test for <component>
  [ ] Task 5  — <section>: Implement <component>
  ...
  [ ] Task N  — Test:      Integration test for full flow
  [ ] Task N+1 — CI:       bin/ci green

============================================
```
````

Then use AskUserQuestion:

> "Does the full plan look right?"
>
> - **Approve — ready to code**
> - **Needs changes** — describe which tasks to revise
> - **Go back to a plan section** — show section picker
> - **Go back to Design** — approach needs rethinking
> - **Go back to Research** — something was missed

**"Go back to Design"** — update Phase 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:design`.

**"Go back to Research"** — update Phase 4 to `pending`, Phase 3 to
`pending`, Phase 2 to `in_progress`, then invoke `flow:research`.

---

## Step 4 — Save plan to state

Write the plan to `.flow-states/<branch>.json` under `plan` using the
**Plan Save Schema** from the framework section above.

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

---

## Done — Update state and complete phase

Update Phase 4 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `5`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

For the banner below, compute `<formatted_time>` from the integer `cumulative_seconds` stored above: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60. Do not write the formatted string back to the state file.

Print inside a fenced code block:

````text
```
============================================
  FLOW v0.13.1 — Phase 4: Plan — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 4: Plan is complete. Ready to begin Phase 5: Code?"
>
> - **Yes, start Phase 5 now** — invoke `flow:code`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 5 now" and "Not yet"

**If Yes** — invoke `flow:code` using the Skill tool.

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

- Always TDD order — test task before every implementation task
- Never skip sections silently — always note when a section is skipped and why
- When going back invalidates sections, explain clearly which sections need re-approval
- Never write implementation code during Plan — task descriptions only
