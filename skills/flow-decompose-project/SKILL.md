---
name: flow-decompose-project
description: "Decompose a large project into GitHub issues with sub-issue and blocked-by relationships, milestones, and phase labels."
---

# Flow Decompose Project

Decompose a large project into many GitHub issues with native sub-issue
relationships, blocked-by dependencies, milestones, and phase labels.
Produces a fully linked issue graph ready for autonomous execution via
`/flow:flow-start` or `/flow:flow-orchestrate`.

## Usage

```text
/flow:flow-decompose-project <project description>
/flow:flow-decompose-project --step 2 --id <id>
/flow:flow-decompose-project --step 3 --id <id>
/flow:flow-decompose-project --step 4 --id <id>
/flow:flow-decompose-project --step 5 --id <id>
/flow:flow-decompose-project --step 6 --id <id>
```

- `/flow:flow-decompose-project <project description>` — start from Step 1
- `/flow:flow-decompose-project --step N --id <id>` — self-invocation: resume at Step N

<HARD-GATE>
Do NOT proceed if no arguments were provided after the command (excluding flags).
Output this error message and stop:

> "Project description required. Usage: `/flow:flow-decompose-project <project description>`"

No interactive prompt. The user re-runs the command with arguments.

</HARD-GATE>

## Concurrency

This skill creates shared GitHub state (issues, milestones, labels,
sub-issue relationships, dependencies). Session state is tracked in
`.flow-states/decompose-project-<id>.json` with a unique ID per session
to prevent concurrent collisions. Issue creation is idempotent by title.

## Step Dispatch

If `--step N --id <id>` was passed, this is a self-invocation from a
previous step. The `--id` flag carries the session-scoped identifier
generated in Step 1. Skip the Announce banner and jump directly to the
Resume Check, using the provided `<id>` for all file paths.

If no `--step` flag was passed, proceed to Announce and then Step 1.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-decompose-project — STARTING
──────────────────────────────────────────────────
```
````

Immediately after the banner, write the per-session "utility skill
in progress" marker so the Stop hook refuses turn-end while this
skill is running. Without the marker the model returns control to
the user when the decompose:decompose Skill tool returns
mid-pipeline at Step 1, breaking the unattended-flow contract this
skill promises across its six-step chain.

Rust resolves the active session_id at the CLI boundary by reading
the `CLAUDE_CODE_SESSION_ID` env var Claude Code supplies to every
Bash subprocess (Claude Code 2.1.132+); on older Claude Code
installs it falls back to the SessionStart capture file. On
2.1.132+ the per-subprocess env value matches what the Stop hook
receives in its stdin payload, so set-time and clear-time resolve
to the same id regardless of concurrent Claude Code activity. The
bash invocation below passes `--skill` only; Rust supplies the
session_id itself.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-utility-in-progress --skill flow:flow-decompose-project
```

If the marker-write call returns `status: error` with
`no session_id available` (no env var AND no capture file — rare,
only on Claude Code installs without per-subprocess env support and
without a SessionStart capture file), the skill proceeds without
the marker. The Stop hook treats a missing marker as a non-block,
so the skill runs without protection but does not break.

The marker is held across the entire Step 1 → Step 2 → Step 3 →
Step 4 → Step 5 → Step 6 chain. Step Dispatch (above) skips the
Announce banner on `--step N` self-invocations, so the marker-set
call fires exactly once at the first invocation. The marker is
cleared at every skill-exit boundary: Step 1 Cancel, Step 2
Cancel, Step 3 epic Cancel-filing-this-issue, Step 3 epic
Cancel-whole-skill, Step 4 child Cancel-whole-skill, and the
Step 6 success path. Every other path holds the marker until
Step 6 completes.

On Claude Code installs without the per-subprocess env var, the
capture-file fallback resolves session_id independently at set and
clear time. A second Claude Code session whose SessionStart hook
overwrites the capture file between this skill's set and clear
calls can leave the marker orphaned at the original id. Recovery
is `rm ~/.claude/flow/utility-in-progress-*.json` after the skill
completes; the Stop hook treats a missing marker as a non-block.

## Resume Check

Use the Read tool to read `.flow-states/decompose-project-<id>.json`, where
`<id>` is the session identifier from the `--id` flag. If no `--id` flag
was passed (first run), there is no file to read — proceed to Step 1.

