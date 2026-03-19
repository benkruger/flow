---
name: flow-learn
description: "Phase 5: Learn — review what went wrong, capture learnings, route to CLAUDE.md or file issues. CLAUDE.md edits committed directly; rules filed as Rule issues to avoid permission prompts."
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
  FLOW v0.33.0 — Phase 5: Learn — STARTING
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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-learn --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

No logging for this phase. Learn runs no Bash commands beyond the entry
gate — there is nothing to log.

## Resume Check

Read `learn_step` from the state file (default `0` if absent).

- If `4` → Step 4 is done. Skip to Step 5.
- If `5` → Steps 4-5 are done. Skip to Step 6.

---

## Step 1 — Gather sources

Read and synthesise before doing anything else.

### Source A — CLAUDE.md rules (all modes)

Read the project's `CLAUDE.md` at `<worktree_path>/CLAUDE.md`. These are
the rules that should have been followed. Note every rule and convention
entry. The global CLAUDE.md is already loaded in conversation context —
no separate read is needed.

### Source B — Conversation context (all modes)

Review the current conversation for:
- Moments where the user corrected Claude
- Responses where Claude was overruled or pushed back
- Misunderstandings that required clarification
- Suggestions Claude made that were rejected
- Background agents launched but whose results were never checked or
  processed (dangling async work)

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

---

## Step 2 — Synthesize findings

Organize all gathered evidence into categories:

**Process violations** — existing rules in CLAUDE.md that were broken or
nearly broken during the session. Quote the specific rule.

**Claude mistakes** — things Claude got wrong that the user had to correct.
Be specific and honest. Name the mistake clearly — do not soften or hedge.

For each mistake, state:
1. What Claude did wrong (the actual behavior, not a euphemism)
2. What the user said or did to correct it (quote or paraphrase)
3. How many rounds of correction it took before Claude got it right

If you cannot answer all three, you are probably softening the mistake.

**Missing rules** — situations where Claude did the wrong thing but no
existing rule covered it. These are gaps in CLAUDE.md.

**Process gaps** — places where the development process itself (tools,
skills, workflows) should be improved. These are not CLAUDE.md rules —
they are process changes.

**Dangling async operations** — background agents that were launched
but whose results were never awaited or processed. Classify these as
Claude mistakes if Claude forgot to check the results, or as process
gaps if the skill that launched the agents lacks follow-up instructions.

---

## Step 3 — Route and apply

This step is fully autonomous — decide destinations and apply all changes
without asking the user.

### Destinations and routing

| Learning type | Destination | Method |
|---|---|---|
| Process rule or architecture | Project CLAUDE.md (`CLAUDE.md`) | Edit on disk |
| Coding anti-pattern or gotcha | `.claude/rules/<topic>.md` | File a "Rule" issue |

### Mandatory output constraint

If Step 2 identified Claude mistakes, every mistake must produce at least
one concrete artifact — a CLAUDE.md edit, a Rule issue, or a Flow issue.
A rule that existed but failed to prevent the mistake is not sufficient
coverage. When an existing rule failed to prevent the mistake, either
strengthen the rule (CLAUDE.md edit) or file a more specific Rule or
Flow issue. Zero artifacts from Step 3 when Step 2 found mistakes is a
skill failure.

CLAUDE.md edits are direct — committed in Step 5.

Rules edits are deferred — filed as GitHub issues with all context needed
for a future session to apply them.

### Writing rules

- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session

### Apply CLAUDE.md changes

For each item routed to CLAUDE.md (process rules, architecture):

1. Compose a learning entry following the writing rules above
2. Read `<worktree_path>/CLAUDE.md`, apply the addition or rewording
3. Do not duplicate existing content

### Handling denied edits

If the user denies an Edit tool call, treat it as "skip this learning"
— not "stop everything." Record which destination was skipped and
continue with the remaining learnings. A denied edit does not block
subsequent destinations, steps, or the phase completion.

### File Rule issues

For each item routed to `.claude/rules/` (coding anti-patterns, gotchas):

1. Compose the rule text following the writing rules above
2. Determine the target file (`.claude/rules/<topic>.md`) and whether
   it is a new rule or an update to an existing rule
3. File a GitHub issue on the target project and record it

The issue body must contain the full rule text, the target file path
(e.g. `.claude/rules/testing-gotchas.md`), whether this is a new rule
or an update, and the section to place it in (if updating an existing
file).

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Rule" --title "<issue_title>" --body-file .flow-issue-body
```

Parse the JSON output. If `"status": "ok"`, record the issue:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Rule" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
```

