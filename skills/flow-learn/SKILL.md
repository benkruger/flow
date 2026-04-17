---
name: flow-learn
description: "Phase 5: Learn — audit rule compliance and identify process gaps. Routes findings to CLAUDE.md, .claude/rules/, and plugin issues."
---

# Learn

## Usage

```text
/flow:flow-learn
/flow:flow-learn --auto
/flow:flow-learn --manual
/flow:flow-learn --continue-step
/flow:flow-learn --continue-step --auto
/flow:flow-learn --continue-step --manual
```

- `/flow:flow-learn` — uses configured mode from the state file (default: auto)
- `/flow:flow-learn --auto` — skip permission promotion prompts, auto-advance to Complete
- `/flow:flow-learn --manual` — prompt for permission promotion and phase transition
- `/flow:flow-learn --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next step via Resume Check

<HARD-GATE>
Run `phase-enter` as your very first action. If it returns an error, stop
immediately and show the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter --phase flow-learn --steps-total 7
```

Parse the JSON output. If `"status": "error"`, STOP and show the error.

If `"status": "ok"`, capture the returned fields:
`project_root`, `branch`, `worktree_path`, `pr_number`, `pr_url`,
`feature`, `slack_thread_ts`, `plan_file`, and `mode` (commit + continue).

</HARD-GATE>

Use the returned fields for all downstream references. Do not re-read
the state file or re-run git commands to gather the same information.
Do not `cd` to the project root — `bin/flow` commands find paths
internally.

Use `<worktree_path>` for CLAUDE.md and `.claude/rules/` edits.
Use `<project_root>` for `.flow-states/` paths only.

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
Your state file (`.flow-states/<branch>.json`) is yours alone. Never
read or write another branch's state. All local artifacts (logs, plan
files, temp files) are scoped by branch name. GitHub state (PRs, issues,
labels) is shared across all engineers — operations that create or modify
shared state must be idempotent.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, use `mode.commit` and `mode.continue` from the `phase-enter` response.
4. If `phase-enter` was skipped (self-invocation), use the mode from the flag that was passed.

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step. Skip the Announce banner and the `phase-enter` call
(do not enter the phase again). Proceed directly to the Resume Check
section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — Phase 5: Learn — STARTING
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
`<project_root>/.flow-states/<branch>.json`. Extract: `notes`, phase
`visit_count` and `cumulative_seconds` for each phase.

**Read the plan file.** Read the plan file at
`<project_root>/<files.plan path>`.

**Read rules files.** Use the Glob tool at
`<worktree_path>/.claude/rules/*.md`, then read each file.

**Get the branch diff.**

```bash
git diff origin/main...HEAD
```

**Launch learn-analyst.** Launch the learn-analyst agent using the Agent
tool:

- `subagent_type`: `"flow:learn-analyst"`
- `description`: `"Compliance audit and process analysis"`

Provide all artifacts in the prompt with labeled sections:

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

---

## Step 2 — Synthesize findings

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=1
```

Take the learn-analyst findings and sort them into three buckets
matching the three tenants.

**Generalization filter.** For each finding, ask: "What general
principle, applicable to future work in this project, would prevent this
class of problem?" If a finding cannot be expressed as a forward-looking
principle — if it only describes the specific code that was just fixed —
drop it. It is a description of what happened, not a learning. For each
dropped finding, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "dismissed" --phase "flow-learn"
```

**Tenant 1 — Process gaps.** Findings where the FLOW plugin's workflow
broke or was missing something, including dangling async operations
(background agent invocations without result handling) and missing
automation. Route to Step 6 (file issues).

**Tenant 2 — Rule compliance.** Findings where an existing rule was
violated. For each violation, note the learn-analyst's enforcement
assessment:

- Rule was unclear or ambiguous → route to Step 3 (clarify rule)
- Rule was clear but ignored → route to Step 3 (clarify rule) AND
  route to Step 6 (file enforcement escalation issue)

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

