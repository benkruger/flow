---
name: flow-plan
description: "Decompose a problem statement into a pre-planned decomposed issue. Accepts either an issue reference (#N, re-plans in place) or a bare prompt (synthesizes What/Why/AC and files a new issue). Runs a Tech-Lead-default planning conversation, dispatches to PM/Tech Lead/CTO sub-agents on explicit user request, then files or edits the issue ready for /flow:flow-start. Usage: /flow:flow-plan #N or /flow:flow-plan <topic>"
---

# Flow Plan

Produce a structured implementation plan and attach it to a
GitHub issue. The skill holds a Tech-Lead-default planning
conversation, runs `decompose:decompose` against the agreed
approach, transforms the synthesis into an Implementation Plan
section wrapped in FLOW-PLAN sentinels, and either edits an
existing issue in place (issue-input mode) or files a new
decomposed issue (bare-prompt mode).

The output is an issue ready for `/flow:flow-start #N` (re-planned
issue) or `/flow:flow-start #M` (new decomposed issue). The
issue body carries the implementation plan that
`bin/flow plan-from-issue` extracts at flow-start.

## Usage

```text
/flow:flow-plan #N
/flow:flow-plan <topic>
```

The skill accepts two argument shapes:

- **`#N`** — a literal `#` followed by a positive integer. Plans
  against existing issue #N: Step 2 fetches the body and Step 6
  edits the issue in place, preserving every byte above the
  opening FLOW-PLAN sentinel.
- **`<topic>`** — any non-empty string that does not match the
  `#N` regex. Seeds discussion in Step 4; Step 6 synthesizes a
  brief `## What` / `## Why` / `## Acceptance Criteria` from the
  conversation and files one new decomposed issue.

The skill takes no flags. A missing argument is rejected at
Step 1.

## Concurrency

The skill mutates shared GitHub state only at the very end of
Step 6, on explicit user approval. In bare-prompt mode it files
one new decomposed issue (creation is idempotent by title — warn
the user before filing a duplicate). In issue-input mode it
edits the existing issue's body and re-applies the `decomposed`
label (the `gh issue edit --add-label` call is idempotent).

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
  FLOW v2.4.0 — flow:flow-plan — STARTING
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
after the skill completes: the Step 1 Conversation Gate when no
argument is provided, the Step 6 validator-max-retries halt, and
the Step 6 success path after the issue has been filed or
edited.
---

## Step 1 — Conversation Gate

`/flow:flow-plan` accepts either of two argument shapes — `#N`
(issue-input mode, which plans against an existing problem
statement and edits that same issue in place) or a bare non-empty
prompt (bare-prompt mode, which synthesizes a brief What/Why/AC
from the conversation and files one new decomposed issue). The
gate routes between the two modes; it does not redirect users to
another skill.

If the argument matches the regex `^#[1-9][0-9]*$` after stripping
whitespace, set the working mode to **issue-input** and proceed
to Step 2. Step 2 fetches the parent issue body and applies the
remaining open-state gate.

If the argument starts with a `#` character but does NOT match
the `^#[1-9][0-9]*$` regex (e.g., a lone `#`, `#0`, `#-1`,
`#1234abc`, `#1.5`), the leading `#` is a strong signal of
intended issue-input mode but the trailing characters are invalid.
Routing such an argument to bare-prompt mode would silently file
a new decomposed issue with the malformed token as the seeding
topic — almost certainly a typo. Clear the utility-in-progress
marker and reject with usage guidance:

> "Argument starts with `#` but does not match `^#[1-9][0-9]*$`.
> To plan against an existing issue, pass a valid number like
> `#1234`. To plan from a bare prompt, drop the leading `#`."

If the argument is a non-empty string that contains no leading
`#` (a bare prompt such as `/flow:flow-plan add a budget cap`),
set the working mode to **bare-prompt** and skip Step 2 entirely.
The prompt seeds the planning discussion at Step 4 — there is no
GitHub issue to fetch in this mode, so Step 2's fetch and gates
do not apply. Continue at Step 3.

<HARD-GATE>

If no argument was provided at all, clear the utility-in-progress
marker so the Stop hook does not refuse turn-end after the
rejection, then output the usage guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

> "Argument required. Usage: `/flow:flow-plan #N` to plan against
> issue N in place, or `/flow:flow-plan <topic>` to synthesize a
> brief problem statement and file a new decomposed issue."

