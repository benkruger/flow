# CLI Output Contracts

When a plan adds a new `bin/flow` subcommand whose output is
consumed by skills, agents, or other subcommands, the plan must
specify the output contract upfront — before Code phase begins —
not discover it during Code Review.

A subcommand's output is a programmatic API surface. Skills shell
into it and parse the result; other subcommands invoke it and
branch on the response. Discovering the contract during
implementation produces drift between callers and the implementation:
fallback behavior gets bolted on after the fact, error semantics
shift mid-PR, and Code Review surfaces gaps the Plan phase should
have closed.

## What Counts as a Consumed Output

A subcommand has a consumed output contract when any of the
following will read its stdout, exit code, or stderr:

- A SKILL.md bash block that captures `$(bin/flow <name>)` and
  substitutes the value into prompts, paths, or downstream
  commands.
- A Rust caller (another subcommand) that invokes
  `bin/flow <name>` via `Command::new` and parses the response.
- A hook script that pipes `bin/flow <name>` output into a
  decision branch.
- An agent prompt that includes `bin/flow <name>` output as
  input.

A subcommand whose output is purely human diagnostic (e.g., a
debug dump no automation reads) is not under this rule.

## Required Plan-Phase Contract

Every plan task that proposes a new consumed-output subcommand
must include — in the Tasks section, not the Risks section — a
contract specification listing four items:

1. **Output format.** JSON, plain text (one value), plain text
   (multi-line), or empty stdout. If JSON, name the top-level
   keys and their types.
2. **Exit codes and meanings.** Every exit code the subcommand
   can produce, paired with a one-line description of what each
   code signals to the caller. The default success/failure
   convention (`0` ok, `1` error) is acceptable but must be
   stated; non-default codes (e.g., `2` for input-resolution
   failures) must be named with their trigger.
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
   right choice for the caller.

## How to Apply

**Plan phase.** Add the four-item contract to the implementation
task. Cite the consumers that will read the output (skill bash
block paths, Rust caller modules) so the contract's audience is
explicit. The contract is a section of the task description, not
a deferred Risks note — Plan-phase reviewers read the Tasks
section and check the contract for completeness.

**Code phase.** Implement the subcommand to match the contract
verbatim. Adding a fallback or changing an exit code mid-Code
phase is a plan deviation per
`.claude/rules/plan-commit-atomicity.md` "Plan Signature
Deviations Must Be Logged" and must be logged via `bin/flow log`
before the commit lands. The Code Review reviewer agent
cross-references the contract against the implementation; any
mismatch (added fallback, changed exit code, new error class)
is a Real finding.

**Code Review phase.** Verify the contract is present in the
plan and that the implementation matches it. Findings:

- Contract missing from plan → process gap (route to Phase 5
  Learn for plan-template improvement, not a Code Review fix
  unless the implementation also drifted).
- Implementation drifted from contract → Real finding, fix in
  Step 4 (either revise the contract via plan deviation log
  and update consumers, or revise the implementation to match
  the original contract).

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
