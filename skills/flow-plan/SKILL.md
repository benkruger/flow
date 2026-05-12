---
name: flow-plan
description: "Decompose a vanilla problem-statement issue into a pre-planned decomposed issue. Reads issue #N, runs a Tech-Lead-default planning conversation, dispatches to PM/Tech Lead/CTO sub-agents on explicit user request, then files a linked decomposed issue ready for /flow:flow-start. Usage: /flow:flow-plan #N"
---

# Flow Plan

Decompose a vanilla problem-statement issue (filed by
`/flow:flow-explore`) into a structured implementation plan and
file it as a new decomposed GitHub issue. The skill reads the
parent issue body, holds a Tech-Lead-default planning conversation,
runs `decompose:decompose` against the agreed approach, transforms
the synthesis into an Implementation Plan section wrapped in
FLOW-PLAN sentinels, files the new issue with the `decomposed`
label, and closes the parent vanilla issue with a comment naming
the decomposed child.

The output is a decomposed issue ready for `/flow:flow-start #M`.
The vanilla issue stays as the durable problem statement; the
decomposed issue carries the implementation plan that
`bin/flow plan-from-issue` extracts at flow-start.

## Usage

```text
/flow:flow-plan #N
```

The `#N` argument is the GitHub issue number of the vanilla
problem-statement issue this skill plans against. The skill takes
no other flags or arguments. Bare-topic invocations (`/flow:flow-plan
some topic`) are rejected with a migration message naming
`/flow:flow-explore`.

## Concurrency

The skill creates shared GitHub state (a new decomposed issue and
a closure of the parent vanilla issue) only at the very end, on
explicit user approval. Issue creation is idempotent by title —
if a decomposed issue with the same title already exists, the
user should be warned before filing a duplicate.

The intermediate side effect is the per-session
utility-in-progress marker (scoped to the user's Claude home, not
the project), which lets the Stop hook refuse turn-end while the
discussion-mode skill is running.

Multiple `/flow:flow-plan` sessions on the same machine in
different terminal windows are independent — each has its own
session id, its own conversation context.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-plan — STARTING
──────────────────────────────────────────────────
```
````

Immediately after the banner, write the per-session "utility skill
in progress" marker so the Stop hook refuses turn-end while this
skill is running. Without the marker the model returns control to
the user when a planning sub-agent Skill tool returns mid-pipeline
at Step 5 (Persona Dispatch) or when `decompose:decompose` returns
mid-pipeline at Step 6 (Wrap-up), breaking the unattended-flow
contract this skill promises.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-plan
```

If the marker-write call returns `status: error` with
`no session_id available`, the skill proceeds without the marker.
The Stop hook treats a missing marker as a non-block, so the skill
runs without protection but does not break.

The marker is held across the entire pipeline. Every skill-exit
boundary clears the marker so the Stop hook releases turn-end
after the skill completes: the Step 1 Conversation Gate when the
argument is missing or malformed, the Step 6 validator-max-retries
halt, and the Step 6 success path after the decomposed issue is
filed and linked.
---

## Step 1 — Conversation Gate

Verify that an issue-reference argument was provided after the
slash command in the form `#N` (a literal `#` followed by a
positive integer).

<HARD-GATE>

If no argument was provided, clear the utility-in-progress marker
so the Stop hook does not refuse turn-end after the rejection,
then output the usage guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

> "Issue reference required. Usage: `/flow:flow-plan #N` where N
> is the GitHub issue number of a vanilla problem-statement issue
> filed by `/flow:flow-explore`. To file a problem statement
> first, run `/flow:flow-explore <topic>`."

If the argument does not match the regex `^#[1-9][0-9]*$` after
stripping whitespace — for example, a bare topic like
`/flow:flow-plan add a budget cap` — clear the marker and output
the migration guidance:

> "Argument must be `#N` (e.g., `#1234`). Topic-style invocations
> are no longer accepted — to discuss a new problem statement
> first, run `/flow:flow-explore <topic>`. That skill files a
> vanilla `## What` / `## Why` / `## Acceptance Criteria` issue,
> and `/flow:flow-plan #N` then plans the implementation against
> that issue."

Do not proceed to Step 2, propose direct edits, commit changes, or
take any action outside this skill without a valid `#N` argument.

</HARD-GATE>

---

## Step 2 — Fetch Vanilla Issue

