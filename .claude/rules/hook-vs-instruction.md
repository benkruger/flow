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
  construction. The gate carries three context-specific
  carve-outs that cover the legitimate skill-driven and
  user-typed commit paths; raw `git commit` is never carved
  out in any context. The active-flow context's skill-commit
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
  their emission. The third (trunk) carve-out — wired ONLY
  into the destination-path integration-branch arm via
  `flow_commit_trunk_carveout_applies` — passes `bin/flow
  ... finalize-commit <msg> <trunk>` when BOTH (a) the
  caller's cwd is NOT inside an active-flow worktree
  (`detect_branch_from_path(cwd)` + `is_flow_active(branch,
  main_root)`) AND (b) the most recent real user turn in the
  persisted transcript typed `/flow:flow-commit` as a slash
  command. This is the supported on-trunk maintainer path
  (bootstrap repair, follow-up after a hot patch). The
  cwd-not-active-flow check is the structural bound that
  prevents a feature-branch worktree's `/flow:flow-commit`
  from spuriously authorizing a trunk commit — the user's
  slash-command intent stays bound to the cwd it was typed
  from. The user-typed slash command is the unforgeable
  trust anchor; `/flow:flow-commit` itself supplies the diff
  review and commit-message review choreography once the
  carve-out lets the call through. The cwd-path arm is NOT
  extended with this carve-out: raw `git commit` (or
  `git -C <trunk> commit`) carries no slash-command marker
  for the gate to anchor on. Both the
  active-flow and bootstrap carve-outs apply branch-
  agnostically — `default_branch_in` resolves the actual
  integration trunk so they apply identically to `main`,
  `staging`, `master`, etc.; the trunk carve-out is likewise
  branch-agnostic. The integration-branch context has no per-
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
- **Agent-prompt out-of-worktree path scan** —
  `validate-pretool`'s Agent branch calls
  `agent_prompt_scan::validate_agent_prompt` on the
  `tool_input.prompt` field after the `subagent_type` check
  passes. The helper extracts path-shape substrings via a
  regex+boundary scan, validates them through
  `is_safe_path_candidate`, resolves relative candidates
  against the cwd-derived `worktree_root` (via
  `compute_worktree_root`), lexically normalizes the result,
  and rejects any candidate that does not start with
  `worktree_root`. Blocks with exit 2 and a structured
  message naming the offending path and the worktree. The
  scan is scoped to active flows and bounded at
  `AGENT_PROMPT_BYTE_CAP` (1 MB) per
  `.claude/rules/external-input-path-construction.md`. See
  `src/hooks/agent_prompt_scan.rs` and
  `.claude/rules/cognitive-isolation.md` "Retry-prompt
  path-scoping constraint".
- **PreToolUse:Agent required-agent recording** —
  `validate-pretool`'s Agent branch calls
  `agent_run_record::record_agent_run` after the prompt scan
  passes and before the call is allowed. When the launched
  `subagent_type` (normalized to the `flow:<name>` form)
  matches a required agent for the active in-progress phase,
  it records the run into `phases.<phase>.agents_returned`
  (set-semantics) so `phase-finalize`'s required-agents gate
  has unforgeable evidence the agent was launched — only a
  real Agent tool call reaches this code, so the model cannot
  fabricate the record by synthesizing a CLI invocation. This
  is a recorder, not a blocking gate: it is fail-open and
  never blocks the Agent launch (missing/corrupt state,
  invalid branch, non-matching subagent, or non-`in_progress`
  phase records nothing). See `src/hooks/agent_run_record.rs`.
- **Autonomous-flow-strict response shape on blocked paths** —
  `validate-worktree-paths::validate()` checks
  `crate::flow_paths::is_autonomous_flow_active(project_root,
  branch)` (branch derived from `worktree_root.file_name()`)
  at both block surfaces. When the predicate returns true, the
  hook returns a structured JSON envelope
  `{"status":"error","reason":...,"blocked_path":...,
  "worktree":...,"autonomous":true}` instead of the
  human-readable BLOCKED message; exit 2 is preserved. The
  `reason` is `out_of_worktree_in_autonomous` for the
  in-project-but-out-of-worktree redirect and
  `out_of_bounds_in_autonomous` for the out-of-project
  fail-closed gate (a path outside `project_root` that is not
  in the approved memory + `/tmp` scratch surface). Default
  (non-autonomous-flow) behavior is the human-readable BLOCKED
  prose for either surface. During an active flow (cwd inside
  a worktree), out-of-project paths are fail-closed except the
  approved surface (`is_approved_out_of_project_path`: memory
  dir + `/tmp` scratch); a blocked path returns exit 2 with no
  native prompt, so an unattended flow never hangs on a native
  permission prompt for an out-of-project path. The remaining
  boundary is non-flow contexts (cwd not inside a worktree),
  where the early "not in a worktree" return leaves path
  jurisdiction to Claude Code. See
  `src/hooks/validate_worktree_paths.rs`.
