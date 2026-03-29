# Cognitive Isolation

## When to Use Foreground Sub-Agents

When a phase needs debiased analysis of work done in the current
session, run the analysis in a foreground sub-agent. The sub-agent
receives only persisted artifacts (state file, diff, plan, notes)
— never conversation history. The parent session stays alive to
receive results and continue the flow.

This pattern exists because the model that built the feature
carries forward its emotional arc — struggles, negotiations,
rationalizations. Inline analysis in the same session produces
self-reporting bias: obvious mistakes get caught, but deep
assumptions feel like facts and go unexamined.

## Never Break the Session

Never force a session break for cognitive isolation. Claude Code
has no auto-resume — a session end requires human intervention to
restart. This breaks `continue=auto` flows and overnight
orchestration.

Sub-agents achieve the same isolation without interrupting session
continuity. They are structurally isolated from conversation
history by design, not by instruction.

## Reference Implementation

The onboarding agent (`agents/onboarding.md`) demonstrates the
pattern: it runs in the foreground during Learn, receives only
the diff and codebase access, and returns findings to the parent
session. Its prompt explicitly states it has no knowledge of the
conversation that produced the changes.

## Checklist for New Consumers

When adding a sub-agent for cognitive isolation:

- Define it as a custom plugin sub-agent (`agents/<name>.md`)
- Scope its input to persisted artifacts only
- Make it read-only (Read, Glob, Grep, Bash — no Edit or Write)
- Add a `PreToolUse` hook declaration for defense in depth
- Invoke it in the foreground so the parent session receives
  results and continues