Read the parent issue's title, body, number, labels, and state via
the `gh` CLI. The body becomes the input the Tech Lead reads to
ground the planning conversation; the labels gate the skill's
posture (`decomposed`-labeled issues have already been planned and
must not be re-planned); the state gates against closed issues
(re-planning a closed problem statement requires the user to reopen
it explicitly). The `state` field is load-bearing — without it,
the closed-issue gate below silently bypasses for every issue
because `state == "closed"` evaluates against an absent field that
is never equal to `"closed"`.

```bash
gh issue view <issue_number> --json title,body,number,labels,state
```

Parse the JSON output. Extract `title`, `body`, `number`, and the
labels array.

<HARD-GATE>

Verify the fetched issue is in a state this skill can plan against:

- The issue MUST exist. If `gh issue view` returns a non-zero exit
  status or an error JSON, clear the marker and report the error
  to the user. Do not proceed.
- The issue's labels MUST NOT include `decomposed`. The
  `decomposed` label marks an issue that has already been planned
  via this skill; re-planning would file a sibling decomposed
  issue against an already-decomposed parent. If the
  issue carries `decomposed`, clear the marker and output:

  > "Issue #N already carries the `decomposed` label. To start
  > implementing it, run `/flow:flow-start #N`. To plan a
  > different problem statement, run `/flow:flow-plan #M` against
  > a vanilla issue."

- The issue's state MUST be open. Closed issues represent
  abandoned or completed work and must not be re-planned without
  the user explicitly reopening them. If the issue is closed,
  clear the marker and report:

  > "Issue #N is closed. Re-open it via `gh issue reopen N`
  > before planning, or pick a different issue."

Do not proceed to Step 3, propose direct edits, commit changes,
or take any action outside this skill until the fetch succeeds
and the gate passes.

</HARD-GATE>

---

## Step 3 — Role Read

Resolve the project root and read `.flow.json` from it. The file
is gitignored and lives only at the main repo root — never in a
linked worktree — so the read must target the main repo path
regardless of whether the skill was invoked outside a worktree or
from inside one.

Run `git worktree list --porcelain`. The path on the first
`worktree` line is the main repo root. Read `<project_root>/.flow.json`
via the Read tool.

The file is written by `/flow:flow-prime` and stores per-user
preferences, including the optional `role` field that records the
user's primary working role.

Extract the `role` field from the JSON. The field is optional —
older `.flow.json` files written before the role-selection step
omit it. Treat absence, an unknown value, or a read failure as "no
preferred default" and proceed silently — never block on a missing
role.

The default voice for `/flow:flow-plan` is the **Tech Lead**. The
skill's purpose — turning a problem statement into an
implementation plan with a Dependency Graph, Tasks, and
file-target callouts — is the Tech Lead's natural domain. Map the
user's `role` field to a complementary suggestion only:

| `.flow.json` role value | Conversation note |
|---|---|
| `"pm"` | The user is a PM — note once that this skill plans implementation (Tech Lead voice); the PM's natural collaborator on this is the Tech Lead. |
| `"tech-lead"` | The user IS a Tech Lead — proceed straight into Tech-Lead-voice discussion without a role-suggestion preamble. |
| `"founder-solo"` | The user wears multiple hats — proceed into Tech-Lead-voice discussion; offer to invite the PM voice via Step 5 if the conversation drifts into user-visible behavior questions. |
| Absent / unknown / read failure | Default to Tech Lead voice silently. |

The note is conversational, not gated. The discussion-mode entry
in Step 4 runs the same way for every role, and any sub-agent
dispatch in Step 5 still requires an explicit user request.

---

## Step 4 — Discussion Mode

The planning room exists so the user can think out loud against a
collaborator who reads the code, asks clarifying questions about
constraints and edge cases, and helps shape the implementation
approach. Discussion mode is the default posture — the skill stays
here until the user explicitly asks for a persona dispatch (Step 5)
or signals that the implementation plan is ready to file (Step 6).

In this step, the skill:

- Surfaces clarifying questions about the implementation approach
  — what files are affected, what existing patterns apply, what
  edge cases need testing, what constraints the parent issue
  doesn't yet name.
- Explores the codebase via the Read tool, the Glob tool, and the
  Grep tool to ground the conversation in current code. Cite file
  paths and line numbers when naming what exists today. (Unlike
  `/flow:flow-explore`, source-code reads ARE permitted here —
  the Tech Lead role requires them.)
