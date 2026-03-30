---
name: flow-learn
description: "Phase 5: Learn — review what went wrong, capture learnings, route to CLAUDE.md and .claude/rules/. Both destinations edited directly on disk and committed via PR."
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
Run this entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
3. **Determine mode:**
   - **State file exists + `phases.flow-code-review.status` == `"complete"`** → **Phase 5** mode
   - **State file exists + phase 4 incomplete** → STOP. "BLOCKED: Phase 4:
     Code Review must be complete. Run /flow:flow-code-review first."
   - **No state file** → Use Glob to check for `flow-phases.json` in the
     project root.
     - Exists → **Maintainer** mode (this is the plugin source repo)
     - Does not exist → **Standalone** mode
</HARD-GATE>

Keep the project root, branch, state data, and detected mode in context.
Use the project root to build state file paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

Compute `<worktree_path>` for repo-destination edits:
- **Phase 5:** `<worktree_path>` = `<project_root>/<state.worktree>` (from the
  state file's `worktree` field, e.g. `<project_root>/.worktrees/<branch>`)
- **Maintainer / Standalone:** `<worktree_path>` = `<project_root>` (no worktree)

Use `<worktree_path>` for CLAUDE.md edits.
Use `<project_root>` for `.flow-states/` paths only.

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
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-learn.commit` and `skills.flow-learn.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=auto, continue=auto

## Self-Invocation Check

If `--continue-step` was passed, this is a self-invocation from a
previous step. Skip the Announce banner and the Update State section
(do not call `phase-transition --action enter` again). Proceed directly
to the Resume Check section.

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

**Phase 5 mode:**

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — Phase 5: Learn — STARTING
──────────────────────────────────────────────────
```
````

**Maintainer or Standalone mode:**

````markdown
```text
──────────────────────────────────────────────────
  Learn — STARTING
──────────────────────────────────────────────────
```
````

## Update State

**Phase 5 only.** Skip for Maintainer and Standalone.

Update state for phase entry:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-learn --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

Set the step tracking fields for TUI progress display:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_steps_total=7
```

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

## Step 1 — Gather sources

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=0
```

Read and synthesise before doing anything else.

### Source A — CLAUDE.md rules (all modes)

Read the project's `CLAUDE.md` at `<worktree_path>/CLAUDE.md`. These are
the rules that should have been followed. Note every rule and convention
entry. The global CLAUDE.md is already loaded in conversation context —
no separate read is needed.

### Source B — Learn-analyst agent (Phase 5 only) / Conversation context (Maintainer and Standalone)

**Phase 5 mode:** Launch the learn-analyst agent for cognitively isolated
analysis. The agent receives only persisted artifacts — never conversation
history. This structural separation eliminates self-reporting bias: the
session that built the feature cannot honestly assess its own mistakes
because it carries forward the emotional arc of the work.

Get the full branch diff:

```bash
git diff origin/main...HEAD
```

Read the state file at `<project_root>/.flow-states/<branch>.json`.
Extract: `notes`, phase `visit_count` and `cumulative_seconds` for each
phase.

Read the plan file at `<project_root>/<files.plan path>`.

Read the project CLAUDE.md at `<worktree_path>/CLAUDE.md`.

Read all `.claude/rules/` files using the Glob tool at
`<worktree_path>/.claude/rules/*.md`, then read each file.

Launch the learn-analyst agent using the Agent tool:

- `subagent_type`: `"flow:learn-analyst"`
- `description`: `"Cognitively isolated learning analysis"`

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
> CLAUDE.MD RULES:
> (full CLAUDE.md content, followed by each .claude/rules/ file)

Wait for the agent to return its structured findings.

**Maintainer and Standalone mode:** Review the current conversation for:
- Moments where the user corrected Claude
- Responses where Claude was overruled or pushed back
- Misunderstandings that required clarification
- Suggestions Claude made that were rejected

Note: context may have been compacted. Use what is available.

### Source C — State file and plan file data (Phase 5 only)

Skip for Maintainer and Standalone.

For each phase, note:
- `visit_count` > 1 → this phase had friction, was revisited
- `cumulative_seconds` — note the time each phase took for context
- `state["notes"]` → explicit corrections captured during the session

Read `plan_file` from the state file to get the plan file path. Use the
Read tool to read the plan file. Note:
- Risks identified in the plan → check if any caused problems during implementation
- Approach rationale → did it hold up through Code and Review?
- Review findings that were caught late

Read `state["notes"]` in full. These are corrections and learnings
captured during the session via `/flow:flow-note`. They are the most direct
signal of what went wrong.

### Source D — Onboarding agent confusion report (Phase 5 only)

Skip for Maintainer and Standalone.

Get the full branch diff to provide to the onboarding agent:

```bash
git diff origin/main...HEAD
```

Launch the onboarding agent using the Agent tool. The agent receives only
the diff and codebase access — no conversation history, no coding rationale,
no plan file. This isolation is the debiasing mechanism: the agent sees
what a newcomer sees, not what the author remembers.

Use the Agent tool with:

- `subagent_type`: `"flow:onboarding"`
- `description`: `"Onboarding perspective analysis"`

Provide the full diff output in the prompt, prefixed with:

> "You are a new team member reading this PR for the first time. The full
> diff is below. Investigate the codebase and write the confusion report."

Wait for the agent to return its structured confusion report.

If the agent reports no findings, note "no comprehension barriers found"
and continue to Step 2.

If the agent reports findings, note each finding for Step 2 synthesis.
Confusion report findings map primarily to the "Missing rules" category
(undocumented patterns and architectural decisions) and occasionally to
"Process gaps" (when a comprehension barrier reveals a FLOW process
deficiency).

---

## Step 2 — Synthesize findings

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=1
```

**Phase 5 mode:** Merge the outputs from the learn-analyst agent (Source B)
and the onboarding agent (Source D) into a single set of categorized
findings. The learn-analyst produces process violations, mistakes, missing
rules, and process gaps from artifact evidence. The onboarding agent
produces comprehension barriers from a newcomer perspective. Map each
finding to the categories below.

**Maintainer and Standalone mode:** Organize all gathered evidence from
Sources A and B into the categories below.

**Process violations** — existing rules in CLAUDE.md that were broken or
nearly broken. Quote the specific rule and cite the evidence source
(learn-analyst finding, note, or conversation observation).

**Mistakes** — things that went wrong during the session. In Phase 5 mode,
these come from the learn-analyst agent's artifact-based analysis (notes,
visit counts, timing anomalies, diff inconsistencies). In
Maintainer/Standalone mode, these come from conversation review. For each
mistake, state:

- What went wrong (cite the evidence)
- What the evidence source is (learn-analyst finding, note text, visit
  count, timing anomaly, diff pattern, or conversation observation)

**Missing rules** — situations where something questionable happened but no
existing rule covered it. In Phase 5 mode, the onboarding agent's
comprehension barriers map primarily here (undocumented patterns and
architectural decisions).

**Process gaps** — places where the development process itself (tools,
skills, workflows) should be improved. These are not coding rules — they
are process changes. The learn-analyst agent detects these from patterns
like dangling async operations (background agent invocations without
result handling), repeated friction (high visit counts), and missing
automation.

---

## Step 3 — Route and apply

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=2
```

This step is fully autonomous — decide destinations and apply all changes
without asking the user.

### Destinations and routing

For each learning, follow this decision procedure to choose the destination:

1. **Identify the topic.** Name the specific domain the learning applies to
   (testing, concurrency, state files, skill authoring, etc.) or identify it
   as project-wide knowledge (architecture, key files, universal conventions).
2. **Check existing rules files.** Use the Glob tool to list files at
   `<worktree_path>/.claude/rules/*.md`. If an existing file covers this
   topic, route to that file (update it). If no existing file matches,
   continue to step 3.
3. **Apply the scope test.** Ask: "Would every Claude session in this project
   need this knowledge, regardless of what it is working on?"
   - If yes → Project CLAUDE.md (`CLAUDE.md`) — Edit on disk
   - If no (only relevant in a specific area) → `.claude/rules/<topic>.md` — Edit on disk
4. **Default to rules when ambiguous.** If the scope test is unclear, route to
   `.claude/rules/`. CLAUDE.md is loaded into every session (token cost
   compounds). Rules files are loaded on demand (zero cost when irrelevant).
   The economic default favors rules.

**Routing examples:**

| Learning | Route to | Reason |
|---|---|---|
| "Never use `replace_all=True` on JSON state files when the old_string appears in multiple contexts" | `.claude/rules/state-files.md` | Domain-specific — only relevant when editing state files |
| "All timestamps use Pacific Time via `flow_utils.now()`" | `CLAUDE.md` | Every session needs this — any phase could generate timestamps |
| "Never create symlinks to real binaries in test fixtures" | `.claude/rules/testing-gotchas.md` | Domain-specific — only relevant when writing tests |
| "Skills are pure Markdown, not executable code" | `CLAUDE.md` | Architectural knowledge every session needs |
| "Never use `cd <path> && git` — use `git -C`" | `.claude/rules/worktree-commands.md` | Domain-specific — only relevant when running git in worktrees |

**Process gap routing:** Learnings about FLOW skill or process behavior
(e.g. how a phase skill should present output, when a skill should
prompt the user) are process gaps — they belong in Step 6, which files
them on the plugin repo with the "Flow" label. Process gaps are not
coding anti-patterns. Skip them in this step and let Step 6 handle them.

### Mandatory output constraint

If Step 2 identified mistakes, every mistake must produce at least
one concrete artifact — a CLAUDE.md edit, a `.claude/rules/` edit, or a
Flow issue. A rule that existed but failed to prevent the mistake is not
sufficient coverage. When an existing rule failed to prevent the mistake,
either strengthen the rule (CLAUDE.md edit) or add a more specific rule
(`.claude/rules/` edit) or file a Flow issue. Zero artifacts from Step 3
when Step 2 found mistakes is a skill failure.

Both CLAUDE.md and `.claude/rules/` edits are direct — committed in Step 5.

### Writing rules

- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session

### Apply CLAUDE.md changes

For each item routed to CLAUDE.md (project-wide conventions, architecture):

1. Compose a learning entry following the writing rules above
2. Read `<worktree_path>/CLAUDE.md` using the Read tool to check
   existing content — do not duplicate
3. Compose the full updated CLAUDE.md content with the learning applied
4. Write the full content to `.flow-states/<branch>-rule-content.md`
   using the Write tool
5. Run the write-rule script to apply the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/CLAUDE.md --content-file .flow-states/<branch>-rule-content.md
```

### Apply rules changes

For each item routed to `.claude/rules/` (domain-specific gotchas, situational instructions):

1. Compose the rule text following the writing rules above
2. Determine the target file (`<worktree_path>/.claude/rules/<topic>.md`)
   and whether it is a new rule or an update to an existing rule
3. Use the Glob tool to check if the file exists at
   `<worktree_path>/.claude/rules/<topic>.md`
4. If the file exists, use the Read tool to read it, then compose the
   full updated content with the rule applied. If the file does not
   exist, compose the full content with a markdown heading matching
   the topic name
5. Write the content to `.flow-states/<branch>-rule-content.md` using
   the Write tool
6. Run the write-rule script to apply the change:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <worktree_path>/.claude/rules/<topic>.md --content-file .flow-states/<branch>-rule-content.md
```

---

## Step 4 — Promote permissions

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=3
```

Promote any session permissions accumulated in `.claude/settings.local.json`
into the persistent `.claude/settings.json`. This runs in all three modes
(Phase 5, Maintainer, Standalone).

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

**Phase 5:** If any changes were made (CLAUDE.md or `.claude/` files),
commit once. Only CLAUDE.md and `.claude/` files are committed — never
application code. If `git add -A` results in nothing staged (stealth
user with excluded files), skip the commit gracefully — do not error.

**Maintainer:** If any changes were made, commit once.

**Standalone:** Skip entirely — no commit.

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

If commit=auto, use `/flow:flow-commit --auto`. Otherwise, use
`/flow:flow-commit`.

After the commit completes, record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

To continue to Step 6, invoke `flow:flow-learn --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 6 — File GitHub issues (Phase 5 only)

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

Skip for Maintainer and Standalone.

### Process gap issues

For each item in "Process gaps", file a GitHub issue on the plugin repo.

The issue title should be a concise description of the process gap. The
issue body should describe the gap generically — no user project details,
no feature-specific context. Focus on what the FLOW process should do
differently.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --label "Flow" --title "<issue_title>" --body-file .flow-issue-body
```

After each successful issue, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flow" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
```

If there are no process gap items, skip this step.

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

  Findings
  --------

  Process violations
  ------------------
  - CLAUDE.md says "never use guard clauses" but Claude
    added an early return in the worker
  - ...

  Mistakes
  --------
  - Suggested git rebase (forbidden — corrected immediately)
  - ...

  Missing rules
  -------------
  - No rule about checking eager-loaded associations
    before using pluck
  - ...

  Process gaps
  ------------
  - /flow:flow-commit should warn when branch is behind
  - ...

  Changes applied
  ---------------
  Project CLAUDE.md: 2 additions (committed)

  Issues filed
  ------------
  [Rule] #44: Add rule — check eager-loaded associations
  [Flow] #42: Commit skill should warn when branch is behind

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Omit "Changes applied" if no CLAUDE.md changes were made. Omit "Issues
filed" if no issues were filed or not in Phase 5 mode.

In the "Changes applied" section, show "(committed)" or "(uncommitted)"
next to each file to indicate whether Step 5 committed it. Show
"(skipped — user denied)" next to any destination where the user denied
the Edit tool call during Step 3.

In the "Issues filed" section, prefix each issue with its label in
brackets (e.g. `[Rule]`, `[Flow]`).

---

## Done

### Phase 5 mode

Complete the phase:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-learn --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — Phase 5: Learn — COMPLETE (<formatted_time>)
  Run /flow:flow-complete to merge the PR and clean up.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

### Slack Notification

Read `slack_thread_ts` from the state file. If present, post a thread reply. Best-effort — skip silently on failure.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow notify-slack --phase flow-learn --message "<message_text>" --thread-ts <thread_ts>
```

If `"status": "ok"`, record the notification:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-notification --phase flow-learn --ts <ts> --thread-ts <thread_ts> --message "<message_text>"
```

If `"status": "skipped"` or `"status": "error"`, continue without error.

<HARD-GATE>
STOP. Re-read `skills.flow-learn.continue` from the state file at
`<project_root>/.flow-states/<branch>.json` before advancing.
The previous phase's continue mode does NOT carry over — each phase
has its own mode.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use the value from the state file. If absent → default to manual.
2. If continue=auto → invoke `flow:flow-complete` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 5: Learn is complete. The PR now includes CLAUDE.md improvements.
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
  Run /flow:flow-continue when ready.
══════════════════════════════════════════════════
```
````

### Maintainer and Standalone mode

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ Learn — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

No phase transition, no transition question.

---

## Hard Rules

- Never commit application code in Learn — only CLAUDE.md and .claude/
- Always read CLAUDE.md before synthesizing findings
- In Phase 5, gather all sources (CLAUDE.md, learn-analyst agent, state/plan data, onboarding agent) before synthesizing findings
- Follow the learning process (Steps 1 through 7) exactly — do not skip or reorder steps
- Decisions on destinations and wording are autonomous — do not ask the user for approval mid-process
- The report in Step 7 is the user's review point — make it comprehensive
- CLAUDE.md and `.claude/rules/` files are written via `bin/flow write-rule` subprocess and committed via `/flow:flow-commit --auto` (Phase 5 and Maintainer) — never via Edit or Write tools on `.claude/` paths
- All edits target the project repo — never user-level `~/.claude/` paths
- Plugin process gaps are filed as GitHub issues on the plugin repo with label "Flow"
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
