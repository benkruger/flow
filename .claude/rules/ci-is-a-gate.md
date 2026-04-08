# CI Is a Gate

`bin/flow` (any subcommand) and `bin/ci` must never run in the
background. Every `bin/flow` subcommand is either a CI gate or a
state mutation — it must complete and return its exit code before
any downstream action proceeds. `bin/ci` is a CI gate by
convention in target projects.

## Why

A background call lets the caller move on before results return:
the commit skill shows the diff, writes the message, and finalizes
the commit before CI has finished. The gate is defeated. Bugs that
CI would have caught land on main. The same applies to state
mutations (`phase-transition`, `finalize-commit`, `phase-enter`)
— backgrounding them creates race conditions with downstream
actions that depend on the state change.

This applies everywhere `bin/flow` runs:

- `bin/flow ci` (CI gate)
- `bin/flow finalize-commit` (runs `ci::run_once()` internally
  before `git commit`)
- `bin/flow phase-transition`, `phase-enter`, `phase-finalize`
  (state mutations)
- Direct `bin/ci` invocations (target project CI)

## Enforcement

The `validate-pretool` PreToolUse hook blocks any Bash tool call
that sets `run_in_background` to a truthy value (bool `true`, the
string `"true"`, `"1"`, or a non-zero number) when the command's
first whitespace-separated token is `bin/flow` (or any absolute
path ending in `/bin/flow`), or when the first token is `bin/ci`
(or any absolute path ending in `/bin/ci`). The suffix match is
intentional: it covers both FLOW's own binary and target projects'
scripts. Bypass attempts fail with exit 2 and a message feeding
back to the caller.

If a command takes long enough to feel like it warrants
backgrounding, that is a signal to speed up the command — not to
hide its gate.