- Identifies risks, edge cases, prior approaches the user may not
  have considered, architectural concerns, and dependencies on
  other in-flight work.
- Iterates with the user across multiple turns — challenging the
  proposed direction, summarizing what has been agreed, asking
  what comes next.

<HARD-GATE>

Discussion mode forbids action. While in this step, the skill must
NOT:

- Propose direct edits to source files, configuration, or rules —
  this is a planning room, not a coding room.
- Commit changes or invoke `/flow:flow-commit`.
- File issues or invoke any filing skill — the wrap-up belongs in
  Step 6 and is gated on an explicit user signal.
- Compose draft Implementation Plan sections inline. Discussion
  produces context for Step 6's decompose pass; rendering a
  markdown-block draft with formal `## Implementation Plan` /
  `### Context` / `### Tasks` headings during discussion mode
  would short-circuit the decompose-driven structure that the
  wrap-up step builds. Naming files, citing line numbers, and
  summarizing the agreed approach in conversational prose is
  fine; rendering a sentinel-wrapped or heading-structured draft
  is not.
- Use `AskUserQuestion` to manufacture a checkpoint the user did
  not ask for. The discussion is conversational; the user drives
  the cadence by sending messages. Per
  `.claude/rules/autonomous-phase-discipline.md`, never
  self-impose a pause via `AskUserQuestion`.
- Auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch (Step 5) requires the user to type the request
  in plain English.

Stay in discussion mode until the user types one of: a persona
request (then proceed to Step 5 for the named persona), a
hand-off signal ("ready", "file it", "let's go") (then proceed to
Step 6), or any other prose (then continue the conversation in
discussion mode).

</HARD-GATE>

---

## Step 5 — Persona Dispatch

When the user explicitly asks for a planning persona's view ("PM
view?", "What does Tech Lead think?", "CTO take?"), summarize the
discussion so far and dispatch to the named sub-agent. The skill
remains the orchestrator; the sub-agent returns a structured
analysis or a refusal block, and the skill renders the result.

### Summarize for the agent prompt

Build the agent prompt with three labeled sections:

- **PARENT_ISSUE** — the title and body of the vanilla
  problem-statement issue the user is planning against. The
  agent reads this to ground its analysis in the user-visible
  problem.
- **CONVERSATION_SUMMARY** — a synthesis of the planning
  discussion so far. Name the constraints surfaced, the files
  explored, and the direction the user has indicated.
- **PROPOSED_APPROACH** — the concrete implementation approach
  being evaluated. Name the files that would be touched, the
  behavior change at issue, and the user-visible outcome.

### Invoke the named agent via the Skill tool

| User request | Sub-agent to invoke |
|---|---|
| PM view | `flow:pm` |
| Tech Lead view | `flow:tech-lead` |
| CTO view | `flow:cto` |

Pass the agent prompt above as the Skill tool's input.

<HARD-GATE>

When the sub-agent returns, render its response verbatim in the
conversation. If the response is a `## SCOPE REFUSAL` block, the
skill MUST surface it as-is and wait for explicit user direction.
The refusal is the agent's signal that the proposed change exceeds
its authority and a different tier should evaluate it.

When a `## SCOPE REFUSAL` block returns, the skill must NOT:

- Auto-escalate to the next tier (PM → Tech Lead, Tech Lead → CTO)
  without the user's explicit go-ahead. The escalation is the
  user's call, not the skill's.
- Re-invoke the same agent with softer framing, narrower scope, or
  any reworded prompt designed to elicit a non-refusal. The agent
  refused on principle, not on framing.
- Perform the refused analysis personally. The orchestrating model
  is not authorized to step into the role the agent declined; the
  refusal exists precisely because the change deserves a different
  level of judgment.

Present the refusal block to the user and ask them how to proceed.
If the user says "escalate to Tech Lead" or "ask the CTO", the
next persona dispatch fires in a fresh Step 5 invocation. If the
user wants to discuss the refusal in plain language, return to
Step 4.

</HARD-GATE>

When the agent returns an in-scope analysis (not a refusal), render
it verbatim and return to Step 4 so the user can react, ask
follow-ups, or request another persona's view. Do not assume the
agent's recommendation is the user's decision.

---

## Step 6 — Wrap-up: Decompose, Transform, Validate, File, Close Parent

