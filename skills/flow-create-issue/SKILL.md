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
```

The skill takes no flags or arguments. Every invocation runs the
full pipeline — Conversation Gate, Capture, Title Authoring,
Decompose, Transform + Draft, File, Filing — and the Decompose step
invokes `decompose:decompose` unconditionally so the Implementation
Plan derives from structured decompose output rather than from
unbounded conversation context.

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

Immediately after the banner, write the per-session "utility skill
in progress" marker so the Stop hook refuses turn-end while this
skill is running. Without the marker the model returns control to
the user when the decompose:decompose Skill tool returns
mid-pipeline, breaking the unattended-flow contract this skill
promises.

Rust resolves the active session_id at the CLI boundary by reading
the `CLAUDE_CODE_SESSION_ID` env var Claude Code supplies to every
Bash subprocess (Claude Code 2.1.132+); on older Claude Code
installs it falls back to the SessionStart capture file. On
2.1.132+ the per-subprocess env value matches what the Stop hook
receives in its stdin payload, so set-time and clear-time resolve
to the same id regardless of concurrent Claude Code activity. The
bash invocations below pass `--skill` only; Rust supplies the
session_id itself.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-create-issue
```

If the marker-write call returns `status: error` with
`no session_id available` (no env var AND no capture file — rare,
only on Claude Code installs without per-subprocess env support and
without a SessionStart capture file), the skill proceeds without
the marker. The Stop hook treats a missing marker as a non-block,
so the skill runs without protection but does not break.

On Claude Code installs without the per-subprocess env var, the
capture-file fallback resolves session_id independently at set and
clear time. A second Claude Code session whose SessionStart hook
overwrites the capture file between this skill's set and clear
calls can leave the marker orphaned at the original id. Recovery
is `rm ~/.claude/flow/utility-in-progress-*.json` after the skill
completes; the Stop hook treats a missing marker as a non-block.

---

## Conversation Gate

Before entering the pipeline, verify that the current conversation contains
brainstorming context — a problem that was explored, a solution that was
discussed and agreed upon. This skill captures solutions, it does not
discover them.

**Signals that context exists** — proceed to Capture:

- Prior `/decompose:decompose` output in the conversation
- Extended back-and-forth about a problem and its solution
- An agreed approach, design, or set of changes discussed
- The user explicitly says "file it", "create an issue", or similar

**Signals that context is missing** — reject:

- The skill was invoked with a bare problem description and no prior discussion
- No decompose output or design iteration is visible in the conversation
- The conversation just started with this invocation

<HARD-GATE>

