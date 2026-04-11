---
name: flow-code-review
description: "Phase 4: Code Review — six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation) launched in parallel. Parent session gathers context, triages findings, and fixes."
---

# FLOW Code Review — Phase 4: Code Review

## Usage

```text
/flow:flow-code-review
/flow:flow-code-review --auto
/flow:flow-code-review --manual
/flow:flow-code-review --continue-step
/flow:flow-code-review --continue-step --auto
/flow:flow-code-review --continue-step --manual
```

- `/flow:flow-code-review` — uses configured mode from the state file (default: manual)
- `/flow:flow-code-review --auto` — auto-fix and auto-commit all findings, auto-advance to Learn
- `/flow:flow-code-review --manual` — requires explicit approval of changes and routing decisions
- `/flow:flow-code-review --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next step via Resume Check

<HARD-GATE>
Run `phase-enter` as your very first action. If it returns an error, stop
immediately and show the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter --phase flow-code-review --steps-total 4
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

## Six Tenants

The Code Review phase assesses the work through six tenants. Every
finding from every agent must map to one of these tenants. Findings
that do not map to a tenant are dropped.

**Tenant 1 — Architecture.** Does the code follow the project's
conventions, rules, and planned approach? Deviations from CLAUDE.md,
`.claude/rules/`, and the implementation plan are findings.

**Tenant 2 — Simplicity.** Is there unnecessary complexity? Duplicated
logic, missed abstractions, over-engineering, conditionals that could be
flattened, names that could be clearer.

**Tenant 3 — Maintainability.** Can a newcomer understand this code
without context from the conversation that produced it? Implicit
assumptions, undocumented patterns, names that only make sense with
tribal knowledge.

**Tenant 4 — Correctness.** Does the code actually work? Logic errors,
edge cases, off-by-one errors, null handling gaps, error propagation,
race conditions, and security vulnerabilities (injection, auth bypass,
data exposure).

**Tenant 5 — Test coverage.** Are the changes adequately tested?
Meaningful assertions, edge cases covered, error paths exercised. Gaps
are proven by adversarial tests that fail.

**Tenant 6 — Documentation.** Do the docs match the code after these
changes? CLAUDE.md, `.claude/rules/`, README, doc comments, and inline
comments that no longer reflect the code's actual behavior.

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
  FLOW v1.1.0 — Phase 4: Code Review — STARTING
──────────────────────────────────────────────────
```
````

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 4] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

## Resume Check

Read `code_review_step` from the state file (default `0` if absent).

- If `1` — Step 1 is done. Skip to Step 2.
- If `2` — Steps 1-2 are done. Skip to Step 3.
- If `3` — Steps 1-3 are done. Skip to Step 4.
- If `4` — All steps are done. Skip to Done.

---

## Step 1 — Gather

Collect all artifacts needed by the agents. No analysis — just
artifact collection.

**Read the plan file.** Read `files.plan` from the state file to get the
plan file path. Use the Read tool to read the plan file.

**Read project conventions.** Use the Read tool to read the project
CLAUDE.md at `<worktree_path>/CLAUDE.md`. Use the Glob tool to find all
`.claude/rules/*.md` files at `<worktree_path>/.claude/rules/*.md`, then
read each file.

**Get the full branch diff.**

```bash
git diff origin/main...HEAD
```

This is the **full diff** — used by the reviewer agent (context-rich).

**Get the substantive diff.**

```bash
git diff origin/main...HEAD -w
```

This is the **substantive diff** — whitespace-only changes filtered out.
Context-sparse agents (pre-mortem, adversarial, documentation) receive
this diff instead of the full diff. On PRs where formatters (cargo fmt,
prettier, black) reformat many files, the substantive diff excludes
formatting noise and preserves the agents' turn budget for behavioral
analysis.

**Derive adversarial test setup.**

The adversarial agent writes a single test file under
`.flow-states/<branch>-adversarial_test.<ext>` and runs it via the
project's `bin/test --file <path>`. The agent picks the extension
itself by looking at the diff (`.rs`, `.py`, `.rb`, `.go`, `.swift`,
`.ts`, etc.) — FLOW no longer dispatches by language, so the choice
lives where the language information actually lives: in the file
contents the agent is reviewing.

Capture these two values for Step 2:

- `<temp_test_file>` = `.flow-states/<branch>-adversarial_test` (the
  agent appends the extension)
- `<test_command>` = `${CLAUDE_PLUGIN_ROOT}/bin/flow test --file <temp_test_file>`

The adversarial agent always launches. If the project's `bin/test`
does not support a `--file` flag (or cannot compile a single file in
isolation), the agent will surface that as a finding rather than
silently skipping.

