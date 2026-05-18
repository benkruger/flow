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
- **Whitelist enforcement under an active flow** — Layer 9
  rejects commands not present in the merged allow list.
- **Direct commits during a flow** — Layer 10 rejects
  `git ... commit` and `bin/flow ... finalize-commit`
  invocations whose effective destination resolves to the
  integration branch named by `default_branch_in` OR to a
  feature branch with an active FLOW state file at
  `.flow-states/<branch>/state.json`. The effective
  destination has two dispatch paths: for `bin/flow
  finalize-commit <msg> <branch>` shapes, Layer 10 binds to
  the explicit `<branch>` positional argument (the
  destination path) and checks the integration-branch arm
  via `match_finalize_commit_destination` and the
  active-flow arm at `<main_root>/.worktrees/<branch>/`; for
  `git commit`, `git -C <path> commit`, and any malformed
  finalize-commit shape whose branch arg cannot be
  extracted, Layer 10 falls back to the caller's process cwd
  (and any `-C <path>` target). The branch-arg extraction
  validates via `FlowPaths::is_valid_branch` so a `/`-,
  `.`/`..`-, or NUL-bearing arg cannot reach path
  construction. Both dispatch paths share two context-
  specific carve-outs that cover the legitimate skill-
  driven commit paths; raw `git commit` is never carved out
  in either context. The active-flow context's skill-commit
  carve-out passes `bin/flow ... finalize-commit` when the
  state file has `_continue_pending == "commit"` AND the
  persisted transcript shows the most recent assistant Skill
  since the most recent user turn is one of `flow:flow-commit`
  or `flow-release` (the shared two-arm
  `transcript_shows_commit_window_skill` predicate). The
  integration-branch context's bootstrap-skill carve-out
  passes `bin/flow ... finalize-commit` when the transcript
  shows the same two-arm match AND a sanctioned bootstrap
  parent (`flow:flow-start`, `flow:flow-prime`, or
  `flow-release`) in the post-user-turn chain. The
  sanctioned-parent set is `BOOTSTRAP_SKILLS` in
  `validate_pretool.rs`. `flow-release` is the bare-name
  project-local maintainer skill at `.claude/skills/flow-release/`;
  the other two bootstrap parents are plugin-marketplace
  skills at `skills/<name>/` and carry the `flow:` prefix in
  their emission. The carve-out is branch-agnostic —
  `default_branch_in` resolves the actual integration trunk so
  the carve-out applies identically to `main`, `staging`,
  `master`, etc. The integration-branch context has no per-
  branch state file at the trunk, so the bootstrap carve-out
  uses a SECOND walker condition where the active-flow
  carve-out uses a state-file marker — both walker conditions
  are load-bearing. The bootstrap carve-out's `-C`-target
  scope exclusion lives on the cwd path only; the
  destination path's integration-branch arm also applies the
  bootstrap carve-out because the branch arg is the
  authoritative routing key. See
  `.claude/rules/concurrency-model.md` "Mechanical
  Enforcement" for the bypass surface, the carve-out trust-
  contract analyses, and the documented v1 gaps.
- **`bin/flow ci` during Code phase** — Layer 11 redirects
  `bin/flow ci` (every variant) to the per-file gate
  (`bin/test tests/<name>.rs`) when an active flow has
  `current_phase == "flow-code"` AND
  `phases.flow-code.status == "in_progress"`. The single
  carve-out is `bin/flow ci --clean` — the documented
  phantom-misses recovery path. The block fires only when ALL
  of: command shape is `bin/flow ... ci` (with `ci` as the
  first non-flag token after the launcher, so sibling
  subcommands taking `ci` as an arg pass through);
  `--clean` (case-insensitive, with optional `=value` suffix)
  is absent; an active flow exists; and the state file's
  normalized `current_phase` and `phases.flow-code.status`
  match. Fail-closed-as-no-block — every state-file read or
  parse error returns "no block" because Layer 11 is
  friction-prevention, not a security gate. The commit-time
  CI gate inside `finalize-commit` calls `ci::run_impl()` as
  a Rust function and never reaches the Bash hook, so cross-
  file regressions are still caught at the commit boundary.
  See `.claude/rules/per-file-coverage-iteration.md`
  "Enforcement".
- **Halt gate on Bash commands** — `validate-pretool` blocks
  flow-advancing Bash commands when the active flow's state
  file has `_halt_pending=true`. The closed allowlist is the
  set of `bin/flow` subcommands that progress the flow:
  `phase-enter`, `phase-finalize`, `phase-transition`,
  `finalize-commit`, `set-utility-in-progress`, and
  `set-timestamp --set code_task=*`. Other `bin/flow`
  subcommands (logging, status, `set-timestamp` on non-counter
  fields, `clear-halt` itself) pass through. The block message
  names `/flow:flow-continue` (resume) and `/flow:flow-abort`
  (give up) as the only sanctioned exits. See
  `.claude/rules/autonomous-phase-discipline.md` "Defense in
  depth — halt gates on Skill and Bash".
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
  `skills.<current_phase>.continue == "auto"`. Two carve-outs
  suppress the block:
    - **User-only-skill carve-out**: when the most recent
      assistant Skill tool_use call (since the most recent
      user turn) targets a skill in
      `crate::hooks::transcript_walker::USER_ONLY_SKILLS`, the
      block is suppressed so user-only skills' confirmation
      prompts fire mid-autonomous. See
      `.claude/rules/user-only-skills.md` Layer 2.
    - **Shared-config carve-out**: when the most recent
      user-role turn carries a `validate_worktree_paths`
      shared-config edit-block tool_result (a `tool_result`
      with `is_error: true` whose content contains the
      literal substring `"is a shared configuration file"`),
      the block is suppressed so the system-initiated
      AskUserQuestion the BLOCKED message demanded fires
      instead of deadlocking. The user-only carve-out is
      checked first. See
      `.claude/rules/autonomous-phase-discipline.md`
      "Shared-Config Carve-Out".