When the user signals readiness — "ready", "file it", "let's go",
"create the issue", or any equivalent phrasing — run the
decompose-and-file pipeline.

### Generate session ID

Generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool.
This ID scopes the body file path (`.flow-issue-body-<id>`) so
concurrent `/flow:flow-plan` invocations cannot collide on the
same temp file.

### Decompose

Invoke `decompose:decompose` via the Skill tool with an
implementation-focused prompt. The prompt must make clear that
the problem statement is already agreed (the parent issue body
fetched in Step 2 captures it); decompose should structure the
implementation into tasks, not re-analyze the problem.

Example prompt structure:

> "Given the following problem statement and the agreed
> implementation approach, decompose the work into ordered tasks
> with dependencies, approach, and file targets. The problem is
> already understood — focus on structuring the implementation.
>
> [Parent issue body — `## What` / `## Why` / `## Acceptance
> Criteria` from Step 2]
>
> [Summary of the agreed approach from the conversation]
>
> [Key files and patterns identified during discussion]"

The decompose output produces a structured DAG with nodes,
dependencies, and a synthesis — this becomes the foundation for
the Implementation Plan.

<HARD-GATE>

When the Skill tool returns from the `decompose:decompose`
invocation, you are still inside flow-plan. The Skill tool's
return is NOT a stopping point — it is a mid-skill handoff. Do
not stop, do not summarize, do not ask the user "want me to
continue?", do not return control to the user. Proceed
immediately to Transform + Draft below using the decompose output
you just received.

If you stop here, the user must prompt you again to continue,
which breaks the unattended flow that flow-plan promises to its
consumers.

</HARD-GATE>

### Transform + Draft

Take the decompose synthesis and transform it into an
Implementation Plan section. The Implementation Plan must contain
these subsections:

- **Context** — Restate what the parent issue's `## What` / `## Why`
  established, in implementation terms. This is the bridge from
  the user-visible problem statement to the implementation plan.
- **Exploration** — What exists in the codebase, affected files,
  patterns discovered.
- **Risks** — What could go wrong, edge cases, constraints.
- **Approach** — The chosen approach and rationale.
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

Tasks must use `#### Task N:` heading format.

### Title Authoring

The decomposed issue's title flows downstream into the branch name
(via `branch_name`), the PR title, the commit subject, and every
user-visible surface. Reuse the parent issue's title verbatim
unless the planning conversation surfaced a sharper framing — in
which case the new title must still pass the plain-English test
applied at `/flow:flow-explore` filing time.

**Required.** Subject + verb + object as a stakeholder would say
it out loud. A non-contributor reading the title in a release-notes
feed should understand what the change is for without consulting
the codebase.

**Forbidden.** Code symbols, function names, file paths, line
numbers, internal acronyms without expansion, one-letter shorthand,
repo-specific jargon.

### Combine into Issue Body

Combine the parent issue's content with the Implementation Plan
into a single decomposed issue body in working memory. The
section order must be:

**What** (from parent) → **Why** (from parent) → **Acceptance
Criteria** (from parent) → **Implementation Plan** (wrapped in
FLOW-PLAN sentinels — containing Context, Exploration, Risks,
Approach, Dependency Graph, Tasks subsections) →
**Parent Issue** (one-line link to issue #N for forensic
detection).

Each top-level section uses `##` headings. The Implementation
Plan's subsections use `###` headings. Task entries within the
Tasks subsection use `####` headings.

**Wrap the Implementation Plan in FLOW-PLAN sentinels.** Place
the literal HTML comment `<!-- FLOW-PLAN-BEGIN -->` on its own
line immediately before the `## Implementation Plan` heading,
and the literal HTML comment `<!-- FLOW-PLAN-END -->` on its own
line immediately after the last Task entry (before the next
`## ` heading). The sentinels delimit the bytes that
`bin/flow plan-from-issue` will extract verbatim and write to
`.flow-states/<branch>/plan.md` when the issue is later picked up
via `/flow:flow-start #M`. Without the sentinel pair,
plan-from-issue rejects the issue with `plan_markers_missing`
and the flow halts.

