# Phase Transition Continue Mode

When a phase skill completes and no `--auto` or `--manual` flag was
passed explicitly, parse `continue_action` from the
`phase-transition --action complete` JSON output before deciding
how to advance.

- If `continue_action` is `"invoke"` → invoke the next phase
  directly. Do not invoke `flow:flow-status`. Do not use
  `AskUserQuestion`.
- If `continue_action` is `"ask"` → invoke `flow:flow-status`,
  then prompt the user via `AskUserQuestion`.

The phase-transition command computes the continue mode from the
state file internally — skills never read nested JSON directly.
Each phase has its own mode. The command output reflects it.
