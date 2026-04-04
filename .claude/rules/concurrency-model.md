# Concurrency Model

Architectural principles (core invariant, two state domains) are in
CLAUDE.md under "Local vs Shared State". This file is the developer
checklist for applying those principles when writing code.

## Before Writing Any Code

Ask: "What happens when two flows hit this at the same time?"

- **File paths** — must be scoped by branch or worktree. Never
  use a fixed path like `/tmp/flow-output` or a repo-root
  singleton. Use `.flow-states/<branch>-*` or worktree-local
  paths.
- **State mutations** — must be isolated to the current flow's
  state file. Never read or write another flow's state.
- **GitHub operations** — must be idempotent. Labels, PR
  updates, and issue comments may race with another flow.
  Design for last-write-wins or check-before-write.
- **Locks** — are only for serializing operations on shared
  resources (like `start.lock` for main-branch operations).
  Most operations should not need locks because they operate
  on branch-scoped resources.
- **Main branch** — is the only shared local resource. Any
  operation on main (pull, commit, push) must be serialized
  via the start lock or avoided entirely.

## Never Edit Source on Main

Never edit source files directly on main. Every change — including
critical bug fixes that block the current workflow — must go through
the FLOW lifecycle on a branch. If a bug blocks flow-start with
issue references, start the flow without issue references to get on
a branch first, then fix the bug there.

## Common Mistakes

- Assuming only one `.flow-states/*.json` file exists
- Using `git checkout` or `git switch` (changes HEAD for all
  worktrees sharing the same repo)
- Writing to a fixed temp file without branch scoping
- Reading main branch state without holding the start lock
- Assuming a GitHub label or issue state hasn't changed since
  last check
