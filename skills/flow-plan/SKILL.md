---
name: flow-plan
description: "Phase 2: Plan — invoke DAG decomposition, explore the codebase, design the approach, and create an implementation plan."
---

# FLOW Plan — Phase 2: Plan

## Usage

```text
/flow:flow-plan
/flow:flow-plan --auto
/flow:flow-plan --manual
```

- `/flow:flow-plan` — uses configured mode from the state file (default: manual)
- `/flow:flow-plan --auto` — auto-advance to Code without asking
- `/flow:flow-plan --manual` — requires explicit approval before advancing

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
4. Note `pr_number`, `prompt`, and `branch` from the state file — you will need them later.

</HARD-GATE>

Keep the project root, branch, state data, and `pr_number` from the gate
in context — use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → continue=auto
2. If `--manual` was passed → continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-plan.continue`.
4. If the state file has no `skills` key → use built-in default: continue=manual

## DAG Mode Resolution

1. Read `skills.flow-plan.dag` from the state file.
2. Valid values: `"auto"` (default), `"always"`, `"never"`.
3. If the key does not exist → use built-in default: `"auto"`.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v0.32.4 — Phase 2: Plan — STARTING
──────────────────────────────────────────────────
```
````

## Update State

Update state for phase entry:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-plan --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

After every Bash command in Steps 1–4, log it to `.flow-states/<branch>.log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 2] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

---

## Resume Check

Check `files.plan` and `files.dag` in the state file:

- If `files.plan` is set (not null), the plan was previously written.
  Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW — Plan already approved
──────────────────────────────────────────────────
  Plan file: <files.plan path>
──────────────────────────────────────────────────
```
````

  Skip to "Done — Update state and complete phase" to finish the phase.

- If `files.dag` is set (not null) but `files.plan` is null, the DAG was
  produced but the plan was not yet written. Read the DAG output file
  at `files.dag` path. Skip to Step 3 (Explore and write plan).

- If both are null, proceed to Step 1.

---

## Step 1 — Feature description and issue context

Use the `prompt` from the state file as the feature description. This is the
full text the user passed to `/flow:flow-start` — it describes what to build.

Do not ask "What are we building?" — the prompt is the input for the planning
phase.

### Fetch referenced issues

Check the prompt for `#N` patterns (e.g., `#107`, `#42`). For each unique
issue number found, fetch the issue body:

```bash
gh issue view <issue_number> --json number,title,body
```

Use the issue body as primary planning context — it contains the detailed
problem description, acceptance criteria, and context that the short prompt
cannot convey. The prompt words alone may be ambiguous; the issue body is
the authoritative source.

If the prompt contains no `#N` patterns, skip this step and use the prompt
as-is.

If a fetch fails (issue does not exist, permissions error, network failure),
note the failure and continue with the remaining issues and prompt text.
Do not stop planning because one issue could not be fetched.

Proceed to Step 2.

---

## Step 2 — DAG decomposition

Check the DAG mode from DAG Mode Resolution:

- If dag=`"never"` → skip to Step 3.
- If dag=`"auto"` or `"always"` → invoke the decompose plugin.

Invoke `/decompose:decompose` using the Skill tool. Pass the feature
description (the `prompt` from Step 1, plus any issue context fetched)
as the task argument.

The decompose plugin will produce structured DAG output:
an impact preview, an XML DAG plan with nodes and dependencies,
node-by-node reasoning, and a synthesis.

After the decompose plugin returns, save the complete decompose output:

1. Capture everything the decompose plugin produced — the XML DAG plan,
   all node executions with quality scores, and the synthesis block.
   Write it verbatim to `<project_root>/.flow-states/<branch>-dag.md`
   using the Write tool, wrapped with a markdown heading:

   ```text
   # DAG Analysis: <feature description>

   <complete output from decompose plugin>
   ```

2. Store the path in the state file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set files.dag=<dag_file_path>
```

Replace `<dag_file_path>` with the relative path `.flow-states/<branch>-dag.md`.

Proceed to Step 3.

---

## Step 3 — Explore and write the plan

Explore the codebase, validate the DAG against reality (if DAG was
produced), and write the implementation plan to a plan file.

If a DAG was produced in Step 2, use it as the foundation:
- Validate that the files and patterns the DAG references actually exist
- Check whether the dependencies the DAG identified make sense
- Look for patterns or constraints the DAG missed

### Framework Conventions

Read the project's CLAUDE.md for framework-specific conventions (architecture
patterns, test conventions, CI fix order). The CLAUDE.md is primed with
framework knowledge during `/flow:flow-prime`. Follow those conventions when
writing the Tasks section of the plan.

Always include TDD order — test task before every implementation task.

### Plan file structure

Write the plan file to `<project_root>/.flow-states/<branch>-plan.md`
where `<branch>` is the feature branch name. This keeps the plan
alongside other feature artifacts in `.flow-states/`.

The plan file should include these sections:

- **Context** — what the user wants to build and why
- **Exploration** — what exists in the codebase, affected files, patterns discovered
- **Risks** — what could go wrong, edge cases, constraints
- **Approach** — the chosen approach and rationale
- **Dependency Graph** (if DAG was produced) — table of tasks with types and dependencies:

```markdown
| Task | Type | Depends On |
|------|------|------------|
| 1. Write conftest fixtures | design | — |
| 2. Write parser tests | test | 1 |
| 3. Implement parser | implement | 2 |
```

- **Tasks** — ordered implementation tasks derived from the dependency graph,
  each with:
  - Description of what to build
  - Files to create or modify
  - TDD notes (what the test should verify)

Proceed to Step 4.

---

## Step 4 — Store plan file and complete phase

Store the plan file path in the state file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set files.plan=<plan_file_path>
```

Replace `<plan_file_path>` with the relative path `.flow-states/<branch>-plan.md`.

Render the complete PR body (artifacts, plan, DAG, timings, and state
are all derived from the state file automatically):

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow render-pr-body --pr <pr_number>
```

Complete the phase:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-plan --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

---

## Done — Banner and transition

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.32.4 — Phase 2: Plan — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-code` directly.

**If continue=manual**, use AskUserQuestion:

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

- Never write implementation code during Plan — task descriptions only
- The plan file lives in `.flow-states/<branch>-plan.md` alongside other feature artifacts
- Store the plan file path in state before completing the phase
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
