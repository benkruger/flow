---
name: flow-learn
description: "Phase 4: Learn — audit rule compliance and identify process gaps. Routes findings to CLAUDE.md, .claude/rules/, and plugin issues."
---

# Learn

## Usage

```text
/flow:flow-learn
/flow:flow-learn --continue-step
```

- `/flow:flow-learn` — uses the configured mode from the state file's `skills.flow-learn` config
- `/flow:flow-learn --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next step via Resume Check

<HARD-GATE>
Run `phase-enter` as your very first action. If it returns an error, stop
immediately and show the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter --phase flow-learn --steps-total 7
```

Parse the JSON output. If `"status": "error"`, STOP and show the error.

If `"status": "ok"`, capture the returned fields:
`project_root`, `branch`, `worktree_path`, `worktree_cwd`,
`relative_cwd`, `pr_number`, `pr_url`, `feature`, `slack_thread_ts`,
and `plan_file`. The autonomy mode is resolved separately in the
Mode Resolution section below via `resolve-skill-mode`.

</HARD-GATE>

Use the returned fields for all downstream references. Do not re-read
the state file or re-run git commands to gather the same information.
Do not `cd` to the project root — `bin/flow` commands find paths
internally.

Use `<worktree_path>` for CLAUDE.md and `.claude/rules/` edits.
Use `<project_root>` for `.flow-states/` paths only.

## Re-anchor cwd

Mono-repo flows started inside a subdirectory (e.g. `api/`) capture
that path as `relative_cwd` and rely on cwd staying at
`<worktree>/<relative_cwd>` so subsequent `bin/flow` calls pass the
cwd-drift guard. Context loss between skill invocations can reset cwd
to the main repo root; the bash block below re-anchors regardless of
how the session got here. Substitute the `worktree_cwd` value from the
phase-enter response — a no-op for root-level flows (where it equals
`worktree_path`) and a real re-anchor for mono-repo flows.

```bash
cd "<worktree_cwd>"
```

## Three Tenants

The Learn phase is an audit, not a retrospective. It does not ask "what
did we learn?" It asks three specific questions:

**Tenant 1 — Did the FLOW process work?** Identify gaps in the plugin's
workflow (tools, skills, hooks, phase gates). These become GitHub issues
filed against `benkruger/flow`.

**Tenant 2 — Did Claude follow the rules?** Audit compliance with
CLAUDE.md and `.claude/rules/`. For each violation, assess the
enforcement level:

- Rule was unclear or ambiguous → clarify the rule wording
- Rule was clear but Claude ignored it → clarify the rule AND file an
  enforcement escalation issue (recommend HARD-GATE or hook)

**Tenant 3 — What rules should exist but don't?** Identify undocumented
patterns and gaps in coverage. Create forward-looking rules that will
prevent future sessions from making the same class of mistake.

Every finding in every step must serve one of these three tenants.
Findings that do not map to a tenant are dropped.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

Resolve `commit` and `continue` on every entry — fresh invocation and
`--continue-step` self-invocation alike — from the state file's
`skills.flow-learn` config via `resolve-skill-mode`. The state file is
the single source of truth for skill autonomy; there are no
`--auto`/`--manual` flags.