**Paraphrase every prose reference to the plan-sentinel pair.**
The literal HTML-comment marker strings only appear in the body
at two positions — the opening sentinel and the closing sentinel.
They must never appear inside prose, headings, code blocks,
examples, or any other surface of the body. `bin/flow
plan-from-issue` extracts the slice between the FIRST occurrence
of each marker, so a literal marker mid-prose silently redirects
the extraction to the wrong slice. Whenever the body needs to
reference the marker pair, paraphrase: "the FLOW-PLAN sentinel
pair", "the plan-extraction markers", "the canonical sentinels
delimiting the plan block".

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
- `"older plugin versions"`, `"prior plugin"` —
  plugin-version-compat reasoning
- `"as PR #<N> chose to"`, `"following the prior PR"` —
  deferring to past decisions

Evaluate matches in context: a bare `PR #<N>` reference used for
forensic detection (linking blocked-by, naming a specific merge)
is fine; a `PR #<N>` reference used to justify the present design
is forbidden. If any match is justifying-shape rather than
identifier-shape, revise the draft. See
`.claude/rules/no-backwards-reasoning.md`.

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
  different heading
- `"would expand scope"` — reflexive scope shrinkage that
  bypasses the three-condition gate in
  `.claude/rules/scope-expansion.md`
- `"separate code surface"` — code-shape framing used as an
  exclusion criterion

Evaluate matches in context: a passing mention that names a
concern is fine; an enumerated section or bulleted list of
exclusions is forbidden. If any match is exclusion-shape rather
than identifier-shape, revise the draft. See
`.claude/rules/include-bias-in-issues.md`.

### Draft Presentation

Present the full draft inline in the response — both title and
body. Do not tell the user to look at a file. Render it as a
formatted markdown block so the user can review every detail.

### Validate + File + Link

Write the issue body to `.flow-issue-body-<id>` using the Write
tool. Per `.claude/rules/filing-issues.md` "The Pattern": when
invoked inside an active FLOW worktree, prepend the worktree
absolute path so the `validate-worktree-paths` hook allows the
Write. When invoked outside a worktree, the relative form
resolves cleanly because Write and `bin/flow issue` both target
the same project root.

Validate the body file through the pre-filing validator with
`--mode decomposed` before asking the filer subcommand to send
it to GitHub. The validator runs the same sentinel-extraction
logic that `bin/flow plan-from-issue` applies at flow-start; any
body that fails this gate is unconsumable downstream and must
NOT be filed:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --mode decomposed --body-file .flow-issue-body-<id>
```

Parse the JSON output. If `status` is `ok`, proceed to the filer
invocation below. If `status` is `error`, run the bounded auto-fix
loop:

#### Validator Auto-Fix Loop (max 5 attempts)

When the validator returns `status: error`, the skill must NOT
prompt the user. The validator's `message` names a concrete defect
(missing FLOW-PLAN sentinel pair, missing required subsection,
`## Implementation Plan` heading on the wrong nesting level, etc.).
Apply a mechanical fix that addresses the named defect — adjust
the sentinel placement, add the missing subsection, normalize the
heading — rewrite the body file with the Write tool, and re-run
the validator. Track the attempt count mentally — the cap is
**5 attempts** including the first failure.

After 5 failed validator runs, clear the utility-in-progress
marker, halt the skill with the structured error envelope, and
print the COMPLETE-FAILED banner. Do NOT file the issue. Do NOT
edit issue #N. Do NOT loop further.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

````markdown
```json
{"status":"error","reason":"validator_max_retries","attempts":5}
```
````

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✗ FLOW v1.1.0 — flow:flow-plan — COMPLETE-FAILED
  Validator rejected the body 5 times. Issue not filed.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Once the validator returns `ok`, file the issue against the
current repo (no `--repo` flag — `flow-plan` always files where
the user is) WITH the `decomposed` label so `flow-issues` and
`flow-orchestrate` recognize it as ready-for-flow-start work:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id> --label decomposed
```

Capture the new issue's number from the URL in the filer's output
(parse the trailing `/issues/M` segment). This is the **decomposed
issue number M** — distinct from the parent **vanilla issue
number N** the user passed at Step 1.

Close the parent vanilla issue with a comment naming the
decomposed child. The decomposed issue's Implementation Plan is
now the full problem-and-solution artifact for this work — the
vanilla problem statement is superseded once the decomposed plan
exists, and leaving the vanilla open duplicates the open surface
for the same problem (`flow-issues` would surface both, and
engineers picking from the backlog could not tell which is the
canonical entry point). The closing comment carries a pointer to
the decomposed child so a reader landing on the closed parent has
a breadcrumb back to the work that supersedes it. Substitute the
parent vanilla issue number (the `#N` the user passed at Step 1)
for `<vanilla_number>` and the new decomposed issue number for
`<M>`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow close-issue --number <vanilla_number> --comment "Decomposed into #<M>. Implementation plan tracked there; closing this problem statement."
```

Parse the JSON result. When the response shape is
`{"status":"error","message":"..."}` the gh subprocess refused the
closure (transient network failure, auth scope mismatch, the
parent was already closed by a parallel operation, etc.). The
decomposed child issue #M already exists at this point — do NOT
re-file it and do NOT retry the closure. Instead, report the
failure inline so the user has a concrete recovery step:

> "Filed decomposed issue #M but failed to close parent #N:
> `<message>`. Close the parent manually with
> `gh issue close <N> --comment "Decomposed into #M. ..."` once
> the underlying gh failure is resolved, then run
> `/flow:flow-start #M`."

