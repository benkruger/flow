---
title: /flow-continue
nav_order: 22
parent: Skills
---

# /flow-continue

**Phase:** Any (no phase gate)

**Usage:** `/flow-continue`

Resume an autonomous flow that paused because the user typed a message
mid-flow. The Stop hook's `check_autonomous_stop` predicate sets
`_halt_pending=true` the moment it observes a real user message during
an in-progress autonomous phase; this skill clears that flag so the
next assistant turn resumes execution.

`/flow-continue` is the ONLY path that clears `_halt_pending`. The
skill is in `USER_ONLY_SKILLS` — the model cannot invoke it, and the
underlying `bin/flow clear-halt` subcommand self-gates by re-checking
the persisted transcript for the user's slash-command invocation. The
two layers together make the halt durable: nothing the model can do
on its own resumes the flow.

---

## What It Does

1. Detects the current branch from `git worktree list --porcelain`
2. Runs `bin/flow clear-halt --branch <branch>`, which:
   - Verifies the persisted transcript's most recent real user turn
     opens with either of the two emission shapes Claude Code uses
     for `/flow:flow-continue` — the two-line
     `<command-message>flow:flow-continue</command-message>\n<command-name>/flow:flow-continue</command-name>`
     (Claude Code 2.1.140+) or the legacy
     `<command-name>/flow:flow-continue</command-name>`. The walker
     accepts either shape via `starts_with` disjunction.
   - Sets `_halt_pending=false` in `.flow-states/<branch>/state.json`
3. Returns control to the user. The next assistant turn picks up the
   paused flow without re-asking the user what to do next.

---

## When to Use It

- You typed a question or correction mid-flow and the model paused
- You see a Stop-hook refusal message naming `/flow-continue` as the
  exit path
- You want to resume an autonomous flow after a deliberate
  conversational interruption

---

## vs /flow-abort

| | `/flow-continue` | `/flow-abort` |
|---|---|---|
| **Effect** | Resume the paused flow | Abandon the flow |
| **State file** | Kept; `_halt_pending` cleared | Deleted |
| **Worktree** | Kept | Removed |
| **PR** | Kept | Closed |

Use `/flow-continue` when you want the flow to keep going. Use
`/flow-abort` to walk away from the work entirely.

---

## Gates

- No phase gate — available whenever a halt is set
- State file not required — `clear-halt` returns `skipped` with
  reason `no_state_file` when no active flow exists on the branch
- Model invocation rejected by the `validate-skill` PreToolUse hook
  (Layer 1 of the user-only-skill enforcement chain)
- Bash-tool bypass rejected by `bin/flow clear-halt`'s own
  transcript self-gate; an unauthorized invocation returns
  `{"status":"error","reason":"unauthorized"}` without mutating
  state
