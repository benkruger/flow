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

Bootstrap exception: three FLOW skills land commits on the
integration branch by design — there is no feature branch to
relocate to for any of them:

- `/flow:flow-start` Step 2 lands a `ci-fixer` dependency-repair
  commit before the user's feature work begins.
- `/flow:flow-prime` Step 6 lands permission and stub-script setup
  that must reach the integration branch before any flow can start.
- `/flow-release` publishes a version-bump commit on the
  integration trunk; there is no feature branch where a release
  tag could live. The slash command is bare (no `flow:` prefix)
  because the skill is project-local at `.claude/skills/flow-release/`.

The bootstrap-skill carve-out in Layer 10 (see "Mechanical
Enforcement" below) sanctions all three windows specifically.

The bootstrap commits land on the integration branch through one of
two surfaces. `/flow:flow-start` and `/flow:flow-prime` route their
commits through `/flow:flow-commit` — the standard delegated commit
path with its diff review, commit-message review, and user-approval
choreography. `/flow-release` calls `bin/flow finalize-commit`
directly because the release skill composes its own explicit
"Release v<new_version>" commit message and uses its own internal
review window: Step 3 displays `git log <last_tag>..HEAD`, Step 4
drafts release notes against that list, and Step 7 writes the
explicit commit-message file. Each path's choreography substitutes
for the other; both run the CI gate inside `finalize-commit`.

Maintainer trunk carve-out: a maintainer who needs a non-bootstrap
commit on the integration branch (bootstrap repair, follow-up
after a hot patch, cleanup commit) types `/flow:flow-commit`
directly on the trunk branch. Layer 10's trunk carve-out (see
"Mechanical Enforcement" below) recognizes the user-typed slash
command and lets the invocation through; `/flow:flow-commit`'s
own diff review and commit-message review supply the choreography.
This path is NOT a hook-level bypass — it is the supported
on-trunk maintainer path, distinct from the rule-level "explicit
user direction" exception below in that the model cannot
synthesize the user-typed slash-command marker the carve-out
anchors on. The carve-out is structurally bounded to cwds that
are NOT inside an active-flow worktree, so a feature-branch
worktree's `/flow:flow-commit` invocation cannot leak to authorize
a trunk commit.

The exception above is rule-level. The hook described in
"Mechanical Enforcement" below is stricter: Layer 10 mechanically
blocks any `git ... commit` or `bin/flow ... finalize-commit`
invocation whose effective destination resolves either to the
integration branch OR to a feature branch with an active FLOW state
file, even when the maintainer has explicitly directed an on-main
or in-flow fix in the current session. A user direction that lifts
the rule-level default does NOT lift the hook-level gate — only
typing `/flow:flow-commit` on the integration branch lifts the
integration-branch arm (via the trunk carve-out below), and there
is no analogous user-direction lift for the active-flow arm. To
commit during an active flow, route through `/flow:flow-commit`
from inside the worktree. This intentional strictness keeps the
hook unambiguous: a single, mechanical answer for "is this commit
allowed?" rather than a context-sensitive predicate the model
could rationalize past.

### Mechanical Enforcement

The `validate-pretool` PreToolUse hook's Layer 10 mechanically
rejects direct commit invocations whose effective destination
resolves either to the integration branch named by
`default_branch_in` OR to a feature branch with an active FLOW
state file at `.flow-states/<branch>/state.json`. The hook checks
two pathways: `git ... commit` and `bin/flow ... finalize-commit`
(recognized by basename suffix so absolute paths like
`/Users/.../bin/flow finalize-commit` block the same way as bare
`bin/flow`). The matcher is robust to a curated set of bypasses:

- **Quoted command names** — `'git'` and `"git"` are dequoted
  before comparison, so the matcher cannot be defeated by a stray
  quote pair around the launcher.
- **`git -c key=value commit ...`** and **`git -C path commit ...`** —
  the matcher walks past these flag pairs to find the effective
  subcommand.
- **Shell-eval wrappers** (`bash -c '<inner>'`, `sh -c '<inner>'`,
  `zsh -c '<inner>'`, `eval '<inner>'`) — Layer 8 in `validate`
  (`.claude/rules/no-escape-hatches.md` Layer B) blocks every
  shell-eval shape BEFORE Layer 10 runs, regardless of the wrapped
  inner command. The wrapper itself is the escape hatch — Layer 10
  never needs to unwrap it.

### Branch-Arg Routing (finalize-commit Destination Path)

For `bin/flow finalize-commit <msg> <branch>` invocations, Layer 10
binds its checks to the explicit `<branch>` argument rather than
the caller's process cwd. The integration-branch check compares
the branch arg against `default_branch_in(<main_root>)` via
`match_finalize_commit_destination`; the active-flow check runs
at `<main_root>/.worktrees/<branch>/` so an active flow on that
worktree fires the gate regardless of where the caller's shell
sits — a sibling tempdir, a monorepo subdirectory of the
integration trunk, or another feature-branch worktree all see
the gate fire on the correct destination.

