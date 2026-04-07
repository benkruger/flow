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
/flow:flow-plan --continue-step
```

- `/flow:flow-plan` — uses configured mode from the state file (default: manual)
- `/flow:flow-plan --auto` — auto-advance to Code without asking
- `/flow:flow-plan --manual` — requires explicit approval before advancing
- `/flow:flow-plan --continue-step` — self-invocation: skip Announce and Update State, dispatch via Resume Check

The `plan-extract` command handles the phase entry gate check (flow-start
must be complete), phase enter, issue fetch, and fast-path extraction for
pre-decomposed issues. It is called as the first action after the Announce
banner. See the Plan Extract section below.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name.

## Mode Resolution

1. If `--auto` was passed → continue=auto
2. If `--manual` was passed → continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-plan.continue`.
4. If the state file has no `skills` key → use built-in default: continue=manual

## DAG Mode Resolution

1. Read `skills.flow-plan.dag` from the state file.
2. Valid values: `"auto"` (default), `"always"`, `"never"`.
3. If the key does not exist → use built-in default: `"auto"`.

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from Step 2
after the decompose plugin returned. Skip the Announce banner and the
Update State section (do not call `phase-transition --action enter` again).
Proceed directly to the Resume Check section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — Phase 2: Plan — STARTING
──────────────────────────────────────────────────
```
````

## Plan Extract

Run plan-extract as the first action. This command handles the phase
entry gate check, phase enter, issue fetch, decomposed label detection,
and plan extraction for pre-decomposed issues — all in a single process
call.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow plan-extract
```

Parse the JSON output. Branch on the `path` field:

**If `path` is `"extracted"` or `"resumed"`** — the plan phase is already
complete. The response contains `plan_content`, `plan_file`, `formatted_time`,
and `continue_action`. Skip directly to the "Fast Path Done" section below.

**If `path` is `"standard"`** — the command entered the phase and fetched
issue context. The response contains `issue_body` (may be null),
`issue_number` (may be null), and `dag_mode`. Note these values and
continue to the Resume Check below.

**If `status` is `"error"`** — show the error message and stop.

<HARD-GATE>
Do NOT proceed past Plan Extract if it returns an error status.
The command checks `phases.flow-start.status` is complete internally.
Show the error message and stop.
</HARD-GATE>

## Logging

After every Bash command in Steps 2–4, log it to `.flow-states/<branch>.log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 2] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

---

## Fast Path Done

When plan-extract returns `"extracted"` or `"resumed"`, the phase is already
complete. Render the plan and transition.

Use the `plan_content` from the plan-extract response. Render it inline in
your response — the complete Context, Exploration, Risks, Approach,
Dependency Graph, and Tasks sections. Run Script Behavior Verification and
Target Path Validation on the plan content (see Step 3 for definitions).
Use Glob and Read to verify files referenced in the Tasks section exist.

Then skip to "Done — Banner and transition", using `formatted_time` and
`continue_action` from the plan-extract response.

---

## Resume Check

Establish context for the standard path: run `git worktree list
--porcelain`. Note the path on the first `worktree` line (this is the
project root). Find the `worktree` entry whose path matches your current
working directory — the `branch refs/heads/<name>` line in that entry is
the current branch (strip the `refs/heads/` prefix). Then read the state
file at
`<project_root>/.flow-states/<branch>.json`. Note `pr_number`, `prompt`,
and `branch` from the state file — you will need them for Steps 2–4.
Keep the project root, branch, state data, and `pr_number` in context.

- If `files.dag` is set (not null) but `files.plan` is null, the DAG was
  produced but the plan was not yet written (dag_file resume). Read the
  DAG output file at `files.dag` path. Skip to Step 3 (Explore and write
  plan).

- If both are null, proceed to Step 2 using `issue_body` and `dag_mode`
  from the plan-extract response.

---

## Step 2 — DAG decomposition

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set plan_step=2
```

### Pre-decomposed issue skip