Do not proceed without an argument. Do not propose direct edits,
commit changes, or take any action outside this skill without
either an `#N` argument or a bare-prompt argument.

</HARD-GATE>

---

## Step 2 — Fetch Issue (issue-input mode only)

Only when Step 1 resolved to **issue-input** mode. In bare-prompt
mode there is no GitHub issue to fetch; skip to Step 3.

Read the issue's title, body, number, labels, and state via the
`gh` CLI. The body becomes the input the Tech Lead reads to
ground the planning conversation; the state gates against closed
issues (planning into a closed problem statement requires the
user to reopen it explicitly, because `bin/flow plan-from-issue`
refuses closed issues at flow-start and the resulting plan would
be unusable). The `state` field is load-bearing — without it,
the closed-issue gate below silently bypasses for every issue
because `state == "closed"` evaluates against an absent field that
is never equal to `"closed"`.

```bash
gh issue view <issue_number> --json title,body,number,labels,state,url
```

Parse the JSON output. Extract `title`, `body`, `number`, `url`,
and the labels array. The `url` field is consumed by Step 6's
`bin/flow add-issue` call so the recorded URL points at the same
issue the user passed.

<HARD-GATE>

Verify the fetched issue is in a state this skill can plan against:

- The issue MUST exist. If `gh issue view` returns a non-zero exit
  status or an error JSON, clear the marker and report the error
  to the user. Do not proceed.
- The issue's state MUST be open. Closed issues represent
  abandoned or completed work and must not be planned against
  without the user explicitly reopening them. `bin/flow
  plan-from-issue` rejects closed issues at flow-start with
  `reason: issue_closed`, so a plan written into a closed issue
  is unusable downstream. If the issue is closed, clear the
  marker and report:

  > "Issue #N is closed. Re-open it via `gh issue reopen N`
  > before planning, or pick a different issue."

The issue's `decomposed` label is NOT a rejection criterion. An
issue that already carries `decomposed` is a candidate for
in-place re-planning — Step 6 issue-input mode preserves the
content above the FLOW-PLAN sentinel and swaps the
sentinel-delimited plan block, leaving the original problem
statement intact.

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

## Step 6 — Wrap-up: Decompose, Transform, Validate, File or Edit

When the user signals readiness — "ready", "file it", "let's go",
"create the issue", or any equivalent phrasing — run the
decompose-and-file pipeline. The output shape depends on the
Step 1 mode:

- **Issue-input mode** edits the existing issue #N in place: the
  content above the opening FLOW-PLAN sentinel is preserved
  verbatim (the original problem statement); any existing
  sentinel-delimited plan block is replaced; the fresh
  sentinel-wrapped `## Implementation Plan` is appended. The
  parent issue stays open; the assignee is not changed.
- **Bare-prompt mode** files one new decomposed issue: the model
  synthesizes a brief `## What` / `## Why` / `## Acceptance
  Criteria` from the conversation, appends the
  sentinel-wrapped plan, and files via `bin/flow issue` with
  `--label decomposed --assignee @me`. There is no parent to
  close.

### Generate session ID

Generate a short session ID using the Bash tool:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id
```

This ID scopes the body file path (`.flow-issue-body-<id>`) so
concurrent `/flow:flow-plan` invocations cannot collide on the
same temp file.

### Decompose

Invoke `decompose:decompose` via the Skill tool with an
implementation-focused prompt. The prompt must make clear that
the problem statement is already agreed; decompose should
structure the implementation into tasks, not re-analyze the
problem.

The prompt's problem-statement input depends on the Step 1 mode:

- **Issue-input mode** — pass the parent issue body fetched at
  Step 2 (the `## What` / `## Why` / `## Acceptance Criteria` the
  user filed).
- **Bare-prompt mode** — pass a brief What/Why/AC synthesized
  from the planning conversation (the bare prompt itself, plus
  any clarification the user added during Step 4 discussion).

Example prompt structure:

> "Given the following problem statement and the agreed
> implementation approach, decompose the work into ordered tasks
> with dependencies, approach, and file targets. The problem is
> already understood — focus on structuring the implementation.
>
> [Problem statement — `## What` / `## Why` / `## Acceptance
> Criteria` — either from the parent issue body (issue-input
> mode) or synthesized from the conversation (bare-prompt mode)]
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

The title flows downstream into the branch name (via
`branch_name`), the PR title, the commit subject, and every
user-visible surface.

