# Concurrency Model

Architectural principles (core invariant, two state domains) are in
CLAUDE.md under "Local vs Shared State". This file is the developer
checklist for applying those principles when writing code.

## Before Writing Any Code

Ask: "What happens when two flows hit this at the same time?"

- **File paths** — must be scoped by branch or worktree. Never
  use a fixed path like `/tmp/flow-output` or a repo-root
  singleton. Use `.flow-states/<branch>/*` or worktree-local
  paths.
- **State mutations** — must be isolated to the current flow's
  state file. Never read or write another flow's state.
- **GitHub operations** — must be idempotent. Labels, PR
  updates, and issue comments may race with another flow.
  Design for last-write-wins or check-before-write.
- **Locks** — are only for serializing operations on shared
  resources (like `start.lock` for base-branch operations).
  Most operations should not need locks because they operate
  on branch-scoped resources.
- **Base branch (the integration branch the flow coordinates
  against — `main` for standard repos, `staging`/`develop`/etc.
  for non-main-trunk repos)** — is the only shared local
  resource. Any operation on the base branch (pull, commit,
  push) must be serialized via the start lock or avoided
  entirely.
- **Start-gate runs CI on the base branch under the start lock
  as a coordination surface**, not a sandboxable safety check.
  The first flow-start repairs dependency breakage once via
  `ci-fixer`; subsequent flows inherit the fix via the CI
  sentinel. Moving the CI check to a disposable worktree would
  force every concurrent flow to rediscover and independently
  repair the same breakage — O(N) work instead of O(1). Tools
  that write artifacts under the base branch's `target/` must
  stay coherent across the many source generations that
  long-lived target dir sees. See CLAUDE.md "Start-Gate CI on
  the Base Branch as Serialization Point" for the full
  architecture.

## Completed Flow State File Leftovers

Cleanup normally deletes `.flow-states/<branch>/state.json` at Complete.
If cleanup fails (kill signal, filesystem error), a state file may
survive with `phases.flow-complete.status == "complete"`. Functions
that scan `.flow-states/` for active flows (e.g. duplicate issue
detection) must skip state files where the flow-complete phase is
complete — these are orphans from finished flows, not active work.

## Lock Name Must Match Release Name

When acquiring a lock, the name used for acquisition must be the
same name used for release. In `start-init`, the canonical branch
name (derived from issue titles via `branch_name()`) must be
resolved BEFORE acquiring the lock, because `start-workspace`
releases the lock under the canonical branch name. If the lock is
acquired under a raw feature name but released under the canonical
name, a lock leak occurs — the orphan lock file blocks all
subsequent flows for 30 minutes until the stale timeout expires.

Pattern: resolve the canonical name first (issue fetch, label
guard, duplicate check), then `acquire(&canonical_name)`. All
error paths before the lock return without touching the lock queue.

## Editing Source on the Base Branch

Default: never edit source files directly on the base branch (the
integration branch the flow coordinates against). Every change
should go through the FLOW lifecycle on a feature branch. If a bug
blocks flow-start with issue references, start the flow without
issue references to get on a feature branch first, then fix the bug
there.

Exception: when the maintainer explicitly directs a fix on the base
branch in the current session — "do this on main", "fix it directly
on main" — edit on the base branch is permitted. The default
protects against drive-by edits the model rationalizes on its own;
explicit user direction is a different category.

The commit itself ALWAYS goes through `/flow:flow-commit`. The
exception unlocks where the diff lives, never how it lands.
Flow-commit runs CI and is never bypassed regardless of phrasing.

## Common Mistakes

- Assuming only one `.flow-states/*.json` file exists
- Using `git checkout` or `git switch` (changes HEAD for all
  worktrees sharing the same repo)
- Writing to a fixed temp file without branch scoping
- Reading base-branch state without holding the start lock
- Assuming a GitHub label or issue state hasn't changed since
  last check
- Acquiring a lock under one name and releasing under another
  (e.g., feature name vs canonical branch name)
