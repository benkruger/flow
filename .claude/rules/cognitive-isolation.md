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

- **Context-rich** (reviewer, learn-analyst) — receives diff, plan,
  CLAUDE.md, and rules inline. Its task is checking against known
  standards where having the standards at hand saves turns.
  Learn-analyst additionally receives state file data (visit counts,
  timings, session notes) to detect process friction and rule
  violations.
- **Context-sparse** (pre-mortem, adversarial, documentation) — receives
  only the diff and must investigate the codebase itself. Less context
  forces independent investigation, surfacing risks and coverage gaps
  that pre-supplied context would mask. The documentation agent receives
  doc paths but must investigate the codebase independently for
  comprehension barriers before reading documentation for drift.

This asymmetry is intentional. See `agents/pre-mortem.md` Design
Note for the full rationale and `agents/reviewer.md` Design Note
for the cross-reference.

## Silent Truncation on maxTurns Exhaustion

Claude Code sub-agents stop silently after reaching their
`maxTurns` ceiling. They produce no error signal — the response
simply ends mid-sentence. The parent skill detects this by
checking the returned output for expected structural markers
(section headers, Finding blocks). Absence of markers means
the agent was truncated, not that it found nothing.

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
receives the diff, state data, plan, and all project rules, and
returns structured compliance findings to the parent session. Its
prompt explicitly states it has no knowledge of the conversation
that produced the changes.

The documentation agent (`agents/documentation.md`) demonstrates the
context-sparse pattern in Code Review (Phase 4): it assesses
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