- **Issue-input mode** — the existing issue's title is what
  flow-start reads; this skill does not rewrite it. The title
  fetched in Step 2 is the title that downstream consumers will
  see.
- **Bare-prompt mode** — synthesize a title from the bare prompt
  and the planning conversation. It must pass the plain-English
  test applied at `/flow:flow-explore` filing time.

**Required.** Subject + verb + object as a stakeholder would say
it out loud. A non-contributor reading the title in a release-notes
feed should understand what the change is for without consulting
the codebase.

**Forbidden.** Code symbols, function names, file paths, line
numbers, internal acronyms without expansion, one-letter shorthand,
repo-specific jargon.

### Reconstruct Issue Body

Build the issue body in working memory. The reconstruction is
mode-dependent.

- **Issue-input mode** — start from the issue body fetched at
  Step 2. Split the body into three pieces using the FLOW-PLAN
  sentinel pair (refer to them by paraphrase here — the literal
  HTML-comment marker strings live only at their canonical
  positions inside the issue body, per the discipline below):
  1. **Prefix** — everything above the first opening FLOW-PLAN
     sentinel (or the whole body if the body contains no
     sentinel). The user's `## What` / `## Why` /
     `## Acceptance Criteria` (or whatever shape the issue body
     had) lives here.
  2. **Old plan block** — the content between the first opening
     sentinel and the matching closing sentinel. Discard this
     entire block; it is the prior implementation plan that the
     fresh decompose output replaces.
  3. **Suffix** — anything below the closing FLOW-PLAN sentinel
     (if present). Preserve verbatim. Engineers and PMs
     sometimes add post-plan notes ("## Discussion Notes",
     "## Open Questions", "## Acceptance Updates") below the
     plan block; those additions must survive re-planning.

  Reassemble the body as `<prefix>` + fresh sentinel-wrapped
  `## Implementation Plan` block + `<suffix>` (concatenated with
  blank-line separators so each section's `## ` heading begins on
  its own line). Do NOT rewrite or summarize the preserved
  pieces; only the sentinel-delimited plan block changes. Re-
  planning is idempotent by construction: every re-plan preserves
  the same prefix AND the same suffix while swapping only the
  plan block.
- **Bare-prompt mode** — synthesize a brief `## What` / `## Why`
  / `## Acceptance Criteria` from the planning conversation (the
  bare prompt the user typed plus any clarification surfaced
  during Step 4 discussion). Append the freshly-decomposed
  sentinel-wrapped `## Implementation Plan` after the Acceptance
  Criteria section. There is no prior body to preserve.

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

### Plan Review

The drafted Implementation Plan has been transformed, combined,
and pre-scanned by the orchestrating model — every check so far
has run *on the orchestrating model's own output*. This subsection
adds a cognitively isolated rule-adherence review before the issue
is filed or edited. The reviewer audits the drafted plan against
the `.claude/rules/` corpus; on `re-decompose` the plan is re-derived
through `decompose:decompose` (never hand-patched) and re-reviewed,
capped at 3 attempts.

Invoke `flow:plan-reviewer` via the **Agent tool** with
`subagent_type: "flow:plan-reviewer"` (the canonical sub-agent
dispatch shape — matching the flow-review and flow-learn agent
invocations; the Skill tool is reserved for skill-to-skill
dispatch, not sub-agent dispatch). The prompt is a three-block
payload: the drafted Implementation Plan body verbatim, the
parent vanilla issue's Acceptance Criteria verbatim, and an
absolute path to the `.claude/rules/` directory. Explicitly
state that no conversation context is provided — the agent must
reason from the artifacts alone.

Substitute `<project_root>` with the absolute path returned by
`pwd` at session start; the bare relative `.claude/rules/` would
resolve against the sub-agent's cwd, which is not guaranteed to
equal the project root.

The prompt structure:

```text
You are reviewing a drafted Implementation Plan for rule
adherence. No conversation context is provided.

DRAFTED_PLAN:
<the full drafted Implementation Plan body verbatim, in full —
copy every line including Context, Exploration, Risks, Approach,
Dependency Graph, and Tasks; do NOT summarize or paraphrase>

ACCEPTANCE_CRITERIA:
<the parent vanilla issue's Acceptance Criteria section, verbatim>

RULES_DIR: <project_root>/.claude/rules/
```

