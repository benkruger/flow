---
name: research
description: "Phase 2: Research — explore the codebase before any design or implementation. Reads affected files, discovers risks, asks clarifying questions, and documents findings in flow-state.json."
model: sonnet
---

# FLOW Research — Phase 2: Research

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.1.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 1: Start must be
     complete. Run /flow:start first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 2: Research — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 2:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if currently null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `2`

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

## Logging

No logging for this phase. Research runs no Bash commands beyond the entry
gate — the sub-agent runs in its own context and the main skill's work is
AskUserQuestion calls and state file writes.

---

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for sub-agent prompts, light mode design object template, and
framework-specific hard rules.

### If Rails

#### Full Mode Sub-Agent Prompt

Provide these instructions to the Step 2 sub-agent (fill in the scope):

> You are exploring a Rails codebase for the FLOW research phase.
> Research scope: <user's description from Step 1 — paste verbatim>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> Systematically read all code relevant to this feature:
>
> **Models** — Find all related models. For each, read the full class
> hierarchy (model + parent + ApplicationRecord). Look for: before_save,
> after_create, before_destroy callbacks. Check for default_scope (soft
> deletes), self.inheritance_column (no STI), belongs_to/has_many with
> dependent: options. Note the Base/Create split pattern.
>
> **Controllers** — Find affected controllers. Note subdomain, BaseController
> inheritance, params pattern (options OpenStruct), response helpers.
>
> **Workers** — Find affected Sidekiq workers. Read pre_perform!/perform!/
> post_perform! structure. Check config/sidekiq.yml for queue names.
>
> **Routes** — Read config/routes/ files relevant to this feature. Note
> scope with module:, as:, controller:, action: pattern.
>
> **Schema** — Read data/release.sql for all relevant tables. Note column
> types, constraints, indexes, foreign keys.
>
> **Tests** — Search test/support/ for existing create_*! helpers for
> affected models. Note existing test patterns.
>
> **Git history** — Run git log --oneline -10 on key files. Use git blame
> on anything non-obvious.
>
> Return your findings as a structured summary:
>
> - Affected files (full paths)
> - Per-model: class hierarchy, callbacks, associations, soft deletes
> - Per-controller: subdomain, BaseController, params pattern
> - Per-worker: queue name, halt conditions
> - Routes: file and pattern
> - Schema: table structure
> - Test helpers: existing create_*! helpers found
> - Risks: anything that could cause problems (callback chains, soft
>   deletes, Current attribute dependencies)

#### Light Mode Sub-Agent Prompt

Provide these instructions to the Light Step 2 sub-agent (fill in the description):

> You are investigating a bug or small change in a Rails codebase.
> Description: <user's description from Light Step 1 — paste verbatim>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> **Start with recent changes:**
>
> 1. Run `git log --oneline -20` to see recent commits
> 2. Look for commits related to the described issue
> 3. If a recent commit looks relevant, run `git show <sha>` to see the diff
>
> **Then read affected files:**
>
> 1. Read only the files directly related to the issue
> 2. For each model, read the full class hierarchy
> 3. Check `test/support/` for relevant create_*! helpers
>
> Do NOT explore the entire codebase. Stay focused on the files
> directly related to the bug or change.
>
> Return a structured summary: recent relevant commits, affected files
> (full paths), root cause or change needed, per-model class hierarchy
> and callbacks (only for affected models), schema changes needed (if
> any), risks, and existing create_*! helpers found.

#### Light Mode Design Object Template

Use this template for `state["design"]` in Light Step 3:

```json
{
  "feature_description": "<user's bug description from Light Step 1>",
  "chosen_approach": "<the fix or change identified during investigation>",
  "rationale": "Identified during light-mode research",
  "schema_changes": [],
  "model_changes": [],
  "controller_changes": [],
  "worker_changes": [],
  "route_changes": [],
  "risks": [],
  "approved_at": "<current UTC timestamp>"
}
```

Populate the change arrays and risks from the investigation findings. Leave
arrays empty where not applicable.

#### Rails-Specific Hard Rules

- Always read the full class hierarchy for every affected model — never just the model file
- Always check `test/support/` for existing helpers before noting that tests will be needed

### If Python

#### Full Mode Sub-Agent Prompt

Provide these instructions to the Step 2 sub-agent (fill in the scope):

