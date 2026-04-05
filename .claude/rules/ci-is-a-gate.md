# CI Is a Gate

`bin/flow ci` and `bin/ci` must never run in the background.
CI is a gate on every commit, every phase transition, and every
merge — it must complete and return its exit code before any
downstream action proceeds.

## Why

A background CI call lets the caller move on before results return:
the commit skill shows the diff, writes the message, and finalizes
the commit before CI has finished. The gate is defeated. Bugs that
CI would have caught land on main.

This applies in every mode:

- FLOW phases (Code, Code Review)
- Maintainer mode (`/flow:flow-commit` on main)
- Standalone mode (direct CI runs)

## Enforcement

The `validate-pretool` PreToolUse hook blocks any Bash tool call
that sets `run_in_background: true` on a command starting with
`bin/flow ci` or `bin/ci`, regardless of whether a FLOW phase is
active. Bypass attempts fail with exit 2 and a message feeding
back to the caller.

If CI takes long enough to feel like it warrants backgrounding,
that is a signal to speed up CI — not to hide its gate.
