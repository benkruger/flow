---
name: flow-learning
description: "Phase 5: Learning — review what went wrong, capture learnings, route each to its correct permanent home. Runs before the PR is merged. The only commits are CLAUDE.md and .claude/ changes."
model: sonnet
---

# Learning

## Usage

```text
/flow:flow-learning
/flow:flow-learning --auto
/flow:flow-learning --manual
```

- `/flow:flow-learning` — uses configured mode from the state file (default: auto)
- `/flow:flow-learning --auto` — skip permission promotion prompts, auto-advance to Cleanup
- `/flow:flow-learning --manual` — prompt for permission promotion and phase transition

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
Use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read the state file at `<project_root>/.flow-states/<branch>.json`. Use `skills.flow-learning.commit` and `skills.flow-learning.continue`.
4. If the state file has no `skills` key → use built-in defaults: commit=auto, continue=auto

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

**Phase 5 mode:**

````markdown
```text
============================================
  FLOW v0.20.1 — Phase 5: Learning — STARTING
============================================
```
````

**Maintainer or Standalone mode:**

````markdown
```text
============================================
  Learning — STARTING
============================================
```
````

## Update State

**Phase 5 only.** Skip for Maintainer and Standalone.

Update state for phase entry:

```bash
bin/flow phase-transition --phase flow-learning --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

No logging for this phase. Learning runs no Bash commands beyond the entry
gate — there is nothing to log.

---

## Step 1 — Gather sources

Read and synthesise before doing anything else.

### Source A — CLAUDE.md rules (all modes)

Read the project's `CLAUDE.md`. These are the rules that should have been
followed. Note every rule and convention entry.

**Note:** Reading `~/.claude/CLAUDE.md` may trigger a Read permission
prompt. This is a known limitation — Claude Code prompts for Read access
to `~/.claude/` paths and this cannot be suppressed via settings.json.
Approve the prompt to continue.

### Source B — Conversation context (all modes)

Review the current conversation for:
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

---

## Step 3 — Route and apply

This step is fully autonomous — decide destinations and apply all changes
without asking the user.

### The 5 destinations

| # | Name | Path | Write method |
|---|------|------|-------------|
| 1 | Global CLAUDE.md | `~/.claude/CLAUDE.md` | Edit directly |
| 2 | Project CLAUDE.md | `CLAUDE.md` in project | Edit on disk |
| 3 | Global rules | `~/.claude/rules/<topic>.md` | Edit directly |
| 4 | Project rules | `.claude/rules/<topic>.md` in project | Edit on disk |
| 5 | Project memory | `~/.claude/projects/<repo-root>/memory/MEMORY.md` | Edit directly |

Destinations 1, 3, 5 are user-private (outside the repo, not committed).
Destinations 2, 4 are on disk — committed in Step 4 if applicable.

### Routing heuristics

Destinations 1-4 are **instructions** — rules Claude must follow.
Destination 5 is **context** — knowledge Claude should know.
Never write to memory (5) what should be an instruction (1-4).

| Learning type | Destination | Example |
|---|---|---|
| Universal process rule | 1 — Global CLAUDE.md | "Always run CI before committing" |
| Project architecture or scope | 2 — Project CLAUDE.md | "Skills are markdown, not code" |
| Universal coding anti-pattern | 3 — Global rules | "Never use update_column in tests" |
| Project-specific coding gotcha | 4 — Project rules | "Use git -C not cd && git" |
| Working knowledge, preferences, TODOs | 5 — Project memory | "User prefers no TaskCreate" |

### Writing rules for CLAUDE.md and rules files

- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session
- Placed in the correct section of the target file

### Apply changes

For each item in "Missing rules":
1. Select a destination using the routing heuristics table
2. Compose a learning entry following the writing rules above
3. Read the target file, apply the addition. Do not duplicate existing content.

For each item in "Process violations":
1. Evaluate whether the existing rule's language was clear enough
2. If the violation happened because the rule was ambiguous or easy to
   overlook, reword the rule at its current destination
3. Read the target file, apply the rewording. Do not duplicate existing content.

### Private destinations (1, 3, 5) — direct edits

For each private destination with changes:
1. Read the target file
2. Apply all additions and rewordings for that destination
3. These are outside the repo — no commit needed

### Repo destinations (2, 4) — committed in Step 4

For each repo destination with changes:
1. Read the target file in the project
2. Apply all additions and rewordings for that destination

---

## Step 4 — Promote local permissions (Maintainer only)

**Skip for Phase 5 and Standalone.**

Invoke `/flow:flow-local-permission`.

If it reports promoted entries, count `.claude/settings.json` as a
repo-destination change for Step 5's commit decision.

---

## Step 5 — Commit (conditional)

**Phase 5:** If any repo-destination changes were made (destinations 2 or
4), commit once via `/flow:flow-commit --auto`. Only CLAUDE.md and `.claude/`
files are committed — never application code. If `git add -A` results in
nothing staged (stealth user with excluded files), skip the commit
gracefully — do not error.

**Maintainer:** If any repo-destination changes were made, commit once via
`/flow:flow-commit --auto`.

**Standalone:** Skip entirely — no commit.

If no repo-destination changes were made, skip this step regardless of mode.

---

## Step 6 — File GitHub issues (Phase 5 only)

Skip for Maintainer and Standalone.

For each item in "Process gaps", file a GitHub issue on the plugin repo:

```bash
gh issue create --repo benkruger/flow --label learning --title "<issue_title>" --body "<issue_body>"
```

The issue title should be a concise description of the process gap. The
issue body should describe the gap generically — no user project details,
no feature-specific context. Focus on what the FLOW process should do
differently.

If there are no process gap items, skip this step.

---

## Step 7 — Present report

Present the full report to the user:

````markdown
```text
============================================
  Learning — Report