Both `finalize_commit::run_impl` and
`match_finalize_commit_destination` reach the route-to-root
decision through the shared
`crate::flow_paths::finalize_commit_destination` helper, so the
hook and the binary agree on the commit destination by
construction rather than by a maintained convention. That helper
normalizes the branch via `normalize_gate_input` per
`.claude/rules/security-gates.md` "Normalize Before Comparing",
so case- or whitespace-variant branch args (`MAIN`, `  main  `)
still match the integration-branch check. On the
`default_branch_in` error path the helper routes to the
per-branch worktree (never the project root) and
`match_finalize_commit_destination` returns no-block, so neither
side treats an undetectable-integration commit as a trunk
destination.

For every other shape — `git commit`, `git -C <path> commit`,
and any malformed `bin/flow finalize-commit` invocation (missing
positional args) — Layer 10 falls back to the cwd path that
checks the hook's process cwd and any `-C <path>` target. The
active-flow skill-commit carve-out and the integration-branch
bootstrap-skill carve-out apply identically across both dispatch
paths. The trunk carve-out is destination-path-only — the
cwd-path arm covers `git commit` shapes that carry no
slash-command marker for the gate to anchor on, so the trunk
carve-out has nothing to match there.

### Active-Flow Trigger

Layer 10 fires in two contexts. The integration-branch context
above defends against direct commits on the trunk. The
**active-flow context** defends against direct commits in any
feature-branch worktree that already has a FLOW lifecycle
running. The trigger is the existence of
`.flow-states/<branch>/state.json` at the resolved project root,
detected via the canonical `is_flow_active(branch, root)` helper
shared with every other flow-aware hook (`validate-ask-user`,
`validate-claude-paths`, `stop_continue`, etc.).

The active-flow context covers the same bypasses as the
integration-branch context and applies to every branch source
Layer 10 considers: the destination path's branch-arg-derived
worktree path (`<main_root>/.worktrees/<branch_arg>/`), the cwd
path's process cwd, and the cwd path's `-C <path>` target. When
both predicates fire on the same source, the integration-branch
message wins.

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
returns false and Layer 10 stays silent. The gate fires only
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
3. `transcript_shows_commit_window_skill(transcript_path, home)`
   returns true — the most recent assistant Skill tool_use call
   since the most recent user turn (resolved by
   `transcript_walker::most_recent_skill_since_user`) names
   `flow:flow-commit` or `flow-release`. In practice every
   active-flow commit names `flow:flow-commit` because the
   release path runs on the integration trunk, not under an
   active flow. The walker is the load-bearing predicate that
   proves the surrounding skill choreography (diff review,
   commit-message review) actually ran; the `_continue_pending`
   marker on its own is belt-and-suspenders for a fresh-session
   resume window. The transcript-walker check is the AND-combined
   condition per `.claude/rules/no-escape-hatches.md` Layer C,
   which closes the bypass-shortcut surface where a model could
   write the marker directly and invoke `bin/flow finalize-commit`
   without going through `/flow:flow-commit`.

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
The integration-branch gate would otherwise block the three
sanctioned skill commit windows that run while cwd is on the
integration branch by design: `/flow:flow-start` Step 2 lands
a `ci-fixer` dependency-repair commit before the user's feature
work begins; `/flow:flow-prime` Step 6 lands permission and
stub-script setup that must reach the integration branch before
any flow can start; and `/flow-release` publishes a
version-bump commit on the integration trunk and calls
`bin/flow finalize-commit` directly rather than delegating to
`/flow:flow-commit`. The carve-out passes the invocation
through iff ALL THREE conditions hold:

1. The command shape is `bin/flow ... finalize-commit`.
   Raw `git commit` is never carved out — `git -C ... commit`
   matches `is_commit_invocation` but not the finalize-commit-
   only predicate. The carve-out is finalize-commit-only by
   design.
