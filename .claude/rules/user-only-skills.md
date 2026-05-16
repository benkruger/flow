# User-Only Skills

Six FLOW skills are reserved for direct user invocation. The model
must never invoke them — neither via the Skill tool, nor by
suggesting that an `AskUserQuestion` answer should be "yes, run
`/flow:flow-X`". Each skill performs an action whose authorization
must come from explicit user intent (the user typing the slash
command) rather than from inferred context.

## The Set

| Skill | Action | Reason for the gate |
|---|---|---|
| `/flow:flow-abort` | Closes the PR, deletes the remote branch, removes the worktree, deletes the state file. | Destructive — losing in-flight work. |
| `/flow:flow-reset` | Same destructive shape but applied across every active flow on the machine. | Destructive — losing in-flight work across multiple flows. |
| `/flow-release` | Bumps version, tags, pushes, and creates a public GitHub Release. | Resource-shipping — visible to plugin marketplace consumers. Project-local skill at `.claude/skills/flow-release/`; Claude Code emits the bare-namespace form because the skill is not in the `flow:` plugin namespace. |
| `/flow-qa` | Files a pre-decomposed QA issue against the FLOW plugin repo (`benkruger/flow`) with assignee `@me`, triggering a full FLOW lifecycle on the QA-pass branch. | Resource-shipping — the filed issue becomes shared GitHub state visible to plugin consumers and pulls a maintainer through Start → Code → Review → Learn → Complete. Project-local skill at `.claude/skills/flow-qa/`; Claude Code emits the bare-namespace form because the skill is not in the `flow:` plugin namespace. |
| `/flow:flow-prime` | Writes `.claude/settings.json` and the four `bin/*` stubs into the project. | Environment-mutating — modifies shared config the project has not yet reviewed. |
| `/flow:flow-continue` | Clears `_halt_pending` so the paused autonomous flow resumes. | Authorization to resume must come from explicit user intent — the user typing the slash command — rather than from model inference. The matching `bin/flow clear-halt` subcommand self-gates on the same transcript marker, so even a model that bypasses Layer 1 cannot clear the halt. |

The criterion is "model must never propose." This is stricter than
the sibling "ask-first" pattern (`/flow:flow-explore`,
`/flow:flow-plan`, `/flow:flow-start`, etc.) where the model may
ask the user whether to proceed but the user then answers and the
model invokes. For user-only skills the model does NOT invoke even
after a hypothetical "yes" answer — the user types the slash
command directly.

## Three-Layer Enforcement Chain

The six skills are protected by three independent mechanical
gates so a single bypass does not defeat the discipline.

### Layer threat mapping

Each layer addresses a specific bypass surface:

- **Layer 1 (`validate-skill`)** — defends against *direct model
  invocation* of a user-only skill via the Skill tool. The hook
  fires PreToolUse:Skill and blocks unless the most recent user-
  role transcript turn typed the matching slash command.
- **Layer 2 (`validate-ask-user` carve-out)** — defends against
  *autonomous-phase deadlock* when a user typing
  `/flow:flow-abort` mid-autonomous-flow needs the
  destructive-confirmation prompt to fire. The carve-out
  suppresses the autonomous-phase block on AskUserQuestion when
  the most recent assistant Skill tool_use call fired a user-only
  skill — meaning the user just typed the slash command and Layer
  1 already verified it.
- **Layer 3 (`validate-claude-paths` transcript root)** — defends
  against *transcript tampering* AND *transcript content
  exfiltration* that would defeat Layer 1's user-invocation check.
  Blocks Edit, Write, Read, Glob, and Grep across the
  `~/.claude/projects/` transcript subtree regardless of flow
  state. The auto-memory subdirectory
  (`~/.claude/projects/<id>/memory/...`) is CARVED OUT so the
  user's MEMORY.md remains readable — the
  `UNIVERSAL_ALLOW Read(~/.claude/projects/*/memory/*)` entry
  documents the same boundary at the settings layer. Internal
  walkers in Layer 1 and Layer 2 use `fs::read_to_string` from
  Rust subprocesses rather than the Read tool, so blocking
  Read/Glob/Grep at the tool layer does not affect them.

If Layer 1's substring or membership check has a bypass, Layers 2
and 3 cannot independently catch the bypass — they are defense in
depth around Layer 1's correctness, not redundant gates over the
same surface. The Layer 1 gate's `normalize_gate_input`
(NUL strip + trim + ASCII lowercase) and slash-command anchoring
are therefore the load-bearing checks; the other two layers
extend the protection but do not replace it.

### Layer 1: `validate-skill` (PreToolUse:Skill)

`src/hooks/validate_skill.rs` runs on every Skill tool call. When
`tool_input.skill` (after normalization) is in the user-only set
AND the most recent user-role turn's `message.content` string in
the persisted transcript at `transcript_path` does NOT begin with
either emission shape Claude Code uses for the matching slash
command — the two-line
`<command-message><skill></command-message>\n<command-name>/<skill></command-name>`
(Claude Code 2.1.140+) or the legacy
`<command-name>/<skill></command-name>` — the hook exits 2 and
Claude Code rejects the tool call. The block message names the
skill (in canonical lowercased form) and points to this rule
file.

The walker
(`src/hooks/transcript_walker.rs::last_user_message_invokes_skill`)
scans backward through the transcript JSONL, stops at the first
user-role turn, and requires the trimmed content to START with
one of the two emission shapes above. The check is a
`starts_with` disjunction (slash-command anchoring on both
shapes): user prose mentioning either marker substring mid-text
fails because the prose's leading bytes are neither
`<command-message>` nor `<command-name>`. Tool_result-wrapped
user turns whose content is an array of blocks (carrying
assistant-echoed text) are explicitly rejected because
`is_real_user_turn` discards array-content turns before the
`starts_with` comparison runs.

