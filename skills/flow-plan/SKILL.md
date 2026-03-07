---
name: flow-plan
description: "Phase 2: Plan — explore the codebase, design the approach, and create an implementation plan using Claude Code's native plan mode."
model: opus
---

# FLOW Plan — Phase 2: Plan

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:flow-start first."
3. Check `phases.flow-start.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 1: Start must be
     complete. Run /flow:flow-start first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.1 — Phase 2: Plan — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-plan --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Enter Plan Mode

Call `EnterPlanMode` now. All subsequent steps run in plan mode —
no file edits are possible until the plan is approved and ExitPlanMode
is called.

## Logging

No logging for this phase. Plan uses Claude Code's native plan mode —
there are no Bash commands to log beyond the entry gate.

---

## Resuming

If `plan_file` in the state file is already set (not null), the plan was
previously written and approved. Output in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW — Plan already approved
============================================
  Plan file: <plan_file path>
============================================
```
````

Skip to "Done — Update state and complete phase" to finish the phase.

---

## Step 1 — Ask what we're building

Print this prompt and then stop — wait for the user to respond before
continuing:

> What are we building? Describe the feature and what success looks like.

If the user says the description is in the PR, read the PR description
using `gh pr view` and use that as input instead.

The user's response is the input for the planning phase.

---

## Step 2 — Explore and write the plan

You are already in plan mode (entered after Update State). Explore the
codebase, design the approach, and write the implementation plan to a
plan file. Use the full power of plan mode: read files, search code,
explore patterns, and design the solution.

### Framework Instructions

Read the `framework` field from the state file and incorporate the
matching framework guidance below into the plan. Do not announce the
framework — just follow the matching section silently.

#### If Rails

When writing the Tasks section of the plan, follow Rails conventions:

- **TDD order** — test task before every implementation task
- **Test helpers** — check `test/support/` for existing `create_*!` helpers; if missing, add a task to create them
- **Model hierarchy** — note Base/Create split, read full class hierarchy before modifying
- **Worker structure** — `pre_perform!`/`perform!`/`post_perform!`, queue from `config/sidekiq.yml`
- **Controller patterns** — `options` OpenStruct params, `render_ok`/`render_error` responses
- **Route patterns** — `scope` with `module:`, `as:`, `controller:`, `action:` explicitly
- **Schema** — `data/release.sql` for table changes

#### If Python

When writing the Tasks section of the plan, follow Python conventions:

- **TDD order** — test task before every implementation task
- **Test fixtures** — check `conftest.py` for existing fixtures; if missing, add a task to create them
- **Module structure** — check for circular import risks, module-level state
- **Script patterns** — argparse, exit codes, error handling

### Plan file structure

The plan file should include these sections:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns discovered
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Tasks** — ordered implementation tasks, each with:
  - Description of what to build
  - Files to create or modify
  - TDD notes (what the test should verify)

---

## Step 3 — Store plan file and exit plan mode

Store the plan file path in the state file BEFORE exiting plan mode.
`ExitPlanMode` may clear context, so nothing after it is guaranteed
to run.

```bash
bin/flow set-timestamp --set plan_file=<plan_file_path>
```

Replace `<plan_file_path>` with the actual path to the plan file that
was written during plan mode.

After the plan file path is stored, call `ExitPlanMode`.

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase flow-plan --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW v0.19.1 — Phase 2: Plan — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:flow-status`, then use AskUserQuestion:

> "Phase 2: Plan is complete. Ready to begin Phase 3: Code?"
>
> - **Yes, start Phase 3 now** — invoke `flow:flow-code`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 3 now" and "Not yet"

**If Yes** — invoke `flow:flow-code` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````text
```
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Never write implementation code during Plan — task descriptions only
- The plan file lives in `~/.claude/plans/` — Claude Code's native location
- Store the plan file path in state before completing the phase
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
