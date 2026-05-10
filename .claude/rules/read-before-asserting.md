# Read Before Asserting

Before stating a fact about how the system works, name the file that
was read in this session to verify it. If no file can be named, the
statement is a guess and must not be presented as fact.

## The Rule

When about to assert any of:

- "X is caused by Y."
- "The reason X happens is Y."
- "Found it — the issue is Y."
- "X works by doing Y."
- "Y is what's happening here."

…stop. Before sending the message, identify the specific file and
line range that was read **in this session** to verify the claim. The
read must be visible in the conversation — a Read tool call, a Grep
tool call with content output, or a Bash output containing the
relevant lines.

If no such read exists, the assertion is grounded in memory or
pattern-matching, not the current code. Two paths are allowed:

1. **Read the file first, then assert.** Run the Read or Grep call,
   then write the assertion citing the file path and line number.
2. **Frame as a hypothesis explicitly.** Say "I haven't read this
   yet, my guess is Y." Then read. Do not skip the read because the
   guess felt right.

## What Is Forbidden

- Confident assertions ("the cause is," "found it," "the explanation
  is") that cite memorized rules, prior conversation knowledge, or
  pattern-matching against similar codebases — without a current-
  session read.
- "Likely," "probably," "almost certainly" used as hedges to dress
  guesses as analysis. A hedged guess is still a guess. Read first.
- Citing a `.claude/rules/` rule's mechanism as the explanation for
  current behavior unless the symptoms named in that rule have been
  observed and verified in the current session.
- Producing a "diagnosis" structured as "the issue is X because of
  Y" when Y was constructed from memory rather than read from the
  source.

## Why

The model has access to Read, Grep, Glob, and Bash. It can verify
nearly any claim about the codebase in seconds. Asserting a fact
without verification is producing output that looks like analysis
but is actually pattern-matching against training data and prior
context. The user cannot distinguish a verified claim from a
memorized-but-wrong one without doing the verification themselves —
at which point the model's output has provided negative value: it
took the user's time and produced something they couldn't trust.

The rule's discipline is asymmetric. Reading the file before
asserting is cheap — typically one tool call, seconds of latency.
Asserting from memory and being wrong is expensive — the user
spends turns correcting, reading the source themselves, and
re-establishing trust. The cost of skipping the read is paid by
the user; the cost of doing the read is paid by the model. The
rule shifts the cost back to where it belongs.

## How to Apply

**Before any assertion about code behavior.** Pause. Ask: did I
read the relevant file in this session? If yes, cite it. If no,
read it first.

**Before "found it" / "the cause is" / "I see the issue" framings.**
These framings claim a verified conclusion. They require a verified
foundation. If the foundation is memory, downgrade the framing to
"I haven't read this yet, my guess is..." and then read.

**When tempted to cite a rule as an explanation.** Rules describe
classes of failure. The current failure is an instance, not the
class. Reading the rule does not verify the instance. The instance
is verified by reading the code that produces it.

**When the user has provided diagnostic output (CI failure, error
message, stack trace).** The output names a file or function. Read
that file or function before explaining it. Treating the output as
a riddle to solve from memory wastes the most concrete evidence
available.

## Cross-References

- `.claude/rules/always-verify.md` — the principle for reporting:
  evidence before claiming completion.
- `.claude/rules/coverage-gap-diagnosis.md` — the specific
  application of this rule to coverage failures.
- `.claude/rules/investigate-root-cause.md` — "No Speculation, No
  Deflection" — the same principle applied to bug reports.
- `.claude/rules/assess-issues.md` — the sibling rule for issue
  assessment; identical principle applied to "is this issue still
  relevant" questions.
