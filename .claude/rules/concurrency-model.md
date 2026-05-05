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

The exception above is rule-level. The hook described in
"Mechanical Enforcement" below is stricter: Layer 10 mechanically
blocks any `git ... commit` or `bin/flow ... finalize-commit`
invocation whose effective cwd resolves either to the integration
branch OR to a feature branch with an active FLOW state file,
even when the maintainer has explicitly directed an on-main or
in-flow fix in the current session. A user direction that lifts
the rule-level default does NOT lift the hook-level gate. To
commit a maintainer carve-out fix, work on a feature branch and
merge through the standard PR path; to commit during an active
flow, route through `/flow:flow-commit`. This intentional
strictness keeps the hook unambiguous: a single, mechanical
answer for "is this commit allowed?" rather than a
context-sensitive predicate the model could rationalize past.

### Mechanical Enforcement

The `validate-pretool` PreToolUse hook's Layer 10 mechanically
rejects direct commit invocations whose effective cwd resolves
either to the integration branch named by `default_branch_in` OR
to a feature branch with an active FLOW state file at
`.flow-states/<branch>/state.json`. The hook checks two pathways:
`git ... commit` and `bin/flow ... finalize-commit` (recognized
by basename suffix so absolute paths like
`/Users/.../bin/flow finalize-commit` block the same way as bare
`bin/flow`). The matcher is robust to a curated set of bypasses:

- **Quoted command names** — `'git'` and `"git"` are dequoted
  before comparison, so the matcher cannot be defeated by a stray
  quote pair around the launcher.
- **`git -c key=value commit ...`** and **`git -C path commit ...`** —
  the matcher walks past these flag pairs to find the effective
  subcommand, so config overrides and explicit-cwd flags do not
  hide the `commit`.
- **`bash -c '<inner>'` and `sh -c '<inner>'`** — one level of
  shell wrapping is unwrapped, and the inner script is
  re-evaluated through the same matcher.
- **`git -C <other_repo> commit ...`** — branch resolution reads
  from BOTH the hook's process cwd AND the `-C` argument's path,
  and Layer 10 blocks if EITHER resolves to its own integration
  branch. So redirecting git's effective cwd onto a different
  repo on `main` does not bypass the gate when the hook is
  running from a feature-branch worktree.
- **`bin/flow <flag> finalize-commit`** — the `bin/flow` arm
  matches `finalize-commit` as any subsequent token, not just
  the immediate next one, so a future global flag (e.g.
  `--verbose`, `--log-level <value>`) cannot slip the
  subcommand past the matcher.

### Active-Flow Trigger

Layer 10 fires in two contexts. The integration-branch context
above defends against direct commits on the trunk. The
**active-flow context** defends against direct commits in any
feature-branch worktree that already has a FLOW lifecycle
running. The trigger is the existence of
`.flow-states/<branch>/state.json` at the resolved project root,
detected via the canonical `is_flow_active(branch, root)` helper
shared with every other flow-aware hook (`validate-ask-user`,
`validate-claude-paths`, `stop_continue`, etc.) — no parallel
detection logic exists.

The active-flow context covers the same bypasses as the
integration-branch context (quoted command names, `git -c k=v`,
`git -C path`, `bash -c`/`sh -c`, `bin/flow <flag>
finalize-commit`) and applies to both candidate cwds (process
cwd and any `-C` target). When both predicates fire on the same
candidate (the rare case of an active flow on the integration
branch itself), the integration-branch message wins.

User-direction interaction mirrors the integration-branch
posture: an explicit user direction in the current session does
NOT lift the active-flow gate. The way to commit during an
active flow is `/flow:flow-commit`, which routes through
`bin/flow finalize-commit` from inside the skill — that path
runs CI before `git commit` and is the only sanctioned commit
surface during a flow. The active-flow gate exists because the
rule "always commit through `/flow:flow-commit`" was previously
mechanical only on the integration branch; PR #1322 surfaced
that feature-branch commits could land without CI, and the
gate's expansion closes the open backdoor.

