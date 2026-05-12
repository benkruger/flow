---
name: flow-explore
description: "Open a problem-statement conversation. Stays in discussion mode with PM as default voice; on user signal, files a vanilla What/Why/Acceptance Criteria issue against the current repo. Usage: /flow:flow-explore <topic>"
---

# Flow Explore

Open a problem-statement conversation about something the user wants
to build, fix, or change. The skill stays in discussion mode by
default — surfacing clarifying questions, exploring prior issues,
identifying the user-visible outcome — and dispatches to PM, Tech
Lead, or CTO planning sub-agents only on explicit user request. When
the user signals "ready" or "file it", the skill captures the agreed
problem statement as a vanilla GitHub issue (`## What`, `## Why`,
`## Acceptance Criteria`) without an Implementation Plan.

The output is a problem statement, not a design. Implementation
decomposition belongs in `/flow:flow-plan #N`, which a Tech Lead
runs against the issue this skill files.

## Usage

```text
/flow:flow-explore <topic>
```

The `<topic>` argument names what the user wants to discuss — a
behavior change, a missing feature, a user-visible bug, a copy
adjustment. The skill takes no other flags or arguments.

## Concurrency

This skill creates shared GitHub state (issues) only at the very
end, on explicit user approval. Issue creation is idempotent by
title — if an issue with the same title already exists, the user
should be warned before filing a duplicate.

The only intermediate side effect is the per-session
utility-in-progress marker (scoped to the user's Claude home, not
the project), which lets the Stop hook refuse turn-end while the
discussion-mode skill is running.

Multiple `/flow:flow-explore` sessions on the same machine in
different terminal windows are independent — each has its own
session id, its own marker, its own conversation context.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-explore — STARTING
──────────────────────────────────────────────────
```
````

Immediately after the banner, write the per-session "utility skill
in progress" marker so the Stop hook refuses turn-end while this
skill is running. Without the marker the model returns control to
the user when a planning sub-agent Skill tool returns mid-pipeline
at Step 4, breaking the unattended-discussion contract this skill
promises.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-explore
```

If the marker-write call returns `status: error` with
`no session_id available`, the skill proceeds without the marker.
The Stop hook treats a missing marker as a non-block, so the skill
runs without protection but does not break.

The marker is held across the entire discussion until the user
signals "ready" or "file it" (Step 5), invokes Cancel from Step 1
when the topic argument is missing, or selects Cancel from the
filing AskUserQuestion in Step 5. Every skill-exit boundary clears
the marker so the Stop hook releases turn-end after the skill
completes.

---

## Step 1 — Conversation Gate

Verify that a topic argument was provided after the slash command.
The topic is what the problem-statement conversation is about;
without it the skill has no anchor for the discussion.

<HARD-GATE>

If no topic argument was provided, clear the utility-in-progress
marker so the Stop hook does not refuse turn-end after the
rejection, then output the usage guidance and stop:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-explore
```

> "Topic required. Usage: `/flow:flow-explore <topic>` where
> `<topic>` names what you want to discuss — a behavior change,
> a missing feature, a user-visible bug, or a copy adjustment."

Do not proceed to Step 2, propose direct edits, commit changes, or
take any action outside this skill without a topic argument.

</HARD-GATE>

---

## Step 2 — Role Read

Resolve the project root and read `.flow.json` from it. The file is
gitignored and lives only at the main repo root — never in a linked
worktree — so the read must target the main repo path regardless
of whether the skill was invoked outside a worktree or from inside
one.

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

The default voice for `/flow:flow-explore` is the **Product
Manager**. The skill's purpose — capturing what's broken and why
it matters in user-visible terms — is the PM's natural domain.
Map the user's `role` field to a complementary suggestion only:

| `.flow.json` role value | Conversation note |
|---|---|
| `"pm"` | The user IS a PM — proceed straight into PM-voice discussion without a role-suggestion preamble. |
| `"tech-lead"` | The user is a Tech Lead — note once that this skill captures problem statements (PM voice) and that implementation discussion belongs in `/flow:flow-plan #N` after filing. |
| `"founder-solo"` | The user wears multiple hats — proceed into PM-voice discussion; offer to invite the Tech Lead voice via Step 4 if the conversation drifts into design. |
| Absent / unknown / read failure | Default to PM voice silently. |

