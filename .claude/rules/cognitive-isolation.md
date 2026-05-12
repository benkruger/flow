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

- **Context-rich** (reviewer, learn-analyst) — receives full diff,
  plan, CLAUDE.md, and rules inline. Its task is checking against
  known standards where having the standards at hand saves turns.
  Learn-analyst additionally receives state file data (visit counts,
  timings, session notes) to detect process friction and rule
  violations.
- **Context-sparse** (pre-mortem, adversarial, documentation) — receives
  only the substantive diff (`git diff -w`, whitespace-only changes
  filtered) and must investigate the codebase itself. Less context
  forces independent investigation, surfacing risks and coverage gaps
  that pre-supplied context would mask. The whitespace filter preserves
  turn budget on PRs with formatting changes. The documentation agent
  receives a narrowed list of doc paths derived from the diff (see
  `skills/flow-review/SKILL.md` Step 1) so its investigation
  surface stays bounded on moderately-sized PRs.

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

The learn-analyst agent (`agents/learn-analyst.md`) demonstrates
the context-rich pattern: it runs in the foreground during Learn,
receives the full diff, state data, plan, and all project rules,
and returns structured compliance findings to the parent session.
Its prompt explicitly states it has no knowledge of the conversation
that produced the changes.

The documentation agent (`agents/documentation.md`) demonstrates the
context-sparse pattern in Review (Phase 3): it assesses
maintainability (comprehension barriers) and documentation accuracy
(drift between docs and code behavior).

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