**Audit tombstone staleness.**

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow tombstone-audit
```

Parse the JSON output. If the `stale` array is non-empty, note the stale
tombstones for removal in Step 4. Each entry has `pr`, `merged_at`, and
`file` fields identifying which test function to remove and from which
file. If the command fails (exit non-zero) or the JSON contains a
`status` field with value `"threshold_error"` or `"error"`, note no
stale tombstones — the audit is best-effort and skipped on API failure.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=1
```

To continue to Step 2, invoke `flow:flow-code-review --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this invocation.

---

## Step 2 — Launch agents

<HARD-GATE>
You MUST launch ALL applicable agents listed below in a single response.
Never skip an agent because another agent already returned findings.
Each agent surfaces independent risk categories that other agents miss —
skipping one defeats cognitive isolation. Do not proceed past this step
until every applicable agent has been launched and returned.
</HARD-GATE>

Launch all applicable agents in a single response using multiple Agent
tool calls. All agents are independent — they share no state and can
run concurrently. Each agent is cognitively isolated from the
conversation that produced the code, eliminating self-reporting bias.

**Reviewer agent** — context-rich (receives diff, plan, CLAUDE.md, rules):

Use the Agent tool with:

- `subagent_type`: `"flow:reviewer"`
- `description`: `"Context-isolated code review"`

Provide all artifacts in the prompt with labeled sections:

> DIFF:
> (full diff output)
>
> PLAN:
> (full plan file content)
>
> CLAUDE.MD:
> (full CLAUDE.md content)
>
> RULES:
> (each .claude/rules/ file, prefixed with its filename)

Prefix the prompt with:

> "You are reviewing code you did not write. The full diff, the plan,
> the project CLAUDE.md, and all project rules are provided inline below.
> Review the diff for architecture adherence, simplicity, correctness,
> and security."

**Pre-mortem agent** — context-sparse (receives only the substantive diff):

Use the Agent tool with:

- `subagent_type`: `"flow:pre-mortem"`
- `description`: `"Pre-mortem incident analysis"`

Provide the substantive diff output in the prompt, prefixed with:

> "This PR was merged and caused a production incident. The substantive
> diff (whitespace-only changes filtered) is below. Investigate the
> codebase and write the incident report. Security failure modes are
> explicitly in scope."

**Adversarial agent** — context-sparse (receives substantive diff, temp
file path, test command, CLAUDE.md path, branch name). Always launch.

Use the `<temp_test_file>` and `<test_command>` derived in Step 1.

Use the Agent tool with:

- `subagent_type`: `"flow:adversarial"`
- `description`: `"Adversarial test generation"`

Provide the substantive diff output in the prompt, along with:

- The temp test file path (`<temp_test_file>`)
- The test command (`<test_command>`)
- The path to the project CLAUDE.md
- The branch name

**Documentation agent** — context-sparse (receives substantive diff, doc paths):

Use the Agent tool with:

- `subagent_type`: `"flow:documentation"`
- `description`: `"Documentation and maintainability review"`

Provide the substantive diff output in the prompt, along with:

- The path to the project CLAUDE.md
- The path to the `.claude/rules/` directory

Prefix the prompt with:

> "You are a new team member reading this PR for the first time. The
> substantive diff (whitespace-only changes filtered) is below.
> Investigate the codebase and documentation for comprehension barriers
> and documentation drift."

Wait for all agents to return.

If the adversarial agent was launched, verify the temp test file was
deleted. If it still exists, delete it:

```bash
rm <temp_test_file>
```

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=2
```

To continue to Step 3, invoke `flow:flow-code-review --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this invocation.

---

## Step 3 — Triage

Triage findings from each agent in order: reviewer, pre-mortem,
adversarial, documentation. For each finding, classify it:

**Real + in-scope** — a credible issue supported by evidence. Apply the
diff-boundary test: if the finding is in a file that appears in
`git diff origin/main...HEAD`, it is in-scope — fix it. This includes
structural issues like duplicate code, missing abstractions, and naming
problems in files the PR created or modified. Route to Step 4 for fixing.

**Real + out-of-scope** — a credible issue in a file that does NOT
appear in the PR diff. The problem pre-dates this PR. File an issue and
move on — do not fix. Never classify a finding as out-of-scope when the
file was created or modified by this PR.

**False positive** — speculative, not supported by the code, or already
covered by tests. Discard with rationale. After classifying each false positive, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "dismissed" --phase "flow-code-review"
```

### Truncation check

Examine each agent's output for expected structure. Valid output contains
`**Finding` blocks with category labels or explicit "No findings" markers.
If an agent's output ends mid-sentence or is missing expected categories,
the agent exhausted its turn budget. Note the incomplete agent in the
triage table so the user knows coverage was partial.

### File out-of-scope issues

For each real + out-of-scope finding, classify as one of:

- **Tech Debt** — working but fragile, duplicated, or convention-violating code
- **Documentation Drift** — docs out of sync with actual behavior