- **Autonomous Stop refusal** — `stop_continue::run` composes
  three predicates in order: `check_in_progress_utility_skill`
  (refuses turn-end when a multi-step utility skill marker
  exists and `decompose:decompose` is the most recent Skill in
  the transcript), `check_continue` (forces continuation when
  `_continue_pending=<skill>` is set, supporting multi-child-
  skill chains), and `check_autonomous_stop` (the unified
  autonomous-mode gate). `check_autonomous_stop` applies three
  rules when the current phase is in-progress + auto:
    - **Rule 1** — no halt and no new user message: refuse the
      Stop with the encouraging message `"Stop Refused:
      Continue, you can do it. Don't give up, you got this!
      No excuses!"`. The autonomous flow must keep going.
    - **Rule 2** — `_halt_pending=true` and no new user
      message: refuse the Stop with a message naming the two
      exits `/flow:flow-continue` (resume) and
      `/flow:flow-abort` (close the flow). Persists across
      every subsequent Stop until the user invokes
      `/flow:flow-continue`.
    - **Conversation pass-through** — a real **conversational
      prose** user message appeared since the model's most
      recent Skill action (detected via
      `transcript_walker::most_recent_user_message_since_skill_action`,
      which filters synthetic `isMeta:true` turns per
      `.claude/rules/transcript-shape.md` AND filters
      imperative slash-command shapes
      `<command-name>/<skill></command-name>` or the
      two-line `<command-message>...</command-message>`
      form): set `_halt_pending=true` and allow the Stop so
      the model can answer. The next Stop without a new
      conversational prose message fires Rule 2.
      `/flow:flow-continue` is the universal resume directive
      — the walker watermarks any preceding prose to `None`
      so the next Stop after a resume fires Rule 1 instead of
      re-arming Rule 2 from the user's prior pause prose. See
      `.claude/rules/transcript-shape.md` "Real User Turns:
      Imperative vs Conversational Shapes" for the
      discriminator.
  See `.claude/rules/autonomous-phase-discipline.md` "The
  Two-Exit Halt Model" for the full design and the lifecycle
  of `_halt_pending`.
- **Halt gate on Skill calls** — `validate-skill` Layer 2
  blocks any Skill tool call when `_halt_pending=true` unless
  the skill is in `USER_ONLY_SKILLS` AND the user typed the
  matching slash command. The user-only allow-path runs
  BEFORE the halt check so `/flow:flow-continue` and
  `/flow:flow-abort` pass cleanly. See
  `.claude/rules/autonomous-phase-discipline.md` "Defense in
  depth — halt gates on Skill and Bash".
- **Decompose-return turn-end during a multi-step utility skill** —
  `stop_continue::check_in_progress_utility_skill` refuses the Stop
  event with `{"decision":"block"}` and the verbatim encouraging
  message `"Stop Refused: Continue, you can do it. Don't give up,
  you got this! No excuses!"` when BOTH (a) the per-session
  utility marker at
  `<home>/.claude/flow/utility-in-progress-<session_id>.json`
  exists and names a skill in
  `crate::commands::utility_marker::MULTI_STEP_UTILITY_SKILLS`,
  AND (b) `crate::hooks::transcript_walker::most_recent_skill_since_user`
  returns `Some("decompose:decompose")` for the hook's
  `transcript_path`. The walker is the discriminator that
  distinguishes "decompose just returned mid-pipeline" (block) from
  "model just sent a normal conversational reply" (no block) — so
  discussion-mode replies during these utility skills end the turn
  cleanly. Composed FIRST in `stop_continue::run`, BEFORE
  `check_continue` and `check_autonomous_stop`.
- **Model invocation of user-only skills** —
  `validate-skill` Layer 1 rejects Skill tool calls naming
  user-only skills when the most recent user-role turn in the
  persisted transcript does NOT contain a matching slash-
  command marker. See `.claude/rules/user-only-skills.md` for
  the full set (including `/flow:flow-continue`) and Layer 1
  details.
- **Edit/Write on `.claude/` paths during a flow** —
  `validate-claude-paths` redirects to
  `bin/flow write-rule` for `CLAUDE.md`,
  `.claude/rules/`, and `.claude/skills/`.
- **Edit/Write/Read/Glob/Grep on `~/.claude/projects/`
  (transcript root) in any context** —
  `validate-claude-paths` rejects Edit, Write, Read, Glob,
  and Grep across the `~/.claude/projects/` subtree
  regardless of flow state, except for the auto-memory
  subdirectory (`~/.claude/projects/<id>/memory/...`) which
  is carved out so the user's MEMORY.md remains readable.
  The transcript files back Layer 1's user-invocation check;
  tampering would defeat `validate-skill`, and a model
  Read/Glob/Grep of the transcript root would surface a
  permission prompt mid-flow. Internal walkers in
  `validate-skill` and `validate-ask-user` use
  `fs::read_to_string` from Rust subprocesses rather than
  the Read tool, so blocking Read/Glob/Grep at the tool
  layer does not affect them. See
  `.claude/rules/user-only-skills.md` Layer 3.
- **Edit/Write on shared config files inside a worktree** —
  `validate-worktree-paths` rejects modifications to
  `.gitignore`, `.gitattributes`, `Makefile`, etc., without
  explicit user confirmation.