Then skip the remaining state-recording steps below (the
`add-issue` and `clear-utility-in-progress` calls), print the
COMPLETE-FAILED banner, and stop. Failing to halt would leave
the utility-in-progress marker cleared with no breadcrumb back
to the open parent — the user must reconcile the GitHub state
before the flow can proceed.

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✗ FLOW v1.1.0 — flow:flow-plan — COMPLETE-FAILED
  Decomposed issue filed; parent closure failed.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

When the response is `{"status":"ok"}`, proceed.

Record the issue in the state file (no-op if no FLOW feature is
active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-plan
```

Clear the utility-in-progress marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

Display the decomposed issue URL to the user, then output the
COMPLETE banner in your response (not via Bash) inside a fenced
code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-plan — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then instruct the user:

> "Filed decomposed issue #M and closed parent #N. To start
> implementing, run `/flow:flow-start #M`."

Do not invoke `/flow:flow-start` yourself — the user types the
slash command directly.

---

## Hard Rules

- **Always require `#N` argument.** Bare-topic invocations are
  rejected at Step 1 with a migration message naming
  `/flow:flow-explore`.
- **Always invoke `bin/flow close-issue` with `--comment`** after
  filing the decomposed issue. Closing the vanilla parent at plan
  time removes the duplicate problem-statement surface so the
  decomposed work is the only open artifact for the problem.
- **Never edit issue #N.** The parent vanilla issue stays as the
  durable problem statement; the decomposed issue is filed as a
  NEW linked issue, not an in-place edit.
- **Always file with `--label decomposed`.** Without the label,
  `flow-issues` and `flow-orchestrate` won't recognize the issue
  as ready-for-flow-start work.
- **Always validate with `--mode decomposed`** before filing.
  Vanilla bodies and decomposed bodies have different shapes; the
  wrong mode silently passes a body that `bin/flow plan-from-issue`
  will later reject.
- Never propose direct edits, commit changes, or file issues
  outside the explicit Step 6 wrap-up.
- Never present draft Implementation Plan sections inline before
  reaching Step 6 Wrap-up. Discussion produces context for the
  decompose pass; rendering a sentinel-wrapped or
  heading-structured draft during discussion is forbidden.
- Never use `AskUserQuestion` during discussion mode. The
  discussion is conversational; the user drives the cadence.
- Never auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch requires the user to type the request in plain
  English.
- Never auto-escalate, re-invoke with softer framing, or perform
  the refused analysis personally when a sub-agent returns a
  `## SCOPE REFUSAL` block. Render the refusal verbatim and wait
  for explicit user direction on the next move.
- Never use `AskUserQuestion` in the Step 6 wrap-up. The user's
  readiness signal from the discussion phase is the single
  authorization to file; a second confirmation gate would break
  the single-signal contract that drives the unattended-flow
  promise.
- Never tell the user to "look at" a file — render all content
  inline.
- Never use Bash to print banners — output them as text in your
  response.
- Always use the Write tool to create body files
  (`.flow-issue-body-<id>`) — never pass body text as a CLI
  argument.
- Never delete the body file — the `bin/flow issue` script
  handles cleanup.
- Treat absence or unknown values of the `.flow.json` `role`
  field as "no preferred default" and proceed silently. Never
  block on a missing field.
- All `bin/flow` calls use `${CLAUDE_PLUGIN_ROOT}/bin/flow` —
  bare `bin/flow` only resolves inside the FLOW repo itself, not
  in target projects.
