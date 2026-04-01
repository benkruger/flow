---
name: flow-create-issue
description: "Capture a brainstormed solution as a pre-planned issue with an Implementation Plan section for fast-tracking through the Plan phase."
---

# Flow Create Issue

Capture a brainstormed solution from the current conversation and file it as a pre-planned GitHub issue. The issue includes an Implementation Plan section (Context, Exploration, Risks, Approach, Dependency Graph, Tasks) that the Plan phase extracts directly — no re-derivation needed.

This skill requires prior brainstorming context in the conversation. The user must have already explored the problem (typically via `/decompose:decompose`) and iterated on a solution before invoking this skill.

## Usage

```text
/flow:flow-create-issue
/flow:flow-create-issue --auto
/flow:flow-create-issue --auto --force-decompose
/flow:flow-create-issue --force-decompose
/flow:flow-create-issue --step 2 --id <id>
```

- `/flow:flow-create-issue` — start from the Conversation Gate
- `/flow:flow-create-issue --auto` — autonomous mode: bypass all interactive gates with sensible defaults
- `/flow:flow-create-issue --auto --force-decompose` — autonomous mode with forced fresh decompose
- `/flow:flow-create-issue --force-decompose` — force a fresh decompose even when prior implementation-focused output exists in the conversation
- `/flow:flow-create-issue --step 2 --id <id>` — self-invocation: skip to Step 2 (Transform + Draft + File)

## Concurrency

This skill creates shared GitHub state (issues). Issue creation is
idempotent by title — if an issue with the same title already exists,
the user should be warned before filing a duplicate.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-create-issue — STARTING
──────────────────────────────────────────────────
```
````

## Step Dispatch

If `--step N --id <id>` was passed, this is a self-invocation from a
previous step. The `--id` flag carries the session-scoped identifier
generated in Step 1. Skip the Announce banner and jump directly to the
Resume Check, using the provided `<id>` for all file paths.

- `--step 2 --id <id>` → Resume Check dispatches to Step 2
- `--step 2 --id <id> --auto` → Resume Check dispatches to Step 2 in autonomous mode
- `--force-decompose` (no `--step`) → Conversation Gate, then Step 1 bypasses Prior Decompose Detection
- `--auto` → autonomous mode: all interactive gates use sensible defaults (see each gate for specifics)

The `--auto` flag must be forwarded through self-invocation. When Step 1
self-invokes with `--step 2 --id <id>`, append `--auto` if it was passed
to the original invocation.

If no `--step` flag was passed, proceed to the Conversation Gate.

## Resume Check

Use the Read tool to read `.flow-states/create-issue-<id>.json`, where
`<id>` is the session identifier from the `--id` flag. If no `--id` flag
was passed (first run), there is no file to read — proceed to the
Conversation Gate.

- If the file does not exist, proceed to the Conversation Gate (first run).
- If `create_issue_step` is `1` — Step 1 is done. Skip to Step 2.

---

## Conversation Gate

If `--auto` was passed, skip the Conversation Gate entirely and proceed
to Step 1. In autonomous mode, the caller is responsible for ensuring
context exists in the conversation.

Before entering the pipeline, verify that the current conversation contains
brainstorming context — a problem that was explored, a solution that was
discussed and agreed upon. This skill captures solutions, it does not
discover them.

**Signals that context exists** — proceed to Step 1:

- Prior `/decompose:decompose` output in the conversation
- Extended back-and-forth about a problem and its solution
- An agreed approach, design, or set of changes discussed
- The user explicitly says "file it", "create an issue", or similar

**Signals that context is missing** — reject:

- The skill was invoked with a bare problem description and no prior discussion
- No decompose output or design iteration is visible in the conversation
- The conversation just started with this invocation

<HARD-GATE>

If `--auto` was passed, this gate does not apply — proceed to Step 1.

If no brainstorming context exists, output this guidance and stop:

> "This skill captures a brainstormed solution as a pre-planned issue.
> Start by running `/decompose:decompose` to research the problem,
> iterate on a solution, then invoke `/flow:flow-create-issue` when
> you have an agreed approach."

Do not proceed to Step 1, propose direct edits, commit changes, or take
any action outside this skill without brainstorming context in the
conversation.

</HARD-GATE>

---

## Step 1 — Capture + Decompose Implementation

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 1 of 2: Capture + Decompose Implementation ──
```
````

Generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool. This ID
scopes all file paths for this session.

**Capture the problem sections** from the conversation context. Synthesize
the discussion into these structured sections — do not re-analyze or
re-explore, just distill what was already discussed:

- **Problem** — What is broken, missing, or inadequate. Include observable
  behavior, evidence from the codebase (file paths, line numbers), and user
  impact. Grounded in the exploration already done in the conversation.
- **Acceptance Criteria** — Binary, testable conditions. Pass/fail with no
  subjective judgment.
- **Files to Investigate** — Real file paths verified during the conversation's
  codebase exploration. Include a brief note on why each is relevant.
- **Out of Scope** — Explicit boundaries to prevent scope creep.
- **Context** — Business reason, architectural constraints, or design decisions.

Write these captured sections to `.flow-states/create-issue-<id>-capture.md`
using the Write tool.

### Prior Decompose Detection

Check the conversation for prior `/decompose:decompose` output that is
implementation-focused. Implementation-focused output contains all of:
task nodes with file targets, implementation ordering (dependency graph
or sequential tasks), and concrete code changes or insertion points.
Problem-analysis output — containing only analysis, questions, or
high-level framing without actionable task structure — does not qualify.

**If the conversation contains implementation-focused decompose output
AND `--force-decompose` was NOT passed:** the existing decompose
synthesis is sufficient. Skip the decompose invocation below. Write
`{"create_issue_step": 1}` to `.flow-states/create-issue-<id>.json`
using the Write tool. Invoke `flow:flow-create-issue --step 2 --id <id>`
using the Skill tool as your final action — append `--auto` if it was
passed to the original invocation. Do not output anything else after
this invocation.

**If the conversation contains only problem-analysis decompose output
(no tasks, no file targets), or no prior decompose output exists, or
`--force-decompose` was passed:** continue with the decompose invocation
below.

**Decompose the implementation.** Invoke `decompose:decompose` via the Skill
tool with an implementation-focused prompt. The prompt must make clear that
the problem and solution are already agreed — decompose should structure the
implementation into tasks, not re-analyze the problem.

Example prompt structure:

> "Given the following agreed solution, decompose the implementation into
> ordered tasks with dependencies, approach, and file targets. The problem
> is already understood — focus on structuring the work.
>
> [Summary of the agreed solution from the conversation]
>
> [Key files and patterns identified during brainstorming]"

The decompose output will produce a structured DAG with nodes, dependencies,
and a synthesis — this becomes the foundation for the Implementation Plan.

<HARD-GATE>

**If `--auto` was passed**, skip the review prompt. Write
`{"create_issue_step": 1}` to `.flow-states/create-issue-<id>.json`
using the Write tool. Invoke
`flow:flow-create-issue --step 2 --id <id> --auto` using the Skill tool
as your final action. Do not output anything else after this invocation.

**If `--auto` was NOT passed**, ask the user to review using
AskUserQuestion with structured parameters:

- **question**: "Review the implementation decomposition above. How would you like to proceed?"
- **header**: "Decompose"
- **options**:
  - label: "Proceed to draft", description: "Move to the draft and file step"
  - label: "Iterate", description: "Re-run decompose with your feedback"
  - label: "Cancel", description: "Stop without filing an issue"

**If "Proceed to draft"** → write `{"create_issue_step": 1}` to
`.flow-states/create-issue-<id>.json` using the Write tool.

Invoke `flow:flow-create-issue --step 2 --id <id>` using the Skill tool
as your final action. Do not output anything else after this invocation.

**If "Iterate"** → re-invoke `decompose:decompose` with the user's
feedback, present the updated synthesis, and ask again.

**If "Cancel"** → clean up the capture file:

```bash
rm .flow-states/create-issue-<id>-capture.md
```

Stop. Do not file an issue.

Do not proceed to Step 2 without explicit user approval.

</HARD-GATE>

---

