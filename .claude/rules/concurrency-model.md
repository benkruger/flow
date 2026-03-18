# Concurrency Model

## The Core Invariant

FLOW's primary use case is N engineers running N flows on N boxes
simultaneously. This is not an edge case — it is the default
operating mode. Every feature must work under these conditions:

- Multiple worktrees active on the same machine
- Multiple engineers working the same repo from different machines
- Multiple flows touching overlapping issues or files

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

## The Two State Domains

| Domain | Scope | Examples | Coordination |
|--------|-------|----------|--------------|
| Local | Per-machine | `.flow-states/`, worktrees, `.flow.json` | None needed — each machine has its own |
| Shared | All engineers | PRs, issues, labels, branches | GitHub is the API — never assume local knowledge of other engineers' state |

## Common Mistakes

- Assuming only one `.flow-states/*.json` file exists
- Using `git checkout` or `git switch` (changes HEAD for all
  worktrees sharing the same repo)
- Writing to a fixed temp file without branch scoping
- Reading main branch state without holding the start lock
- Assuming a GitHub label or issue state hasn't changed since
  last check