If no brainstorming context exists, clear the utility-in-progress
marker so the Stop hook does not refuse turn-end after the rejection,
then output this guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue
```

> "This skill captures a brainstormed solution as a pre-planned issue.
> Start by running `/decompose:decompose` to research the problem,
> iterate on a solution, then invoke `/flow:flow-create-issue` when
> you have an agreed approach."

Do not proceed to Capture, propose direct edits, commit changes, or take
any action outside this skill without brainstorming context in the
conversation.

</HARD-GATE>

---

## Capture

Generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool. This ID
scopes the body file path (`.flow-issue-body-<id>`) so concurrent
`flow-create-issue` invocations cannot collide on the same temp file.

**Capture the problem sections** from the conversation context. Synthesize
the discussion into these structured sections in working memory — do not
re-analyze or re-explore, just distill what was already discussed:

- **Problem** — What is broken, missing, or inadequate. Include observable
  behavior, evidence from the codebase (file paths, line numbers), and user
  impact. Grounded in the exploration already done in the conversation.
- **Acceptance Criteria** — Binary, testable conditions. Pass/fail with no
  subjective judgment.
- **Files to Investigate** — Real file paths verified during the conversation's
  codebase exploration. Include a brief note on why each is relevant.
- **Context** — Business reason, architectural constraints, or design decisions.

---

## Title Authoring

The issue title flows downstream into the branch name (via
`branch_name`), the PR title (via `derive_feature`), the commit
subject, and the TUI feature line — every user-visible surface
inherits whatever you write here. Titles must read as plain English
to a stakeholder who is not a contributor; titles that smuggle in
code symbols, internal acronyms, or one-letter shorthand corrupt
every downstream surface they reach.

### Required

Titles must describe the user-visible problem or outcome in plain
English. Subject + verb + object as a reader would say it out loud.
A non-contributor reading the title in a release-notes feed should
understand what the change is for without consulting the codebase.

### Forbidden

The following must not appear in the title — they belong in the
issue body, the plan, or the code, never in the headline string:

- **Code symbols** — function names, type names, identifiers like
  `code_tasks_total`, command names like `bin/flow`.
- **Field names and file paths** — `state["foo"]`, `src/utils.rs`,
  any `module::function` reference.
- **Line numbers** — `:42`, `lines 100-120`.
- **Internal acronyms without expansion** — TUI, DAG, RAII,
  sentinel, hash, gate, agent shorthand. Expand on first use, or
  paraphrase entirely.
- **One-letter shorthand** — `X-of-Y`, `M of N`, single-letter
  variable names.
- **Abbreviations a non-contributor would not recognize** — repo-
  specific jargon, internal product code-names, in-flight
  refactor labels.

### Bad → Good Examples

| Bad (what flow-create-issue produces today) | Good (what the rule requires) |
|---|---|
| Wire code_tasks_total writer and put X-of-Y first in Code-phase TUI annotation | Show task progress as "step 3 of 7" in the Code phase status display |
| Fix three-hook deadlock on shared-config edits in autonomous flows | Stop the abort skill from deadlocking when a flow edits shared config |
| Add structural code_read field to pre-mortem agent finding schema | Have the pre-mortem agent record which files it read for each finding |

The title is the seed for every downstream identifier the user
will see. A title that fails this rule produces an unreadable
branch, an unreadable PR title, an unreadable commit subject, and
an unreadable TUI line — fixing the title at the source is much
cheaper than patching every downstream surface.

---

## Decompose

Invoke `decompose:decompose` via the Skill tool with an
implementation-focused prompt. The decompose pass runs on every
`flow-create-issue` invocation — there is no skip path and no
override flag. The prompt must make clear that the problem and
solution are already agreed (the Conversation Gate already
verified that brainstorming context exists upstream); decompose
should structure the implementation into tasks, not re-analyze
the problem.

Example prompt structure:

> "Given the following agreed solution, decompose the implementation into
> ordered tasks with dependencies, approach, and file targets. The problem
> is already understood — focus on structuring the work.
>
> [Summary of the agreed solution from the conversation]
>
> [Key files and patterns identified during brainstorming]"

The decompose output produces a structured DAG with nodes, dependencies,
and a synthesis — this becomes the foundation for the Implementation Plan.

<HARD-GATE>

When the Skill tool returns from the decompose:decompose invocation,
you are still inside flow-create-issue. The Skill tool's return is
NOT a stopping point — it is a mid-skill handoff. Do not stop, do
not summarize, do not ask the user "want me to continue?", do not
return control to the user. Proceed immediately to Transform + Draft
below using the decompose output you just received.

If you stop here, the user must prompt you again to continue, which
breaks the unattended flow that flow-create-issue promises to its
consumers. The whole point of the skill is that one invocation
produces a filed issue without further user input.

</HARD-GATE>

---

## Transform + Draft

Take the decompose synthesis from the invocation you just ran and
transform it into an Implementation Plan section that matches the
plan file format used by `flow-plan`. The Implementation Plan must
contain these subsections:

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

Combine the captured problem sections with the Implementation Plan
into a single issue body in working memory. The section order must be:

**Problem** (from capture) → **Acceptance Criteria** (from capture) →
**Implementation Plan** (from transform, wrapped between sentinels —
containing Context, Exploration, Risks, Approach, Dependency Graph,
Tasks subsections) → **Files to Investigate** (from capture) →
**Context** (from capture — business reason).

Each top-level section uses `##` headings. The Implementation Plan's
subsections use `###` headings. Task entries within the Tasks subsection
use `####` headings.

