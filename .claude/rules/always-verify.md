# Always Verify

Code changes are complete when the relevant check passes, not
when the edit lands. An edit is a proposal; the check passing is
the confirmation. A change reported as done must carry evidence
that the check passed — typically the exit code of a concrete
command.

## What counts as verification

For each class of change, the verification command is known and
deterministic:

- Rust source: `bin/flow ci` (full) or the narrowest single-phase
  variant (`bin/flow ci --format`, `--lint`, `--build`, `--test`).
- A single test: `bin/flow ci --test -- <name>`.
- Skill content, rule content, or CLAUDE.md: the contract test
  that covers the affected artifact, invoked via
  `bin/flow ci --test -- <test_name>`.
- Config file touching the toolchain or permissions
  (`.claude/settings.json`, `.config/nextest.toml`, `Cargo.toml`,
  `hooks/hooks.json`): `bin/flow ci`.

Pick the narrowest check that fully exercises the change. Run
broader checks when the change's blast radius is uncertain — the
few extra minutes are cheaper than a broken change landing
labeled as done.

## Format and lint gates

Format and lint are deterministic gates. Their output is the fix:
the formatter or linter either emits a concrete diff or names the
violation and its location. Applying that fix and re-running the
gate is the end of the interaction. Format and lint are not
decision surfaces — there is no judgment call about whether the
project uses the style the formatter enforces.

When a gate reports a violation, the work is: apply the fix, run
the gate again, confirm it passes.

## Reporting discipline

Reports that describe a change as complete must name the
verification that produced the confirmation:

- Complete: "Change X landed; `bin/flow ci --format` exited 0."
- Incomplete: "Change X landed; should work."

A red check is a blocker, not a status update. When a
verification comes back red, the next action is investigation
and re-application, not a completion report.

## Cross-References

- `.claude/rules/verify-runtime-path.md` — trace the real
  execution path before writing a fix.
- `.claude/rules/verify-automation-e2e.md` — run the full
  execution path of a new automation feature before shipping.
- `.claude/rules/investigate-root-cause.md` — "No Speculation,
  No Deflection" — never claim something "might be fixed"
  without verifying.
- `.claude/rules/ci-is-a-gate.md` — `bin/flow ci` is the gate;
  it must not be backgrounded or bypassed.
- `.claude/rules/forward-facing-authoring.md` — the discipline
  this rule itself follows: state the principle abstractly,
  never encode the incident that motivated the rule.