============================================

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
  Global CLAUDE.md: 2 additions
  Project rules (.claude/rules/testing.md): 1 addition
  Project CLAUDE.md: 1 addition (committed / uncommitted)

  Issues filed
  ------------
  #42: Commit skill should warn when branch is behind
  #43: Plan skill should prompt for queue awareness

============================================
```
````

Omit "Changes applied" if no changes were made. Omit "Issues filed" if
no issues were filed or not in Phase 5 mode.

In the "Changes applied" section, show "(committed)" or "(uncommitted)"
next to each repo-destination file to indicate whether Step 4 committed it.

---

## Done

### Phase 5 mode

Complete the phase:

```bash
bin/flow phase-transition --phase flow-learning --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.20.1 — Phase 5: Learning — COMPLETE (<formatted_time>)
  Merge the PR, then run /flow:flow-cleanup.
============================================
```
````

Invoke `flow:flow-status`.

**If continue=auto**, skip the transition question and invoke `flow:flow-cleanup` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 5: Learning is complete. The PR now includes CLAUDE.md improvements. Ready to begin Phase 6: Cleanup?"
>
> - **Yes, start Phase 6 now** — invoke `flow:flow-cleanup`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:flow-note` with their message
3. Re-ask with only "Yes, start Phase 6 now" and "Not yet"

**If Yes** — invoke `flow:flow-cleanup` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:flow-continue when ready to continue.
============================================
```
````

### Maintainer and Standalone mode

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  Learning — COMPLETE
============================================
```
````

No phase transition, no transition question.

---

## Hard Rules

- Never commit application code in Learning — only CLAUDE.md and .claude/
- Always read CLAUDE.md and conversation context before synthesizing findings
- In Phase 5, read all three sources before synthesizing findings
- Follow the learning process (Steps 1 through 7) exactly — do not skip or reorder steps
- Decisions on destinations and wording are autonomous — do not ask the user for approval mid-process
- The report in Step 7 is the user's review point — make it comprehensive
- Global writes (`~/.claude/CLAUDE.md`, `~/.claude/rules/`, `~/.claude/projects/`) are direct edits — never committed
- Repo writes (`CLAUDE.md`, `.claude/rules/`) go through `/flow:flow-commit --auto` (Phase 5 and Maintainer)
- Plugin improvement notes are filed as GitHub issues on the plugin repo — never committed to the target project
- Only CLAUDE.md and `.claude/` files are modified — never application code
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
