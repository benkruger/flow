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
- **Base branch** (the integration branch the flow coordinates
  against — `main` for standard repos, `staging`/`develop`/etc.
  for non-main-trunk repos) is the only shared local resource.
  Any operation on the base branch (pull, commit, push) must be
  serialized via the start lock or avoided entirely.
- **Start-gate runs CI on the base branch under the start lock
  as a coordination surface**, not a sandboxable safety check.
  The first flow-start repairs dependency breakage once via
  `ci-fixer`; subsequent flows inherit the fix via the CI
  sentinel. Moving the CI check to a disposable worktree would
  force every concurrent flow to rediscover and independently
  repair the same breakage — O(N) work instead of O(1). See
  CLAUDE.md "Start-Gate CI on the Base Branch as Serialization
  Point" for the full architecture.

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

Bootstrap exception: `/flow:flow-start` Step 2 lands a `ci-fixer`
dependency-repair commit, and `/flow:flow-prime` Step 6 lands
permission and stub-script setup. Both run while cwd is on the
integration branch by design — there is no feature branch to
relocate to during bootstrap. The bootstrap-skill carve-out in
Layer 9 (see "Mechanical Enforcement" below) sanctions these two
windows specifically.

The commit itself ALWAYS goes through `/flow:flow-commit`. The
exception unlocks where the diff lives, never how it lands.
Flow-commit runs CI and is never bypassed regardless of phrasing.

The exception above is rule-level. The hook described in
"Mechanical Enforcement" below is stricter: Layer 9 mechanically
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

The `validate-pretool` PreToolUse hook's Layer 9 mechanically
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
  subcommand.
- **Shell-eval wrappers** (`bash -c '<inner>'`, `sh -c '<inner>'`,
  `zsh -c '<inner>'`, `eval '<inner>'`) — Layer 7.5 in `validate`
  (`.claude/rules/no-escape-hatches.md` Layer B) blocks every
  shell-eval shape BEFORE Layer 9 runs, regardless of the wrapped
  inner command. The wrapper itself is the escape hatch — Layer 9
  never needs to unwrap it.

### Active-Flow Trigger

Layer 9 fires in two contexts. The integration-branch context
above defends against direct commits on the trunk. The
**active-flow context** defends against direct commits in any
feature-branch worktree that already has a FLOW lifecycle
running. The trigger is the existence of
`.flow-states/<branch>/state.json` at the resolved project root,
detected via the canonical `is_flow_active(branch, root)` helper
shared with every other flow-aware hook (`validate-ask-user`,
`validate-claude-paths`, `stop_continue`, etc.).

The active-flow context covers the same bypasses as the
integration-branch context and applies to both candidate cwds
(process cwd and any `-C` target). When both predicates fire on
the same candidate, the integration-branch message wins.

User-direction interaction mirrors the integration-branch
posture: an explicit user direction in the current session does
NOT lift the active-flow gate. The way to commit during an
active flow is `/flow:flow-commit`, which routes through
`bin/flow finalize-commit` from inside the skill — that path
runs CI before `git commit` and is the only sanctioned commit
surface during a flow.