2. The transcript shows a sanctioned commit-window skill,
   recognized through EITHER of two walker arms:
   - `last_user_message_invokes_skill(transcript_path,
     "flow-release", home)` returns true — the most recent
     user-role turn carries the
     `<command-name>/flow-release</command-name>` marker (or the
     two-line `<command-message>` shape). `flow-release` is a
     user-only skill recorded only as a user-role turn, never as
     an assistant `Skill` tool_use, so this is its production
     recognition path. The release skill calls
     `bin/flow finalize-commit` directly rather than delegating
     to `/flow:flow-commit`. Its trust comes from its own
     internal review window: Step 3 displays
     `git log <last_tag>..HEAD`, Step 4 drafts release notes
     against that list, and Step 7 writes an explicit
     "Release v<new_version>" commit-message file before
     `finalize-commit` reads it. The bare name (no `flow:`
     prefix) reflects the literal `input.skill` value Claude
     Code emits for the project-local skill at
     `.claude/skills/flow-release/`.
   - `transcript_shows_commit_window_skill(transcript_path,
     home)` returns true — the most recent assistant Skill
     tool_use call since the most recent user turn (resolved by
     `transcript_walker::most_recent_skill_since_user`) names
     `flow:flow-commit`, the delegated commit path used by
     `/flow:flow-start` and `/flow:flow-prime`. The trust is the
     standard `/flow:flow-commit` choreography: diff review,
     commit-message review, user approval.

   Each path's internal choreography substitutes for the other.
   The `/flow-release` user-turn arm is scoped to
   `bootstrap_carveout_applies` rather than placed inside the
   shared `transcript_shows_commit_window_skill` predicate:
   that predicate is also consumed by the active-flow carve-out
   (`check_active_flow_at`), and recognizing `/flow-release`
   there would widen the active-flow gate, which the
   integration-trunk-only `flow-release` skill must never touch.
3. A sanctioned bootstrap parent — `flow:flow-start`,
   `flow:flow-prime`, or `flow-release` — is recognized since
   the most recent real user turn, resolved by
   `transcript_walker::any_skill_in_set_since_user(transcript_path, home, BOOTSTRAP_SKILLS)`.
   The walker recognizes the parent either as an assistant
   `Skill` tool_use OR as the user-typed slash-command boundary
   turn itself — the latter is required because `flow:flow-prime`
   and `flow-release` are user-only skills Claude Code records
   only as user-role turns, never as assistant `Skill`
   tool_use. The sanctioned-parent set is the module-level
   `const BOOTSTRAP_SKILLS` in `validate_pretool.rs`; extending
   the set is a Plan-phase decision documented in a new flow.

The carve-out names no branch — `default_branch_in()` resolves
the actual integration branch from `git symbolic-ref --short
refs/remotes/origin/HEAD`, so the carve-out works identically
for repos on `staging`, `master`, `develop`, etc. When git
cannot resolve the integration branch, `default_branch_in`
returns an `Err` rather than guessing a default; the gate's
integration-branch arm cannot fire under that input and the
fall-through proceeds to the active-flow arm.

