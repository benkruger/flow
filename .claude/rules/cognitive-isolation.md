# Cognitive Isolation

## When to Use Foreground Sub-Agents

When a phase needs debiased analysis of work done in the current
session, run the analysis in a foreground sub-agent. The sub-agent
receives only persisted artifacts — never conversation history.
The parent session stays alive to receive results and continue
the flow.

This pattern exists because the model that built the feature
carries forward its emotional arc — struggles, negotiations,
rationalizations. Inline analysis in the same session produces
self-reporting bias: obvious mistakes get caught, but deep
assumptions feel like facts and go unexamined.

## Two-Tier Context Model

Not all sub-agents receive the same artifacts. The amount of
context is a design choice matched to the agent's task:

- **Context-rich** (reviewer) — receives the full diff (as a file
  path it Reads) plus the plan, CLAUDE.md, and the rule corpus
  inline. Its task is checking against known standards where having
  the standards at hand saves turns.
- **Context-sparse** (pre-mortem, adversarial, documentation,
  learn-analyst) — receive the substantive diff as a file path
  (`git diff -w`, whitespace-only changes filtered) and must
  investigate the standards themselves. Less context forces
  independent investigation, surfacing risks and coverage gaps that
  pre-supplied context would mask, and keeps the prompt bounded so a
  large diff cannot overflow it and starve the agent of findings.
  The whitespace filter preserves turn budget on PRs with formatting
  changes. The documentation agent receives a narrowed list of doc
  paths derived from the diff (see `skills/flow-review/SKILL.md`
  Step 1) so its investigation surface stays bounded on
  moderately-sized PRs. Learn-analyst keeps two small artifacts
  inline — the state file data (visit counts, timings, session
  notes) and the plan — but reads CLAUDE.md and the full
  `.claude/rules/` corpus on demand; it must read the whole corpus
  rather than a diff-narrowed subset, because a diff under `src/`
  can violate a prose-authoring rule no path heuristic would
  surface. The state file data is only meaningful against known
  process expectations — a high visit count signals friction only
  if you know the expected count is one.

This asymmetry is intentional. See `agents/pre-mortem.md` Design
Note for the full rationale and `agents/reviewer.md` Design Note
for the cross-reference.

## Silent Truncation on maxTurns Exhaustion

Claude Code sub-agents stop silently after reaching their
`maxTurns` ceiling. They produce no error signal — the response
simply ends mid-sentence. The parent skill detects this by
checking the returned output for the literal `END-OF-FINDINGS`
completion marker every high-investigation agent declares in its
Output Format section. Absence of the marker means the agent was
truncated, not that it found nothing. See "Context Budget +
Truncation Recovery" below for the recovery path.

## Context Budget + Truncation Recovery

The `maxTurns` ceiling is the soft enforcer of agent context
budget, but agents that walk large investigation surfaces (full
`docs/` tree, full `src/` tree across many files, all rule files)
routinely exhaust it before producing findings — autocompact-
thrash on moderately-sized PRs has been observed at 362% context
utilization with zero findings returned. The discipline below
makes truncation detectable AND recoverable instead of silent and
lossy.

### Target context utilization

Aim for **250–300% maximum context utilization** at agent return
time. Beyond ~300%, autocompact-thrash dominates and the agent
spends its remaining turns paging context in and out without
producing analysis. Two levers keep the budget bounded:

- **Narrow the investigation surface upstream.** The skill computes
  a narrowed list of artifacts (doc paths, file families, tenant
  scope) and embeds it in the agent prompt instead of pointing at
  a directory root. Reference: the documentation agent's
  `DOC_PATHS:` contract, derived by
  `skills/flow-review/SKILL.md` Step 1 from
  `git diff --name-only`.
- **Split when narrowing is not enough.** When even the narrowed
  surface exceeds the budget, partition the input and re-invoke
  the agent once per partition. See "Partition strategies" below.

### Completion-marker contract

Every high-investigation agent (reviewer, learn-analyst,
documentation) declares a literal `## END-OF-FINDINGS` marker as
the final output of its response. The marker tells the parent
skill the agent reached the natural end of its analysis rather
than running out of turns mid-finding.

The marker is a structural contract, not advice. The skill
detects truncation by **marker absence** rather than by guessing
from prose shape (mid-sentence ends, missing expected categories)
— absence is unambiguous.

A contract test in `tests/skill_contracts.rs` asserts every
high-investigation agent declares the marker in its Output Format
section (`reviewer_agent_declares_end_of_findings_marker`,
`learn_analyst_agent_declares_end_of_findings_marker`,
`documentation_agent_declares_end_of_findings_marker`). New
high-investigation agents added to `agents/` MUST declare the
marker AND extend the contract test with a per-agent sibling.

### Skill-side detection and re-invocation

When the parent skill receives an agent's response:

1. **Marker present.** The agent reached natural end. Use the
   findings as-is; proceed to triage.
2. **Marker absent.** The agent was truncated. Re-invoke the agent
   with a narrower scope per the partition strategies below, then
   combine findings across the multiple invocations.