**Mandatory output constraint.** Every finding that survives the
generalization filter must produce at least one concrete artifact — a
rule edit or a GitHub issue. Findings too specific to generalize were
already dropped in Step 2.

### Writing rules

- Write for Claude, not for humans — the audience is a future Claude
  session
- Be direct, specific, and actionable — describe the exact situation
  and the exact required behavior
- Be as dense and complete as the finding requires — include the why
  and the how, not just the what
- Generic and reusable — not tied to the specific feature or session

### Apply CLAUDE.md changes

For each item routed to CLAUDE.md (project-wide conventions,
architecture):

**Compose** a learning entry following the writing rules above.

**Read** `<worktree_path>/CLAUDE.md` using the Read tool to check
existing content — do not duplicate.

**Compose** the full updated CLAUDE.md content with the learning
applied.

**Write** the full content to `.flow-states/<branch>-rule-content.md`
using the Write tool.

**Apply** the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/CLAUDE.md --content-file .flow-states/<branch>-rule-content.md
```

After each CLAUDE.md write, record the finding. Use outcome `rule_written` when adding new content, `rule_clarified` when updating existing content:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "<outcome>" --phase "flow-learn" --path "CLAUDE.md"
```

### Apply rules changes

For each item routed to `.claude/rules/` (domain-specific gotchas,
situational instructions):

**Determine** the target file
(`<worktree_path>/.claude/rules/<topic>.md`) and whether it is a new
rule or an update to an existing rule.

**Check** if the file exists using the Glob tool at
`<worktree_path>/.claude/rules/<topic>.md`.

**If exists**, use the Read tool to read it, then compose the full
updated content with the rule applied. **If new**, compose the full
content with a markdown heading matching the topic name.

**Write** the content to `.flow-states/<branch>-rule-content.md` using
the Write tool.

**Apply** the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/.claude/rules/<topic>.md --content-file .flow-states/<branch>-rule-content.md
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
`.claude/settings.json`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow promote-permissions --worktree-path <worktree_path>
```

Parse the JSON output:

- `"status": "skipped"` — no `settings.local.json` exists. Continue.
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
your final action. If commit=auto was resolved, pass `--auto` as well.

If any changes were made (CLAUDE.md or `.claude/` files), commit once.
Only CLAUDE.md and `.claude/` files are committed — never application
code. If `git add -A` results in nothing staged (stealth user with
excluded files), skip the commit gracefully — do not error.

Set the continuation context and flag before committing.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set learn_step=5, then self-invoke flow:flow-learn --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set learn_step=5, then self-invoke flow:flow-learn --continue-step --manual."
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

Write the issue body to `.flow-states/<branch>-issue-body-content.md` using
the Write tool, then route it to `.flow-issue-body` in the project root
via `bin/flow write-rule` (avoids Claude Code's Write-tool preflight on a
pre-existing body file — see `.claude/rules/file-tool-preflights.md`):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <project_root>/.flow-issue-body --content-file .flow-states/<branch>-issue-body-content.md
```

Then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --label "Flow" --title "<issue_title>" --body-file .flow-issue-body
```

After each successful issue, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flow" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
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
in Step 1. Omit "Changes applied" if no changes were made. Omit "Issues
filed" if no issues were filed.

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

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 5: Learn — COMPLETE (<formatted_time>)
  Run /flow:flow-complete to merge the PR and clean up.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

<HARD-GATE>
STOP. Parse `continue_action` from the `phase-finalize` output above
to determine how to advance.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use `continue_action` from the `phase-finalize` output.
   If `continue_action` is `"invoke"` → continue=auto.
   If `continue_action` is `"ask"` → continue=manual.
2. If continue=auto → invoke `flow:flow-complete` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 5: Learn is complete. The PR now includes rule improvements.
      Ready to begin Phase 6: Complete?"
      Options: "Yes, start Phase 6 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 6 now" and "Not yet"
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
- Plugin process gaps and enforcement escalations are filed as GitHub issues on the plugin repo with label "Flow"
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