If the plan-extract response returned a non-null `issue_body` and the
issue has the "decomposed" label (plan-extract detected this internally),
the DAG file and plan file were already created by plan-extract — and the
`"extracted"` path was returned. This Step 2 section only runs for the
`"standard"` path, meaning the issue was NOT decomposed or had no
`## Implementation Plan` section.

If the `issue_body` from plan-extract is non-null and represents an
older-format decomposed issue (no Implementation Plan section), write the
issue body to `<project_root>/.flow-states/<branch>-dag.md` using the
Write tool, wrapped with a markdown heading:

```text
# Pre-Decomposed Analysis: <feature description>

<issue body>
```

Store the path in the state file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set files.dag=<dag_file_path>
```

Replace `<dag_file_path>` with the relative path `.flow-states/<branch>-dag.md`.

Proceed directly to Step 3. Do not set `_continue_pending` or
`_continue_context`. Do not self-invoke. Execution continues in the
same turn.

### Standard DAG decomposition

If the issue is not decomposed, check the `dag_mode` from the
plan-extract response:

- If `dag_mode` is `"never"` → skip to Step 3.
- If `dag_mode` is `"auto"` or `"always"` → invoke the decompose plugin.

Before invoking the decompose plugin, set the continuation flags so the
stop-continue hook forces continuation after decompose returns:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Decompose returned. Save the complete DAG output verbatim to the DAG file, store the path in state, clear _continue_pending, then self-invoke flow:flow-plan --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=decompose
```

Invoke `/decompose:decompose` using the Skill tool. Pass the feature
description (the `prompt` from the state file, plus the `issue_body` from plan-extract if non-null)
as the task argument.

The decompose plugin will produce structured DAG output:
an impact preview, an XML DAG plan with nodes and dependencies,
node-by-node reasoning, and a synthesis.

After the decompose plugin returns, save the complete decompose output:

1. Capture everything the decompose plugin produced — the XML DAG plan,
   all node executions with quality scores, and the synthesis block.
   Do not summarize, condense, reorganize, or rewrite any part of the
   decompose output. The saved file must contain the full response
   exactly as the plugin produced it.
   Write it verbatim to `<project_root>/.flow-states/<branch>-dag.md`
   using the Write tool, wrapped with a markdown heading:

   ```text
   # DAG Analysis: <feature description>

   <complete output from decompose plugin>
   ```

2. Store the path in the state file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set files.dag=<dag_file_path>
```

Replace `<dag_file_path>` with the relative path `.flow-states/<branch>-dag.md`.

Self-invoke `flow:flow-plan --continue-step` using the Skill tool as your
final action. Do not output anything else after this invocation.

---

## Step 3 — Explore and write the plan

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set plan_step=3
```

### Pre-Planned Issue Extraction

If the DAG file (from Step 2) contains an `## Implementation Plan`
section, the issue was filed by `/flow:flow-create-issue` and already
contains a complete plan. Extract it instead of re-deriving.

**Detection.** Use the Read tool to read the DAG file at
`<project_root>/<files.dag path>`. Search for `## Implementation Plan`
in the content.

**If found — extract and validate:**

- Extract all content between `## Implementation Plan` and the next
  `##`-level heading (typically `## Files to Investigate`).
- Promote all headings by one level: `###` becomes `##`, `####` becomes
  `###`. This converts the issue's nested headings into the plan file's
  top-level structure.
- Write the promoted content to
  `<project_root>/.flow-states/<branch>-plan.md` using the Write tool.
- Light validation: use Glob and Read to verify that files referenced in
  the Tasks section exist. Note any missing files (they may need to be
  created by the implementation) but do not block or re-derive the plan.
- Run Script Behavior Verification and Target Path Validation (below)
  on the extracted plan content, then proceed to Step 4.

**If not found** — the issue is an older-format decomposed issue without
a plan. Use the issue body content from the DAG file as a head start
for plan writing:
- Acceptance criteria inform task definitions — each criterion maps to
  one or more implementation tasks