The walker reads the LAST `TRANSCRIPT_BYTE_CAP` bytes of the
file (50 MB) so the most recent turns are always visible
regardless of total transcript size. Per
`.claude/rules/external-input-path-construction.md`, the
`transcript_path` is validated through
`crate::session_metrics::is_safe_transcript_path` — which rejects
empty, NUL-byte, relative, ParentDir-component, and
prefix-escaping paths.

Layer 1 also enforces the halt gate (see
`.claude/rules/autonomous-phase-discipline.md` "Defense in
depth — halt gates on Skill and Bash"): when `_halt_pending=true`
in the state file, every Skill call is blocked except the
user-only exits the user has already typed. The user-only
allow-path runs BEFORE the halt check so a user-typed
`/flow:flow-continue` or `/flow:flow-abort` passes the gate
cleanly.

### Layer 2: `validate-ask-user` carve-out

`src/hooks/validate_ask_user.rs::user_only_skill_carve_out_applies`
allows `AskUserQuestion` to fire even during in-progress
autonomous phases when the most recent assistant turn fires at
least one Skill tool_use whose `input.skill` (after normalization)
is in `USER_ONLY_SKILLS`. Without this carve-out, a user typing
`/flow:flow-abort` during an in-progress autonomous Code phase
would deadlock — the abort skill's destructive-confirmation
prompt would be blocked by the existing autonomous-phase-
discipline gate.

The carve-out fires after `validate` would have returned a block
and overrides it. The walker function
(`most_recent_skill_in_user_only_set`) stops at the most recent
user turn going backward — older Skill calls outside that window
are invisible. Multi-tool assistant turns are scanned in full
(extract_skill_invocations returns all Skill names), so a user-
only Skill appearing second or later in the turn's content
array still satisfies the carve-out.

The user-only-skill carve-out is the first sanctioned exception
to the autonomous-phase AskUserQuestion block. The shared-config
carve-out (see `.claude/rules/autonomous-phase-discipline.md`
"Shared-Config Carve-Out") is the second; both fire in
`validate-ask-user::run_impl_main` between the autonomous-phase
block and the block return, with the user-only check ordered
first.

### Layer 3: `validate-claude-paths` transcript root lockdown

`src/hooks/validate_claude_paths.rs::is_transcript_path` walks the
target path's components and matches whenever any segment is
`.claude` followed by `projects` (case-insensitive). The match
covers the `~/.claude/projects/` subtree EXCEPT for the
auto-memory subdirectory: when the third component (counting from
the `.claude/projects/<project-id>/` level) is `memory`, the
matcher carves out so memory files remain readable. The block
fires regardless of flow state because transcript tampering can
subvert Layer 1, AND a model Read/Glob/Grep of the transcript
root sits outside the project root and would surface a permission
prompt mid-flow.

The hook is registered on the `Edit`, `Write`, and `Read|Glob|Grep`
matchers in `hooks/hooks.json`. The path-extraction helper
(`get_file_path`) reads `tool_input.file_path`,
`tool_input.path`, and `tool_input.pattern` in priority order so
Glob and Grep payloads (which carry the target path in `path` or
`pattern` rather than `file_path`) are matched correctly.

The block message leads with a redirect to
`bin/flow write-rule --path .claude/rules/<topic>.md` so a
behavioral constraint the model wanted to persist as memory has a
concrete path to land as a project rule instead. The message
points at `.claude/rules/persistence-routing.md` as the routing
decision tree.

Internal walkers in `validate-skill` and `validate-ask-user` use
`fs::read_to_string` from inside Rust subprocesses, not the Read
tool, so blocking the Read tool at this layer does not affect
them. The model's Read tool calls on protected paths
(`.claude/rules/`, `.claude/skills/`, `CLAUDE.md`) are also
preserved (only Edit and Write redirect to `bin/flow write-rule`)
— a model that needs to read a rule file during a flow can still
do so.

Residual surface: Bash file-mutation commands beyond redirect
(`cp`, `mv`, `dd`) are not blocked by this layer — `validate-pretool`
covers redirect (`>`, `>>`, `tee`). A future tightening could add
those tokens to `validate-pretool`'s deny list when the target
resolves under `~/.claude/projects/`.

## How to Add a Skill to the User-Only Set

1. Add the skill name (`flow:flow-<name>`) to `USER_ONLY_SKILLS` in
   `src/hooks/transcript_walker.rs`.
2. Add the skill row to the table in this rule file with action
   description and threat-shape rationale.
3. Add a `validate_user_only_skill_<name>_is_in_set` test in
   `tests/hooks/validate_skill.rs`.
4. Confirm the skill's `SKILL.md` has a HARD-GATE that prompts the
   user before performing the destructive / resource-shipping
   action.

## How to Apply (Skill Authoring)

When designing a new skill that performs a destructive,
resource-shipping, or environment-mutating action, decide whether
it belongs in the user-only set or the ask-first set:

- **User-only** — the action's authorization must come from
  explicit user intent. Adding the skill name to
  `USER_ONLY_SKILLS` enables Layer 1 enforcement automatically.
- **Ask-first** — the model may invoke after asking the user via
  `AskUserQuestion`. No mechanical block; the discipline is
  documented in `.claude/rules/flow-requires-user-initiative.md`.

Default to user-only when the action's blast radius spans
shared resources (PRs, branches, releases, project config).
Reserve ask-first for scoped actions whose error path is local
recovery (`/flow:flow-explore` files a problem statement and
`/flow:flow-plan` files an implementation plan but the user can
close either; `/flow:flow-start` opens a worktree but the user
can abort).

See `.claude/rules/autonomous-phase-discipline.md` "User-Only
Skill Carve-Out" for the interaction with autonomous phases.
