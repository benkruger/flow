---
name: reflect
description: "Phase 6: Reflect — review what went wrong, capture learnings, route each to its correct permanent home. Runs before the PR is merged. The only commits are CLAUDE.md and .claude/ changes."
model: sonnet
---

# Reflect

<HARD-GATE>
Run this entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
3. **Determine mode:**
   - **State file exists + `phases.5.status` == `"complete"`** → **Phase 6** mode
   - **State file exists + phase 5 incomplete** → STOP. "BLOCKED: Phase 5:
     Security must be complete. Run /flow:security first."
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

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

**Phase 6 mode:**

````markdown
```text
============================================
  FLOW v0.14.0 — Phase 6: Reflect — STARTING
============================================
```
````

**Maintainer or Standalone mode:**

````markdown
```text
============================================
  Reflect — STARTING
============================================
```
````

## Update State

**Phase 6 only.** Skip for Maintainer and Standalone.

Update state for phase entry:

```bash
bin/flow phase-transition --phase 6 --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

No logging for this phase. Reflect runs no Bash commands beyond the entry
gate — there is nothing to log.

---

## Step 1 — Gather sources

Read and synthesise before doing anything else.

### Source A — CLAUDE.md rules (all modes)

Read the project's `CLAUDE.md`. These are the rules that should have been
followed. Note every rule, convention, and lesson learned entry.

### Source B — Conversation context (all modes)

Review the current conversation for:
- Moments where the user corrected Claude
- Responses where Claude was overruled or pushed back
- Misunderstandings that required clarification
- Suggestions Claude made that were rejected

Note: context may have been compacted. Use what is available.

### Source C — State file and plan file data (Phase 6 only)

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
captured during the session via `/flow:note`. They are the most direct
signal of what went wrong.

### Source D — Worktree auto-memory (Phase 6 only)

Skip for Maintainer and Standalone.

Claude writes auto-memory during feature work to a path scoped to the
worktree. This memory will be lost when Cleanup removes the worktree.

1. Read `state["worktree"]` to get the worktree absolute path
2. Escape the path: replace `/` with `-`, drop the leading `-`
   (e.g. `/Users/ben/code/hh/.worktrees/my-feature` becomes
   `Users-ben-code-hh-.worktrees-my-feature`)
3. Read `~/.claude/projects/<escaped-path>/memory/MEMORY.md`
   Use the Read tool for this — the path is outside the project directory
   and Bash cat would trigger a permission prompt.
4. If it exists, include its contents as evidence alongside Sources A-C
5. If it does not exist (no auto-memory was written), skip silently

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

**Worth preserving** (Phase 6 only) — items from the worktree auto-memory
(Source D) that contain useful patterns, observations, or context that
future sessions should know. Filter for durable value — not everything in
auto-memory is worth keeping. Skip this category if Source D was empty or
did not exist, or if not in Phase 6 mode.

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

| Learning type | Recommended destination |
|---|---|
| Process/behavior rule ("always X before Y") | 1 — Global CLAUDE.md |
| Project architecture discovery | 2 — Project CLAUDE.md |
| Universal coding style or anti-pattern | 3 — Global rules |
| Project-specific coding gotcha | 4 — Project rules |
| Informal pattern, observation, ephemeral note | 5 — Project memory |

### Writing rules for CLAUDE.md and rules files

- Write for Claude, not for humans — the audience is a future Claude session
- Be direct, specific, and actionable — describe the exact situation and the
  exact required behavior
- One to three sentences maximum
- Generic and reusable — not tied to the specific feature or session
- Placed in the correct section of the target file

### Apply changes

For each item in "Missing rules" and "Worth preserving":
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

### Worktree memory rescue (Phase 6 only)

Skip for Maintainer and Standalone.

If Source D contained items that were routed to a destination above, they
are already handled. For any remaining useful items in the worktree
auto-memory that were not surfaced as findings, merge them into project
memory (destination 5: `~/.claude/projects/<repo-root>/memory/MEMORY.md`)
so they survive cleanup.

To determine `<repo-root>`: read `state["worktree"]`. The worktree is
inside the project (e.g., `/Users/ben/code/hh/.worktrees/my-feature`).
The repo root is the worktree's parent's parent — strip `.worktrees/<name>`
(e.g., `/Users/ben/code/hh`). Escape: replace `/` with `-`, drop the
leading `-`.

---

## Step 4 — Commit (conditional)

**Phase 6:** If any repo-destination changes were made (destinations 2 or
4), commit once via `/flow:commit --auto`. Only CLAUDE.md and `.claude/`
files are committed — never application code. If `git add -A` results in
nothing staged (stealth user with excluded files), skip the commit
gracefully — do not error.

**Maintainer:** If any repo-destination changes were made, commit once via
`/flow:commit --auto`.

**Standalone:** Skip entirely — no commit.

If no repo-destination changes were made, skip this step regardless of mode.

---

## Step 5 — File GitHub issues (Phase 6 only)

Skip for Maintainer and Standalone.

For each item in "Process gaps", file a GitHub issue on the plugin repo:

```bash
gh issue create --repo benkruger/flow --label reflect --title "<issue_title>" --body "<issue_body>"
```

The issue title should be a concise description of the process gap. The
issue body should describe the gap generically — no user project details,
no feature-specific context. Focus on what the FLOW process should do
differently.

If there are no process gap items, skip this step.

---

## Step 6 — Present report

Present the full report to the user:

````markdown
```text
============================================
  Reflect — Report
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
  - /flow:commit should warn when branch is behind
  - ...

  Worth preserving (from worktree memory)
  ----------------------------------------
  - Tests with Time.zone.now fail near midnight
  - ...

  Changes applied
  ---------------
  Global CLAUDE.md: 2 additions
  Project rules (.claude/rules/testing.md): 1 addition
  Project memory: 3 items rescued from worktree
  Project CLAUDE.md: 1 addition (committed / uncommitted)

  Issues filed
  ------------
  #42: Commit skill should warn when branch is behind
  #43: Plan skill should prompt for queue awareness