- Files-to-investigate inform exploration starting points — read those
  files first
- Out-of-scope boundaries constrain the plan — do not add tasks outside
  the stated scope
- The issue body has already been validated by the user — do not
  re-evaluate the problem statement

Continue with the standard exploration and plan-writing flow below.

### Script Behavior Verification

When an issue body or extracted plan asserts specific script behavior
(e.g. "field X is populated after Step Y", "script A reads B from C"),
verify each assertion by reading or grepping the relevant script source
before building the plan on that assumption. Issue authors — including
Claude in prior sessions — can be wrong about what a script does
internally. A single grep of the script for the claimed field or function
catches false assumptions before they become bugs in the implementation.

For each behavioral assertion found in the issue body or plan:

- Identify the script and field or function referenced
- Use the Read or Grep tool to check the actual source
- If the claim is accurate, proceed with it in the plan
- If the claim is false, note the discrepancy in the Risks section and
  adjust the plan accordingly

### Target Path Validation

During exploration or extraction, verify that each file target identified
for editing is inside the repo working tree. Files outside the repo —
such as paths starting with `~/`, `/Users/`, or any absolute path not
under the working directory — are not tracked by git. Changes to those
files will not appear in `git status` or the PR diff.

**Hard rule for `.claude/rules/` and `CLAUDE.md`:** These paths are
ALWAYS repo-level during any FLOW phase. If a target resolves to
`~/.claude/rules/` or `~/.claude/CLAUDE.md`, override it to the
repo-local equivalent (`.claude/rules/` or `CLAUDE.md` in the working
tree). There is no "may be intentional" exception — user-level paths
are never valid write targets during FLOW phases.

For other out-of-repo paths: if the prompt or issue body contains
keywords like "repo", "version-controlled", "shared", "committed", or
"tracked", default to the repo-local equivalent. Otherwise, note the
out-of-repo path in the plan's Risks section so the user is aware.

### Standard Exploration

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

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set plan_step=4
```

Store the plan file path in the state file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set files.plan=<plan_file_path>
```

Replace `<plan_file_path>` with the relative path `.flow-states/<branch>-plan.md`.

Count the total number of implementation tasks in the Tasks section of
the plan file and store the count for TUI progress display:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_tasks_total=<n>
```

Replace `<n>` with the total task count from the plan.

Render the complete PR body (artifacts, plan, DAG, timings, and state
are all derived from the state file automatically):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow render-pr-body --pr <pr_number>
```

Complete the phase:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-plan --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

---

## Done — Banner and transition

### Render Plan

Use the Read tool to read the plan file at
`<project_root>/.flow-states/<branch>-plan.md`. Render the full plan
content inline in your response text — the complete Context, Exploration,
Risks, Approach, Dependency Graph, and Tasks sections. Do not summarize
or truncate. The user must be able to review the plan before the phase
completes.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 2: Plan — COMPLETE (<formatted_time>)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

### Slack Notification

Read `slack_thread_ts` from the state file. If present, post a thread reply. Best-effort — skip silently on failure.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow notify-slack --phase flow-plan --message "<message_text>" --thread-ts <thread_ts>
```

If `"status": "ok"`, record the notification:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-notification --phase flow-plan --ts <ts> --thread-ts <thread_ts> --message "<message_text>"
```

If `"status": "skipped"` or `"status": "error"`, continue without error.

<HARD-GATE>
STOP. Parse `continue_action` from the `phase-transition --action complete`
output above to determine how to advance.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use `continue_action` from the phase-transition output.
   If `continue_action` is `"invoke"` → continue=auto.
   If `continue_action` is `"ask"` → continue=manual.
2. If continue=auto → invoke `flow:flow-code` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 2: Plan is complete. Ready to begin Phase 3: Code?"
      Options: "Yes, start Phase 3 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 3 now" and "Not yet"
   d. If Yes → invoke `flow:flow-code` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-code` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-code when ready.
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
