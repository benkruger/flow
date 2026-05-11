---
name: flow-review
description: "Phase 3: Review — six tenants assessed by four cognitively isolated agents (reviewer, pre-mortem, adversarial, documentation) launched in parallel. Parent session gathers context, triages findings, and fixes."
---

# FLOW Review — Phase 3: Review

## Usage

```text
/flow:flow-review
/flow:flow-review --auto
/flow:flow-review --manual
/flow:flow-review --continue-step
/flow:flow-review --continue-step --auto
/flow:flow-review --continue-step --manual
```

- `/flow:flow-review` — uses configured mode from the state file (default: manual)
- `/flow:flow-review --auto` — auto-fix and auto-commit all findings, auto-advance to Learn
- `/flow:flow-review --manual` — requires explicit approval of changes and routing decisions
- `/flow:flow-review --continue-step` — self-invocation: skip Announce and Update State, dispatch to the next step via Resume Check

<HARD-GATE>
Run `phase-enter` as your very first action. If it returns an error, stop
immediately and show the error to the user.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-enter --phase flow-review --steps-total 4
```

Parse the JSON output. If `"status": "error"`, STOP and show the error.

If `"status": "ok"`, capture the returned fields:
`project_root`, `branch`, `worktree_path`, `worktree_cwd`,
`relative_cwd`, `pr_number`, `pr_url`, `feature`, `slack_thread_ts`,
`plan_file`, and `mode` (commit + continue).

</HARD-GATE>

Use the returned fields for all downstream references. Do not re-read
the state file or re-run git commands to gather the same information.
Do not `cd` to the project root — `bin/flow` commands find paths
internally.

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

## Six Tenants

The Review phase assesses the work through six tenants. Every
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

**Tenant 5 — Test coverage.** Every production line must be exercised by
a named test. Any uncovered line is a Real finding. Meaningful
assertions, edge cases covered, error paths exercised. Gaps are proven
by adversarial tests that fail.

**Tenant 6 — Documentation.** Do the docs match the code after these
changes? CLAUDE.md, `.claude/rules/`, README, doc comments, and inline
comments that no longer reflect the code's actual behavior.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone. Never
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
  FLOW v1.1.0 — Phase 3: Review — STARTING
──────────────────────────────────────────────────
```
````

## Logging

After every Bash command completes, log it to `.flow-states/<branch>/log`
using `bin/flow log`.

Run the command first, then log the result. Pipeline the log call with the
next command where possible (run both in parallel in one response).

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow log <branch> "[Phase 3] Step X — desc (exit EC)"
```

Get `<branch>` from the state file.

## Resume Check

Read `review_step` from the state file (default `0` if absent).

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

**Resolve the integration branch.** Before constructing the diff
ranges, run `bin/flow base-branch` to retrieve the base branch the
flow coordinates against (the integration branch captured at
flow-start). Capture its stdout — call the value `<base_branch>` —
and substitute it into the diff commands below. A repo whose
default branch is `staging` produces `<base_branch> = staging`; a
standard repo produces `<base_branch> = main`.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow base-branch
```

