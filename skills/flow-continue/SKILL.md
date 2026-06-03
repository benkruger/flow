---
name: flow-continue
description: "Clear the autonomous-flow halt set when the user spoke mid-flow. Invokes `bin/flow clear-halt` so the next assistant turn resumes execution. User-only: the model cannot invoke this skill."
---

# FLOW Continue

Resume an autonomous flow that paused because the user typed a message
mid-flow. The Stop hook's `check_autonomous_stop` predicate set
`_halt_pending=true` when it observed the user message; this skill
clears that flag so the next assistant turn proceeds. Works universally:
clears any pending halt and acts as a watermark over preceding
conversation, so the autonomous flow resumes whether it was paused by
a user message, a network error, or a rate-limit interrupt.

`/flow:flow-continue` is the ONLY path that clears `_halt_pending`.
The skill is in `USER_ONLY_SKILLS` — the model cannot invoke it, and
`bin/flow clear-halt` independently self-gates by checking the
persisted transcript for the user's slash-command invocation. The
two layers together make the halt durable: nothing the model can do
on its own resumes the flow.

## Usage

```text
/flow:flow-continue
```

No flags, no arguments. The skill takes a single mechanical action.

## Concurrency

This flow is one of potentially many running simultaneously — on this
machine (multiple worktrees) and across machines (multiple engineers).
Your state file (`.flow-states/<branch>/state.json`) is yours alone.
Never read or write another branch's state.

## Announce

Output the following banner in your response (not via Bash) inside a
fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.6.1 — flow:flow-continue — STARTING
──────────────────────────────────────────────────
```
````

## Step 1 — Detect current branch

Run `git worktree list --porcelain`. The `worktree` entry whose path
matches your current working directory carries a `branch
refs/heads/<name>` line — strip the `refs/heads/` prefix to get the
current branch. Hold the branch name in context for Step 2.

## Step 2 — Clear the halt

```bash
${CLAUDE_PLUGIN_ROOT}/bin/flow clear-halt --branch <branch>
```

Parse the JSON `status`:

- `"ok"` — halt cleared, autonomous execution resumes on the next
  assistant turn. Print "Halt cleared. Resuming."
- `"skipped"` with `"reason":"no_state_file"` — no active flow on
  this branch (state file absent). Nothing to clear. Print
  "No active flow on `<branch>` — nothing to clear."
- `"error"` — print the `message` field verbatim so the user sees
  the failure reason (`unauthorized`, `invalid_branch`,
  `no_transcript_path`, `state_write_failed`). The unauthorized
  branch fires when the most recent real user turn's
  `message.content` does not START with either of the two emission
  shapes Claude Code uses for the slash command — the two-line
  `<command-message>flow:flow-continue</command-message>\n<command-name>/flow:flow-continue</command-name>`
  (Claude Code 2.1.140+) or the legacy
  `<command-name>/flow:flow-continue</command-name>`. The walker
  accepts either via `starts_with` disjunction. A Bash-tool bypass
  attempt this skill makes impossible by definition.

## Done

Output the following banner in your response (not via Bash) inside a
fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.6.1 — flow:flow-continue — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Return to the user. The next assistant turn picks up the paused flow.

## Hard Rules

- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