Parse the agent's response. First check for the literal
`## END-OF-FINDINGS` marker as the final structural element. If
the marker is ABSENT, the agent was truncated by `maxTurns`
exhaustion (per `.claude/rules/cognitive-isolation.md` "Context
Budget + Truncation Recovery"). Re-invoke the agent with a
narrower scope partitioned by rule-family (e.g., split
`.claude/rules/` into `architecture/` + `correctness/` + the
rest, one re-invocation per partition), then combine findings
across runs. If a re-invocation itself returns without the
marker, that is double-truncation — record the truncation in
the final user-visible violations block rather than splitting
infinitely.

When the marker is present, parse `VERDICT:` from the response.

- **`VERDICT: pass`** → the plan satisfies the project rules. Fall
  through to `Validate the Body` unchanged.
- **`VERDICT: re-decompose`** → at least one component is
  unmotivated or one applicable rule is violated. Capture the
  `Violations:` block from the agent's output and run the
  re-decompose loop below.

<HARD-GATE>
Do NOT proceed to `Validate the Body` when the verdict is
`re-decompose`. Do NOT hand-patch the plan to address violations.
The plan must be re-derived through `decompose:decompose` with
the violations fed back as input. Hand-patching defeats the
cognitive-isolation contract — the orchestrating model would be
editing its own output to satisfy the reviewer, reintroducing the
bias the gate exists to break.
</HARD-GATE>

#### Plan-Reviewer Loop (max 3 attempts)

When the verdict is `re-decompose`, run a bounded retry loop
mirroring the Validator Auto-Fix Loop shape below. The cap is
**3 attempts** including the first failed review.

For each retry attempt:

1. Construct a fresh `decompose:decompose` prompt that includes
   the parent vanilla issue context, the prior plan synthesis,
   and the `Violations:` block returned by the reviewer.
2. Invoke `decompose:decompose` via the Skill tool and wait for
   the new synthesis.
3. Re-run `Transform + Draft` on the new synthesis to produce a
   revised Implementation Plan.
4. Re-run the Pre-Draft scans on the revised body.
5. Re-invoke `flow:plan-reviewer` against the revised plan via
   the Agent tool with `subagent_type: "flow:plan-reviewer"`.
6. Check for the `## END-OF-FINDINGS` marker. If absent, re-invoke
   with narrower scope per the truncation-recovery path above
   before parsing `VERDICT:`.
7. Parse `VERDICT:` again. On `pass` exit the loop and fall
   through to `Validate the Body`. On `re-decompose`
   increment the attempt counter and continue.

After 3 failed reviews, clear the utility-in-progress marker,
halt the skill with the structured error envelope, print the
final `Violations:` block, and print the COMPLETE-FAILED banner.
Do NOT file or edit the issue. Do NOT loop further.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

````markdown
```json
{"status":"error","reason":"plan_reviewer_max_retries","attempts":3}
```
````

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✗ FLOW v2.4.0 — flow:flow-plan — COMPLETE-FAILED
  Plan reviewer rejected the draft 3 times. Issue not filed.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Render the final `Violations:` block verbatim so the user sees
which rules the plan repeatedly violated. The user can then
decide whether to invoke `/flow:flow-plan #N` again with
additional discussion-mode context, abort the planning attempt,
or escalate the rule design (if a rule itself is at fault).

### Validate the Body

Write the reconstructed body to `.flow-issue-body-<id>` using the
Write tool. Per `.claude/rules/filing-issues.md` "The Pattern":
when invoked inside an active FLOW worktree, prepend the worktree
absolute path so the `validate-worktree-paths` hook allows the
Write. When invoked outside a worktree, the relative form
resolves cleanly because Write and `bin/flow issue` both target
the same project root.

Validate the body file through the pre-filing validator with
`--mode decomposed` before either filing the new issue or editing
the existing one. The validator runs the same sentinel-extraction
logic that `bin/flow plan-from-issue` applies at flow-start; any
body that fails this gate is unconsumable downstream and must NOT
be sent to GitHub:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --mode decomposed --body-file .flow-issue-body-<id>
```

Parse the JSON output. If `status` is `ok`, proceed to the
mode-specific filing branch below. If `status` is `error`, run
the bounded auto-fix loop:

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
edit any issue. Do NOT loop further.

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
  ✗ FLOW v2.4.0 — flow:flow-plan — COMPLETE-FAILED
  Validator rejected the body 5 times. Issue not filed.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

### File or Edit (branch on Step 1 mode)

Once the validator returns `ok`, branch on the Step 1 mode.

**Issue-input mode — edit #N in place.** Push the rebuilt body
into the same issue, re-applying the `decomposed` label (the
label may already be present; `--add-label` is idempotent).
Substitute the issue number (the `#N` the user passed at Step 1)
for `<N>`:

```bash
gh issue edit <N> --body-file .flow-issue-body-<id> --add-label decomposed
```

When `gh issue edit` returns a non-zero exit status (transient
network failure, auth scope mismatch, the issue closed between
Step 2 and now, etc.), the body update may have partially landed
— gh writes the body before applying labels, so a mid-call
failure can leave the issue with the new body but the
`decomposed` label still missing, making it invisible to
`flow-issues` / `flow-orchestrate`. Surface the gh exit status
and stderr inline to the user with a concrete recovery command,
then clear the utility-in-progress marker and stop. Do NOT
retry the edit silently. The recovery command shape:

> "`gh issue edit <N>` failed. Inspect via
> `gh issue view <N> --json body,labels`. If the body landed but
> `decomposed` is missing, run
> `gh issue edit <N> --add-label decomposed`. If the body did not
> land, the `.flow-issue-body-<id>` file is still on disk —
> re-run `gh issue edit <N> --body-file
> .flow-issue-body-<id> --add-label decomposed`."

`gh issue edit` does not auto-delete the body file. Delete the
temp file explicitly via the Write tool by writing an empty
string to `.flow-issue-body-<id>`, then ignore the empty file —
the worktree cleanup at Phase 5 Complete handles disposal. (The
Bash permission allow-list refuses ad-hoc `rm` calls during a
flow per `.claude/rules/permissions.md`; rewriting to empty is
the sanctioned alternative.) Skip this empty-write step when
`gh issue edit` failed — preserving the body file on disk gives
the user a concrete artifact to retry the edit against.

Capture the issue's URL from the `gh issue view` JSON fetched at
Step 2 (the `url` field), then record the issue in the state file
(no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-plan
```

**Bare-prompt mode — file a new issue.** File against the
current repo (no `--repo` flag — `flow-plan` always files where
the user is) WITH the `decomposed` label so `flow-issues` and
`flow-orchestrate` recognize it as ready-for-flow-start work,
and WITH `--assignee @me` so the new decomposed issue is
assigned to the planner who ran `flow-plan`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id> --label decomposed --assignee @me
```

Capture both the new issue's URL and number from the filer's
output. `bin/flow issue` returns a JSON envelope including the
full GitHub URL of the issue it created; the trailing
`/issues/M` segment of that URL gives the **decomposed issue
number M**. Keep both the full URL (as `<issue_url>`) and the
number for the recording call below — bare-prompt mode has no
Step 2 fetch to pull these from.

Record the issue in the state file (no-op if no FLOW feature is
active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<issue_title>" --url "<issue_url>" --phase flow-plan
```

### Finish

Clear the utility-in-progress marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-plan
```

Display the issue URL to the user, then output the COMPLETE
banner in your response (not via Bash) inside a fenced code
block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.4.0 — flow:flow-plan — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then instruct the user. The message names the same issue number
the next phase will pick up.

- **Issue-input mode:** "Re-planned issue #N in place. To start
  implementing, run `/flow:flow-start #N`."
- **Bare-prompt mode:** "Filed decomposed issue #M. To start
  implementing, run `/flow:flow-start #M`."

Do not invoke `/flow:flow-start` yourself — the user types the
slash command directly.

---

## Hard Rules

- **Accept `#N` or a bare prompt.** Step 1 routes `#N` to
  issue-input mode (Step 2 fetches the issue and Step 6 edits it
  in place) and bare non-empty prompts to bare-prompt mode (Step 2
  is skipped; Step 6 synthesizes a brief What/Why/AC and files
  one new decomposed issue).
- **Issue-input mode edits #N in place; bare-prompt mode files
  one new issue; never close a parent issue.** Issue-input mode
  preserves every byte above the opening FLOW-PLAN sentinel
  (including the original `## What` / `## Why` /
  `## Acceptance Criteria`) and swaps the sentinel-delimited
  plan block. Bare-prompt mode files one new decomposed issue
  with `--label decomposed --assignee @me`. There is no
  parent-closure step in either mode.
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
- All `bin/flow` calls in skill bash blocks use the plugin root
  prefix — bare `bin/flow` only resolves inside the FLOW repo
  itself, not in target projects.