**Capture both diffs to files.** `bin/flow capture-diff` runs both
`git diff origin/<base_branch>...HEAD` (full) and the `-w` variant
(substantive — whitespace-only changes filtered out) and writes the
results to canonical paths under `.flow-states/<branch>/`. The agents
read those files via the Read tool instead of receiving the diff
bytes inline in their prompts, keeping the parent skill's prompt
budget bounded as PR size grows. Substitute `<branch>` with the
flow's branch name and `<base_branch>` with the value captured
above.

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow capture-diff --branch <branch> --base <base_branch>
```

Parse the JSON output. The `full` field is the path to the full
diff file — call it `<full_diff_file>` and pass it to the reviewer
agent (context-rich). The `substantive` field is the path to the
substantive diff file — call it `<substantive_diff_file>` and pass
it to the pre-mortem, adversarial, and documentation agents
(context-sparse). On PRs where formatters (cargo fmt, prettier,
black) reformat many files, the substantive diff excludes
formatting noise and preserves the agents' turn budget for
behavioral analysis.

**Compute affected doc paths.**

The documentation agent's turn budget is bounded; pointing it at the
full `<worktree_path>/docs/` tree on moderately-sized PRs causes
autocompact-thrash before findings are produced. The skill computes a
narrowed list of doc paths likely affected by the diff via filename
heuristics, and the agent investigates ONLY those paths.

Capture the changed-file list:

```bash
git diff --name-only origin/<base_branch>...HEAD
```

From the changed-file list, derive `<doc_paths>` (a list of paths
under `<worktree_path>` to embed inline in the documentation agent
prompt):

- For each `skills/<name>/SKILL.md` in the diff → add
  `<worktree_path>/docs/skills/<name>.md`
- For each phase skill (`skills/flow-<phase>/SKILL.md`) in the diff →
  also add `<worktree_path>/docs/phases/phase-<N>-<phase>.md` (look up
  `<N>` from `flow-phases.json` — phase order is the array index plus
  one)
- For each `.claude/rules/*.md` in the diff → include the rule path
  itself so the agent can check cross-references in sibling rule files
- Always include `<worktree_path>/CLAUDE.md`
- If the diff touches `src/state.rs`, `src/phase_*.rs`, or
  `flow-phases.json` → add
  `<worktree_path>/docs/reference/flow-state-schema.md`
- If the diff touches `agents/*.md` → add
  `<worktree_path>/docs/reference/agents.md` if it exists

Capture this list as `<doc_paths>` for use in Step 2. The narrowed
list is exhaustive for documentation drift in this PR — the
documentation agent does not need to walk the full docs tree.

**Derive adversarial test setup.**

The adversarial agent writes a single probe test file inside the
project's test tree so the language test runner can discover and
execute it. The exact path is owned by the project — declared via
the project's `bin/test --adversarial-path` invocation — the same
way each project owns the four `bin/{format,lint,build,test}`
toolchain decisions. Worktree removal at Phase 5 Complete disposes
of the probe as a side effect of removing the worktree directory,
so no separate cleanup hook is needed.

Run this from the current working directory (the agent's
`worktree_cwd` captured at flow-start — the worktree root for
project-root flows, or the service subdirectory
`.worktrees/<branch>/<service>/` for mono-repo flows) and capture
stdout:

```bash
bin/test --adversarial-path
```

Strip trailing whitespace (newline, spaces, tabs) from the
captured stdout before using the value — `bin/test` is project-
owned bash that may print the path via `echo` (which appends a
newline) or include trailing whitespace from line-continuation
quoting. The contract is a single-line path; the skill normalizes
defensively.

<HARD-GATE>
If `bin/test` exits 2, surface the stderr message and halt — the
project must configure `bin/test --adversarial-path` (uncomment
the runner block and set the matching path comment in `bin/test`)
before Review can run. Do NOT proceed to Step 2.

This is an infrastructure halt, not a decision point. There is a
single option only — fix the unconfigured stub: uncomment the
runner block in `bin/test` and set the matching path comment, then
re-run Review. Do NOT enumerate alternatives ("(1) skip the
adversarial agent, (2) abort the workflow, (3) configure
`bin/test`") — every non-fix path silently weakens the quality
gate.

Per `.claude/rules/anti-patterns.md` "Never Offer to Skip Workflow
Steps" and `.claude/rules/fix-infrastructure-bugs.md` "Fix
Infrastructure Bugs Immediately": when an infrastructure halt
fires inside a workflow, the response is to fix the underlying
problem and resume — never to bypass the step that detected it.

</HARD-GATE>

The returned path may be absolute or relative. A relative path is
resolved against the cwd you ran `bin/test --adversarial-path`
from (the `worktree_cwd`), so it lands inside the worktree. An
absolute path must already point inside the worktree — the
`validate-worktree-paths` hook will block the adversarial agent's
Write tool call otherwise; surface that rejection as a finding if
it happens. The recommended convention is a path relative to the
cwd, matching the `assets/bin-stubs/test.sh` examples
(`tests/test_adversarial_flow.rs`,
`test/adversarial_flow_test.rb`, etc.).

Capture these two values for Step 2 (use the trimmed path
verbatim, including extension — the project's `bin/test` owns
both):

- `<temp_test_file>` = (trimmed output of `bin/test --adversarial-path`)
- `<test_command>` = `${CLAUDE_PLUGIN_ROOT}/bin/flow ci --test --file <temp_test_file>`

The adversarial agent always launches when `bin/test
--adversarial-path` returns a configured path. If the project's
`bin/test` does not support a `--file` flag (or cannot compile a
single file in isolation), the agent will surface that as a
finding rather than silently skipping.

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
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set review_step=1
```

To continue to Step 2, invoke `flow:flow-review --continue-step`
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

Provide all artifacts in the prompt with labeled sections. The diff
is passed as a file path the agent reads via the Read tool rather
than as inline bytes:

> DIFF_FILE: <full_diff_file>
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

> "You are reviewing code you did not write. The path to the full
> diff (DIFF_FILE) is provided below; Read it via the Read tool
> before analyzing. The plan, the project CLAUDE.md, and all project
> rules are provided inline. Review the diff for architecture
> adherence, simplicity, correctness, and security."

**Pre-mortem agent** — context-sparse (receives only the substantive diff):

Use the Agent tool with:

- `subagent_type`: `"flow:pre-mortem"`
- `description`: `"Pre-mortem incident analysis"`

Provide the substantive diff as a file path the agent reads via
the Read tool. Embed `SUBSTANTIVE_DIFF_FILE: <substantive_diff_file>`
in the prompt and prefix with:

> "This PR was merged and caused a production incident. The path to
> the substantive diff (whitespace-only changes filtered) is provided
> below (SUBSTANTIVE_DIFF_FILE). Read the file via the Read tool
> before analyzing. Investigate the codebase and write the incident
> report. Security failure modes are explicitly in scope."

**Adversarial agent** — context-sparse (receives substantive diff, temp
file path, test command, CLAUDE.md path, branch name). Always launch.

Use the `<temp_test_file>` and `<test_command>` derived in Step 1.
The path is supplied verbatim — the project's `bin/test
--adversarial-path` already chose it (including the file
extension), so the agent does not pick a path or extension itself.

Use the Agent tool with:

- `subagent_type`: `"flow:adversarial"`
- `description`: `"Adversarial test generation"`

Provide the substantive diff as a file path the agent reads via
the Read tool. Embed `SUBSTANTIVE_DIFF_FILE: <substantive_diff_file>`
in the prompt, along with:

- The temp test file path (`<temp_test_file>`, including extension)
- The test command (`<test_command>`)
- The path to the project CLAUDE.md
- The branch name

The agent must Read the substantive diff file before analyzing.

**Documentation agent** — context-sparse (receives substantive diff,
narrowed doc-paths list, doc roots):

Use the Agent tool with:

- `subagent_type`: `"flow:documentation"`
- `description`: `"Documentation and maintainability review"`

Provide the substantive diff as a file path the agent reads via
the Read tool. Embed `SUBSTANTIVE_DIFF_FILE: <substantive_diff_file>`
in the prompt, along with:

- The narrowed list of doc paths (`<doc_paths>` from Step 1) — embed
  inline, one path per line, under a `DOC_PATHS:` header
- The path to the project CLAUDE.md
- The path to the `.claude/rules/` directory (for cross-reference
  checks only — the agent must investigate ONLY the listed doc paths
  for documentation drift, not the full `<worktree_path>/docs/` tree)

Prefix the prompt with:

> "You are a new team member reading this PR for the first time. The
> path to the substantive diff (whitespace-only changes filtered) is
> provided below (SUBSTANTIVE_DIFF_FILE). Read the diff file via the
> Read tool before analyzing, along with the NARROWED LIST of doc
> paths likely affected by this PR (under the DOC_PATHS header). Read
> each listed doc path and check it against the diff for drift. Do
> NOT walk the full `<worktree>/docs/` tree — the listed paths are
> exhaustive for documentation drift in this PR. Investigate the
> codebase for comprehension barriers as usual."

Wait for all agents to return.

**Classify each agent's response.** Apply the three classes in
priority order — truncation first, external failure second,
normal completion otherwise. Priority order matters because a
truncated agent's prose may coincidentally contain
external-failure substrings, but the correct response to
truncation is re-invocation against a narrowed partition, not
recording the agent as skipped.

**Class 1 — Truncation.** For each high-investigation agent
(reviewer, learn-analyst, documentation), check whether the
returned output contains the literal `END-OF-FINDINGS`
completion marker as the final structural element. Marker
absence alone means the agent was truncated by `maxTurns`
exhaustion — regardless of whether any partial `**Finding`
block was produced. An agent that exhausts its turn budget
DURING investigation (before producing any finding) is the
case the recovery path most needs to catch: requiring a partial
finding to trigger Class 1 would silently classify
early-truncation as "found nothing." See
`.claude/rules/cognitive-isolation.md` "Context Budget +
Truncation Recovery" — "Absence of the marker means the agent
was truncated, not that it found nothing."

When truncation is detected on an agent:

1. Identify the partition strategy. For documentation (a
   `DOC_PATHS:`-driven agent), partition the diff by file family
   (`src/`, `tests/`, `agents/`, `skills/`, `.claude/`, `docs/`).
   For reviewer, partition by tenant family (architecture +
   simplicity vs. correctness + security). For learn-analyst,
   partition by tenant (process gap, rule compliance, missing
   rule).
2. Re-invoke the truncated agent once per non-empty partition,
   with the scope narrowed to that partition only. Keep the
   agent's other inputs (plan, CLAUDE.md, rules, narrowed
   doc-paths list) unchanged.
3. Combine findings from every invocation as if they had come
   from a single run. Each finding still maps to one of the
   six tenants for triage in Step 3.

If a re-invocation itself returns without the completion marker,
that is double-truncation — the partition was still too large.
Note the agent as truncated in the triage summary (Step 3)
rather than splitting infinitely. The user decides whether to
accept partial coverage or rerun Review on a smaller
subset of the diff.

**Class 2 — External failure.** When the agent has produced
zero structured `**Finding` blocks AND no `END-OF-FINDINGS`
marker AND the response contains a canonical external-failure
marker (`rate_limit`, `429`, `usage_limit`, `API Error`, `rate
limit exceeded`), the agent hit an upstream API or quota
failure rather than running out of turns mid-investigation. The
agent has produced no findings the parent can use, but a future
flow-review re-invocation could succeed.

The zero-findings precondition is load-bearing: an agent that
produced one or more `**Finding` blocks AND mentions
`rate_limit` in its prose (e.g., a security finding about
rate-limiting code) is NOT externally-failed — it is a normal
completion with findings that happen to discuss the term. Class
2 only fires when the response is structurally empty (no
findings at all) and the failure marker is the explanation.

Substring match is ASCII-case-insensitive on the agent's full
response. False positives in this class silently discard
legitimate findings (the agent gets recorded as skipped and
its `**Finding` blocks never reach Step 3 triage), so the
zero-findings precondition exists specifically to prevent the
substring-in-prose case.

Classify the reason from the marker observed:

- `rate_limit`, `rate limit exceeded`, `429` → `rate_limit`
- `API Error`, `usage_limit` → `api_error`
- Anything else that signals upstream skip → `other`

Then invoke `bin/flow add-skipped-agent` to record the entry so
`phase-finalize` knows to gate the Done step:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-skipped-agent --branch <branch> --agent <name> --reason <classified>
```

Do not retry the agent in this step. The Done section's handler
for the `agents_skipped` error reason gives the user the
retry/accept/abort choice once all agents have been classified.

**Class 3 — Normal completion.** The response contains the
`END-OF-FINDINGS` marker (high-investigation agents) or has
exited cleanly (other agents). Findings flow to Step 3 triage
unchanged.

The probe file lives inside the worktree's test tree, so worktree removal at Phase 5 Complete (or `/flow:flow-abort`) disposes of it automatically as a side effect of `git worktree remove`. The basename glob is also pre-listed in `.git/info/exclude` (`test_adversarial_flow.*`, `*_adversarial_flow_test.rb`) so the throwaway probe never appears in a user's `git status` output alongside intentional changes.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set review_step=2
```

To continue to Step 3, invoke `flow:flow-review --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this invocation.

---

## Step 3 — Triage

Triage findings from each agent in order: reviewer, pre-mortem,
adversarial, documentation. For each finding, classify it as **Real**
(fix in Step 4) or **False positive** (dismiss with rationale).

There is no filing path. All real findings are fixed during Code
Review — see `.claude/rules/review-scope.md`.

### Supersession check

Run the supersession check before classification. The supersession
test catches code that the current PR has made permanently redundant
— code that would leave the PR's behavior unchanged if deleted.
Routing such code to Step 4 for deletion regardless of file location
keeps dead-on-merge code from surviving into main.

Run the supersession test from `.claude/rules/supersession.md`. For
every finding, ask: **"Would deleting the code this finding describes
leave the PR's behavior unchanged?"**

If yes, the finding is in-scope for deletion regardless of which file
the code lives in — route it to Step 4 for deletion.

If no, proceed with the Real / False positive classification below.

If uncertain whether the code is superseded, treat as "no" and proceed
with the classification.

### Classification

**Real** — a credible issue supported by evidence. Includes structural
issues like duplicate code, missing abstractions, and naming problems.
Route to Step 4 for fixing. All real findings are fixed in this PR —
regardless of which file they live in.

**False positive** — speculative, not supported by the code, or already
covered by tests. Discard with rationale. After classifying each false positive, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "dismissed" --phase "flow-review"
```

### Truncation check

Step 2's recovery path re-invokes any agent that returned without the
`END-OF-FINDINGS` completion marker (per
`.claude/rules/cognitive-isolation.md` "Context Budget + Truncation
Recovery"), so by Step 3 every high-investigation agent's output
either ends with the marker (natural completion) or has been
re-invoked across partitions until it does — with the combined
findings already merged into a single set for that agent.

The remaining truncation-detection responsibility in Step 3 is for
the **double-truncation** case: an agent whose Step 2 re-invocation
itself returned without the marker. Note that agent in the triage
summary so the user knows coverage was partial. Pre-mortem and
adversarial agents do not declare the marker (their investigation
surface is naturally bounded by the diff itself); judge their
completeness by whether they produced at least one structured
`**Finding` block or an explicit "No findings" report.

### Triage summary

Show each finding with its source agent, tenant, triage decision, and
rationale inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  FLOW — Review — Step 3: Triage — SUMMARY
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

  Truncated agents: none

  Real findings to fix : N
  False positives      : N

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

If all agents report no findings, show the triage summary with zero
findings, then skip the commit and proceed directly to Done.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set review_step=3
```

To continue to Step 4, invoke `flow:flow-review --continue-step`
using the Skill tool as your final action. If commit=auto was resolved,
pass `--auto` as well. Do not output anything else after this invocation.

---

## Step 4 — Fix

Fix all real findings from Step 3.

If no real findings exist, skip this step and proceed to Done.

### Fix each finding

For each real finding, fix the issue in code. After fixing each finding, record it:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow add-finding --finding "<description>" --reason "<reason>" --outcome "fixed" --phase "flow-review"
```

After fixing all findings, run CI once. Use a 10-minute Bash tool
timeout (`timeout: 600000`) — CI runs can take 3–4 minutes and the
default 2-minute timeout would background the process, defeating the
gate (per `.claude/rules/ci-is-a-gate.md`).

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

If a finding is too significant to fix in Review:

If commit=auto, fix it directly without asking.

If commit=manual, use AskUserQuestion:

> - **Go back to Code** — implementation issue
> - **Go back to Plan** — plan was missing something

**Go back to Code:** update Phase 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-code`.

**Go back to Plan:** update Phases 4 and 3 to `pending`, Phase 2 to
`in_progress`, then invoke `flow:flow-plan`.

### Commit

Set the continuation context and flag before committing.

If commit=auto, use the first form. If commit=manual, use the second:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set review_step=4, then self-invoke flow:flow-review --continue-step --auto."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set "_continue_context=Set review_step=4, then self-invoke flow:flow-review --continue-step --manual."
```

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set _continue_pending=commit
```

Invoke `/flow:flow-commit`.

Record step completion:

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow set-timestamp --set review_step=4
```

To continue to Done, invoke `flow:flow-review --continue-step` using
the Skill tool as your final action. If commit=auto was resolved, pass
`--auto` as well. Do not output anything else after this invocation.

---

## Done — Update state and complete phase

Finalize the phase (complete + Slack notification in one call):

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow phase-finalize --phase flow-review --branch <branch> --thread-ts <slack_thread_ts>
```

Omit `--thread-ts` if `slack_thread_ts` was not returned by `phase-enter`.

Parse the JSON output.

**Handle the `agents_skipped` error reason.** When the response
shape is `{"status":"error","reason":"agents_skipped","skipped":[...],"message":"..."}`,
one or more review agents were classified as skipped in Step 2.
The phase has not been advanced — the gate ran before any
state mutation so the user can decide how to proceed.

Present the skipped list inline (one entry per line:
`<agent> — <reason> @ <timestamp>`), then use AskUserQuestion:

> "Review left <count> agents skipped (<reasons>). How should we
> proceed?"
>
> Options:
>
> - **Retry the skipped agents** — re-invoke just the skipped
>   agents from Step 2 (reusing the same `<full_diff_file>` and
>   `<substantive_diff_file>` paths captured in Step 1). After
>   the retry, re-run `phase-finalize` without
>   `--accept-skipped-agents`; the gate fires again if any
>   retried agent skips a second time, so the user can iterate.
> - **Proceed with --accept-skipped-agents** — re-run
>   `phase-finalize --phase flow-review --branch <branch>
>   --thread-ts <slack_thread_ts> --accept-skipped-agents`. The
>   skipped entries stay in state for the Learn phase audit but
>   Review advances.
> - **Abort back to Code phase** — invoke
>   `bin/flow phase-transition --action back --phase flow-code`
>   to drop Review's in-progress markers and return to Code. The
>   skipped entries remain in state for forensic purposes; the
>   next Review entry starts with a fresh classification pass.

Do NOT advance to the COMPLETE banner until the user picks one
of the three options and the chosen path returns a
`{"status":"ok",...}` envelope.

When the response is `{"status":"error", ...}` for any OTHER
reason, report the error and stop.

Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — Phase 3: Review — COMPLETE (<formatted_time>)
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
   Do NOT run `bin/flow status`. Do NOT use AskUserQuestion.
   This is the FINAL action in this response — nothing else follows.
3. If continue=manual → you MUST do all of the following before proceeding:
   a. Run `bin/flow status` via Bash and print its stdout in your
      response inside a fenced code block:

      ```bash
      ${CLAUDE_PLUGIN_ROOT}/bin/flow status
      ```

   b. Use AskUserQuestion:
      "Phase 3: Review is complete. Ready to begin Phase 4: Learn?"
      Options: "Yes, start Phase 4 now", "Not yet",
      "I have a correction or learning to capture"
   c. If "I have a correction or learning to capture":
      ask what to capture, invoke `/flow:flow-note`, then re-ask with
      only "Yes, start Phase 4 now" and "Not yet"
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

- Always run `bin/flow ci` after any fix made during Review
- Never transition to Learn unless `bin/flow ci` is green
- Fix every real finding from agent triage — do not leave findings unaddressed
- Follow the project CLAUDE.md conventions when fixing
- All analysis comes from cognitively isolated agents — the parent session never reviews the diff itself
- Parent session gathers, launches, triages, and fixes — it does not analyze
- Every finding must map to one of the six tenants — findings that do not map are dropped
- One commit for all Review fixes (Step 4), not one commit per finding
- After each step completes, advance to the next step via self-invocation — never pause or wait for user input between steps (Gather, Launch, Triage, Fix advance automatically; only the Done HARD-GATE can pause)
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
- When in autonomous mode, classify tool failures per `.claude/rules/autonomous-flow-self-recovery.md` — mechanical fixes are in-flow, substantive failures prompt the user
- Never discard uncommitted changes to unblock a workflow step — if any git command fails due to uncommitted changes, show `git diff` to the user and ask how to proceed