The note is conversational, not gated. The discussion-mode entry
in Step 3 runs the same way for every role, and any sub-agent
dispatch in Step 4 still requires an explicit user request.

---

## Step 3 — Discussion Mode

The exploration room exists so the user can articulate the problem
out loud against a collaborator who reads prior issues, asks
clarifying questions, and helps name the user-visible outcome.
Discussion mode is the default posture — the skill stays here until
the user explicitly asks for a persona dispatch (Step 4) or signals
that the problem statement is ready to file (Step 5).

In this step, the skill:

- Surfaces clarifying questions about the topic — what user-visible
  outcome is wanted, what's broken or missing today, who is
  affected, what success looks like.
- Reads prior GitHub issues (via `gh issue view` and
  `gh issue list`) when the topic touches existing work, to
  ground the conversation in what the project has already
  considered.
- Identifies acceptance criteria — binary, testable conditions
  that distinguish "done" from "not done".
- Iterates with the user across multiple turns — challenging the
  proposed framing, summarizing what has been agreed, asking
  what's still unclear.

<HARD-GATE>

Discussion mode forbids action AND forbids implementation work.
While in this step, the skill must NOT:

- **Read source code.** This skill captures problem statements,
  not designs. Reading `src/` or production code shifts the
  discussion into Tech Lead territory and produces an issue body
  that smuggles in implementation details. When the user asks
  about how a thing works internally, redirect: "That's an
  implementation question for `/flow:flow-plan #N` after we file
  the problem statement here." Reading prior GitHub issues via
  `gh issue view` is fine — those are user-visible artifacts.
- **Never invoke `decompose:decompose`.** Decomposition is
  implementation work and belongs in `/flow:flow-plan #N`.
- **Write FLOW-PLAN sentinel markers** (`<!-- FLOW-PLAN-BEGIN -->`
  / `<!-- FLOW-PLAN-END -->`) anywhere in the issue body or
  inline output. Vanilla bodies must not contain sentinels;
  `bin/flow validate-issue-body --mode vanilla` rejects bodies
  that carry them.
- **Compose an `## Implementation Plan` heading.** Vanilla
  bodies must not contain that heading; the validator rejects
  bodies that do.
- Propose direct edits to source files, configuration, or rules
  — this is a thinking room, not a coding room.
- Commit changes or invoke `/flow:flow-commit`.
- File issues outside the explicit Step 5 wrap-up.
- Use `AskUserQuestion` to manufacture a checkpoint the user did
  not ask for. The discussion is conversational; the user drives
  the cadence by sending messages. Per
  `.claude/rules/autonomous-phase-discipline.md`, never
  self-impose a pause via `AskUserQuestion`.
- Auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch (Step 4) requires the user to type the request
  in plain English.

Stay in discussion mode until the user types one of: a persona
request (then proceed to Step 4 for the named persona), a
hand-off signal ("ready", "file it", "let's go") (then proceed to
Step 5), or any other prose (then continue the conversation in
discussion mode).

</HARD-GATE>

---

## Step 4 — Persona Dispatch

When the user explicitly asks for a planning persona's view ("PM
view?", "What does Tech Lead think?", "CTO take?"), summarize the
discussion so far and dispatch to the named sub-agent. The skill
remains the orchestrator; the sub-agent returns a structured
analysis or a refusal block, and the skill renders the result.

### Summarize for the agent prompt

Build the agent prompt with two labeled sections:

- **CONVERSATION_SUMMARY** — a synthesis of the problem-statement
  discussion so far. Name the topic, the constraints surfaced, the
  user-visible outcome the user has indicated.
- **PROPOSED_PROBLEM_STATEMENT** — the concrete problem under
  discussion in user-visible terms. Name what's broken or missing,
  who is affected, and what success looks like.

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
  without the user's explicit go-ahead.
- Re-invoke the same agent with softer framing.
- Perform the refused analysis personally.

Present the refusal block to the user and ask them how to proceed.
If the user says "escalate to Tech Lead" or "ask the CTO", the
next persona dispatch fires in a fresh Step 4 invocation. If the
user wants to discuss the refusal in plain language, return to
Step 3.

</HARD-GATE>

When the agent returns an in-scope analysis (not a refusal), render
it verbatim and return to Step 3 so the user can react, ask
follow-ups, or request another persona's view.

