# CLI Output Contracts

When a plan adds a new `bin/flow` subcommand or extends a
`bin/<tool>` stub (`bin/test`, `bin/format`, `bin/lint`,
`bin/build`) with a new flag whose stdout, exit code, or stderr is
consumed by skills, agents, or other subcommands, the plan must
specify the output contract upfront — before Code phase begins —
not discover it during Code Review.

A subcommand's output (or a stub's flag output) is a programmatic
API surface. Skills shell into it and parse the result; other
subcommands invoke it and branch on the response. Discovering the
contract during implementation produces drift between callers and
the implementation: fallback behavior gets bolted on after the
fact, error semantics shift mid-PR, and Code Review surfaces gaps
the Plan phase should have closed.

## What Counts as a Consumed Output

A subcommand or stub-flag has a consumed output contract when any
of the following will read its stdout, exit code, or stderr:

- A SKILL.md bash block that captures `$(bin/flow <name>)` (or
  `$(bin/<tool> --<flag>)`) and substitutes the value into
  prompts, paths, or downstream commands.
- A Rust caller (another subcommand) that invokes the program via
  `Command::new` and parses the response.
- A hook script that pipes the output into a decision branch.
- An agent prompt that includes the output as input.

A subcommand or flag whose output is purely human diagnostic
(e.g., a debug dump no automation reads) is not under this rule.

## Required Plan-Phase Contract

Every plan task that proposes a new consumed-output subcommand or
a new consumed-output flag on an existing `bin/<tool>` stub must
include — in the Tasks section, not the Risks section — a
contract specification listing four items:

1. **Output format.** JSON, plain text (one value), plain text
   (multi-line), or empty stdout. If a single-line path or
   identifier, name whether it is absolute or relative; if
   relative, name the base directory it resolves against. If
   JSON, name the top-level keys and their types.
2. **Exit codes and meanings.** Every exit code the subcommand
   can produce, paired with a one-line description of what each
   code signals to the caller. The default success/failure
   convention (`0` ok, `1` error) is acceptable but must be
   stated; non-default codes (e.g., `2` for input-resolution
   failures) must be named with their trigger and how the caller
   distinguishes them from accidental scripting errors that
   happen to share the same exit code.
3. **Error messages.** What appears on stderr for each error
   class. Errors fall into two buckets: `infrastructure errors`
   (subprocess failed, file system unavailable — caller cannot
   recover) and `business errors` (input invalid, state missing
   — caller may have a fallback). Name each error class and
   which bucket it belongs to.
4. **Fallback behavior.** When the subcommand returns a default
   value instead of an error (e.g., `"main"` when the
   integration-branch field is missing), name the trigger
   condition explicitly and describe why the fallback is the
   right choice for the caller. If the contract is "no
   fallback — fail closed," state that explicitly so the Code
   phase doesn't accidentally introduce one.

## Caller-Side Normalization

When the consumer is a SKILL.md bash block that captures stdout,
the contract must also specify how the caller normalizes the
captured value. Project-owned `bin/<tool>` scripts are bash
written by the project author — they may print via `echo` (which
appends a trailing newline), include trailing whitespace from
line-continuation quoting, or wrap the value in quotes. The
contract is a single-line value; the caller normalizes
defensively.

The plan task must name the normalization rule explicitly:

- **Trailing-whitespace strip.** The caller strips trailing
  newlines, spaces, and tabs before using the captured value.
  Add an explicit instruction to the SKILL.md prose: "Strip
  trailing whitespace before using the value."
- **Multi-line rejection.** If the contract is a single-line
  value, the caller treats multi-line stdout as a contract
  violation (surface as a finding) rather than silently using
  the first line.
- **Empty-stdout handling.** If the consumer is `(stdout)`,
  empty stdout means the contract was violated (the script ran
  to exit 0 without producing the expected value). The caller
  must distinguish this from a configured-but-empty value.

## How to Apply

**Plan phase.** Add the four-item contract plus the
caller-side normalization rule to the implementation task. Cite
the consumers that will read the output (skill bash block paths,
Rust caller modules) so the contract's audience is explicit. The
contract is a section of the task description, not a deferred
Risks note — Plan-phase reviewers read the Tasks section and
check the contract for completeness.

**Code phase.** Implement the subcommand or flag to match the
contract verbatim. Adding a fallback or changing an exit code
mid-Code phase is a plan deviation per
`.claude/rules/plan-commit-atomicity.md` "Plan Signature
Deviations Must Be Logged" and must be logged via `bin/flow log`
before the commit lands. The Code Review reviewer agent
cross-references the contract against the implementation; any
mismatch (added fallback, changed exit code, new error class) is
a Real finding.

**Code Review phase.** Verify the contract is present in the
plan and that the implementation matches it. Findings:

- Contract missing from plan → process gap (route to Phase 5
  Learn for plan-template improvement, not a Code Review fix
  unless the implementation also drifted).
- Implementation drifted from contract → Real finding, fix in
  Step 4 (either revise the contract via plan deviation log
  and update consumers, or revise the implementation to match
  the original contract).
- Caller-side normalization missing or incomplete → Real
  finding, fix in Step 4 by adding the explicit normalization
  prose to the SKILL.md.

## Cross-References

- `.claude/rules/docs-with-behavior.md` — the new-subcommand
  documentation rule. New subcommands also require CLAUDE.md
  Key Files entries and `docs/reference/flow-state-schema.md`
  updates when state mutations are involved; this rule covers
  the API contract layer of the same change.
- `.claude/rules/plan-commit-atomicity.md` "Plan Signature
  Deviations Must Be Logged" — the discipline for handling
  contract changes discovered during Code phase.
- `.claude/rules/external-input-validation.md` — when
  subcommand output flows back into another subcommand's
  input, the consumer must validate the value through
  fallible constructors (`FlowPaths::try_new`, etc.).
- `.claude/rules/security-gates.md` "Normalize Before
  Comparing" — when subcommand output is compared against
  string constants in a gate, the comparison must normalize
  both sides.
- `.claude/rules/tool-dispatch.md` — bin-stub conventions for
  the `bin/format` / `bin/lint` / `bin/build` / `bin/test`
  family. Flag additions to those stubs that produce consumed
  output also fall under this rule.
