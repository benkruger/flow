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
- **"Cancel"** → stop. Do not file any issues.

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

For each child issue, draft:

- **Title** — concise, actionable
- **Body** — Problem, Acceptance Criteria, Files to Investigate, Out of
  Scope, Context sections.
- **Labels** — `decomposed` plus the auto-derived phase label
- **Dependencies** — which other child issues this depends on (by title,
  resolved to numbers in Step 4)

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
- **"Cancel"** → stop.

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
`.flow-states/decompose-project-<id>-epic-body` using the Write tool, then:

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

Write the issue body to `.flow-states/decompose-project-<id>-issue-body`
using the Write tool, then create the issue:

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
