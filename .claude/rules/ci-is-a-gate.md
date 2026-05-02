# CI Is a Gate

`bin/flow` (any subcommand) must never run in the background. Every
`bin/flow` subcommand is either a CI gate or a state mutation — it
must complete and return its exit code before any downstream action
proceeds.

## Why

A background call lets the caller move on before results return:
the commit skill shows the diff, writes the message, and finalizes
the commit before CI has finished. The gate is defeated. Bugs that
CI would have caught land on the base branch. The same applies to
state mutations (`phase-transition`, `finalize-commit`,
`phase-enter`) — backgrounding them creates race conditions with
downstream actions that depend on the state change.

This applies everywhere `bin/flow` runs:

- `bin/flow ci` (CI gate)
- `bin/flow finalize-commit` (runs `ci::run_impl()` internally
  before `git commit`)
- `bin/flow phase-transition`, `phase-enter`, `phase-finalize`
  (state mutations)

## 10-Minute Bash Tool Timeout

The Bash tool's default timeout is 2 minutes (120,000 ms). `bin/flow
ci` and its transitive callers (`start-gate`, `finalize-commit`,
`complete-fast`) routinely run 3–4 minutes on clean builds. A Bash
tool call that hits the default timeout is backgrounded by Claude
Code — the tool result returns without the command having finished
— which defeats the same "wait for the gate" invariant as
`run_in_background: true`.

Every SKILL.md bash block that invokes a CI-running `bin/flow`
subcommand must be preceded by adjacent prose instructing the model
to set `timeout: 600000` (10 minutes) on the Bash tool call. The
prose must appear in the 5 non-blank lines immediately preceding the
opening ` ```bash ` fence, and the backward walk stops at any prior
fence — so adjacent bash blocks in the same section must each carry
their own preamble, not inherit from a distant section across
unrelated blocks.

The CI-running subcommand family (as of issue #1182):

- `bin/flow ci` — the direct CI runner
- `bin/flow start-gate` — runs CI on the base branch under the start
  lock per CLAUDE.md "Start-Gate CI on the Base Branch as
  Serialization Point"
- `bin/flow finalize-commit` — runs `ci::run_impl()` before
  `git commit` per CLAUDE.md "CI is enforced inside
  `finalize-commit` itself"
- `bin/flow complete-fast` — runs a local CI dirty check before
  the Complete merge

The canonical instruction wording is:

> Use a 10-minute Bash tool timeout (`timeout: 600000`) — CI runs
> can take 3–4 minutes and the default 2-minute timeout would
> background the process, defeating the gate (per
> `.claude/rules/ci-is-a-gate.md`).

Contextual adaptations (for example `… for the retry on the same
reason`, `… on every invocation`) are fine as long as the `timeout:
600000` numeric form or the `10-minute Bash tool timeout` prose form
is present in the window.

## Enforcement

The `validate-pretool` PreToolUse hook blocks any Bash tool call
that sets `run_in_background` to a truthy value (bool `true`, the
string `"true"`, `"1"`, or a non-zero number) when the command's
first whitespace-separated token is `bin/flow` (or any absolute
path ending in `/bin/flow`). Bypass attempts fail with exit 2 and
a message feeding back to the caller.

The 10-minute timeout instruction is backed by a contract test —
`skill_ci_invocations_specify_long_timeout` in
`tests/skill_contracts.rs` — that scans every SKILL.md under both
`skills/` and `.claude/skills/` for fenced bash blocks matching the
CI-running regex and asserts the preceding 5 non-blank prose lines
contain `timeout: 600000` (exact numeric, enforced with a trailing
non-digit anchor so typo'd values like `timeout: 6000000` are
rejected) OR the literal prose phrase `10-minute Bash tool timeout`.
The backward walk stops at any prior fence, so unrelated
intermediate bash blocks cannot chain preamble coverage to distant
CI calls. Unclosed ```bash fences at EOF are surfaced as violations
rather than silently passing.

If a command takes long enough to feel like it warrants
backgrounding, that is a signal to speed up the command — not to
hide its gate.