Dispatch based on `decompose_step`:

- `0` or absent — proceed to Step 1
- `1` — skip to Step 2
- `2` — skip to Step 3
- `3` — skip to Step 4
- `4` — skip to Step 5
- `5` — skip to Step 6

---

## Step 1 — Describe and Decompose

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 1 of 6: Describe and Decompose ──
```
````

Take the user's project description and invoke the `decompose:decompose`
plugin via the Skill tool. The decomposition must include deep codebase
exploration using Glob, Grep, and Read to ground every finding.

Present the full DAG synthesis to the user.

<HARD-GATE>

Ask the user to review the decomposition using AskUserQuestion:

- **"Proceed to review"** → generate a session ID by running
  `${CLAUDE_PLUGIN_ROOT}/bin/flow generate-id` via the Bash tool.
  Write `{"decompose_step": 1}` to
  `.flow-states/decompose-project-<id>.json` using the Write tool.
  Save the full decompose output to
  `.flow-states/decompose-project-<id>-dag.md` using the Write tool.
  Then invoke `flow:flow-decompose-project --step 2 --id <id>` using
  the Skill tool as your final action.
- **"Iterate"** → re-invoke `decompose:decompose` with feedback, present
  the updated synthesis, and ask again.
- **"Cancel"** → clear the utility-in-progress marker so the Stop
  hook does not refuse turn-end after cancellation, then stop. Do
  not file any issues.

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-decompose-project
  ```

Do not proceed to Step 2 without explicit user approval. Do not propose
direct edits, commit changes, or take any action outside this skill.

</HARD-GATE>

---

## Step 2 — Review Issue List

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 2 of 6: Review Issue List ──
```
````

Use the Read tool to read the DAG output from
`.flow-states/decompose-project-<id>-dag.md`.

From the DAG synthesis, build a complete issue list:

1. **Parent epic** — a single umbrella issue describing the full project
2. **Child issues** — one per DAG leaf task, in topological order (leaves
   first so dependencies exist when referenced)
3. **Phase labels** — auto-derive from DAG groupings (e.g., "Phase 1: API",
   "Phase 2: SPA"). Each child issue gets a phase label.

For both the parent epic AND each child issue, draft:

- **Title** — concise, actionable
- **Body** — see the Body Shape Contract below. The same contract
  applies to the parent epic and to every child issue — this skill
  is the single source of truth for body shape, and Steps 3 and 4
  just write the bytes that this step produces.
- **Labels** — `decomposed` plus the auto-derived phase label
  (child issues only; the parent epic is filed without these
  labels in Step 3)
- **Dependencies** — which other child issues this depends on
  (by title, resolved to numbers in Step 4); the parent epic has
  no dependencies

### Body Shape Contract

Every issue body — parent epic AND every child — uses this section
order:

1. **Problem** (`## Summary` heading) — what is broken, missing, or
   inadequate. Include observable behavior, evidence from the
   codebase (file paths, line numbers), and user impact. Grounded
   in the exploration the decompose step already performed.
2. **Acceptance Criteria** — binary, testable conditions. Pass/fail
   with no subjective judgment.
3. **Implementation Plan** — wrapped in the FLOW-PLAN sentinel
   pair (see the wrapping rule below) and containing these
   `###` subsections in order:
   - **Context** — what the user wants to build and why
   - **Exploration** — what exists in the codebase, affected
     files, patterns discovered
   - **Risks** — what could go wrong, edge cases, constraints
   - **Approach** — the chosen approach and rationale
   - **Dependency Graph** — table of tasks with types and
     dependencies
   - **Tasks** — ordered implementation tasks using `#### Task N:`
     headers (these become `### Task N:` headings in the
     `.flow-states/<branch>/plan.md` file after `bin/flow
     plan-from-issue` extraction). The `#### Task N:` header
     format — not a numbered list — is the heading shape
     `count_tasks` recognises to populate `code_tasks_total`.
   - **Acceptance Criteria** — binary, testable conditions for
     the implementation
4. **Files to Investigate** — real file paths verified during
   decomposition with a brief note on why each is relevant.
5. **Context** — business reason, architectural constraints, or
   design decisions.

