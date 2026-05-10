# Coverage Gap Diagnosis

When `bin/flow ci` reports coverage below 100/100/100, the diagnostic
path is fixed and mechanical. Skipping any step and substituting
speculation is forbidden.

## The Sequence

When CI reports a coverage gap on file `<file>`:

1. **Run `bin/test --show <file>` immediately.** The output names every
   uncovered region with `^0` markers next to the source. The first
   analytical action is reading those markers — not theorizing about
   why coverage might be low. The model has the same access to this
   tool as the user; running it is the model's job, not the user's.

2. **Read the source around every `^0`.** Identify what branch each
   marker covers and what input would exercise that branch. Cite the
   file path and line number when reporting findings.

3. **Read the test file for that source.** Look for tests that target
   the uncovered branch. If a test exists and is failing to cover the
   branch, the test is the bug. Read the test's doc comment first —
   it often names the failure mode in plain prose.

4. **Read sibling callers of any function involved.** If a similar
   function elsewhere handles the same case differently, that pattern
   is the candidate fix. Cite the sibling by file path and line number.

5. **Only after steps 1–4 have produced concrete evidence, propose a
   fix.** The proposal must reference specific lines from the
   investigation, not memorized rules.

## What Is Forbidden

- Speculating about stale instrumented binaries, phantom coverage,
  profdata races, or any other tooling-internal explanation **without**
  running `bin/test --show <file>` first and naming concrete `^0`
  evidence.
- Asserting "found it," "the cause is," or any other conclusive
  framing before reading the actual `^0` lines.
- Asking the user to run a diagnostic command the model can run
  itself.
- Asking the user to paste output the model can produce by running
  the same tool against the same profdata.
- Citing a rule (e.g. `per-file-coverage-iteration.md` "phantom
  misses") as an explanation for the current failure unless the
  symptoms named in that rule have been confirmed by running the
  diagnostic the rule names (multiple stale crate hashes via
  `bin/test --funcs`, etc.).
- Framing a diagnosis as a "guess" or "hypothesis" and proceeding as
  if it were verified. A guess is not a diagnosis.

## Why

`bin/test --show` reads the profdata produced by the most recent
test run. The model has the same access to it the user does. Coverage
gaps are not mysterious — they have specific source locations recorded
in the profdata. Speculation about why they exist before reading those
locations is always more expensive than just reading them.

The rule is a hard sequence because the failure mode under speculation
is dishonest output: "explanations" derived from memorized rules
rather than from the file in front of the model. The user cannot
distinguish a memorized-but-wrong explanation from a read-and-verified
one without doing the diagnosis themselves — at which point the model
has provided negative value.

## Cross-References

- `.claude/rules/always-verify.md` — the broader principle (verify
  with concrete evidence before reporting).
- `.claude/rules/per-file-coverage-iteration.md` — the per-file gate
  and `--show`/`--funcs` tools this rule operationalizes.
- `.claude/rules/reachable-is-testable.md` "Covered elsewhere is not
  a terminal state" — the sibling rule that forbids speculative
  coverage attribution.
- `.claude/rules/read-before-asserting.md` — the broader pattern
  this rule is one instance of.