Write the issue body to `.flow-issue-body` in the project root using the
Write tool, then file:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Tech Debt" --title "<issue_title>" --body-file .flow-issue-body
```

Or for documentation:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow issue --label "Documentation Drift" --title "<issue_title>" --body-file .flow-issue-body
```

After filing, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-issue --label "Tech Debt" --title "<issue_title>" --url "<issue_url>" --phase "flow-code-review"
```

After each filed issue, also record the finding:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "filed" --phase "flow-code-review" --issue-url "<issue_url>"
```

### Triage summary

Show each finding with its source agent, tenant, triage decision, and
rationale inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  FLOW — Code Review — Step 3: Triage — SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Reviewer
  --------
  - [T1 Architecture] [REAL] <finding description>
  - [T2 Simplicity] [FALSE POSITIVE] <reason>

  Pre-Mortem
  ----------
  - [T4 Correctness] [REAL] <finding description>

  Adversarial
  -----------
  - [T5 Test coverage] [REAL] <finding description>

  Documentation
  -------------
  - [T6 Documentation] [REAL] <finding description>
  - [T3 Maintainability] [OUT OF SCOPE] filed #123

  Truncated agents: none

  Real findings to fix : N
  Out-of-scope filed   : N
  False positives       : N

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If all agents report no findings, show the triage summary with zero
findings, then skip the commit and proceed directly to Done.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=3
```

To continue to Step 4, invoke `flow:flow-code-review --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this invocation.

---

## Step 4 — Fix

Fix all real in-scope findings from Step 3.

If no real in-scope findings exist, skip this step and proceed to Done.

### Fix each finding

For each real in-scope finding, fix the issue in code. After fixing each finding, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "fixed" --phase "flow-code-review"
```

After fixing all findings, run CI once:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow ci
```

<HARD-GATE>
`bin/flow ci` must be green before committing.
If CI fails, identify the breaking fix and iterate until green.

</HARD-GATE>

### Remove stale tombstones

After fixing all findings and running CI above, remove any stale
tombstones identified in Step 1. For each stale entry:

- Open the `file` from the audit output
- Find the test function guarding the stale PR
- Remove the entire test function (including its doc comment and
  `#[test]` attribute)
- If the removal leaves an empty section comment (e.g.
  `// --- Tombstone tests ---` with no tests below it), remove the
  section comment too

Stale tombstone removal is a mechanical operation — no judgment call
needed. The tombstone-audit command already verified that the PR was
merged before the oldest open PR was created, meaning no active branch
could resurrect the deleted code.

### Back navigation

If a finding is too significant to fix in Code Review:

If commit=auto, fix it directly without asking.

If commit=manual, use AskUserQuestion:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something

**Go back to Code:** update Phase 4 to `pending`, Phase 3 to
`in_progress`, then invoke `flow:flow-code`.

**Go back to Plan:** update Phases 4 and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

### Commit

Set the continuation context and flag before committing.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set code_review_step=4, then self-invoke flow:flow-code-review --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set code_review_step=4, then self-invoke flow:flow-code-review --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Invoke `/flow:flow-commit`.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set code_review_step=4
```

To continue to Done, invoke `flow:flow-code-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Done — Update state and complete phase

Finalize the phase (complete + Slack notification in one call):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-finalize --phase flow-code-review --branch <branch> --thread-ts <slack_thread_ts>
```

Omit `--thread-ts` if `slack_thread_ts` was not returned by `phase-enter`.

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 4: Code Review — COMPLETE (<formatted_time>)
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
2. If continue=auto → invoke `flow:flow-learn` directly using the Skill tool.
   Do NOT invoke `flow:flow-status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Invoke `flow:flow-status`
   b. Use AskUserQuestion:
      "Phase 4: Code Review is complete. Ready to begin Phase 5: Learn?"
      Options: "Yes, start Phase 5 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 5 now" and "Not yet"
   d. If Yes → invoke `flow:flow-learn` using the Skill tool
   e. If Not yet → print the paused banner below
   f. Do NOT invoke `flow:flow-learn` until the user responds

Do NOT skip this check. Do NOT auto-advance when the mode is manual.

</HARD-GATE>

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

````markdown
```text
══════════════════════════════════════════════════
  ◆ FLOW — Paused
  Run /flow:flow-learn when ready.
══════════════════════════════════════════════════
```
````

---

## Hard Rules

- Always run `bin/flow ci` after any fix made during Code Review
- Never transition to Learn unless `bin/flow ci` is green
- Fix every real in-scope finding from agent triage — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- All analysis comes from cognitively isolated agents — the parent session never reviews the diff itself
- Parent session gathers, launches, triages, and fixes — it does not analyze
- Every finding must map to one of the six tenants — findings that do not map are dropped
- One commit for all Code Review fixes (Step 4), not one commit per finding
- After each step completes, advance to the next step via self-invocation — never pause or wait for user input between steps (Gather, Launch, Triage, Fix advance automatically; only the Done HARD-GATE can pause)
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