**Wrap the Implementation Plan in the FLOW-PLAN sentinel pair.**
Place the literal HTML comment `<!-- FLOW-PLAN-BEGIN -->` on its
own line immediately before the `## Implementation Plan` heading,
and the literal HTML comment `<!-- FLOW-PLAN-END -->` on its own
line immediately after the last Task entry (before the next `##`
heading). The sentinel pair delimits the bytes that `bin/flow
plan-from-issue` extracts verbatim and writes to
`.flow-states/<branch>/plan.md` when the issue is later picked up
via `/flow:flow-start #N`. Without the sentinel pair,
`plan-from-issue` rejects the issue with `plan_markers_missing`
and the flow halts.

**Paraphrase every prose reference to the plan-sentinel pair.**
The literal HTML-comment marker strings only appear in each body
at two positions — the opening sentinel and the closing sentinel.
They must never appear inside prose, headings, code blocks,
examples, or any other surface of the body. `bin/flow
plan-from-issue` extracts the slice between the FIRST occurrence
of each marker, so a literal marker mid-prose silently redirects
the extraction to the wrong slice — exactly the failure mode
`bin/flow validate-issue-body` exists to detect. Whenever a body
needs to reference the marker pair (for example, when an issue's
topic is the sentinel protocol itself), paraphrase every
reference. Acceptable wording: "the FLOW-PLAN sentinel pair",
"the plan-extraction markers", "the canonical sentinels
delimiting the plan block". The validator's `marker_count_wrong`
branch catches violations downstream; this rule prevents them
upstream so the Revise loop in Step 3 or Step 4 is not entered
unnecessarily.

The wrapped block looks like this in each issue body:

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

### Backwards-Reasoning Scan

After composing each child issue body and before presenting the
issue list, scan every child body for the following forbidden
phrasings, which ground the current decomposition in a historical
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
identifier-shape in any child body, revise that body. Re-evaluate
the underlying decision on the code's current merits, not on
historical context. The scan applies to every child issue
produced by this skill, not just the first one.
See `.claude/rules/no-backwards-reasoning.md`.

### Include-Bias Scan

After composing each child issue body and before presenting the
issue list, scan every child body for the following forbidden
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
adjacent concern surfaced during decomposition belongs as a
task in the appropriate child issue unless one of the narrow
valid exclusions (user explicitly rejected, requires different
design conversation, blocks primary completion) applies. If any
match is exclusion-shape rather than identifier-shape in any
child body, revise that body: convert the deferral into an
inclusion task, or name the concrete blocker in that issue's
Context section as one sentence. The scan applies to every
child issue produced by this skill, not just the first one. The
lifecycle cost of splitting a concern out of an issue is
multiples larger than including it in the current exploration
budget. See `.claude/rules/include-bias-in-issues.md`.

Present the full issue list as a table:

| # | Title | Phase | Depends On |
|---|-------|-------|------------|
| 1 | Epic: <project_name> | — | — |
| 2 | <first leaf task> | Phase 1: API | — |
| 3 | <second task> | Phase 1: API | 2 |

Below the table, show each issue's full body text so the user can
review every detail.

<HARD-GATE>

Ask the user for the milestone due date and approval using AskUserQuestion:

> "Review the issue list above. What is the milestone due date (YYYY-MM-DD)?
> Enter a date to proceed, or 'revise' to make changes."

- **Date provided** → record the due date. Write
  `{"decompose_step": 2, "due_date": "<date>"}` to
  `.flow-states/decompose-project-<id>.json` using the Write tool.
  Save the approved issue list to
  `.flow-states/decompose-project-<id>-issues.json` using the Write
  tool (array of `{title, body, labels, depends_on_indices}` objects).
  Then invoke `flow:flow-decompose-project --step 3 --id <id>` using
  the Skill tool as your final action.
- **"Revise"** → ask what to change, update the list, and re-present.
  Iterate until approved.
- **"Cancel"** → clear the utility-in-progress marker so the Stop
  hook does not refuse turn-end after cancellation, then stop.

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-decompose-project
  ```

Do not proceed to Step 3 without explicit user approval. Do not propose
direct edits, commit changes, or take any action outside this skill.

</HARD-GATE>

---

## Step 3 — Create Epic and Milestone

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 3 of 6: Create Epic and Milestone ──
```
````