The pre-flow editing scenario remains unblocked: if no state
file exists at `.flow-states/<branch>/state.json` (the user
hasn't run `/flow:flow-start` yet), the active-flow predicate
returns false and Layer 10 stays silent. The gate fires only
once a flow is genuinely active.

**Skill-commit carve-out.** The active-flow gate would otherwise
block the legitimate skill path itself, because
`/flow:flow-commit` invokes `bin/flow finalize-commit` via the
Bash tool and `is_commit_invocation` matches both `git commit`
and `bin/flow ... finalize-commit`. The carve-out passes the
invocation through iff BOTH conditions hold for the candidate
cwd:

1. The command shape is `bin/flow ... finalize-commit` (NOT
   `git commit`). Raw `git commit` is never legitimate during a
   flow even with the marker present — the skill never invokes
   bare git commit, so the marker plus a `git commit` command is
   always a bypass attempt.
2. The state file at `.flow-states/<branch>/state.json` has
   `_continue_pending == "commit"`. The flow-code, flow-code-
   review, and flow-learn skills all set this field via
   `bin/flow set-timestamp` immediately before invoking
   `/flow:flow-commit`, and `phase_enter()` clears it on phase
   advance — so the marker is `"commit"` only during the skill-
   driven commit window.

The integration-branch context is NOT carved out — commits on
the integration branch are blocked regardless of the marker.

Trust contract: the `_continue_pending` field is writable by
the model (the same `bin/flow set-timestamp` call that the
skills use is reachable from any Bash invocation). A model that
deliberately bypasses `/flow:flow-commit` could write the
marker, then call `bin/flow finalize-commit` directly, skipping
the skill's diff review and commit-message review. The hook
preserves the CI invariant — `finalize-commit` runs
`ci::run_impl()` before `git commit` regardless — but the
surrounding choreography is upheld by the rule discipline, not
by the hook. A stronger one-shot-token gate (token written by
`/flow:flow-commit`'s preflight, validated and consumed by
`finalize-commit`, checked by Layer 10) is the next iteration
if the marker-only design proves insufficient in practice.

### Known Limitations in v1

The current matcher does not defend against the following shapes.
Each is captured by an explicit test (or, where the test would be
contrived, by the absence of a matching shape in normal session
flow) so future widening of the matcher is a deliberate decision
rather than an accident:

- **Env-var indirection.** `GIT_DIR=/path git commit` and
  `GIT_WORK_TREE=...` redirect git's view of the repo via env
  vars rather than CLI flags. Env vars are not visible to the
  matcher in the simple form.
- **User-defined git aliases.** `git ci -m x` (with
  `alias.ci = commit` configured) shows `ci` to the matcher, not
  `commit`. Git resolves aliases internally after the hook fires.
- **Command-construction launchers.** `xargs git commit`,
  `node finalize-commit`, and similar shapes hide the commit
  invocation behind another binary. The matcher only fires on
  recognized first tokens (`git`, `bin/flow`, `bash`, `sh`).
- **Nested shell wrappers.** `bash -c 'bash -c "..."'` is
  unwrapped at most one level — a deeper nesting falls through.
- **Bash with flags before `-c`.** `bash --norc -c '...'` does
  not match the literal `bash -c ` prefix the unwrapper looks
  for, so the inner script is not re-evaluated.
- **Repos with no configured `origin/HEAD`.** `default_branch_in`
  falls back to `"main"` when `git symbolic-ref --short
  refs/remotes/origin/HEAD` fails. A user committing on a
  branch literally named `main` in a remote-less repository
  will be blocked — a documented false-positive that the
  remote-aware path covers correctly.

These limitations are not security holes — they are documented
v1 boundaries. The default-no-edit-on-the-base-branch discipline
above remains the primary instrument; Layer 10 is the
merge-conflict trip-wire for the four shapes Claude is most
likely to produce by accident.

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
