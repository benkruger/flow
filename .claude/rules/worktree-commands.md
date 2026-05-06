# Worktree Commands

## Use `git -C` instead of `cd && git`

Never use `cd <path> && git <command>`. Claude Code's built-in "bare
repository attacks" heuristic fires on any `cd <path> && git` compound
command, regardless of the allow list in settings.json.

Use `git -C <path> <command>` instead — it runs git in the target
directory without changing the shell's working directory, and it matches
a single permission pattern (`Bash(git -C *)`).

## File tool paths must use the worktree

When working in a worktree (pwd contains `.worktrees/`), ALL file
tool paths (Edit, Read, Write, Grep, Glob) for repo-tracked files
must use the worktree absolute path from `pwd`, not the main repo
path. The worktree has its own copy of every tracked file. Editing
the main repo's copy does not affect the worktree.

Before every Edit or Write call, verify the path starts with the
current working directory (from `pwd`), not the project root from
`git worktree list`.

Shared paths that live OUTSIDE the worktree are fine to access
directly: `.flow-states/`, `~/.claude/`, plugin cache paths.

### Mechanical enforcement at the subprocess layer

The Edit/Write tool surface is gated by the `validate-claude-paths`
PreToolUse hook, which redirects `CLAUDE.md`, `.claude/rules/*`, and
`.claude/skills/*` writes to `bin/flow write-rule` during an active
flow. Without an additional check at the subprocess layer, a model
could call `bin/flow write-rule --path <main_repo>/.claude/rules/foo.md
--content-file <temp>` and bypass the hook entirely, writing to the
main-repo copy of a rule that the worktree should own.

`src/write_rule.rs::run_impl_main` closes that hole. After the
managed-artifact canonicalization gate, a worktree-path guard
classifies the `--path` basename via
`crate::protected_paths::is_protected_path`. When the basename is
protected AND a flow is active for the current branch (state file at
`<main_root>/.flow-states/<branch>/state.json` exists), the gate
rejects any path that doesn't normalize to a descendant of
`<main_root>/.worktrees/<branch>/`. Rejection returns
`{"status":"error","step":"worktree_path_validation"}` on stdout and
exits 1, with the canonical worktree destination named in the
response so the caller can retry against the right path.

The gate runs BEFORE `read_content_file` so a rejection does not
destroy the caller's input — the model can re-issue the call against
the correct worktree path with the same content file. Pass-through
behavior is preserved for non-protected basenames and for
invocations without an active flow (prime-time, one-off rule edits
on main).

See `src/write_rule.rs::worktree_path_guard` for the implementation
and `tests/write_rule.rs` for the protected-path matrix
(main-repo-rejected vs. worktree-passes vs. no-active-flow vs.
non-protected-passes).

## Never invoke cargo directly

Never run `cargo test`, `cargo build`, or any `cargo` subcommand
directly via the Bash tool. Use `bin/flow ci --test -- <filter>` for
targeted test execution and `bin/flow ci` for full builds. All
build tool access routes through `bin/flow ci` (with optional
`--format`/`--lint`/`--build`/`--test` flags for single-phase runs).

Direct cargo invocations bypass the permission whitelist, which causes
RTK to intercept the command and prompt the user. This is especially
dangerous inside sub-agents, where the prompt appears in an unexpected
context and the sub-agent cannot handle it.