---

## Step 5 — Wrap-up: Capture, Validate, File

When the user signals readiness — "ready", "file it", "let's go",
"create the issue", or any equivalent phrasing — capture the agreed
problem statement, validate it, file it as a vanilla GitHub issue,
and clear the marker.

### Capture

Generate a short session ID by running
`${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool.
This ID scopes the body file path
(`.flow-issue-body-<id>`) so concurrent
`/flow:flow-explore` invocations cannot collide on the same temp
file.

**Capture the problem-statement sections** from the conversation
context. Synthesize the discussion into these structured sections
in working memory — do not re-analyze or re-explore, just distill
what was already discussed:

- **What** — Plain-English description of the user-visible problem
  or desired outcome. What is broken, missing, or inadequate.
  Subject + verb + object as a stakeholder would say it out loud.
- **Why** — Why this matters. The business reason, the user
  impact, the risk of not addressing it.
- **Acceptance Criteria** — Binary, testable conditions. Pass/fail
  with no subjective judgment. The list a Tech Lead can hand to a
  test author and reach unambiguous agreement on "done".

### Title Authoring

The issue title flows downstream into the branch name (via
`branch_name`), the PR title, the commit subject, and every
user-visible surface. Titles must read as plain English to a
stakeholder who is not a contributor.

**Required.** Subject + verb + object as a reader would say it out
loud. A non-contributor reading the title in a release-notes feed
should understand what the change is for without consulting the
codebase.

**Forbidden.** The following must not appear in the title:

- Code symbols, function names, identifiers.
- Field names, file paths, `module::function` references.
- Line numbers.
- Internal acronyms without expansion.
- One-letter shorthand (`X-of-Y`, `M of N`).
- Repo-specific jargon, internal product code-names.

### Pre-Draft Backwards-Reasoning Scan

Before presenting the draft, scan the body for the following
forbidden phrasings, which ground the current decision in a
historical artifact rather than the code's current merits:

- `"PR #<N> decided"`, `"the prior PR chose"`, `"the previous
  commit"` — historical decision cited as authority
- `"kept for backward compatibility"`, `"compat shim"`, `"legacy
  alias for older"` — preservation justified by inherited
  reasoning rather than a current consumer
- `"older plugin versions"`, `"prior plugin"` — plugin-version-
  compat reasoning
- `"as PR #<N> chose to"`, `"following the prior PR"` —
  deferring to past decisions

Evaluate matches in context: a bare `PR #<N>` reference used for
forensic detection (linking blocked-by, naming a specific merge)
is fine; a `PR #<N>` reference used to justify the present design
is forbidden. If any match is justifying-shape rather than
identifier-shape, revise the draft. See
`.claude/rules/no-backwards-reasoning.md`.

### Pre-Draft Include-Bias Scan

Before presenting the draft, scan the body for the following
forbidden phrasings, which signal defensive scope shrinkage rather
than genuine exclusion grounded in a concrete blocker:

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

### Combine into Issue Body

Combine the captured sections into a single issue body in working
memory. The section order must be:

**What** → **Why** → **Acceptance Criteria**

Each top-level section uses `##` headings. The body must NOT
contain FLOW-PLAN sentinel markers and must NOT contain an
`## Implementation Plan` heading — those belong in the decomposed
issue that `/flow:flow-plan #N` files later.

### Draft Presentation

Present the full draft inline in the response — both title and
body. Do not tell the user to look at a file. Render it as a
formatted markdown block so the user can review every detail.

### File AskUserQuestion Gate

<HARD-GATE>

After presenting the draft, ask the user to confirm via
AskUserQuestion with structured parameters:

- **question**: "Review the draft above. Ready to file?"
- **header**: "File Issue"
- **options**:
  - label: "File issue", description: "File against the current repo as a vanilla problem-statement issue"
  - label: "Revise draft", description: "Edit the draft based on your feedback"
  - label: "Cancel", description: "Stop without filing an issue"

Do not file the issue, propose direct edits, commit changes, or
take any action outside this skill without explicit user approval
via AskUserQuestion — even if the answer appears obvious from
context.

**If "File issue"** → proceed to Validate + File below.

**If "Revise draft"** → revise based on the user's feedback and
re-present the draft. After revising, re-present the draft and ask
the same AskUserQuestion. Iterate as many times as needed.