**Wrap the Implementation Plan in FLOW-PLAN sentinels.** Place the
literal HTML comment `<!-- FLOW-PLAN-BEGIN -->` on its own line
immediately before the `## Implementation Plan` heading, and the
literal HTML comment `<!-- FLOW-PLAN-END -->` on its own line
immediately after the last Task entry (before the next `## ` heading).
The sentinels delimit the bytes that `bin/flow plan-from-issue` will
extract verbatim and write to `.flow-states/<branch>/plan.md` when the
issue is later picked up via `/flow:flow-start #N`. Without the
sentinel pair, plan-from-issue rejects the issue with
`plan_markers_missing` and the flow halts.

**Paraphrase every prose reference to the plan-sentinel pair.** The
literal HTML-comment marker strings only appear in the body at two
positions — the opening sentinel and the closing sentinel. They
must never appear inside prose, headings, code blocks, examples,
or any other surface of the body. `bin/flow plan-from-issue`
extracts the slice between the FIRST occurrence of each marker, so
a literal marker mid-prose silently redirects the extraction to
the wrong slice — exactly the failure mode `bin/flow
validate-issue-body` exists to detect. Whenever the body needs to
reference the marker pair (for example, when the issue topic is
the sentinel protocol itself), paraphrase every reference.
Acceptable wording: "the FLOW-PLAN sentinel pair", "the
plan-extraction markers", "the canonical sentinels delimiting the
plan block". The validator's `marker_count_wrong` branch catches
violations downstream; this rule prevents them upstream so the
Revise loop is not entered unnecessarily.

The wrapped block looks like this in the issue body:

```markdown
<!-- FLOW-PLAN-BEGIN -->
## Implementation Plan

### Context
...

### Exploration
...

### Tasks

#### Task 1: ...
...
<!-- FLOW-PLAN-END -->
```

### Pre-Draft Backwards-Reasoning Scan

Before presenting the draft, scan the body — including the
Implementation Plan subsections — for the following forbidden
phrasings, which ground the current decision in a historical
artifact rather than the code's current merits:

- `"PR #<N> decided"`, `"the prior PR chose"`, `"the previous
  commit"` — historical decision cited as authority
- `"kept for backward compatibility"`, `"compat shim"`, `"legacy
  alias for older"` — preservation justified by inherited
  reasoning rather than a current consumer
- `"older plugin versions"`, `"prior plugin"` — plugin-version-
  compat reasoning (the FLOW plugin auto-updates and has no
  installed base)
- `"as PR #<N> chose to"`, `"following the prior PR"` —
  deferring to past decisions

Evaluate matches in context: a bare `PR #<N>` reference used for
forensic detection (linking blocked-by, naming a specific merge)
is fine; a `PR #<N>` reference used to justify the present design
is forbidden. If any match is justifying-shape rather than
identifier-shape, revise the draft. Re-evaluate the underlying
decision on the code's current merits, not on historical context.
See `.claude/rules/no-backwards-reasoning.md`.

### Pre-Draft Include-Bias Scan

Before presenting the draft, scan the body — including the
Implementation Plan subsections — for the following forbidden
phrasings, which signal defensive scope shrinkage rather than
genuine exclusion grounded in a concrete blocker:

- `"Out of scope"` — defensive enumeration of exclusions written
  before concrete blockers have surfaced; the scan reads
  case-flexibly, so common section-heading title-case forms in
  issue bodies are also flagged
- `"Non-goals"` — same defensive-enumeration shape under a
  different heading; a bulleted list of "things we are not
  doing" is speculation, not analysis
- `"would expand scope"` — reflexive scope shrinkage that
  bypasses the three-condition gate in
  `.claude/rules/scope-expansion.md`
- `"separate code surface"` — code-shape framing used as an
  exclusion criterion; "separate surface" describes the code,
  not the work

Evaluate matches in context: a passing mention that names a
concern is fine; an enumerated section or bulleted list of
exclusions is forbidden. The default is inclusion — every
adjacent concern surfaced during exploration belongs as a task
unless one of the narrow valid exclusions (user explicitly
rejected, requires different design conversation, blocks
primary completion) applies. If any match is exclusion-shape
rather than identifier-shape, revise the draft: convert the
deferral into an inclusion task, or name the concrete blocker
in the Context section as one sentence. The lifecycle cost of
splitting a concern out of an issue is multiples larger than
including it in the current exploration budget. See
`.claude/rules/include-bias-in-issues.md`.