## Step 2 — Transform + Draft + File

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 2 of 2: Transform + Draft + File ──
```
````

Use the Read tool to read `.flow-states/create-issue-<id>.json` to confirm
this is a valid self-invocation.

### Transform Decompose Output into Implementation Plan

Take the decompose synthesis from the conversation — either from a prior
`/decompose:decompose` invocation (when Step 1 skipped decompose) or from
Step 1's decompose invocation — and transform it into an
Implementation Plan section that matches the plan file format used by
`flow-plan`. The Implementation Plan must contain these subsections:

- **Context** — What the user wants to build and why
- **Exploration** — What exists in the codebase, affected files, patterns discovered
- **Risks** — What could go wrong, edge cases, constraints
- **Approach** — The chosen approach and rationale
- **Dependency Graph** — Table of tasks with types and dependencies:

```markdown
| Task | Type | Depends On |
|------|------|------------|
| 1. Write tests | test | — |
| 2. Implement feature | implement | 1 |
```

- **Tasks** — Ordered implementation tasks, each with:
  - Description of what to build
  - Files to create or modify
  - TDD notes (what the test should verify)

Tasks must use `#### Task N:` heading format (these become `### Task N:`
headings in the plan file after heading promotion by `flow-plan`).

### Combine into Issue Body

Read the captured problem sections from
`.flow-states/create-issue-<id>-capture.md` using the Read tool.

Combine the captured sections with the Implementation Plan into a single
issue body. The section order must be:

**Problem** (from capture) → **Acceptance Criteria** (from capture) →
**Implementation Plan** (from transform, containing Context, Exploration,
Risks, Approach, Dependency Graph, Tasks subsections) →
**Files to Investigate** (from capture) → **Out of Scope** (from capture) →
**Context** (from capture — business reason).

Each top-level section uses `##` headings. The Implementation Plan's
subsections use `###` headings. Task entries within the Tasks subsection
use `####` headings.

### Draft Presentation

Present the full draft inline in the response — both title and body. Do
not tell the user to look at a file. Render it as a formatted markdown
block so the user can review every detail.

### Repo Detection

Before presenting the filing options, detect the current repository:

```bash
git remote get-url origin
```

If the URL contains `benkruger/flow`, this is the FLOW plugin repo itself.
Both "Target project" and "FLOW plugin" would resolve to the same
repository, so skip the repo selection and present a simplified prompt.

<HARD-GATE>

**If `--auto` was passed**, skip the review prompt entirely. File the
issue to the current project (target project path).
Proceed directly to the Filing section below.

**If `--auto` was NOT passed**, present the review prompt:

**If the current repo is `benkruger/flow`**, ask the user to review the
draft using AskUserQuestion with structured parameters:

- **question**: "Review the draft above. Ready to file?"
- **header**: "File Issue"
- **options**:
  - label: "File issue", description: "File against the current project with decomposed label"
  - label: "Revise draft", description: "Edit the draft based on your feedback"
  - label: "Re-decompose", description: "Restart from scratch with a new decomposition"

**If the current repo is NOT `benkruger/flow`**, ask the user to review
the draft and choose where to file using AskUserQuestion with structured
parameters:

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

**If "File issue"** (FLOW repo) or **"Target project"** → file the issue
using the target project path (see Filing below).

**If "FLOW plugin"** → file the issue using the FLOW plugin path (see
Filing below).

**If "Revise draft"** → revise the draft based on feedback and re-present.
If the feedback is substantial (changes the problem understanding or
approach), re-run `decompose:decompose` with the updated understanding.
If the feedback is editorial (wording, scope adjustments), revise the
draft directly. After revision, ask again with the same AskUserQuestion.

**If "Re-decompose"** → clean up session files first:

```bash
rm .flow-states/create-issue-<id>.json
```

```bash
rm .flow-states/create-issue-<id>-capture.md
```

Then invoke `flow:flow-create-issue --force-decompose` using the Skill
tool as your final action (no `--step` or `--id` flags — restart from
scratch with forced decompose to bypass Prior Decompose Detection). Do
not output anything else after this invocation.

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

Clean up session files:

```bash
rm .flow-states/create-issue-<id>.json
```

```bash
rm .flow-states/create-issue-<id>-capture.md
```

Display the issue URL to the user, then output the COMPLETE banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-create-issue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Never file an issue without explicit user approval in interactive mode — the Step 2 AskUserQuestion is the mandatory gate unless `--auto` was passed
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- The issue body must be self-contained — a fresh session with no memory of this conversation must be able to execute it
- Always use the Write tool to create body files (`.flow-issue-body-<id>`) — never pass body text as a CLI argument
- Never delete the body file — the `bin/flow issue` script handles cleanup
- Step 1 ends by invoking the skill itself as the final action — Step 2 is terminal
- The Implementation Plan section must use heading levels that match the plan file format after promotion by `flow-plan` (### in the issue becomes ## in the plan file)
