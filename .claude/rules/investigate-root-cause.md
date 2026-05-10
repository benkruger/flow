# Investigate Root Cause

When a bug surfaces, investigate the system design — never patch
the symptom by overwriting a file or applying a local fix. Trace
the root cause through the full mechanism before proposing any fix.

Ask "why didn't the existing mechanism handle this?" not "how do
I manually fix the output?"

When the user asks for something to be codified as a rule or test,
do it immediately in the same session. Do not defer or forget.

## No Speculation, No Deflection

Never claim something "might be fixed" or "should work now" without
verifying the actual state first. Check before speaking.

When the user reports a bug, diagnose it fully and propose a concrete
fix in one message. Never redirect the diagnosis back to the user by
asking what the fix should be.

## Cross-References

- `.claude/rules/no-backwards-reasoning.md` — sibling rule on
  reasoning discipline. Investigate the system as it stands today;
  do not rationalize current behavior from a historical decision
  ("this was decided in PR #NNN") rather than from current code
  merits.