Use the Read tool to read the session state from
`.flow-states/decompose-project-<id>.json` to get the `due_date`.
Use the Read tool to read the approved issue list from
`.flow-states/decompose-project-<id>-issues.json`.

Detect the repo:

```bash
git remote get-url origin
```

Parse `owner/repo` from the remote URL.

Create the milestone:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow create-milestone --repo <repo> --title "<project_name>" --due-date <due_date>
```

Parse the JSON output. Record the milestone number.

Create the parent epic issue. The `--milestone` flag accepts the milestone
title (not the numeric ID) — use the same `<project_name>` that was passed
to `create-milestone --title`. Write the epic body to
`.flow-states/decompose-project-<id>-epic-body` using the Write tool.

Validate the epic body through the pre-filing validator before
asking the filer subcommand to send it to GitHub. The validator
runs the same sentinel-extraction logic that `bin/flow
plan-from-issue` applies at flow-start; any body that fails this
gate is unconsumable downstream and must NOT be filed:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --body-file .flow-states/decompose-project-<id>-epic-body
```

Parse the JSON output. If `status` is `ok`, proceed to the filer
invocation below. If `status` is `error`, do NOT file the issue.
Surface the validator's `message` field to the user via
AskUserQuestion with three options:

- **"Revise the epic body and retry"** — ask what to change, edit
  the Write tool output at
  `.flow-states/decompose-project-<id>-epic-body`, then re-run
  `bin/flow validate-issue-body` from the top of this step. Loop
  until the validator returns `status:ok`.
- **"Cancel filing this issue"** — skip the epic-filing path. The
  flow halts at Step 3 since the epic is the parent of every
  child issue; clear the utility-in-progress marker so the Stop
  hook releases turn-end:

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-decompose-project
  ```

- **"Cancel the whole skill"** — same clear-marker call as above,
  then stop without filing any issues.

Once the validator returns `ok`, file the epic:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo <repo> --title "Epic: <project_name>" --body-file .flow-states/decompose-project-<id>-epic-body --milestone "<project_name>"
```

Parse the JSON output. Record the epic issue number and database ID.

Update the session state with milestone and epic info. Write the updated
state to `.flow-states/decompose-project-<id>.json` using the Write tool,
adding `milestone_number`, `epic_number`, and `epic_id` fields.
Set `decompose_step` to `3`.

Then invoke `flow:flow-decompose-project --step 4 --id <id>` using the
Skill tool as your final action.

---

## Step 4 — Create Child Issues

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 4 of 6: Create Child Issues ──
```
````

Use the Read tool to read the session state and approved issue list.

Create each child issue in topological order (leaves first). For each:

Write the child issue body to
`.flow-states/decompose-project-<id>-issue-body` using the Write
tool.

Validate the child body through the pre-filing validator before
asking the filer subcommand to send it to GitHub. The validator
runs the same sentinel-extraction logic that `bin/flow
plan-from-issue` applies at flow-start; any body that fails this
gate is unconsumable downstream and must NOT be filed:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body --body-file .flow-states/decompose-project-<id>-issue-body
```

Parse the JSON output. If `status` is `ok`, proceed to the filer
invocation below. If `status` is `error`, do NOT file this child.
Surface the validator's `message` field to the user via
AskUserQuestion with three options:

- **"Revise this child body and retry"** — ask what to change, edit
  the Write tool output at
  `.flow-states/decompose-project-<id>-issue-body`, then re-run
  `bin/flow validate-issue-body` from the top of this iteration.
  Loop until the validator returns `status:ok`, then continue to
  the filer call for this child.
- **"Skip filing this child"** — record the skip and continue to
  the next child in the topological order. Any sibling child
  whose `depends_on_indices` references this skipped child has
  no `--blocking-number` to pass to `bin/flow link-blocked-by`
  in Step 5, so that dependency edge is silently dropped (Step 5
  is best-effort by design). The Step 6 report surfaces the
  partial coverage; the user can re-run decomposition for the
  missing child later, but the blocked-by graph for already-filed
  siblings will need manual repair via `gh issue edit` or a
  follow-up `bin/flow link-blocked-by` call.
