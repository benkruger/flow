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
- If `create_issue_step` is `1` — Step 1 is done. Skip to Step 2 (which re-reads the file to determine single vs. multi-issue mode).

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

When the discussion produces one or more concrete problems the user wants
to file, ask using AskUserQuestion with structured parameters:

- **question**: "Ready to file?"
- **header**: "File Issue?"
- **options**:
  - label: "File an issue", description: "Proceed to the 2-step pipeline with the refined problem"
  - label: "File multiple issues", description: "Draft and file all refined problems as independent issues"
  - label: "Done exploring", description: "End without filing an issue"

If "File an issue" — proceed to Step 1 below with the single refined
problem statement.

If "File multiple issues" — collect all refined problems identified during
the exploration. For each problem, note a concise title and a summary
paragraph describing the problem. Proceed to Step 1 with the full list.

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

Invoke the `decompose:decompose` plugin with the user's problem description
via the Skill tool. If Step 1 received a list of problems (from "File
multiple issues"), invoke decompose once with all problems described
together — the DAG analysis should consider interactions between them.

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
scopes all file paths for this session).

If Step 1 received a list of problems (from "File multiple issues" in
Exploration Mode), write the following to
`.flow-states/create-issue-<id>.json` using the Write tool:

```json
{"create_issue_step": 1, "multi": true, "issues": [{"title": "...", "summary": "..."}]}
```

The `issues` array contains one object per problem with `title` (concise
issue title) and `summary` (problem description paragraph).

If Step 1 received a single problem, write `{"create_issue_step": 1}` to
`.flow-states/create-issue-<id>.json` using the Write tool.

In both cases, invoke `flow:flow-create-issue --step 2 --id <id>` using
the Skill tool as your final action. Do not output anything else after
this invocation.

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

Use the Read tool to read `.flow-states/create-issue-<id>.json`. If
`multi` is `true`, skip to the **Multi-Issue Path** section below.
Otherwise, continue with the single-issue path.

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

---

### Multi-Issue Path

Draft all issues from the `issues` array in the session state file. For
each issue, craft a title and full body using the Required Sections format
(Problem, Acceptance Criteria, Files to Investigate, Out of Scope,
Context). Use the decompose synthesis from Step 1 as the foundation.

Present all drafts as a numbered set inline in the response — each issue
clearly labeled with its number and title, followed by the full body.

<HARD-GATE>

Ask the user to review all drafts and choose where to file using
AskUserQuestion with structured parameters:

- **question**: "Review the drafts above. Where should these issues be filed?"
- **header**: "File Issues"
- **options**:
  - label: "Target project", description: "File all against the current project"
  - label: "FLOW plugin", description: "File all against benkruger/flow"
  - label: "Revise drafts", description: "Edit the drafts based on your feedback"
  - label: "Re-decompose", description: "Restart from scratch with a new decomposition"

Do not proceed to file any issue, propose direct edits, commit changes,
or take any action outside this skill without explicit user approval via
AskUserQuestion — even if the answer appears obvious from context.

**If "Target project"** or **"FLOW plugin"** → file all issues (see Multi-Issue Filing below).

**If "Revise drafts"** → revise based on feedback and re-present all
drafts. The user may drop issues, add issues, or edit individual drafts.
After revision, ask again with the same AskUserQuestion.

**If "Re-decompose"** → clean up the session state file first:

```bash
rm .flow-states/create-issue-<id>.json
```

Then invoke `flow:flow-create-issue` using the Skill tool as your final
action (no `--step` or `--id` flags — restart from scratch). Do not
output anything else after this invocation.

Iterate as many times as needed. No issues are filed until the user
explicitly chooses a filing target.

</HARD-GATE>

### Multi-Issue Filing

Write all body files first using parallel Write tool calls — one per
issue, each to `.flow-issue-body-<id>-N` in the project root (e.g.,
`.flow-issue-body-<id>-1` for the first issue, `.flow-issue-body-<id>-2`
for the second).

Then file all issues in parallel using multiple Bash calls in one
response. Each `bin/flow issue` call is independent (different body
file, different GitHub API call).

**If target project**, file each issue:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id>-1 --label decomposed
```

**If FLOW plugin bug**, file each issue:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --title "<issue_title>" --body-file .flow-issue-body-<id>-1 --label "Flow"
```

After all issues are filed, record each one sequentially (no-op if no
FLOW feature is active — the `add-issue` calls mutate a shared state
file and must run one at a time). Use the same label as the filing call:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

Or for FLOW plugin issues:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flow" --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

After recording all issues, clean up the session state file:

```bash
rm .flow-states/create-issue-<id>.json
```

Display all issue URLs to the user, then output the COMPLETE banner:

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
- Never create dependency links between issues — use `flow-decompose-project` for dependent issue graphs. Independent issues from one exploration session are fine.
- Always use the Write tool to create body files (`.flow-issue-body-<id>` or `.flow-issue-body-<id>-N`) — never pass body text as a CLI argument
- Never delete the body file — the `bin/flow issue` script handles cleanup
- Step 1 ends by invoking the skill itself as the final action — Step 2 is terminal
