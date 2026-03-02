---
name: review
description: "Phase 6: Review — systematic code review against design, research risks, and framework anti-patterns. Fixes issues found, runs bin/ci after any fix, then transitions to Reflect."
model: sonnet
---

# FLOW Review — Phase 6: Review

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Find the project root: run `git worktree list --porcelain` and note the
   path on the first `worktree` line.
2. Get the current branch: run `git branch --show-current`.
3. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
4. Check `phases.5.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 5: Code must be
     complete. Run /flow:code first."
</HARD-GATE>

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 6: Review — STARTING
============================================
```
````

## Update State

Read `.flow-states/<branch>.json`. cd into the worktree.

Update Phase 6:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `6`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 6] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for the diff analysis sub-agent prompt and framework-specific
hard rules.

### If Rails

#### Diff Analysis Sub-Agent Prompt

Provide these instructions to the Step 1 sub-agent (fill in the details):

> You are analyzing a feature diff for the FLOW review phase.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for git commands
> (git diff, git log, git blame). Never use Bash for any other purpose —
> no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Approved design:
> <paste state["design"] — chosen_approach, schema_changes, model_changes,
> controller_changes, worker_changes, route_changes>
>
> Research risks:
> <paste state["research"]["risks"]>
>
> Plan tasks:
> <paste state["plan"]["tasks"] summaries>
>
> First, get the full diff:
>
> ```bash
> git diff origin/main...HEAD
> ```
>
> Read every changed file completely. Then check:
>
> **Design alignment:**
>
> - Do schema changes match design["schema_changes"]?
> - Do model decisions match design["model_changes"]?
> - Do controller/route changes match design?
> - Do worker changes match design?
> - Flag any deviation — minor drift or major mismatch.
>
> **Research risk coverage:**
>
> - For each risk in the list, confirm it was handled in the diff.
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
> 1. Design alignment issues (with file:line references)
> 2. Uncovered research risks (with which risk and why)
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
> (git diff, git log, git blame). Never use Bash for any other purpose —
> no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Approved design:
> <paste state["design"] — chosen_approach, module_changes, test_changes,
> script_changes>
>
> Research risks:
> <paste state["research"]["risks"]>
>
> Plan tasks:
> <paste state["plan"]["tasks"] summaries>
>
> First, get the full diff:
>
> ```bash
> git diff origin/main...HEAD
> ```
>
> Read every changed file completely. Then check:
>
> **Design alignment:**
>
> - Do module changes match design["module_changes"]?
> - Do test changes match design["test_changes"]?
> - Do script changes match design["script_changes"]?
> - Flag any deviation — minor drift or major mismatch.
>
> **Research risk coverage:**
>
> - For each risk in the list, confirm it was handled in the diff.
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
> 1. Design alignment issues (with file:line references)
> 2. Uncovered research risks (with which risk and why)
> 3. Anti-pattern violations (with file:line and what to fix)
>
> If a category has no findings, say so explicitly.

#### Python-Specific Hard Rules

- Any `# noqa` or `# type: ignore` comment in the diff is a finding — remove it and fix the code
- Any modification to lint configuration in the diff is a finding — revert it and fix the code

---

## Step 1 — Launch diff analyzer sub-agent

Read the following from the state file (small, structured — keep in main context):
- `state["design"]` — what was approved to be built
- `state["plan"]["tasks"]` — what was planned
- `state["research"]["risks"]` — risks identified during Research

Then launch a mandatory sub-agent to analyze the full diff. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Review diff analysis"`

Provide the sub-agent with the **Diff Analysis Sub-Agent Prompt** from the
framework section above (fill in the feature name, design, risks, and tasks).

Wait for the sub-agent to return before proceeding.

---

## Step 2 — Review sub-agent findings

Read the sub-agent's structured findings. For each category:

**Design alignment issues** — Confirm each finding against the state file.
Minor drift is a note. Major drift means go back to Code.

**Uncovered research risks** — Confirm each finding. An unaddressed risk
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

**Significant finding** (logic error, missing risk coverage, design mismatch):
- Use AskUserQuestion:
  > "Found a significant issue: <description>. How would you like to proceed?"
  >
  > - **Fix it here in Review**
  > - **Go back to Code**
  > - **Go back to Plan**

After fixing any findings, run `/flow:commit` for the Review fixes.

Then run `bin/ci` — required before any state transition.

<HARD-GATE>
bin/ci must be green before transitioning to Reflect.
Any fix made during Review requires bin/ci to run again.
</HARD-GATE>

---

## Step 4 — Present review summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 6: Review — SUMMARY
============================================

  Design alignment  : ✓ matches approved design
  Research risks    : ✓ all risks accounted for

  Findings fixed
  --------------
  - <description of fix and why>
  - <description of fix and why>
  - <description of fix and why>

  bin/ci            : ✓ green

============================================
```
````

---

## Back Navigation

Use AskUserQuestion if a finding is too significant to fix in Review:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something
> - **Go back to Design** — approach needs rethinking
> - **Go back to Research** — something fundamental was missed

Update state for all phases between current and target before invoking
the target skill.

---

## Done — Update state and complete phase

Update Phase 6 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `7`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

For the banner below, compute `<formatted_time>` from the integer `cumulative_seconds` stored above: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60. Do not write the formatted string back to the state file.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 6: Review — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 6: Review is complete. Ready to begin Phase 7: Security?"
>
> - **Yes, start Phase 7 now** — invoke `flow:security`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 7 now" and "Not yet"

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

- Always run `bin/ci` after any fix made during Review
- Never transition to Reflect unless bin/ci is green
- Never skip the design alignment check
- Never skip the research risk coverage check
- Read the full diff before starting — no partial reviews
- Plus the **Framework-Specific Hard Rules** from the framework section above