- **Misplaced worktree-internal `.flow-states/` Write/Edit
  auto-rewrite** — `validate-worktree-paths` does NOT block a
  misplaced worktree-internal `.flow-states/` Write or Edit.
  `run_impl_main` consults `misplaced_flow_states_rewrite`
  first; on a match it returns `WorktreeAction::Rewrite` and
  `run()` emits a PreToolUse `updatedInput` JSON envelope to
  stdout (exit 0) that replaces only `file_path` with the
  canonical `<project_root>/.flow-states/` destination — every
  other tool-input field is preserved verbatim. Claude Code
  reissues the call against the canonical path with no
  model-visible stumble. A misplaced `.flow-states/`
  Read/Glob/Grep still blocks (exit 2) via `validate()`
  because it has no `content`/`file_path` pairing that can be
  auto-redirected without returning data the model did not ask
  for. See `src/hooks/validate_worktree_paths.rs` and
  `.claude/rules/file-tool-preflights.md` "Canonical Location
  for `.flow-states/`".
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
      the block is suppressed so a system-initiated
      confirmation prompt raised in response to that
      shared-config block can fire instead of deadlocking
      against the autonomous-phase block. The BLOCKED message
      itself instructs the user to reply with `approve
      shared-config: <path>` and the model to run `bin/flow
      approve-shared-config` — the model never fires an
      `AskUserQuestion` for a shared-config edit, so the
      carve-out is the system-initiated-prompt safety net,
      not a model-`AskUserQuestion` release. The user-only
      carve-out is checked first. See
      `.claude/rules/autonomous-phase-discipline.md`
      "Shared-Config Carve-Out".
- **Autonomous Stop refusal** — `stop_continue::run` composes
  three predicates in order: `check_in_progress_utility_skill`
  (refuses turn-end when a multi-step utility skill marker
  exists and the decompose skill (bare `decompose` or
  namespaced `decompose:decompose`) is the most recent Skill
  in the transcript), `check_continue` (forces continuation when
  `_continue_pending=<skill>` is set, supporting multi-child-
  skill chains), and `check_autonomous_stop` (the unified
  autonomous-mode gate). The hook refuses the autonomous
  flow's end AFTER the turn-end happens — every turn ends
  when the model stops emitting tool calls, and the refusal
  text becomes the next turn's input via hook feedback. "Stop
  refused" never means "the model cannot end the turn"; it
  means "the autonomous flow's end is refused, queue another
  turn." See `.claude/rules/no-performative-pause.md` for the
  prose discipline that flows from this semantics.
  `check_autonomous_stop` applies three rules when the current
  phase is in-progress + auto:
    - **Rule 1** — no halt and no new user message: queue
      another turn carrying one of two refusal messages,
      selected by the autonomous-stalling counter. Below
      `CONSECUTIVE_UNCHANGED_THRESHOLD` consecutive Stops in
      autonomous flow-code without a `code_task` advance, the
      generic encouraging text fires: `"Stop Refused:
      Continue, you can do it. Don't give up, you got this!
      No excuses!"`. At or above the threshold, the refusal
      swaps to `RULE_1_STOP_REFUSED_POINTED_MESSAGE` — a
      pointed text that names the autonomous-stalling pattern
      and demands a task-advancing tool call. The counter pair
      `_last_observed_code_task` and
      `_consecutive_unchanged_count` records the running
      state and is cleared by `phase_enter()` on every phase
      entry. Non-flow-code autonomous phases (Review, Learn,
      Complete) get the generic encouraging text only — they
      have no `code_task` analog. The turn-end was real; the
      harness queues the refusal text as the next turn's
      input. Framing this turn-end as "the model cannot stop"
      inverts the semantics; see
      `.claude/rules/no-performative-pause.md`. See
      `.claude/rules/autonomous-phase-discipline.md`
      "Forbidden Stalling Frames" for the pointed-swap
      design.
    - **Rule 2** — `_halt_pending=true` and no new user
      message: queue another turn carrying a refusal message
      that names the two exits `/flow:flow-continue` (resume)
      and `/flow:flow-abort` (close the flow). Persists across
      every subsequent Stop until the user invokes
      `/flow:flow-continue`.
    - **Conversation pass-through** — a real **conversational
      prose** user message appeared since the model's most
      recent Skill action (detected via
      `transcript_walker::most_recent_user_message_since_skill_action`,
      which filters synthetic `isMeta:true` and
      `isCompactSummary:true` turns per
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
  returns the decompose skill (bare `decompose` or namespaced
  `decompose:decompose`, recognized by `is_decompose_skill`) for the
  hook's `transcript_path`. The walker is the discriminator that
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