The re-invocation is the recovery path. Without it, truncated
agents silently produce zero findings — the documentation agent
in particular ships nothing on moderately-sized PRs.

### Partition strategies

Three partitions cover the cases observed:

- **Split-by-file-family.** Partition the diff by directory
  family (`src/`, `tests/`, `agents/`, `skills/`, `.claude/`,
  `docs/`). Re-invoke the agent once per partition that contains
  changes. Combine findings across the runs. Best for agents
  whose investigation cost scales with file count (documentation,
  reviewer).
- **Split-by-finding-type.** Partition the agent's task by tenant
  or finding category (architecture, simplicity, correctness,
  security; or for learn-analyst, the three category tenants).
  Re-invoke once per category with explicit instruction to scope
  output to that category. Best for agents whose investigation
  cost scales with the breadth of categories examined (reviewer,
  learn-analyst).
- **Split-by-phase.** When the diff spans multiple FLOW phases
  (Plan-phase rule changes, Code-phase implementation, Code
  Review-phase agent changes), partition by phase. Best for
  documentation drift checks where each phase has its own doc
  surface.

The skill chooses the partition based on what the agent's Input
contract suggests: `DOC_PATHS:`-driven agents partition by file
family; tenant-driven agents partition by finding type. When in
doubt, file family is the safest default.

### When to surface to user

A re-invocation that itself returns without the marker is a
double-truncation — the partition was still too large. The skill
surfaces the truncated agent in the triage summary rather than
splitting infinitely. The user decides whether to accept partial
coverage or rerun Review against a smaller subset of the
diff.

### Retry-prompt path-scoping constraint

Every retry prompt — partitioned re-invocation OR re-invocation
after marker-absent detection — MUST scope every path it names
to the active worktree. Out-of-worktree paths in an agent's
`prompt` field would otherwise reach the sub-agent's Read tool
and surface a Claude Code permission prompt mid-autonomous-flow,
defeating the autonomous-mode contract.

The constraint is enforced by two mechanical layers in addition
to the SKILL.md HARD-GATE in `skills/flow-review/SKILL.md` Step 2:

- **Parent-side prompt scan** in
  `src/hooks/validate_pretool.rs::run()` wired into the Agent
  tool branch. The scan calls
  `src/hooks/agent_prompt_scan.rs::validate_agent_prompt` on the
  `prompt` field; the helper extracts path-shape substrings,
  validates them, joins relative candidates onto
  `worktree_root`, lexically normalizes the result, and rejects
  any candidate that does not start with `worktree_root`. Blocks
  with exit 2 and a structured message naming the offending path
  and the worktree. This is the layer that prevents the
  permission-prompt scenario above: the out-of-worktree path
  never reaches the sub-agent's prompt, so the sub-agent never
  Reads it.
- **Autonomous-flow-strict response shape** in
  `src/hooks/validate_worktree_paths.rs::validate()` at both
  block surfaces (the in-project-but-out-of-worktree redirect
  and the out-of-project fail-closed gate). When the flow is
  configured for autonomous execution (per
  `crate::flow_paths::is_autonomous_flow_active`), the hook
  returns a structured JSON envelope keyed on
  `out_of_worktree_in_autonomous` (out-of-worktree) or
  `out_of_bounds_in_autonomous` (out-of-project) instead of the
  human-readable BLOCKED message. Both forms are exit-2 blocks
  fed back as a tool rejection (the block itself is identical);
  the `reason` field lets the autonomous flow classify the
  rejection programmatically rather than scraping the prose
  message, and serves as the stable detection anchor for a
  future system-initiated-prompt carve-out.

Residual gap: out-of-project paths (`~/.config`, `.venv` outside
the worktree, arbitrary source files) are fail-closed during an
active flow — allowed only for the approved memory + `/tmp`
scratch surface (`is_approved_out_of_project_path`) and blocked
(exit 2, no prompt) otherwise. The remaining boundary is non-flow
contexts (cwd not inside a worktree), where the early "not in a
worktree" return leaves path jurisdiction to Claude Code. The
retry-prompt HARD-GATE is the upstream defense — drop the
requirement from the prompt rather than redirecting toward a
different out-of-worktree path.

The one sanctioned exception is a path under this flow's own
`<project_root>/.flow-states/<branch>/` subtree: `agent_prompt_scan`
carves that subtree out (see `.claude/rules/hook-cwd-resolution.md`
"agent_prompt_scan `.flow-states/` Carve-Out"), so a Review retry
prompt carrying the substantive-diff path there need not drop it —
the scan allows it even though it sits outside the worktree.

When in doubt about any OTHER out-of-worktree path, drop it from the
retry prompt entirely. The agent's investigation can succeed by
reading in-worktree files alone; out-of-worktree references are
almost always incidental to the actual review task.

## Never Supplement Agent Work From the Parent Session

When an agent malfunctions — truncates, hallucinates a constraint,
refuses to act, or returns the completion marker over findings
that were not actually produced from the inputs — the parent
skill's sanctioned responses are exactly two:

1. **Re-invoke** the agent with a narrowed partition (per
   "Partition strategies" above) and combine the results.
2. **Surface** the malfunction to the user as a process gap.
   Record the agent's state in the triage summary; do not
   advance the step counter past the unfinished work.

Both responses preserve cognitive isolation. The forbidden third
response is **parent supplementation**: the parent session reads
the inputs the agent should have read, produces the analysis the
agent should have produced, records the result as if the agent
produced it, and advances the step counter. This is the failure
mode that this rule exists to prevent.

### Why this matters

Cognitive isolation is the design that lets Review (and Learn)
detect what the parent session missed. The parent built the
feature; its assessment is biased by the emotional arc of the
work. The agent is the structural mechanism that breaks the
bias. The moment the parent does the agent's job, the bias
returns — the audit trail then shows "agent reviewed X" when
"parent reviewed X" is what actually happened, and every
downstream consumer (Learn-phase analyst, post-merge audit,
human reviewer) is misled.

The marker alone is not sufficient evidence that the agent did
its work. An agent can return `## END-OF-FINDINGS` over
findings drawn from session memory rather than from freshly-Read
input files. The parent must judge the agent's findings against
the agent's tool-call record:

- Did the agent Read the files the prompt named?
- Do the findings reference content from those files, or do they
  reference content the parent session could already see?

If the agent's findings are hollow — marker present but the work
absent — the response is the same as marker-absent: re-invoke or
surface. Never supplement.

### How to apply

**Parent skill code (or skill author).** When designing an agent
invocation, do not write code paths that "fall back to the
parent" on agent failure. The two sanctioned responses are
re-invoke and surface. Add no third path.

**Parent skill at runtime.** When an agent's response is
ambiguous (marker present, findings hollow; OR marker absent
after one re-invocation), STOP. Do not Read the inputs yourself
to verify the agent's claims. Do not draft findings the agent
should have drafted. Surface the malfunction to the user via the
triage summary and let the user decide whether to retry, accept
partial coverage, or abort.

**Reviewer agent (when this rule is being checked in Review).**
Any diff that adds a "if agent fails, parent does X instead" code
path or skill instruction is a Real finding. The fix is to delete
the fallback and surface the malfunction instead.

## Never Break the Session

Never force a session break for cognitive isolation. Claude Code
has no auto-resume — a session end requires human intervention to
restart. This breaks `continue=auto` flows and overnight
orchestration.

Sub-agents achieve the same isolation without interrupting session
continuity. They are structurally isolated from conversation
history by design, not by instruction.

## Reference Implementation

The reviewer agent (`agents/reviewer.md`) demonstrates the
context-rich pattern: it runs in the foreground during Review,
receives the full diff (as a file path it Reads) plus the plan,
CLAUDE.md, and the rule corpus inline, and returns structured
findings to the parent session.

The documentation and learn-analyst agents (`agents/documentation.md`,
`agents/learn-analyst.md`) demonstrate the context-sparse pattern:
documentation assesses maintainability and documentation drift in
Review (Phase 3); learn-analyst audits rule compliance and process
gaps in Learn (Phase 4). Both receive the substantive diff as a file
path and investigate the standards themselves — learn-analyst reads
CLAUDE.md and the `.claude/rules/` corpus on demand rather than
inline. Each agent's prompt states it has no knowledge of the
conversation that produced the changes.

## Checklist for New Consumers

When adding a sub-agent for cognitive isolation:

- Define it as a custom plugin sub-agent (`agents/<name>.md`)
- Scope its input to persisted artifacts only
- Make it read-only (Read, Glob, Grep, Bash — no Edit or Write)
- The global `PreToolUse` hook in `hooks/hooks.json` enforces
  Bash restrictions automatically — do not add hooks to agent
  frontmatter (unsupported by Claude Code's plugin agent system)
- Invoke it in the foreground so the parent session receives
  results and continues
- For high-investigation agents (full repo walks, large doc
  trees), declare the `## END-OF-FINDINGS` completion marker in
  Output Format AND add a per-agent sibling test in
  `tests/skill_contracts.rs` per "Completion-marker contract"
  above
- If the agent is required for its phase (i.e., the phase
  `phase-finalize` should refuse to advance when the agent did
  not run), register it in
  `src/required_agents.rs::REQUIRED_AGENTS`. The constant binds
  each phase to its required-agent set so the
  `phase-finalize` required-agents gate reads
  `phases.<phase>.agents_returned` — written by the
  `PreToolUse:Agent` recorder (`src/hooks/agent_run_record.rs`)
  when the agent is launched — and rejects with reason
  `required_agent_not_returned` when any required agent is
  missing. The
  `tests/skill_contracts.rs::required_agents_matches_skill_invocations`
  contract test binds the constant to the matching SKILL.md
  `subagent_type: "flow:<name>"` invocations — adding a new
  agent invocation in a phase SKILL.md without extending the
  constant fails CI
