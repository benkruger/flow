# Hook vs Instruction

## When to Use a PreToolUse Hook

Use a hook — not a skill instruction — when:

- Claude ignoring the instruction causes a permission prompt
  or blocks the flow
- The behavior must be enforced across all phases, skills,
  and sub-agents universally
- You find yourself adding the same instruction to multiple
  skills independently
- The consequence of non-compliance is user-visible (blocked
  prompt, wrong file edited, permission denied)

## When Skill Instructions Suffice

Use skill instructions when:

- The behavior is specific to one phase or step
- Non-compliance is annoying but not flow-blocking
- The instruction is contextual (depends on plan content,
  user preferences, or phase-specific state)

## The Principle

Skill instructions are advisory — Claude can ignore them.
Hooks are enforcement — they run as code before the tool
executes. If "Claude might not follow this" has a
consequence that blocks the user, it must be a hook.

## Mechanically-Enforced Gates

The following invariants have escalated from instruction-level
to hook-level enforcement because instructions alone proved
insufficient:

- **Compound commands and command substitution** —
  `validate-pretool` Layers 1–2 block `&&`, `||`, `;`, `|`,
  `>`, `<`, `$(...)`, and backticks outside quoted arguments.
- **`exec` prefix** — Layer 3 blocks `exec <cmd>` to avoid
  Claude Code's "evaluates arguments as shell code" heuristic.
- **`git restore .`** — Layer 5 blocks the blanket form to
  preserve working changes; per-file `git restore <file>`
  remains allowed.
- **`git diff` with file-path arguments** — Layer 6 redirects
  to the Read tool and the Grep tool.
- **Deny-list permissions** — Layer 7 honours
  `.claude/settings.json` deny patterns ahead of allow.
- **Whitelist enforcement under an active flow** — Layer 8
  rejects commands not present in the merged allow list.
- **Direct commits during a flow** — Layer 9 rejects
  `git ... commit` and `bin/flow ... finalize-commit`
  invocations whose effective cwd (or any `git -C` target)
  resolves to the integration branch named by
  `default_branch_in` OR to a feature branch with an active
  FLOW state file at `.flow-states/<branch>/state.json`. The
  active-flow context carries a skill-commit carve-out:
  `bin/flow ... finalize-commit` passes through when the state
  file has `_continue_pending == "commit"` (the marker the
  commit-invoking skills set before invoking
  `/flow:flow-commit`); raw `git commit` is never carved out.
  See `.claude/rules/concurrency-model.md` "Mechanical
  Enforcement" for the bypass surface, the carve-out's trust
  contract, and the documented v1 gaps.
- **`run_in_background` on `bin/flow` and `bin/ci`** — the
  pre-validation path in `validate-pretool` rejects any
  background invocation of `bin/flow` (any subcommand) and
  `bin/ci` regardless of flow-active state.
- **`general-purpose` sub-agents during a flow** — the
  `validate-pretool` Agent path rejects empty or
  `general-purpose` `subagent_type` calls when a flow is
  active.
- **`AskUserQuestion` during an autonomous in-progress phase**
  — `validate-ask-user` rejects with exit 2 when
  `phases.<current_phase>.status == "in_progress"` AND
  `skills.<current_phase>.continue == "auto"`. Carve-out: when
  the most recent assistant Skill tool_use call (since the most
  recent user turn) targets a skill in
  `crate::hooks::transcript_walker::USER_ONLY_SKILLS`, the
  block is suppressed so user-only skills' confirmation prompts
  fire mid-autonomous. See `.claude/rules/user-only-skills.md`
  Layer 2.
- **Voluntary turn-end during autonomous in-progress phases** —
  `stop_continue::check_autonomous_in_progress` refuses the Stop
  event with `{"decision":"block"}` and an autonomous-mode reason
  when the current phase is in-progress AND configured `auto` AND
  `_continue_pending` is empty. Closes the text-only-stop hole
  that PreToolUse hooks cannot reach: a model that ends the turn
  with prose alone (no tool call) is invisible to PreToolUse, but
  the Stop hook fires and refuses the turn-end. The block runs
  AFTER `check_first_stop` and `check_continue` so discussion
  mode and multi-child-skill chains keep their semantics.
- **Prose-based pauses at Code-phase task-entry boundaries** —
  `stop_continue::check_prose_pause_at_task_entry` refuses the
  Stop event with `{"decision":"block"}` and a prose-pause reason
  when ALL seven guards hold: `current_phase == "flow-code"`,
  `phases.flow-code.status == "in_progress"`,
  `skills.flow-code.continue == "auto"`, `code_task == 0`,
  `_continue_pending` is empty, the most recent assistant
  transcript turn contains a `?` outside fenced code blocks and
  inline code spans, and the most recent assistant turn
  contains no `tool_use` block. Composed BEFORE
  `check_autonomous_in_progress` so its more specific message
  (citing `.claude/rules/autonomous-flow-self-recovery.md`) wins
  for the prose-pause shape; other text-only stops fall through
  to the generic predicate. Closes the AskUserQuestion-bypass
  surface where a model emits a prose question and ends the turn
  without any tool call (validate-ask-user only fires on
  `AskUserQuestion` tool calls). See
  `.claude/rules/autonomous-phase-discipline.md` "Prose-Based
  Pauses Bypass AskUserQuestion".
- **Model invocation of user-only skills** —
  `validate-skill` rejects Skill tool calls naming
  `flow:flow-abort`, `flow:flow-reset`, `flow:flow-release`, or
  `flow:flow-prime` when the most recent user-role turn in the
  persisted transcript does NOT contain a matching
  `<command-name>/<skill></command-name>` substring. See
  `.claude/rules/user-only-skills.md` Layer 1.
- **Edit/Write on `.claude/` paths during a flow** —
  `validate-claude-paths` redirects to
  `bin/flow write-rule` for `CLAUDE.md`,
  `.claude/rules/`, and `.claude/skills/`.
- **Edit/Write on `~/.claude/projects/` (transcript root) in
  any context** — `validate-claude-paths` rejects Edit/Write
  regardless of flow state. The transcript file backs Layer 1's
  user-invocation check; tampering would defeat
  `validate-skill`. Read access is preserved for the walker
  itself. See `.claude/rules/user-only-skills.md` Layer 3.
- **Edit/Write on shared config files inside a worktree** —
  `validate-worktree-paths` rejects modifications to
  `.gitignore`, `.gitattributes`, `Makefile`, etc., without
  explicit user confirmation.
