# Phase Transition Continue Mode

When a phase skill completes and no `--auto` or `--manual` flag was
passed explicitly, read `skills.<phase>.continue` from the state file
before deciding how to advance.

- If the value is `"auto"` → invoke the next phase directly. Do not
  invoke `flow:flow-status`. Do not use `AskUserQuestion`.
- If the value is `"manual"` or absent → invoke `flow:flow-status`,
  then prompt the user via `AskUserQuestion`.

The state file is the source of truth for continue mode — not the
previous phase's mode, not a default assumption. Each phase has its
own mode. Read it every time.
