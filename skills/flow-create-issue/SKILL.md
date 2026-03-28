---
name: flow-create-issue
description: "Explore a design question or decompose a concrete problem via DAG analysis, iterate with the user, then file a work-ready issue."
---

# Flow Create Issue

Explore a design question or decompose a concrete problem into a fully detailed, work-ready GitHub issue. Classifies the user's input first: exploratory questions get an interactive design discussion grounded in the codebase, while concrete problems go straight to the `decompose:decompose` plugin for DAG-based analysis. Both paths iterate with the user until the issue is comprehensive enough for `/flow:flow-start` to execute fully autonomously.

## Usage

```text
/flow:flow-create-issue <problem description>
/flow:flow-create-issue --step 2 --id <id>
```

- `/flow:flow-create-issue <problem description>` — start from Step 1 (Decompose)
- `/flow:flow-create-issue --step 2 --id <id>` — self-invocation: skip to Step 2 (Draft + File)

## Concurrency

This skill creates shared GitHub state (issues). Issue creation is
idempotent by title — if an issue with the same title already exists,
the user should be warned before filing a duplicate.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — flow:flow-create-issue — STARTING
──────────────────────────────────────────────────
```
````

## Step Dispatch

If `--step N --id <id>` was passed, this is a self-invocation from a
previous step. The `--id` flag carries the session-scoped identifier
generated in Step 1. Skip the Announce banner and jump directly to the
Resume Check, using the provided `<id>` for all file paths.

- `--step 2 --id <id>` → Resume Check dispatches to Step 2

If no `--step` flag was passed, proceed to Input Classification.

## Resume Check

Use the Read tool to read `.flow-states/create-issue-<id>.json`, where
`<id>` is the session identifier from the `--id` flag. If no `--id` flag
was passed (first run), there is no file to read — proceed to Input
Classification.

- If the file does not exist, proceed to Input Classification (first run).
- If `create_issue_step` is `1` — Step 1 is done. Skip to Step 2.

---

## Input Classification

Before entering the 2-step pipeline, evaluate the user's input to determine
whether it describes a concrete problem or an exploratory design question.

**Concrete problem signals** — proceed to Step 1:

- Bug or failure language: "fails", "broken", "error", "crashes", "wrong"
- Specific symptoms: error messages, stack traces, reproduction steps
- Issue references: `#N` patterns pointing to existing issues
- Action verbs: "fix", "add", "implement", "update", "change"
- A clear description of what is wrong and what should be different

**Exploratory question signals** — proceed to Exploration Mode:

- Question form: "what could we", "how might we", "what if", "should we"
- Brainstorming language: "ideas for", "thoughts on", "explore", "investigate"
- No specific failure or symptom described
- Asks about possibilities or design options rather than problems

**If ambiguous** — ask the user which mode they prefer using AskUserQuestion
with structured parameters:

- **question**: "This could be explored as a design question or decomposed as a concrete issue. Which would you prefer?"
- **header**: "Input Mode"
- **options**:
  - label: "Design exploration", description: "Discuss the topic interactively before filing"
  - label: "Decompose and file", description: "Enter the 2-step pipeline now"

Route based on the user's choice.

---

## Exploration Mode

This mode facilitates a design discussion grounded in the codebase. The goal
is to help the user think through a problem space, not to file an issue
immediately.

1. Acknowledge that this is a design exploration, not an issue filing pipeline
2. Explore the codebase relevant to the topic using Glob, Grep, and Read to
   ground the discussion in what actually exists
3. Present findings and design options to the user
4. Discuss interactively — use AskUserQuestion to gather the user's
   perspective, iterate on ideas, and refine the direction

<HARD-GATE>

**Exit paths:**

Do not proceed to Step 1, propose direct edits, commit changes, or take
any action outside this skill without explicit user approval via
AskUserQuestion.

When the discussion produces a concrete problem the user wants to file,
ask using AskUserQuestion with structured parameters:

- **question**: "Ready to file this as an issue?"
- **header**: "File Issue?"
- **options**:
  - label: "File an issue", description: "Proceed to the 2-step pipeline with the refined problem"
  - label: "Done exploring", description: "End without filing an issue"

If "File an issue" — proceed to Step 1 below with the refined problem
statement. If the user identifies multiple issues — ask which to start
with, then proceed to Step 1 for the chosen issue.

If "Done exploring" — stop without filing. No issue is required.

When the user wants to cancel:

- Stop without filing

</HARD-GATE>

---