> You are exploring a Python codebase for the FLOW research phase.
> Research scope: <user's description from Step 1 — paste verbatim>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> Systematically read all code relevant to this feature:
>
> **Modules** — Find all related Python modules. Read each module fully.
> Look for: imports, class definitions, function signatures, module-level
> state, `__init__.py` exports.
>
> **Scripts** — Find affected CLI scripts or entry points (in `bin/`,
> `scripts/`, or package `__main__.py`). Read argument parsing, main
> flow, and error handling.
>
> **Configuration** — Read relevant config files: `pyproject.toml`,
> `setup.cfg`, `conftest.py`, CI config, `.yml` files.
>
> **Tests** — Find existing test files for affected modules. Read
> `conftest.py` for shared fixtures. Note existing test patterns and
> helper functions.
>
> **Git history** — Run git log --oneline -10 on key files. Use git blame
> on anything non-obvious.
>
> Return your findings as a structured summary:
>
> - Affected files (full paths)
> - Per-module: imports, classes, key functions, dependencies
> - Per-script: arguments, main flow, error handling
> - Test fixtures: existing conftest.py fixtures and helpers found
> - Risks: anything that could cause problems (circular imports,
>   global state, missing error handling)

#### Light Mode Sub-Agent Prompt

Provide these instructions to the Light Step 2 sub-agent (fill in the description):

> You are investigating a bug or small change in a Python codebase.
> Description: <user's description from Light Step 1 — paste verbatim>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for commands
> explicitly listed in these instructions. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project
> tooling (pytest, python, pip, etc.).
>
> **Start with recent changes:**
>
> 1. Run `git log --oneline -20` to see recent commits
> 2. Look for commits related to the described issue
> 3. If a recent commit looks relevant, run `git show <sha>` to see the diff
>
> **Then read affected files:**
>
> 1. Read only the files directly related to the issue
> 2. For each module, read its imports and dependencies
> 3. Check `conftest.py` for relevant fixtures
>
> Do NOT explore the entire codebase. Stay focused on the files
> directly related to the bug or change.
>
> Return a structured summary: recent relevant commits, affected files
> (full paths), root cause or change needed, module dependencies,
> risks, and existing test fixtures found.

#### Light Mode Design Object Template

Use this template for `state["design"]` in Light Step 3:

```json
{
  "feature_description": "<user's bug description from Light Step 1>",
  "chosen_approach": "<the fix or change identified during investigation>",
  "rationale": "Identified during light-mode research",
  "module_changes": [],
  "test_changes": [],
  "script_changes": [],
  "risks": [],
  "approved_at": "<current UTC timestamp>"
}
```

Populate the change arrays and risks from the investigation findings. Leave
arrays empty where not applicable.

#### Python-Specific Hard Rules

- Always read module imports and dependencies before modifying
- Always check `conftest.py` for existing fixtures before noting that tests will be needed

---

## Light Mode Check

After the gate, check `state["mode"]`. If it equals `"light"`, follow the
**Light Mode Protocol** below instead of the standard Steps 1-6.

---

## Light Mode Protocol

When `state["mode"] == "light"`, Research does double duty: investigate the
issue AND write a simplified design object so Plan and Review work unchanged.

### Light Step 1 — What are we investigating?

Use AskUserQuestion with one question:

**Question 1:** "Describe the bug or change. What is the expected behavior, and what is happening instead?"
- I'll describe it (select Other and type your description)

Store the user's description in `state["research"]["scope"]`.

### Light Step 2 — Recent changes first

Check recent git history before deep exploration:

1. Launch a sub-agent. Use the Task tool:
   - `subagent_type`: `"general-purpose"`
   - `description`: `"Light research — recent changes first"`

   Provide the sub-agent with the **Light Mode Sub-Agent Prompt** from the
   framework section above (fill in the user's description from Light Step 1).

2. Wait for the sub-agent to return.

### Light Step 3 — Document findings and write design object

Write research findings to `state["research"]` (same structure as full mode).

**Also write** a simplified `state["design"]` object using the **Light Mode
Design Object Template** from the framework section above. Populate the change
arrays and risks from the investigation findings. Leave arrays empty where
not applicable.

### Light Step 4 — Present findings

Show the user a clean summary (same format as full mode Step 6).

Then skip to **Done** below — transition to Phase 4: Plan (not Phase 3:
Design, which was already marked complete+skipped by `/flow:start --light`).

---

## Step 1 — What are we researching?

Before reading any code, ask the user what to focus on. The feature name
from `/flow:start` is just a branch label — it does NOT define the
research scope. The user must describe what to research in their own words.

If this is a return visit (`visit_count` > 1), show what was previously
found and ask: "What gaps should we fill this time?" Do not discard prior
findings — extend them.

Otherwise, use AskUserQuestion with two questions:

**Question 1:** "What type of work is this?"

- New feature
- Change to existing feature
- Bug investigation
- Refactor / restructure

**Question 2:** "Describe what we should research. What area of the codebase should we explore, and what are we trying to understand?"
- I'll describe it (select Other and type your description)
- I'm not sure yet — help me figure out where to start

The user's answer to Question 2 directs the entire exploration. If they
select "I'm not sure yet", ask follow-up questions to narrow the scope
before proceeding. Do not assume scope from the feature branch name.

Store the user's description in `state["research"]["scope"]` in the
state file.

---

## Step 2 — Launch codebase explorer sub-agent

Launch a mandatory sub-agent to explore the codebase. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Research codebase exploration"`

Provide the sub-agent with the **Full Mode Sub-Agent Prompt** from the
framework section above (fill in the scope from Step 1).

Wait for the sub-agent to return before proceeding. Use the sub-agent's
findings as the basis for all subsequent steps — do not re-read files
the sub-agent already covered.

---

## Step 3 — Formulate clarifying questions

Based on exploration, identify everything that is ambiguous or unclear about the feature. Write down all questions before presenting them.

Good research questions:
- Scope boundaries ("Does this apply to all accounts or just paying ones?")
- Edge cases ("What happens if the webhook arrives twice?")
- Existing behaviour ("Should this replace the current X or run alongside it?")
- Constraints ("Are there rate limits we need to respect?")
- Rollback ("What's the behaviour if this fails mid-way?")

Do NOT ask about things that can be inferred from the codebase. Only ask when genuinely unclear.

---

## Step 4 — Ask clarifying questions

Group questions into batches of up to 4. Present each batch using `AskUserQuestion` — the tab UI allows the user to navigate freely between questions with arrow keys.

For each batch, use a single `AskUserQuestion` call with up to 4 questions. Each question should have 2–4 options where possible (multiple choice is easier to answer than open-ended). Always include an "Other / I'll explain" option implicitly via the tool's built-in Other option.

If answers from the first batch reveal new questions, present a second batch.

Record every question and answer in `flow-state.json["research"]["clarifications"]`:

```json
[
  { "question": "What happens to existing webhooks when...", "answer": "They should be..." }
]
```

---

## Step 5 — Document findings

Write the full research findings into `flow-state.json["research"]`:

```json
{
  "research": {
    "clarifications": [...],
    "affected_files": [
      "<path/to/affected_file_1>",
      "<path/to/affected_file_2>",
      "<path/to/affected_file_3>"
    ],
    "risks": [
      "<specific risk discovered during exploration>",
      "<another risk with details on why it matters>"
    ],
    "open_questions": [
      "<question that could not be resolved from the codebase alone>"
    ],
    "summary": "<plain English summary of what was found, what will be touched, and the most significant risk>"
  }
}
```

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

---

## Step 6 — Present findings

Show the user a clean summary. Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW — Phase 2: Research — FINDINGS
============================================

  Affected Files
  --------------
  - <path/to/file_1>
  - <path/to/file_2>
  - ... (all files)

  Risks Discovered
  ----------------
  - <risk description>
  - ...

  Open Questions
  --------------
  - <unresolved question>

  Summary
  -------
  <summary text>

============================================
```
````

---

## Done — Update state and complete phase

Update `.flow-states/<branch>.json`:
1. Calculate `cumulative_seconds`: `current_time - session_started_at` + existing `cumulative_seconds`. Do not print the calculation.
2. Set Phase 2 `status` to `complete`
3. Set Phase 2 `completed_at` to current UTC timestamp
4. Set Phase 2 `session_started_at` to `null`
5. If `state["mode"] == "light"`: set `current_phase` to `4` (Design was skipped). Otherwise: set `current_phase` to `3`.

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields listed above in memory, then use the Write tool to
write the entire file back. Never use the Edit tool for state file
changes — field names repeat across phases and cause non-unique match
errors.

For the banner below, compute `<formatted_time>` from the integer `cumulative_seconds` stored above: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60. Do not write the formatted string back to the state file.

### If light mode (`state["mode"] == "light"`)

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 2: Research — COMPLETE (<formatted_time>)
  Next: Phase 4: Plan  (/flow:plan)
  (Light mode — Design was skipped)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 2: Research is complete. Ready to begin Phase 4: Plan?"
>
> - **Yes, start Phase 4 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 4 now" and "Not yet"

**If Yes** — invoke the `flow:plan` skill using the Skill tool.

### If standard mode (no `state["mode"]` or not `"light"`)

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.13.1 — Phase 2: Research — COMPLETE (<formatted_time>)
  Next: Phase 3: Design  (/flow:design)
============================================
```
````

Invoke the `flow:status` skill to show the current state, then use AskUserQuestion:

> "Phase 2: Research is complete. Ready to begin Phase 3: Design?"
>
> - **Yes, start Phase 3 now**
> - **Not yet**
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 3 now" and "Not yet"

**If Yes** — invoke the `flow:design` skill using the Skill tool.

### Either mode — If Not yet

Print inside a fenced code block:

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

- Never propose a solution during Research — that is Design's job (in light mode, the design object is factual, not a design choice)
- Never write or modify any application code
- If returning to Research, read prior findings first and extend — never discard
- Plus the **Framework-Specific Hard Rules** from the framework section above
