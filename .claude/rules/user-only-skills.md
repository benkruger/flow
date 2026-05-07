# User-Only Skills

Four FLOW skills are reserved for direct user invocation. The model
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
| `/flow:flow-release` | Bumps version, tags, pushes, and creates a public GitHub Release. | Resource-shipping — visible to plugin marketplace consumers. |
| `/flow:flow-prime` | Writes `.claude/settings.json` and the four `bin/*` stubs into the project. | Environment-mutating — modifies shared config the project has not yet reviewed. |

The criterion is "model must never propose." This is stricter than
the sibling "ask-first" pattern (`/flow:flow-create-issue`,
`/flow:flow-start`, etc.) where the model may ask the user whether
to proceed but the user then answers and the model invokes. For
user-only skills the model does NOT invoke even after a
hypothetical "yes" answer — the user types the slash command
directly.

## Three-Layer Enforcement Chain

The four skills are protected by three independent mechanical
gates so a single bypass does not defeat the discipline.

### Layer 1: `validate-skill` (PreToolUse:Skill)

`src/hooks/validate_skill.rs` runs on every Skill tool call. When
`tool_input.skill` is in the user-only set AND the persisted
transcript at `transcript_path` does NOT contain a matching
`<command-name>/<skill></command-name>` substring in the most
recent user-role turn, the hook exits 2 and Claude Code rejects
the tool call. The block message names the skill and points to
this rule file.

The walker
(`src/hooks/transcript_walker.rs::last_user_message_invokes_skill`)
scans backward through the transcript JSONL, stops at the first
user-role turn, and checks that turn for the matching tag. The
read is capped at `TRANSCRIPT_BYTE_CAP` (50 MB) per
`.claude/rules/external-input-path-construction.md`.

### Layer 2: `validate-ask-user` carve-out

`src/hooks/validate_ask_user.rs::user_only_skill_carve_out_applies`
allows `AskUserQuestion` to fire even during in-progress
autonomous phases when the most recent assistant Skill tool_use
call (since the most recent user turn) targets a user-only skill.
Without this carve-out, a user typing `/flow:flow-abort` during
an in-progress autonomous Code phase would deadlock — the abort
skill's destructive-confirmation prompt would be blocked by the
existing autonomous-phase-discipline gate.

The carve-out fires after `validate` would have returned a block
and overrides it. The walker function
(`most_recent_skill_in_user_only_set`) stops at the most recent
user turn going backward — older Skill calls outside that window
are invisible.

### Layer 3: `validate-claude-paths` transcript root lockdown

`src/hooks/validate_claude_paths.rs::is_transcript_path` blocks
Edit/Write on `~/.claude/projects/<project>/<session>.jsonl` —
the persisted transcript root. The block fires regardless of
flow state because transcript tampering can subvert Layer 1: a
hand-injected fake user `<command-name>` line in an old
transcript would bypass the user-invocation check.

Read access is preserved because Layer 1 and Layer 2 walkers
themselves need to scan the file. The hook is registered for the
Edit and Write matchers only in `hooks/hooks.json`.

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
   `tests/validate_skill.rs`.
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
recovery (`/flow:flow-create-issue` files an issue but the user
can close it; `/flow:flow-start` opens a worktree but the user
can abort).