## Step 1 — Decompose

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 1 of 2: Decompose ──
```
````

Invoke the `decompose:decompose` plugin with the user's problem description via the Skill tool.

The decomposition **must** include deep codebase exploration. During DAG execution:

- Use **Glob** to find relevant files by pattern
- Use **Grep** to search for related code, constants, error messages, and patterns
- Use **Read** to understand current behavior, architecture, and constraints
- Trace call chains and dependencies to identify all affected files
- Verify that referenced files, functions, and patterns actually exist

The decomposition is the foundation. Every claim in the final issue must be grounded in evidence from the codebase — not theoretical or assumed. If decompose produces findings that reference files or behavior, verify them.

Present the full DAG synthesis to the user.

<HARD-GATE>

Ask the user to review the decomposition using AskUserQuestion with
structured parameters:

- **question**: "Review the decomposition above. How would you like to proceed?"
- **header**: "Decompose"
- **options**:
  - label: "Proceed to draft", description: "Move to the draft and file step"
  - label: "Iterate", description: "Re-run decompose with your feedback"
  - label: "Cancel", description: "Stop without filing an issue"

**If "Proceed to draft"** → generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool (this ID
scopes all file paths for this session). Write
`{"create_issue_step": 1}` to `.flow-states/create-issue-<id>.json`
using the Write tool, then invoke `flow:flow-create-issue --step 2 --id <id>`
using the Skill tool as your final action. Do not output anything else
after this invocation.

**If "Iterate"** → re-invoke `decompose:decompose` with the user's
feedback, present the updated synthesis, and ask again.

**If "Cancel"** → stop. Do not file an issue.

Do not proceed to Step 2 without explicit user approval.

</HARD-GATE>

---

## Step 2 — Draft + File

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 2 of 2: Draft + File ──
```
````

Take the decompose synthesis and craft a single GitHub issue. The issue must contain enough detail that a fresh Claude session running `/flow:flow-start work on issue #N` can execute it fully autonomously — no questions asked.

### Required Sections

**Problem** — What is broken, missing, or inadequate. Include observable behavior, evidence from the codebase (file paths, line numbers, code snippets), and user impact. This is not a guess — it is grounded in the codebase exploration from Step 1.

**Acceptance Criteria** — A checklist of binary, testable conditions. Each criterion must be pass/fail with no subjective judgment. These become the definition of done for the autonomous session.

Example:

```text
- [ ] `lib/session-start.sh` detects multiple active features and lists them
- [ ] `bin/ci` passes with no new warnings
- [ ] Test coverage for the new detection logic in `tests/test_session_start.py`
```

**Files to Investigate** — Real file paths verified during codebase exploration. These are starting points for the Plan phase. Include a brief note on why each file is relevant.

Example:

```text
- `lib/session-start.sh` — contains the hook logic that needs modification
- `tests/test_session_start.py` — existing tests to extend
- `hooks/hooks.json` — hook registration that may need updating
```

**Out of Scope** — Explicit boundaries to prevent autonomous scope creep. Name specific things that should NOT be changed, even if they seem related. This is critical for autonomous execution — without it, Claude will "improve" adjacent code.

**Context** — Business reason, architectural constraints, or design decisions that inform the implementation. Include anything a fresh session needs to understand *why* this work matters and *how* it fits into the broader system.

### Draft Presentation

Present the full draft inline in the response — both title and body. Do not tell the user to look at a file. Render it as a formatted markdown block so the user can review every detail.

<HARD-GATE>

Ask the user to review the draft and choose where to file using
AskUserQuestion with structured parameters:

- **question**: "Review the draft above. Where should this issue be filed?"
- **header**: "File Issue"
- **options**:
  - label: "Target project", description: "File against the current project"
  - label: "FLOW plugin", description: "File against benkruger/flow"
  - label: "Revise draft", description: "Edit the draft based on your feedback"
  - label: "Re-decompose", description: "Restart from scratch with a new decomposition"

Do not proceed to file the issue, propose direct edits, commit changes,
or take any action outside this skill without explicit user approval via
AskUserQuestion — even if the answer appears obvious from context.

**If "Target project"** or **"FLOW plugin"** → file the issue (see Filing below).

**If "Revise draft"** → revise the draft based on feedback and re-present.
If the feedback is substantial (changes the problem understanding or
approach), re-run `decompose:decompose` with the updated understanding.
If the feedback is editorial (wording, scope adjustments), revise the
draft directly. After revision, ask again with the same AskUserQuestion.

**If "Re-decompose"** → clean up the session state file first:

```bash
rm .flow-states/create-issue-<id>.json
```

Then invoke `flow:flow-create-issue` using the Skill tool as your final
action (no `--step` or `--id` flags — restart from scratch). Do not
output anything else after this invocation.

Iterate as many times as needed. The issue is not filed until the user
explicitly chooses a filing target.

</HARD-GATE>

### Filing

Write the issue body to `.flow-issue-body-<id>` in the project root using the Write tool.

**If target project:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id> --label decomposed
```

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

**If FLOW plugin bug:**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --title "<issue_title>" --body-file .flow-issue-body-<id> --label "Flow"
```

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flow" --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

Clean up the state file:

```bash
rm .flow-states/create-issue-<id>.json
```

Display the issue URL to the user, then output the COMPLETE banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — flow:flow-create-issue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never file an issue without explicit user approval — the Step 2 AskUserQuestion is the mandatory gate
- Never skip codebase exploration — every file path and code reference must be verified
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- The issue body must be self-contained — a fresh session with no memory of this conversation must be able to execute it
- Never create sub-issues or linked issues — file a single comprehensive issue
- Always use the Write tool to create the body file (`.flow-issue-body-<id>`) — never pass body text as a CLI argument
- Never delete the body file — the `bin/flow issue` script handles cleanup
- Step 1 ends by invoking the skill itself as the final action — Step 2 is terminal