**If "Cancel"** → clear the utility-in-progress marker, then stop
without filing. Do not write the body file. Do not output the
COMPLETE banner.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-explore
```

</HARD-GATE>

### Validate + File

Write the issue body to `.flow-issue-body-<id>` using the Write
tool. Per `.claude/rules/filing-issues.md` "The Pattern": when
invoked inside an active FLOW worktree, prepend the worktree
absolute path so the `validate-worktree-paths` hook allows the
Write. When invoked outside a worktree, the relative form
resolves cleanly because Write and `bin/flow issue` both target
the same project root.

Validate the body file through the pre-filing validator with
`--mode vanilla` before asking the filer subcommand to send it to
GitHub:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --mode vanilla --body-file .flow-issue-body-<id>
```

Parse the JSON output. If `status` is `ok`, proceed to the filer
invocation below. If `status` is `error`, do NOT file the issue.
Show the validator's `message` field to the user, return to the
Revise loop in the File AskUserQuestion Gate above with the user's
feedback set to the validator's `message`, and re-present the
corrected draft. Iterate until the validator returns `ok`.

Once the validator returns `ok`, file the issue against the
current repo (no `--repo` flag — `flow-explore` always files where
the user is) and **without** `--label decomposed` (this is a
vanilla problem-statement, not a pre-planned issue):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --title "<issue_title>" --body-file .flow-issue-body-<id>
```

Capture the returned issue URL.

Record the issue in the state file (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --title "<issue_title>" --url "<issue_url>" --phase flow-explore
```

Clear the utility-in-progress marker:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-explore
```

Display the issue URL to the user, then output the COMPLETE banner
in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-explore — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then instruct the user on the next step:

> "Filed issue #N. To plan the implementation, run
> `/flow:flow-plan #N` — a Tech Lead conversation that decomposes
> the problem statement into an implementation plan and files a
> linked decomposed issue ready for `/flow:flow-start`."

Do not invoke `/flow:flow-plan` yourself — the user types the
slash command directly.

---

## Hard Rules

- **Never read source code during discussion.** This skill captures
  problem statements, not designs. Reading `src/`,
  production code, or repo-tracked implementation files shifts the
  conversation into Tech Lead territory. Reading prior GitHub
  issues via `gh issue view` is fine — those are user-visible
  artifacts.
- **Never invoke `decompose:decompose`.** Decomposition is
  implementation work and belongs in `/flow:flow-plan #N`.
- **Never write FLOW-PLAN sentinel markers** in issue bodies or
  inline output. Vanilla bodies must not contain sentinels.
- **Never compose an `## Implementation Plan` heading.** Vanilla
  bodies must not contain that heading.
- **Never apply `--label decomposed`** when filing. Vanilla
  issues are problem statements; the decomposed label is reserved
  for issues filed by `/flow:flow-plan #N`.
- Never present draft issue bodies inline before reaching Step 5
  Wrap-up. Discussion produces context for the wrap-up; rendering
  a markdown-block draft with formal section headings during
  discussion mode is forbidden.
- Never use `AskUserQuestion` during discussion mode. The
  discussion is conversational; the user drives the cadence.
- Never auto-dispatch to a planning sub-agent on inferred scope.
  Persona dispatch requires the user to type the request in plain
  English.
- Never auto-escalate, re-invoke with softer framing, or perform
  the refused analysis personally when a sub-agent returns a
  `## SCOPE REFUSAL` block. Render the refusal verbatim and wait
  for explicit user direction on the next move.
- Never file an issue without explicit user approval — the
  AskUserQuestion in Step 5 is the mandatory gate.
- Never tell the user to "look at" a file — render all content
  inline.
- Never use Bash to print banners — output them as text in your
  response.
- The issue body must be self-contained — a fresh session with no
  memory of this conversation must be able to read it and
  understand the problem.
- Always use the Write tool to create body files
  (`.flow-issue-body-<id>`) — never pass body text as a CLI
  argument.
- Never delete the body file — the `bin/flow issue` script
  handles cleanup.
- All `bin/flow` calls use `${CLAUDE_PLUGIN_ROOT}/bin/flow` — bare
  `bin/flow` only resolves inside the FLOW repo itself, not in
  target projects.