============================================
```
````

Omit the "Worth preserving" section if not in Phase 6 mode, or if Source D
was empty or had nothing worth keeping. Omit "Changes applied" if no
changes were made. Omit "Issues filed" if no issues were filed or not in
Phase 6 mode.

In the "Changes applied" section, show "(committed)" or "(uncommitted)"
next to each repo-destination file to indicate whether Step 4 committed it.

---

## Done

### Phase 6 mode

Complete the phase:

```bash
bin/flow phase-transition --phase 6 --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.14.0 — Phase 6: Reflect — COMPLETE (<formatted_time>)
  Merge the PR, then run /flow:cleanup.
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 6: Reflect is complete. The PR now includes CLAUDE.md improvements. Ready to begin Phase 7: Cleanup?"
>
> - **Yes, start Phase 7 now** — invoke `flow:cleanup`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 7 now" and "Not yet"

**If Yes** — invoke `flow:cleanup` using the Skill tool.

**If Not yet**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

### Maintainer and Standalone mode

Print inside a fenced code block:

````markdown
```text
============================================
  Reflect — COMPLETE
============================================
```
````

No phase transition, no transition question.

---

## Hard Rules

- Never commit application code in Reflect — only CLAUDE.md and .claude/
- Always read CLAUDE.md and conversation context before synthesizing findings
- In Phase 6, read all four sources before synthesizing findings
- Follow the reflection process (Steps 1 through 6) exactly — do not skip or reorder steps
- Decisions on destinations and wording are autonomous — do not ask the user for approval mid-process
- The report in Step 6 is the user's review point — make it comprehensive
- Global writes (`~/.claude/CLAUDE.md`, `~/.claude/rules/`, `~/.claude/projects/`) are direct edits — never committed
- Repo writes (`CLAUDE.md`, `.claude/rules/`) go through `/flow:commit --auto` (Phase 6 and Maintainer)
- Plugin improvement notes are filed as GitHub issues on the plugin repo — never committed to the target project
- Only CLAUDE.md and `.claude/` files are modified — never application code