The pre-flow editing scenario remains unblocked: if no state
file exists at `.flow-states/<branch>/state.json` (the user
hasn't run `/flow:flow-start` yet), the active-flow predicate
returns false and Layer 9 stays silent. The gate fires only
once a flow is genuinely active.

**Skill-commit carve-out (active-flow context).** The active-flow
gate would otherwise block the legitimate skill path itself,
because `/flow:flow-commit` invokes `bin/flow finalize-commit`
via the Bash tool. The carve-out passes the invocation through
iff ALL THREE conditions hold for the candidate cwd:

1. The command shape is `bin/flow ... finalize-commit` (NOT
   `git commit`). Raw `git commit` is never legitimate during a
   flow even with the marker present.
2. The state file at `.flow-states/<branch>/state.json` has
   `_continue_pending == "commit"`. The flow-code, flow-code-
   review, and flow-learn skills all set this field via
   `bin/flow set-timestamp` immediately before invoking
   `/flow:flow-commit`, and `phase_enter()` clears it on phase
   advance.
3. The most recent assistant Skill tool_use call since the most
   recent user turn — resolved by
   `transcript_walker::most_recent_skill_since_user(transcript_path, home)`
   — is `flow:flow-commit`. The walker is the load-bearing
   predicate that proves the surrounding skill choreography
   (diff review, commit-message review) actually ran; the
   `_continue_pending` marker on its own is belt-and-suspenders
   for a fresh-session resume window. The transcript-walker
   check is the AND-combined condition per
   `.claude/rules/no-escape-hatches.md` Layer C, which closes
   the bypass-shortcut surface where a model could write the
   marker directly and invoke `bin/flow finalize-commit` without
   going through `/flow:flow-commit`.

Trust contract: the `_continue_pending` field is writable by
the model (the same `bin/flow set-timestamp` call that the
skills use is reachable from any Bash invocation). Without the
transcript-walker condition, a model that deliberately bypassed
`/flow:flow-commit` could write the marker, then call
`bin/flow finalize-commit` directly, skipping the skill's diff
review and commit-message review. The walker check closes that
bypass: only the genuine `/flow:flow-commit` skill invocation
produces an assistant Skill tool_use with `skill ==
"flow:flow-commit"` since the most recent user turn. The hook
preserves the CI invariant — `finalize-commit` runs
`ci::run_impl()` before `git commit` regardless — AND the
surrounding choreography is now upheld by the hook, not by rule
discipline alone.

**Bootstrap-skill carve-out (integration-branch context).**
The integration-branch gate would otherwise block the two
sanctioned skill commit windows that run while cwd is on the
integration branch by design: `/flow:flow-start` Step 2 lands
a `ci-fixer` dependency-repair commit before the user's feature
work begins, and `/flow:flow-prime` Step 6 lands permission and
stub-script setup that must reach `origin/main` (or the
configured integration branch) before any flow can start. The
carve-out passes the invocation through iff ALL THREE conditions
hold:

1. The command shape is `bin/flow ... finalize-commit`.
   Raw `git commit` is never carved out — `git -C ... commit`
   matches `is_commit_invocation` but not the finalize-commit-
   only predicate. The carve-out is finalize-commit-only by
   design.
2. The most recent assistant Skill tool_use call since the most
   recent user turn — resolved by
   `transcript_walker::most_recent_skill_since_user(transcript_path, home)`
   — is `flow:flow-commit`. Same predicate the active-flow
   carve-out uses; same proof that `/flow:flow-commit` is the
   surrounding skill.
3. A sanctioned bootstrap parent — `flow:flow-start` or
   `flow:flow-prime` — appears in the assistant Skill chain
   since the most recent real user turn, resolved by
   `transcript_walker::any_skill_in_set_since_user(transcript_path, home, BOOTSTRAP_SKILLS)`.
   The sanctioned-parent set is the module-level `const
   BOOTSTRAP_SKILLS` in `validate_pretool.rs`; extending the
   set is a Plan-phase decision documented in a new flow.

The carve-out names no branch — `default_branch_in()` resolves
the actual integration branch from `git symbolic-ref --short
refs/remotes/origin/HEAD` (fallback `"main"`), so the carve-out
works identically for repos on `staging`, `master`,
`develop`, etc.

The carve-out is **cwd-only**. `check_commit_during_flow` does
NOT consult `bootstrap_carveout_applies` at the `-C` target's
`match_branch_at(target)` callsite. The transcript walker is
session-scoped (the persisted transcript records the model's
session activity regardless of which repo the work targets), so
a bootstrap chain accrued in one repo's session activity could
otherwise authorize a commit redirected via
`git -C <other-repo>` to a different repo's integration branch.
Both legitimate bootstrap windows (flow-start Step 2 and
flow-prime Step 6) run with cwd ON the integration branch by
design — neither uses `-C` to shift git's effective cwd — so
restricting the carve-out to the cwd callsite has no production
consumer cost. Restricting it preserves cross-repo safety: a
`git -C <integration-branch-target> commit` from any cwd
remains blocked at the `-C` target's `match_branch_at` even
when the session's transcript carries a valid bootstrap chain
for an unrelated repo.

Trust-contract substitution: the active-flow context uses
(command shape + state-file marker + walker) — marker is
belt-and-suspenders, walker is load-bearing. The integration-
branch context has no per-branch state file at the integration
trunk, so the bootstrap carve-out uses (command shape + walker +
walker) — both walker conditions are load-bearing. The
choreography is verified entirely from the persisted transcript
because there is no analogous marker to write.

Window closure: the walker stops at the most recent real user
turn going backward. If the user types another message after
`/flow:flow-prime` completes and then directly invokes
`/flow:flow-commit`, the sanctioned-parent Skill call is OUTSIDE
the carve-out window and
`any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` returns false.
Historical authorization cannot carry forward past the next
user turn.

Threat-model bound: the carve-out's authorization window
remains open until the user's next prose message. Within that
window, a model that has the user-authorized invocation of
`/flow:flow-start` or `/flow:flow-prime` could in principle
direct additional integration-branch commits after the
bootstrap skill's own commit lands. This is bounded by:
(a) the cwd-only scope above, which prevents cross-repo
redirection; (b) the CI gate inside `finalize-commit`, which
runs unconditionally regardless of the carve-out; and (c) the
FLOW threat model, which targets accidental commits (the
sanctioned parents have legitimate commit needs) rather than
adversarial model behavior (an actively-malicious model with
user-granted bootstrap authority is outside scope). Tightening
the window further — e.g., adding a per-machine
`bootstrap-pending` marker that flow-start/flow-prime set and
finalize-commit clears — is a future design conversation, not a
defect in v1.

### Known Limitations

The current matcher does not defend against the following shapes.
Each is captured by an explicit test (or, where the test would be
contrived, by the absence of a matching shape in normal session
flow) so future widening of the matcher is a deliberate decision
rather than an accident:

- **Env-var indirection.** `GIT_DIR=/path git commit` and
  `GIT_WORK_TREE=...` redirect git's view of the repo via env
  vars rather than CLI flags.
- **User-defined git aliases.** `git ci -m x` (with
  `alias.ci = commit` configured) shows `ci` to the matcher, not
  `commit`.
- **Repos with no configured `origin/HEAD`.** `default_branch_in`
  falls back to `"main"` when `git symbolic-ref --short
  refs/remotes/origin/HEAD` fails.

Shell-eval wrappers (`bash -c`, `sh -c`, `zsh -c`, `eval`),
command-construction launchers (`xargs git commit`,
`node finalize-commit`), and inter-process injection
(`tmux send-keys`, `screen -X`) are blocked structurally by
Layer 7.5 BEFORE Layer 9 runs, so the wrapped invocations never
reach the commit-invocation matcher. See
`.claude/rules/no-escape-hatches.md` for the canonical
program/flag table.

These limitations are documented v1 boundaries, not security
holes. The default-no-edit-on-the-base-branch discipline
above remains the primary instrument; Layer 9 is the
merge-conflict trip-wire for the shapes Claude is most likely to
produce by accident.

## Common Mistakes

- Assuming only one `.flow-states/*.json` file exists
- Using `git checkout` or `git switch` (changes HEAD for all
  worktrees sharing the same repo)
- Writing to a fixed temp file without branch scoping
- Reading base-branch state without holding the start lock
- Assuming a GitHub label or issue state hasn't changed since
  last check
- Acquiring a lock under one name and releasing under another