### Draft Presentation

Present the full draft inline in the response — both title and body. Do
not tell the user to look at a file. Render it as a formatted markdown
block so the user can review every detail.

---

## File

<HARD-GATE>

After presenting the draft, ask the user to confirm via AskUserQuestion
with structured parameters:

- **question**: "Review the draft above. Ready to file?"
- **header**: "File Issue"
- **options**:
  - label: "File issue", description: "File against the current repo with the decomposed label"
  - label: "Revise draft", description: "Edit the draft based on your feedback"
  - label: "Cancel", description: "Stop without filing an issue"

Do not file the issue, propose direct edits, commit changes, or take
any action outside this skill without explicit user approval via
AskUserQuestion — even if the answer appears obvious from context.

**If "File issue"** → proceed to Filing below.

**If "Revise draft"** → revise based on the user's feedback and
re-present the draft. If the feedback is substantial (changes the
problem understanding or approach), re-run `decompose:decompose` with
the updated understanding and re-transform. If the feedback is
editorial (wording, scope adjustments), edit the draft directly.
**When in doubt, treat the feedback as substantial and re-run
`decompose:decompose`** — the safe default is the conservative action
(per `.claude/rules/skill-authoring.md` "Safe Defaults for Subjective
Classification"); editing a draft built on a misaligned decompose ships
an incorrect Implementation Plan. After revising, re-present the draft
and ask the same AskUserQuestion. Iterate as many times as needed.

**If "Cancel"** → clear the utility-in-progress marker so the Stop
hook does not refuse turn-end after cancellation, then stop without
filing. Do not write the body file. Do not output the COMPLETE
banner.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue
```

</HARD-GATE>

---

## Filing

Write the issue body to `.flow-issue-body-<id>` in the project root
using the Write tool. The body file is the validator's input and the
filer's input in the same path — same bytes on disk for both
subprocesses, no copy.

Validate the body file through the pre-filing validator before
asking the filer subcommand to send it to GitHub. The validator
runs the same sentinel-extraction logic that `bin/flow
plan-from-issue` applies at flow-start; any body that fails this
gate is unconsumable downstream and must NOT be filed:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --body-file .flow-issue-body-<id>
```

Parse the JSON output. If `status` is `ok`, proceed to the filer
invocation below. If `status` is `error`, do NOT file the issue.
Show the validator's `message` field to the user, return to the
Revise loop in the File step above with the user's feedback set to
the validator's `message`, and re-present the corrected draft.
Iterate until the validator returns `ok`. A body that the
validator rejects would also be rejected by `plan-from-issue` at
flow-start, so filing it produces an unusable issue that the next
`/flow:flow-start` invocation cannot consume.

Once the validator returns `ok`, file the issue against the current
repo (no `--repo` flag — `flow-create-issue` always files where the
user is):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id> --label decomposed
```

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-create-issue
```

Clear the utility-in-progress marker so the Stop hook stops refusing
turn-end now that the skill has completed its work:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-create-issue
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

- Never file an issue without explicit user approval — the AskUserQuestion before filing is the mandatory gate
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- The issue body must be self-contained — a fresh session with no memory of this conversation must be able to execute it
- Always use the Write tool to create body files (`.flow-issue-body-<id>`) — never pass body text as a CLI argument
- Never delete the body file — the `bin/flow issue` script handles cleanup
- The Implementation Plan section must use heading levels that match the plan file format after promotion by `flow-plan` (### in the issue becomes ## in the plan file)
- Paraphrase every prose reference to the plan-sentinel pair — the literal HTML-comment marker strings appear only at the actual delimiters of the wrapped Implementation Plan, never inside prose, headings, code blocks, or examples. A duplicate marker mid-prose silently redirects `bin/flow plan-from-issue` extraction to the wrong slice.