- **"Cancel the whole skill"** — clear the utility-in-progress
  marker so the Stop hook releases turn-end, then stop without
  filing any remaining children:

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-decompose-project
  ```

Once the validator returns `ok` for this child, file it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo <repo> --title "<title>" --body-file .flow-states/decompose-project-<id>-issue-body --label decomposed --milestone "<project_name>"
```

Parse the JSON output and record `{title, number, id}` in the mapping.

Record the issue (no-op if no FLOW feature is active):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label decomposed --title "<title>" --url "<issue_url>" --phase flow-decompose-project
```

After all issues are created, write the complete mapping to the session
state file (`issues` array with `{title, number, id}` objects).
Set `decompose_step` to `4`.

Then invoke `flow:flow-decompose-project --step 5 --id <id>` using the
Skill tool as your final action.

---

## Step 5 — Link Relationships

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 5 of 6: Link Relationships ──
```
````

Use the Read tool to read the session state to get `epic_number` and
the `issues` mapping.

### Sub-issue relationships (children to epic)

For each child issue, link it as a sub-issue of the epic:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow create-sub-issue --repo <repo> --parent-number <epic_number> --child-number <child_number>
```

Best-effort — log failures but continue.

### Blocked-by relationships (between children)

For each child issue that has dependencies, create the blocked-by link:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow link-blocked-by --repo <repo> --blocked-number <child_number> --blocking-number <dep_number>
```

Best-effort — log failures but continue.

Set `decompose_step` to `5` in the session state.

Then invoke `flow:flow-decompose-project --step 6 --id <id>` using the
Skill tool as your final action.

---

## Step 6 — Report

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
  ── Step 6 of 6: Report ──
```
````

Use the Read tool to read the session state.

Present a summary table:

| # | Title | Issue | Phase | Dependencies |
|---|-------|-------|-------|--------------|
| — | Epic: <name> | #N | — | — |
| 1 | <task> | #N | Phase 1 | — |
| 2 | <task> | #N | Phase 1 | #N |

Include:

- Total issues created
- Milestone link
- Sub-issue relationships created (count)
- Blocked-by dependencies created (count)
- Any failures encountered

Clean up the session files:

```bash
rm .flow-states/decompose-project-<id>.json .flow-states/decompose-project-<id>-dag.md .flow-states/decompose-project-<id>-issues.json
```

Clear the utility-in-progress marker so the Stop hook stops refusing
turn-end now that the skill has completed its work:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-utility-in-progress --skill flow:flow-decompose-project
```

Output the COMPLETE banner:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-decompose-project — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

---

## Hard Rules

- Never file issues without explicit user approval — Steps 1 and 2 are mandatory gates
- Never skip codebase exploration in the decompose step
- Never tell the user to "look at" a file — render all content inline
- Never use Bash to print banners — output them as text in your response
- Always use the Write tool to create body files — never pass body text as a CLI argument
- Never delete body files — the `bin/flow issue` script handles cleanup
- Each step ends by invoking the skill itself as the final action — never continue to the next step in the same invocation
- All `bin/flow` calls use `${CLAUDE_PLUGIN_ROOT}/bin/flow`
- Session state files use the `<id>` prefix to prevent concurrent collisions
- Issue creation order is topological — leaves first so dependency numbers exist
- Phase labels are auto-derived from DAG groupings, not user-specified
- Milestone due date is required — asked during Step 2 review
- Sub-issue and blocked-by linking is best-effort — failures do not block the skill
- Every issue body (epic and children) wraps its Implementation Plan in the FLOW-PLAN sentinel pair so `bin/flow plan-from-issue` can extract the plan at flow-start
- Tasks within the Implementation Plan use `#### Task N:` headers (not numbered list items) so `count_tasks` recognises the heading shape and populates `code_tasks_total`
- Always invoke `${CLAUDE_PLUGIN_ROOT}/bin/flow validate-issue-body` before `${CLAUDE_PLUGIN_ROOT}/bin/flow issue` — on validator error, route through the Revise loop in Step 3 (epic) or Step 4 (per child)
- Paraphrase every prose reference to the FLOW-PLAN sentinel pair — the literal HTML-comment marker strings appear only at the actual delimiters of the wrapped Implementation Plan