The carve-out is **cwd-only** for the cwd-path dispatch. The
destination-path dispatch applies the same carve-out to its
integration-branch arm. `check_commit_during_flow` does NOT
consult `bootstrap_carveout_applies` at the cwd path's `-C`
target's `match_branch_at(target)` callsite. The transcript
walker is session-scoped (the persisted transcript records the
model's session activity regardless of which repo the work
targets), so a bootstrap chain accrued in one repo's session
activity could otherwise authorize a commit redirected via
`git -C <other-repo>` to a different repo's integration branch.
All three legitimate bootstrap windows (flow-start Step 2,
flow-prime Step 6, and flow-release's commit step) run with
cwd ON the integration branch by design — none uses `-C` to
shift git's effective cwd — so restricting the carve-out away
from the `-C` callsite has no production consumer cost.
Restricting it preserves cross-repo safety: a
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
`/flow:flow-prime` (or `/flow:flow-start` or `/flow-release`)
completes and then directly invokes `/flow:flow-commit`, the
sanctioned-parent Skill call is OUTSIDE the carve-out window
and `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` returns
false. Historical authorization cannot carry forward past the
next user turn.

Threat-model bound: the carve-out's authorization window
remains open until the user's next prose message. Within that
window, a model that has the user-authorized invocation of
`/flow:flow-start`, `/flow:flow-prime`, or `/flow-release`
could in principle direct additional integration-branch commits
after the bootstrap skill's own commit lands. This is bounded
by: (a) the cwd-only scope above, which prevents cross-repo
redirection; (b) the CI gate inside `finalize-commit`, which
runs unconditionally regardless of the carve-out; and (c) the
FLOW threat model, which targets accidental commits (the
sanctioned parents have legitimate commit needs) rather than
adversarial model behavior (an actively-malicious model with
user-granted bootstrap authority is outside scope). Tightening
the window further — e.g., adding a per-machine
`bootstrap-pending` marker that flow-start/flow-prime/flow-release
set and finalize-commit clears — is a future design conversation,
not a defect in v1.

**Trunk carve-out (integration-branch context).** The
integration-branch gate would also block a maintainer typing
`/flow:flow-commit` directly on the trunk branch — a path the
project supports for non-bootstrap commits (bootstrap repair,
follow-up after a hot patch, a cleanup commit a maintainer wants
to land on the trunk without spinning up a feature worktree).
The trunk carve-out fires (suppresses the integration-branch
block) iff TWO AND-combined conditions hold, in addition to the
command-shape precondition the destination-path arm enforces via
`extract_finalize_commit_branch_arg`:

1. The caller's cwd is NOT inside an active-flow worktree.
   Resolved via `detect_branch_from_path(cwd)` +
   `is_flow_active(branch, main_root)`. The user typing
   `/flow:flow-commit` from inside a feature-branch worktree
   bound their intent to THAT worktree's branch — not to the
   integration trunk. Without this check, a model on a
   feature-branch worktree could fire `bin/flow finalize-commit
   msg.txt <trunk>` and the user-typed slash command would
   spuriously authorize a trunk commit (pre-mortem F1 bypass
   shape). The active-flow arm's own carve-out
   (`check_active_flow_at`) handles the legitimate feature-
   branch commit path via the `_continue_pending=commit` marker
   plus assistant-Skill `flow:flow-commit`; the trunk carve-out's
   cwd-not-active-flow check is the structural bound that
   prevents the user's feature-branch intent from leaking to
   authorize a trunk commit. The bound is symmetric with the
   bootstrap carve-out's cwd-only scope on the `-C` target
   callsite — both serve to keep trust localized to the cwd
   where the user-typed intent was genuinely formed.
2. `last_user_message_invokes_skill(transcript_path,
   "flow:flow-commit", home)` returns true — the most recent
   real user turn in the persisted transcript STARTS with the
   namespaced `<command-name>/flow:flow-commit</command-name>`
   slash-command emission (or the two-line `<command-message>`
   shape Claude Code 2.1.140+ emits). The user-typed slash command
   is the unforgeable trust anchor: only a user-typed turn can
   satisfy `last_user_message_invokes_skill`, so the model
   cannot synthesize the marker and route around
   `/flow:flow-commit`'s surrounding diff review and
   commit-message review.

Scoping: this carve-out is wired ONLY into the destination-path
integration-branch arm of `check_commit_during_flow`. The
active-flow arm is gated by `_continue_pending=commit` +
assistant-Skill `flow:flow-commit`; weakening that to accept a
user-typed slash command would let a maintainer skip the
feature-branch flow-commit choreography entirely. The cwd-path
arm covers `git commit` and `git -C <trunk> commit` shapes —
neither carries a slash-command marker for the gate to anchor on,
so the trunk carve-out has nothing to match there. Raw `git
commit` on the integration branch therefore remains blocked
unconditionally, even with a user-typed `/flow:flow-commit` turn
earlier in the transcript: the finalize-commit invocation shape
is itself the signal that the maintainer reached for
`/flow:flow-commit` deliberately.

The carve-out is branch-agnostic — like the bootstrap carve-out,
the user-typed slash command is the trust anchor, not any
particular branch name. The carve-out applies identically to
`main`, `staging`, `master`, `develop`, and any other configured
trunk.

Trust contract: CI verifies code quality, not commit discipline.
The reviewable choreography that Layer 10 protects is supplied
by the `/flow:flow-commit` skill itself (`skills/flow-commit/SKILL.md`):
diff review, commit-message review, user approval. The user's
slash-command invocation triggers exactly that choreography for
the on-trunk commit, identical to every feature-branch commit.
CI runs unconditionally inside `finalize_commit::run_impl`
regardless of the carve-out — that's the protective backstop for
code quality. The skill's choreography is the protective frontstop
for commit discipline. The carve-out preserves both because the
same `/flow:flow-commit` skill that fires for feature-branch
commits ALSO fires for the trunk commit when the cwd-not-active-
flow check passes.

Window bound: the carve-out's authorization window stays open
from the user-typed `/flow:flow-commit` turn until the next real
user turn — the same documented bound the bootstrap carve-out
carries. A user message after `/flow:flow-commit` completes —
followed by a separate `bin/flow finalize-commit msg.txt
<trunk>` invocation — puts the slash-command turn OUTSIDE the
carve-out window, so `last_user_message_invokes_skill` returns
false and the block fires.

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

Shell-eval wrappers (`bash -c`, `sh -c`, `zsh -c`, `eval`),
command-construction launchers (`xargs git commit`,
`node finalize-commit`), and inter-process injection
(`tmux send-keys`, `screen -X`) are blocked structurally by
Layer 8 BEFORE Layer 10 runs, so the wrapped invocations never
reach the commit-invocation matcher. See
`.claude/rules/no-escape-hatches.md` for the canonical
program/flag table.

These limitations are documented v1 boundaries, not security
holes. The default-no-edit-on-the-base-branch discipline
above remains the primary instrument; Layer 10 is the
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
