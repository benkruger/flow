# Hook cwd Resolution

PreToolUse enforcement hooks must resolve the working directory they
reason about from the hook payload's `cwd` field — via
`crate::hooks::resolve_hook_cwd` — not from the hook subprocess's own
`std::env::current_dir()`.

## Why

Claude Code spawns each PreToolUse hook as a subprocess and supplies
the session's (and a sub-agent's) working directory in the payload
`cwd` field. During an active flow that directory is the worktree.
The hook subprocess's own `std::env::current_dir()`, by contrast, can
resolve to the main repo root (the directory the session was started
in) rather than the worktree. When a hook reads `env::current_dir()`
and it points at the main repo root, every worktree-derived gate sees
the wrong directory:

- `validate-worktree-paths` — `compute_worktree_paths` returns `None`,
  the gate self-disables, and an out-of-worktree tool call falls
  through to a native permission prompt that hangs an autonomous flow.
- `validate-claude-paths` — `flow_active` stays false and the
  `.claude/` redirect silently disables.
- `validate-skill` — the state file (and thus the halt gate) does not
  resolve.
- `validate-pretool` — the five cwd consumers (branch detection,
  `main_root`, `flow_active`, the agent-prompt `worktree_root`, and
  the Layer 10/11 + halt gates) all see the wrong directory; most
  visibly the agent-prompt scan resolves no worktree and skips,
  letting an out-of-worktree path reach a sub-agent.

## The Rule

`resolve_hook_cwd(hook_input: &Value) -> Option<String>` returns the
payload `cwd` when present and non-empty, else falls back to
`std::env::current_dir()`. Every hook `run()` wrapper resolves cwd
through it:

```rust
let cwd = hook_input.as_ref().and_then(crate::hooks::resolve_hook_cwd);
```

A new PreToolUse hook that reasons about the worktree, branch, state
file, or `.flow-states/` paths MUST resolve its cwd through
`resolve_hook_cwd`, never directly from `env::current_dir()`. The
fallback preserves behavior when no payload `cwd` is supplied; an
empty `cwd` string is treated as absent so a degenerate payload falls
back rather than producing an empty path.

## agent_prompt_scan `.flow-states/` Carve-Out

Once the worktree gate resolves correctly from the payload cwd, the
parent-side Agent prompt scan (`validate_agent_prompt`) engages on
every Review sub-agent launch. The reviewer launch's prompt carries
the substantive-diff path under `<project_root>/.flow-states/<branch>/`,
which is outside the worktree. Without a carve-out, engaging the gate
would hard-block every Review sub-agent launch — trading the
native-prompt failure for a hard-block failure.

`validate_agent_prompt` therefore allows candidates that normalize
under this flow's own `<project_root>/.flow-states/<branch>/` subtree
— scoped to the current branch, NOT the whole `.flow-states/` root
that every concurrent flow shares, so a sub-agent prompt cannot be
pointed at another concurrent flow's per-branch state. project_root
and the branch are derived by reusing `compute_worktree_paths` on the
`worktree_root` (no disk access): it returns
`(project_root, <project_root>/.worktrees/<branch>)`, and the branch
is the tail after the `/.worktrees/` anchor (sliced, not via
`Path::file_name`, so the derivation is branchless and the 100%
coverage gate has no unreachable arm). Its rightmost-occurrence
`rfind` resolves a project_root that itself contains `.worktrees/`.
When `worktree_root` lacks the `/.worktrees/` anchor the derivation
yields `None` and the carve-out does not apply — the candidate stays
blocked.

## How to Apply

When authoring or modifying a PreToolUse hook:

1. Resolve cwd in the `run()` wrapper via `resolve_hook_cwd`, then
   thread the single resolved value to every cwd consumer so they
   cannot diverge.
2. Never call `env::current_dir()` directly for the gate cwd.
3. When the hook scans Agent prompts for out-of-worktree paths, allow
   the current flow's own `<project_root>/.flow-states/<branch>/`
   subtree so legitimate sub-agent launches (which carry the diff
   path there) are not hard-blocked — scoped to the branch, not the
   whole `.flow-states/` root.

## Cross-References

- `src/hooks/mod.rs` — `resolve_hook_cwd` and its doc comment.
- `src/hooks/agent_prompt_scan.rs` — the `.flow-states/` carve-out in
  `validate_agent_prompt`.
- `.claude/rules/cognitive-isolation.md` "Retry-prompt path-scoping
  constraint" — the retry-prompt discipline the carve-out interacts
  with: a `.flow-states/<branch>/` diff path need not be dropped from
  a retry prompt because the scan now allows it.
- `.claude/rules/hook-state-timing.md` — the sibling discipline for
  WHEN hooks read state fields; this rule covers WHICH directory the
  reads resolve against.
- `.claude/rules/config-source-mapping.md` — the sibling discipline
  for config-file-to-reader mappings; the hook payload `cwd` is a
  runtime input (not a config file), so its source mapping is recorded
  here rather than there.
- `.claude/rules/external-input-path-construction.md` — the byte-cap
  and validation discipline the resolved cwd flows into.