If `bin/flow issue` fails, note the failure and continue with
remaining items.

---

## Step 4 — Promote local permissions (all modes)

Set the continuation context and flag before invoking the child skill:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set learn_step=4, clear _continue_pending, then self-invoke flow:flow-learn --continue-step."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=local-permission
```

Invoke `/flow:flow-local-permission`.

If it reports promoted entries, count `.claude/settings.json` as a
repo-destination change for Step 5's commit decision.

Record step completion:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=4
```

Clear the continuation flag:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

To continue to Step 5, invoke `flow:flow-learn --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 5 — Commit (conditional)

If no changes were made in Steps 3-4, record step completion and
self-invoke to skip the commit:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

Then invoke `flow:flow-learn --continue-step` using the Skill tool as
your final action. If commit=auto was resolved, pass `--auto` as well.

**Phase 5:** If any changes were made (CLAUDE.md or `.claude/` files),
commit once. Only CLAUDE.md and `.claude/` files are committed — never
application code. If `git add -A` results in nothing staged (stealth
user with excluded files), skip the commit gracefully — do not error.

**Maintainer:** If any changes were made, commit once.

**Standalone:** Skip entirely — no commit.

Set the continuation context and flag before committing:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set learn_step=5, then self-invoke flow:flow-learn --continue-step."
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

If commit=auto, use `/flow:flow-commit --auto`. Otherwise, use
`/flow:flow-commit`.

After the commit completes, clear the continuation flag and record step
completion:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=
```

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set learn_step=5
```

To continue to Step 6, invoke `flow:flow-learn --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Step 6 — File GitHub issues (Phase 5 only)

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
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --repo benkruger/flow --label "Flow" --title "<issue_title>" --body-file .flow-issue-body
```

After each successful issue, record it:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Flow" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
```

### Documentation drift issues

For each item where documentation is out of sync with actual behavior
(discovered during Step 2 synthesis), file an issue on the target project.

The issue body should describe what is stale and what the current
behavior actually is.

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Documentation Drift" --title "<issue_title>" --body-file .flow-issue-body
```

After each successful issue, record it:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Documentation Drift" --title "<issue_title>" --url "<issue_url>" --phase "flow-learn"
```

If there are no process gap or documentation drift items, skip this step.

---

## Step 7 — Present report

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

  Claude mistakes
  ---------------
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
  [Documentation Drift] #45: README still references old auth flow

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
brackets (e.g. `[Rule]`, `[Flow]`, `[Documentation Drift]`).

---

## Done

### Phase 5 mode

Complete the phase:

```bash
exec ${CLAUDE_PLUGIN_ROOT}/bin/flow phase-transition --phase flow-learn --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.33.0 — Phase 5: Learn — COMPLETE (<formatted_time>)
  Run /flow:flow-complete to merge the PR and clean up.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

<HARD-GATE>
STOP. Re-read `skills.flow-learn.continue` from the state file at
`<project_root>/.flow-states/<branch>.json` before advancing.
The previous phase's continue mode does NOT carry over — each phase
has its own mode.

1. If `--auto` was passed to this skill invocation → continue=auto.
   If `--manual` was passed → continue=manual.
   Otherwise, use the value from the state file. If absent → default to manual.
2. If continue=auto → invoke `flow:flow-complete` directly.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
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
- Always read CLAUDE.md and conversation context before synthesizing findings
- In Phase 5, read all three sources before synthesizing findings
- Follow the learning process (Steps 1 through 7) exactly — do not skip or reorder steps
- Decisions on destinations and wording are autonomous — do not ask the user for approval mid-process
- If the user denies an Edit tool call during Step 3, skip that learning and continue — a denied edit means "skip this one," not "stop the phase"
- The report in Step 7 is the user's review point — make it comprehensive
- CLAUDE.md learnings are edited on disk and committed via `/flow:flow-commit --auto` (Phase 5 and Maintainer)
- Rules learnings (`.claude/rules/`) are filed as GitHub issues with label "Rule" — never edited directly, to avoid permission prompts
- Plugin process gaps are filed as GitHub issues on the plugin repo with label "Flow"
- Documentation drift is filed as GitHub issues on the target project with label "Documentation Drift"
- Only CLAUDE.md and `.claude/settings.json` files are modified on disk — never application code or `.claude/rules/`
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