Resolve the current branch first: run `git worktree list --porcelain`,
note the project root (the path on the first `worktree` line), find
the `worktree` entry whose path matches the current working directory,
and take the `branch refs/heads/<name>` line from that entry (strip
the `refs/heads/` prefix). Call this `<branch>`. Then run the resolver:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow resolve-skill-mode --skill flow-learn --branch <branch>
```

Parse the JSON output. `commit` and `continue` are each `"auto"` or
`"manual"`:

- `commit=auto` — skip permission-promotion prompts and commit
  automatically.
- `commit=manual` — prompt for permission promotion before committing.
- `continue=auto` — auto-advance to Complete when Learn completes.
- `continue=manual` — prompt before advancing to Complete.

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step. Skip the Announce banner and the `phase-enter` call
(do not enter the phase again). Run `## Mode Resolution` above (it
runs on every entry), then proceed directly to the Resume Check
section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.4.0 — Phase 4: Learn — STARTING
──────────────────────────────────────────────────
```
````

## Logging

No logging for this phase. Learn runs no Bash commands beyond the entry
gate — there is nothing to log.

## Resume Check

Read `learn_step` from the state file (default `0` if absent).

- If `3` → Step 3 is done. Skip to Step 4.
- If `4` → Steps 3-4 are done. Skip to Step 5.
- If `5` → Steps 3-5 are done. Skip to Step 6.
- If `6` → Steps 3-6 are done. Skip to Step 7.

---

## Step 1 — Gather and launch agent

Gather all artifacts, then launch the learn-analyst agent for
cognitively isolated analysis. The agent receives only persisted
artifacts — never conversation history. This structural separation
eliminates self-reporting bias: the session that built the feature
cannot honestly assess its own compliance because it carries forward
the emotional arc of the work.

**Read project rules.** Read the project's `CLAUDE.md` at
`<worktree_path>/CLAUDE.md`. Note every rule and convention entry. The
global CLAUDE.md is already loaded in conversation context — no separate
read is needed.

**Read state file data.** Read the state file at
`<project_root>/.flow-states/<branch>/state.json`. Extract: `notes`, phase
`visit_count` and `cumulative_seconds` for each phase.

**Read the plan file.** Read the plan file at
`<project_root>/<files.plan path>`.

**Read rules files.** Use the Glob tool at
`<worktree_path>/.claude/rules/*.md`, then read each file.

**Resolve the integration branch.** Run `bin/flow base-branch` to
retrieve the base branch the flow coordinates against (the
integration branch captured at flow-start). Capture its stdout —
call the value `<base_branch>` — and substitute it into the
`git diff` command below. A repo whose default branch is `staging`
produces `<base_branch> = staging`; a standard repo produces
`<base_branch> = main`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

**Get the branch diff.** Substitute `<base_branch>` with the value
you just captured.

```bash
git diff origin/<base_branch>...HEAD
```

**Launch learn-analyst.** Launch the learn-analyst agent using the Agent
tool:

- `subagent_type`: `"flow:learn-analyst"`
- `description`: `"Compliance audit and process analysis"`

Provide all artifacts in the prompt with labeled sections, plus
the framing block below so the agent rewards filtering rather
than producing:

> FRAMING:
> Most Learn phases produce zero findings. Your job is to filter,
> not to find things to say. Apply the two-gate test to every
> candidate finding before reporting it:
> (1) Forward-facing — would a future session who has never read
> this PR understand and apply the principle? If no, drop.
> (2) Value — was the gap caught and remediated in this PR? If
> yes, drop (the system already closed it).
> "No findings" in any category is the most common correct answer.
> Return only findings that pass both gates. If all categories
> produce zero, say so explicitly with "No findings" markers — do
> NOT invent findings to fill sections.
>
> DIFF:
> (full diff output)
>
> STATE FILE DATA:
> (notes array, phase timings, visit counts)
>
> PLAN:
> (full plan file content)
>
> PROJECT CLAUDE.MD:
> (full CLAUDE.md content)
>
> RULES FILES:
> (each .claude/rules/ file, with its filename as a header)

Wait for the agent to return its structured findings.

**Truncation check.** Examine the learn-analyst output for expected
structure. Valid output contains `**Finding` blocks with category labels
(Process gap, Rule compliance, Missing rule) or explicit "No findings"
markers for each category. If the output contains some but not all
categories, the agent truncated mid-analysis — use the findings from
completed categories and note the incomplete categories for the Step 7
report. If the output contains no `**Finding` blocks and no category
markers, the agent exhausted its turn budget without producing structured
output. Note for the Step 2 synthesis: "Learn-analyst agent exhausted
turn budget without producing structured findings."

### Per-agent accounting (record + retry-3-then-skip)

Account for the learn-analyst agent in state so the
`phase-finalize` required-agents gate can confirm it ran.

**Normal completion — record the return.** When the agent
produced structured output cleanly, invoke `record-agent-return`
to write the verified entry into
`phases.flow-learn.agents_returned`. The subcommand reads the
persisted Claude Code transcript and confirms the Agent
tool_use/tool_result pair exists for `subagent_type:
"flow:learn-analyst"` after the most recent `phase-enter --phase
flow-learn` Bash marker — closing the inline-synthesis bypass
where a model could write findings without actually invoking the
agent.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow record-agent-return --branch <branch> --agent learn-analyst --phase flow-learn
```

Parse the JSON output. If `status==ok`, the agent is accounted
for. If `status==error` (reason `transcript_verification_failed`
or any other), enter the retry path below.

**Truncation, external failure, or recording failure — retry up
to 3 attempts, then note.** Read
`phases.flow-learn.agent_retry_counts.learn-analyst` from state
(default `0`). If the count is less than 3, increment via
`bin/flow set-timestamp` and re-invoke the agent with the same
prompt:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set phases.flow-learn.agent_retry_counts.learn-analyst=<count+1>
```

If the count has reached 3, the agent has exhausted its retries.
Record the skip:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-skipped-agent --branch <branch> --agent learn-analyst --reason exhausted_retries --phase flow-learn
```

<HARD-GATE>
When the learn-analyst agent has exhausted retries, you MUST NOT
synthesize its findings inline. The agent's analysis is
unavailable for this Learn pass — record the skip via
`add-skipped-agent`, then proceed to Step 2 with the explicit
acknowledgment that Tenant 1/2/3
findings were not produced for this PR. Fabricating an agent's
analysis from session memory defeats cognitive isolation per
`.claude/rules/cognitive-isolation.md` "Never Supplement Agent
Work From the Parent Session".
</HARD-GATE>

---

## Step 2 — Synthesize findings

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=1
```

**Correction notes are mandatory user directives.** This paragraph
exists so a correction the user captured mid-flow via
`/flow:flow-note` always reaches a durable home — even when the
learn-analyst produced nothing. Enumerate every `state.notes` entry
whose `type` field equals `correction`.
Every `correction` note is a non-negotiable user directive: it MUST be routed to a durable rule and never dropped.
A correction note is exempt from the two-gate filter below and from
the zero-artifact default — those govern learn-analyst candidate
findings, not directives the user typed. Do NOT drop a correction
note and do NOT defer it to a separate flow; each one is routed in
Step 3. The parent reads `state.notes` directly here, so the
routing survives learn-analyst truncation or retry exhaustion — and
reading `state.notes` directly is directive-execution, not
agent-analysis fabrication, so it does not violate
`.claude/rules/cognitive-isolation.md` "Never Supplement Agent Work
From the Parent Session" (that rule forbids inventing a missing
agent's findings, not executing a directive the user typed).

Take the learn-analyst findings (when present) and sort them
into three buckets matching the three tenants.

**Default is zero artifacts.** Most Learn phases produce no rule
edits and no filed issues. The skill is an audit, not a writing
prompt: a finding's first stop is the filter, not the routing
table. If you find yourself reaching for an artifact, default
back to "drop and record in commit message" until the finding
proves it deserves more.

**Two-gate filter.** Every candidate finding must pass BOTH
tests below before it can produce any artifact (rule edit or
issue). If a finding fails either, drop it and record it as
dismissed:

1. **Forward-facing test.** A future session who has never read
   this PR must be able to understand and apply the rule. If
   the rule's only example is the current PR, or the principle
   only makes sense given the incident that produced it, the
   finding is incident provenance — drop it. Per
   `.claude/rules/forward-facing-authoring.md`: "If the rule
   only makes sense when the reader knows the specific incident
   that spawned it, the finding is too incident-specific to
   codify."
2. **Value test.** If the gap was caught by another phase gate
   AND remediated in this PR (code fix, rule clarification, or
   new rule), the system already closed the gap. Drop it. Per
   `.claude/rules/filing-issues.md` "Value Test Before Filing":
   a Review-caught-and-fixed violation is the system
   working, not a gap.

For each dropped finding, record the dismissal:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "dismissed" --phase "flow-learn"
```

What survives both filters: patterns observed across multiple
PRs (recurring violations the agent saw in prior flows), classes
of bug this PR caught but couldn't fix structurally (open gaps
with explicit follow-up scope), rules that are clear but
consistently ignored across flows. What does NOT survive:
"here's what we did differently this time," "here's the
specific bug we just hit and fixed," rule clarifications whose
only example is the current PR.

**Tenant 1 — Process gaps.** Findings where the FLOW plugin's workflow
broke or was missing something, including dangling async operations
(background agent invocations without result handling) and missing
automation. Apply the value test from
`.claude/rules/filing-issues.md` "Value Test Before Filing" before
routing:

> Was the gap caught by another phase gate AND remediated in this
> PR (code fix, rule clarification, or new rule)?

If yes → the system already closed the gap. Record it in the commit
message and the Learn report. Do NOT route to Step 6. Dismiss with
the rationale "caught by Review and remediated in this PR — no
open gap." If no → real gap, route to Step 6.

The trap: framing "Plan phase didn't catch X but Review did"
as a process gap. Review IS part of the process. The
cognitive-isolation design exists precisely to catch what the Plan
author missed. A Review-caught-and-fixed violation is the
system working, not a gap.

**Tenant 2 — Rule compliance.** Findings where an existing rule was
violated. For each violation, note the learn-analyst's enforcement
assessment:

- Rule was unclear or ambiguous → route to Step 3 (clarify rule)
- Rule was clear but ignored → route to Step 3 (clarify rule). Apply
  the same value test before filing an enforcement escalation issue:
  has the same violation been observed across multiple flows
  (pattern, not one-off) AND has instruction-level enforcement
  demonstrably failed to fix it? If yes, route to Step 6. If no,
  the rule clarification is sufficient.

**Tenant 3 — Missing rules.** Findings where no rule covers the
situation but should. Route to Step 3 (create new rule).

If the learn-analyst was truncated and some categories are missing,
note which categories are unavailable at the top of the synthesis.
Use only the findings from completed categories.

---

## Step 3 — Route and apply

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=2
```

This step is fully autonomous — decide destinations and apply all
changes without asking the user.

### Routing

**Correction notes (mandatory).** Each `correction` note enumerated
in Step 2 MUST be routed to a durable home — a `.claude/rules/`
file or `CLAUDE.md` — using the `### Apply CLAUDE.md changes` and
`### Apply rules changes` mechanics below. Apply the
`.claude/rules/persistence-routing.md` decision tree to choose the
destination. Those mechanics record the write via
`bin/flow add-finding`, which is the audit trail proving the
directive was honored. The zero-artifact default does not apply to
correction notes — each one must reach a durable home. Routing is
idempotent: "mandatory" means the directive lands durably, not that
every run writes a fresh artifact. If a correction note's directive
is already captured by an existing rule or `CLAUDE.md` entry —
including one written by an interrupted earlier run of this step —
that already satisfies the mandate, so do not duplicate it.

**Tenant 1 findings (process gaps)** — skip this step. Process gaps go
to Step 6 (GitHub issues).

**Tenant 2 findings** — for each rule compliance violation:

- Clarify the violated rule in its current location (CLAUDE.md or
  `.claude/rules/<topic>.md`). Make the wording unambiguous so future
  sessions cannot misinterpret it.
- If the learn-analyst assessed the rule as "clear but ignored," also
  note the finding for Step 6 to file an enforcement escalation issue.

**Tenant 3 findings** — for each missing rule, determine the
destination:

1. Check existing rules files. Use the Glob tool to list files at
   `<worktree_path>/.claude/rules/*.md`. If an existing file covers
   this topic, route to that file (update it).
2. Apply the scope test. "Would every Claude session in this project
   need this knowledge, regardless of what it is working on?"
   - If yes → Project CLAUDE.md
   - If no → `.claude/rules/<topic>.md`
3. Default to rules when ambiguous. CLAUDE.md is loaded into every
   session (token cost compounds). Rules files are loaded on demand.

**Routing examples:**

| Finding | Route to | Reason |
|---|---|---|
| "Never use `replace_all=True` on JSON state files when the old_string appears in multiple contexts" | `.claude/rules/state-files.md` | Domain-specific — only relevant when editing state files |
| "All timestamps use Pacific Time via `now_pacific()` in `src/utils.rs`" | `CLAUDE.md` | Every session needs this — any phase could generate timestamps |

**Merge clustered findings.** If multiple findings target the same
file, merge them into a single edit rather than separate writes.

**Zero-artifact default carries through.** Step 2's two-gate
filter is the source of truth — if a finding made it past both
gates, it has earned its artifact (rule edit or GitHub issue).
If Step 2 dropped everything, Step 3 produces nothing and that
is the correct outcome. Do not invent artifacts to fill the
section. Most Learn phases land here with an empty list.

### Writing rules

- Write for Claude, not for humans — the audience is a future Claude
  session
- Be direct, specific, and actionable — describe the exact situation
  and the exact required behavior
- Be as dense and complete as the finding requires — include the why
  and the how, not just the what
- Generic and reusable — not tied to the specific feature or session

### Apply CLAUDE.md changes

Before routing any finding to CLAUDE.md, apply the
**obey-vs-describe test** per
`.claude/rules/persistence-routing.md`. The scope test resolving
to CLAUDE.md is not sufficient on its own.

For each candidate addition, name the specific behavior the entry
controls every session — "all timestamps use Pacific Time," "never
invoke cargo directly," "use bin/flow finalize-commit." If no
behavior can be named, the finding describes how the system works
rather than how the model should behave. Route to one of the
three alternative destinations below — each with its own write
mechanic and `add-finding` outcome:

- **Module doc comment** in `src/<name>.rs` — Rust code mechanics
  that future readers find via grep or rustdoc. Use the Edit tool
  to update the module doc comment directly. Record via
  `bin/flow add-finding --outcome "rule_written" --path
  "src/<name>.rs"`.
- **`docs/` subtree** — long-form architecture, schema reference,
  public-facing material loaded on demand. Use the Edit tool to
  update the relevant doc file. Record via
  `bin/flow add-finding --outcome "rule_written" --path
  "docs/<relative>"`.
- **Discard** — when the Discoverability test resolves negatively,
  the next session can derive the content by reading the code or
  existing rules. Record via `bin/flow add-finding --outcome
  "dismissed"` with rationale citing the Discoverability test.

A description that fails the obey-vs-describe test belongs in a
module doc comment, the `docs/` subtree, or discard — not
CLAUDE.md. CLAUDE.md is always loaded into every session's
context, so every descriptive byte compounds token cost on every
subsequent turn. Only behavioral instructions the model obeys and
one-line pointer indexes to rule files belong here.

**Correction notes are imperatives by definition.** A finding that
originates from a `correction` note enumerated in Step 2 (per the
"Correction notes (mandatory)" requirement) passes the
obey-vs-describe test as obey — the user's correction is itself a
directive. Correction notes always route durably to a
`.claude/rules/` file or CLAUDE.md; the "discard" branch above
does not apply.

For each item that passes the obey-vs-describe test (project-wide
conventions, architecture pointers):

**Compose** a learning entry following the writing rules above.

**Read** `<worktree_path>/CLAUDE.md` using the Read tool to check
existing content — do not duplicate.

**Compose** the full updated CLAUDE.md content with the learning
applied.

**Write** the full content to `.flow-states/<branch>/rule-content.md`
using the Write tool.

**Apply** the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/CLAUDE.md --content-file .flow-states/<branch>/rule-content.md
```

After each CLAUDE.md write, record the finding. Use outcome `rule_written` when adding new content, `rule_clarified` when updating existing content:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "<outcome>" --phase "flow-learn" --path "CLAUDE.md"
```

### Apply rules changes

Before routing any finding to `.claude/rules/`, apply the
**obey-vs-describe test** per
`.claude/rules/persistence-routing.md`. The scope test resolving
to `.claude/rules/` is not sufficient on its own — rule files
collect behavioral instructions the model obeys, not architecture
descriptions, so a descriptive finding compounds token cost
across every flow that loads the rule.

If no behavior the model OBEYS every session can be named, route
to one of the three alternative destinations enumerated in `###
Apply CLAUDE.md changes` above — module doc comment, the `docs/`
subtree, or discard — with the same `add-finding` outcomes. The
correction-notes exception applies here too: correction notes are
imperatives by definition and always route durably.

For each item that passes the obey-vs-describe test (domain-specific
gotchas, situational instructions):

**Determine** the target file
(`<worktree_path>/.claude/rules/<topic>.md`) and whether it is a new
rule or an update to an existing rule.

**Check** if the file exists using the Glob tool at
`<worktree_path>/.claude/rules/<topic>.md`.

**If exists**, use the Read tool to read it, then compose the full
updated content with the rule applied. **If new**, compose the full
content with a markdown heading matching the topic name.

**Write** the content to `.flow-states/<branch>/rule-content.md` using
the Write tool.

**Apply** the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/.claude/rules/<topic>.md --content-file .flow-states/<branch>/rule-content.md
```

After each rules file write, record the finding. Use outcome `rule_written` for new files, `rule_clarified` for updates to existing files:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "<outcome>" --phase "flow-learn" --path ".claude/rules/<topic>.md"
```

---

## Step 4 — Promote permissions

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=3
```

Promote any session permissions accumulated in
`.claude/settings.local.json` into the persistent
`.claude/settings.json`. The `--confirm-on-flow-branch` flag is
required because the active-flow gate inside `promote-permissions`
otherwise rejects mid-flow runs (see
`.claude/rules/permissions.md` "Never Edit Permissions Mid-Flow").
Learn is the one sanctioned mid-flow caller.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow promote-permissions --worktree-path <worktree_path> --confirm-on-flow-branch
```

Parse the JSON output:

- `"status": "skipped"`, `"reason": "no_local_file"` — no
  `settings.local.json` exists. Continue.
- `"status": "skipped"`, `"reason": "active_flow"` — should never
  appear here because `--confirm-on-flow-branch` is passed; if it
  does, the flag was dropped. Log the response and continue rather
  than retry.
- `"status": "ok"` — permissions promoted. If `promoted` is non-empty,
  note that `.claude/settings.json` has changed for the commit decision
  in Step 5.
- `"status": "error"` — log the error and continue. Do not block the
  Learn phase for a promotion failure.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=4
```

---

## Step 5 — Commit (conditional)

If no changes were made in Steps 3-4, record step completion and
self-invoke to skip the commit:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

Then invoke `flow:flow-learn --continue-step` using the Skill tool as
your final action.

If any changes were made (CLAUDE.md or `.claude/` files), commit once.
Only CLAUDE.md and `.claude/` files are committed — never application
code. If `git add -A` results in nothing staged (stealth user with
excluded files), skip the commit gracefully — do not error.

Set the continuation context and flag before committing. The
self-invocation carries no mode flag — the resumed run re-resolves
`commit`/`continue` from the state file via `## Mode Resolution`:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set learn_step=5, then self-invoke flow:flow-learn --continue-step."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Invoke `/flow:flow-commit`.

After the commit completes, record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

To continue to Step 6, invoke `flow:flow-learn --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 6 — File GitHub issues

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

File GitHub issues for findings that require plugin changes.

### Process gap issues (Tenant 1)

For each process gap finding from Step 2, file a GitHub issue on the
plugin repo. The issue title should be a concise description of the
gap. The issue body should describe the gap generically — no user
project details, no feature-specific context. Focus on what the FLOW
process should do differently.

### Enforcement escalation issues (Tenant 2)

For each rule compliance finding where the learn-analyst assessed the
rule as "clear but ignored," file a GitHub issue on the plugin repo.
The issue title should name the rule and recommend the enforcement
mechanism (HARD-GATE or hook). The issue body should describe the
violation, cite the rule, and explain why instruction-level enforcement
is insufficient.

### Filing process

Write the issue body to `.flow-states/<branch>/issue-body-content.md` using
the Write tool, then route it to `.flow-issue-body` in the project root
via `bin/flow write-rule` (avoids Claude Code's Write-tool preflight on a
pre-existing body file — see `.claude/rules/file-tool-preflights.md`):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <project_root>/.flow-issue-body --content-file .flow-states/<branch>/issue-body-content.md
```

Then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --title "<issue_title>" --body-file .flow-issue-body
```

After each successful issue, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
```

After each filed issue, also record the finding:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "filed" --phase "flow-learn" --issue-url "<issue_url>"
```

If there are no findings to file, skip this step.

---

## Step 7 — Present report

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=6
```

Present the full report to the user:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Learn — Report
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Process gaps
  ------------
  - /flow:flow-commit should warn when branch is behind
  - ...

  Rule compliance
  ---------------
  - CLAUDE.md "never use guard clauses" — violated, rule
    was ambiguous — clarified wording
  - ...

  Missing rules
  -------------
  - No rule about checking eager-loaded associations
    before using pluck — rule created
  - ...

  Truncated agent
  ---------------
  ⚠ learn-analyst — partial findings (N of 3 categories
    completed)

  Changes applied
  ---------------
  .claude/rules/testing-gotchas.md: 1 addition (committed)

  Issues filed
  ------------
  [Process gap] #44: Commit skill should warn when branch
    is behind
  [Escalation] #45: Enforce "never use guard clauses" via
    HARD-GATE in flow-code

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Omit "Truncated agent" if the learn-analyst was not flagged as truncated
in Step 1. Omit "Changes applied" if no changes were made. Omit
"Issues filed" if no issues were filed.

In the "Changes applied" section, show "(committed)" or "(uncommitted)"
next to each file to indicate whether Step 5 committed it. Show
"(skipped — user denied)" next to any destination where the user denied
the Edit tool call during Step 3.

In the "Issues filed" section, prefix each issue with its type in
brackets: `[Process gap]` for Tenant 1, `[Escalation]` for Tenant 2.

---

## Done

Finalize the phase (complete + Slack notification in one call):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-finalize --phase flow-learn --branch <branch> --thread-ts <slack_thread_ts>
```

Omit `--thread-ts` if `slack_thread_ts` was not returned by `phase-enter`.

Parse the JSON output.

**Handle the `required_agent_not_returned` error reason.** When
the response shape is
`{"status":"error","reason":"required_agent_not_returned","missing":[...],"message":"..."}`,
the learn-analyst agent is recorded in neither
`agents_returned` nor `agents_skipped`. The required-agents
gate ran before any state mutation. The phase has not been
advanced.

Three recovery shapes apply, ordered cheapest first:

- **learn-analyst was invoked but `record-agent-return` was not
  called.** The persisted transcript still carries the
  `tool_use`/`tool_result` pair. Retroactively invoke the
  recording subcommand:

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow record-agent-return --branch <branch> --agent learn-analyst --phase flow-learn
  ```

  When the response is `{"status":"ok",...}`, re-run
  `phase-finalize`. When the response is
  `{"status":"error","reason":"transcript_verification_failed"}`,
  fall through to the next recovery shape.

- **learn-analyst was never invoked (Step 1 loop bypassed).**
  Re-invoke the agent from Step 1's prompt template, classify
  the return, and either call `record-agent-return` (Class 3) or
  `add-skipped-agent` (Class 1/2 after the 3-attempt cap). Then
  re-run `phase-finalize`.

- **learn-analyst cannot be retried in this session.** Record it
  as skipped via the existing path:

  ```bash
  ${CLAUDE_PLUGIN_ROOT}/bin/flow add-skipped-agent --branch <branch> --agent learn-analyst --reason exhausted_retries --phase flow-learn
  ```

  Then re-run `phase-finalize` with `--accept-skipped-agents`.

Do NOT advance to the COMPLETE banner until learn-analyst is
accounted for AND a subsequent `phase-finalize` call returns
`{"status":"ok",...}`.

When the response is `{"status":"error", ...}` for any OTHER
reason, report the error and stop.

Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.4.0 — Phase 4: Learn — COMPLETE (<formatted_time>)
  Run /flow:flow-complete to merge the PR and clean up.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

<HARD-GATE>
STOP. Parse `continue_action` from the `phase-finalize` output above
to determine how to advance.

1. Use `continue_action` from the `phase-finalize` output —
   `phase-finalize` computes it from the state file's
   `skills.flow-learn.continue` config.
   If `continue_action` is `"invoke"` → continue=auto.
   If `continue_action` is `"ask"` → continue=manual.
2. If continue=auto → invoke `flow:flow-complete` directly using the Skill tool.
   Do NOT run `bin/flow status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Run `bin/flow status` via Bash and print its stdout in your
      response inside a fenced code block:

      ```bash
      ${CLAUDE_PLUGIN_ROOT}/bin/flow status
      ```

   b. Use AskUserQuestion:
      "Phase 4: Learn is complete. The PR now includes rule improvements.
      Ready to begin Phase 5: Complete?"
      Options: "Yes, start Phase 5 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 5 now" and "Not yet"
   d. If Yes → invoke `flow:flow-complete` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-complete` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-complete when ready.
══════════════════════════════════════════════════
```
````

---

## Hard Rules

- Never commit application code in Learn — only CLAUDE.md and .claude/
- Always read CLAUDE.md and rules files before launching the learn-analyst agent
- Gather all artifacts (CLAUDE.md, state file, plan, rules, diff) before launching the agent
- Follow the learning process (Steps 1 through 7) exactly — do not skip or reorder steps
- Every finding must map to one of the three tenants — findings that do not map are dropped
- Apply the generalization filter to all findings — no backward-looking output about already-fixed code
- Routing decisions and rule wording are autonomous — do not ask the user for approval mid-process
- The report in Step 7 is the user's review point — make it comprehensive
- CLAUDE.md and `.claude/rules/` files are written via `bin/flow write-rule` subprocess and committed via `/flow:flow-commit` — never via Edit or Write tools on `.claude/` paths
- All edits target the project repo — never user-level `~/.claude/` paths
- Plugin process gaps and enforcement escalations are filed as GitHub issues on the plugin repo (`benkruger/flow`) — see `.claude/rules/filing-issues.md` "Repo Routing"
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- When in autonomous mode, classify tool failures per `.claude/rules/autonomous-flow-self-recovery.md` — mechanical fixes are in-flow, substantive failures prompt the user
