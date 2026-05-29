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

The CI-running subcommand family:

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

## Long-Running Foreground Poll Subcommands

A second family of `bin/flow` subcommands runs long in the
foreground not because they compute, but because they POLL: each
blocks on a real `thread::sleep` retry loop with a bounded cap until
an external condition resolves. They do NOT run CI.

- `bin/flow start-init` — blocks on the start lock via
  `acquire_with_wait` (default cap ~8 min) until the lock frees or
  the cap is exhausted, then returns `ready` / `locked` / `error`.
- `bin/flow wait-for-release-ci` — polls `gh run list` (default cap
  ~8 min) until the latest integration-branch run for HEAD reaches a
  terminal conclusion, then returns `ready` / `still_pending` /
  `error`.

Both disciplines from the CI-running family apply unchanged:

1. **Never background.** The `run_in_background` block in
   Enforcement below fires on `bin/flow` regardless of subcommand,
   so a poll subcommand can never be backgrounded.
2. **10-minute Bash tool timeout.** Every SKILL.md bash block that
   invokes one of these must carry the `timeout: 600000` preamble in
   the 5 non-blank lines before the fence — the cap can run to ~8
   minutes and the default 2-minute Bash timeout would background
   the process mid-poll, defeating the wait.

The one difference is the closing advice below: a CI run that feels
slow is a signal to speed up the command, but a poll subcommand's
wall-clock time is an INTENTIONAL bounded cap — there is nothing to
speed up. On cap exhaustion the invoking skill re-runs the single
line (flow-start re-runs `start-init` on `locked`; flow-release
re-runs `wait-for-release-ci` on `still_pending`); it never
backgrounds and never falls back to a timer-based re-invocation.

## Enforcement

The `validate-pretool` PreToolUse hook blocks any Bash tool call
that sets `run_in_background` to a truthy value (bool `true`, the
string `"true"`, `"1"`, or a non-zero number) when the command's
first whitespace-separated token is `bin/flow` (or any absolute
path ending in `/bin/flow`). Bypass attempts fail with exit 2 and
a message feeding back to the caller.

The 10-minute timeout instruction is backed by two contract tests
in `tests/skill_contracts.rs`. Both scan fenced bash blocks and
assert the preceding 5 non-blank prose lines contain `timeout:
600000` (exact numeric, enforced with a trailing non-digit anchor
so typo'd values like `timeout: 6000000` are rejected) OR the
literal prose phrase `10-minute Bash tool timeout`. The backward
walk stops at any prior fence, so unrelated intermediate bash blocks
cannot chain preamble coverage to distant calls. Unclosed ```bash
fences at EOF are surfaced as violations rather than silently
passing.

- `skill_ci_invocations_specify_long_timeout` — the CI-running
  regex `bin/flow (ci|start-gate|finalize-commit|complete-fast)\b`,
  scanning `skills/`.
- `skill_poll_invocations_specify_long_timeout` — the poll-subcommand
  regex `bin/flow (start-init|wait-for-release-ci)\b`, scanning both
  `skills/` (where `start-init` lives) and `.claude/skills/` (where
  the project-local `flow-release` invokes `wait-for-release-ci`).

If a CI-running command takes long enough to feel like it warrants
backgrounding, that is a signal to speed up the command — not to
hide its gate. Poll subcommands are the exception: their wall-clock
time is an intentional bounded cap, so the response is to wait (and,
on cap exhaustion, re-run the single line), never to background.
