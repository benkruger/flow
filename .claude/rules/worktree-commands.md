# Worktree Commands

## Use `git -C` instead of `cd && git`

Never use `cd <path> && git <command>`. Claude Code's built-in "bare
repository attacks" heuristic fires on any `cd <path> && git` compound
command, regardless of the allow list in settings.json.

Use `git -C <path> <command>` instead — it runs git in the target
directory without changing the shell's working directory, and it matches
a single permission pattern (`Bash(git -C *)`).

## Use dedicated tools instead of Bash for reads

Never use `grep`, `cat`, `head`, `tail`, or `find` via the Bash tool.
Use the Grep tool for content search, the Read tool for file contents,
and the Glob tool for file discovery. Dedicated tools never trigger
permission prompts.

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

## Never invoke Python directly

Never run `python3` or `.venv/bin/python3` via the Bash tool.
Use `bin/flow`, `bin/ci`, or `bin/test` — they handle the venv
automatically and match existing permission patterns.

This ban includes `python3 -m ruff`, `python3 -m pytest`, and any
other `python3 -m <tool>` invocation — all are python3 direct calls.
When a linter or formatter needs to be invoked outside `bin/ci`, use
the standalone venv binary directly (e.g. `.venv/bin/ruff`), not
`python3 -m <tool>`.

## Never invoke cargo directly

Never run `cargo test`, `cargo build`, or any `cargo` subcommand
directly via the Bash tool. Use `bin/test --rust <filter>` for Rust
test execution and `bin/ci` for full builds. These wrappers match
existing permission patterns in `.claude/settings.json`.

Direct cargo invocations bypass the permission whitelist, which causes
RTK to intercept the command and prompt the user. This is especially
dangerous inside sub-agents, where the prompt appears in an unexpected
context and the sub-agent cannot handle it.

## Use `ruff --fix` when ruff reports a fixable error

When `bin/flow ci` fails with a ruff lint error marked `[*]` (fixable),
run `.venv/bin/ruff check --fix <file>` directly rather than bisecting
import orderings, whitespace, or formatting by hand. Ruff's fixer
knows the canonical form; manual trial-and-error wastes time and
rarely produces the exact output ruff expects.

The same applies to format errors: when `bin/flow ci` reports
"Would reformat: <file>", run `.venv/bin/ruff format <file>` directly.

The `.venv/bin/ruff` binary is a standalone executable, not a python3
invocation — it does not violate the "Never invoke Python directly"
rule above. The rule bans `.venv/bin/python3` (and `python3 -m ruff`,
which is the same thing); the standalone `.venv/bin/ruff` binary is
permitted when no `bin/flow` wrapper covers the operation.
