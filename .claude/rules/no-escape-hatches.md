# No Escape Hatches

Use sanctioned tools; never route around them. The model's action
surface is a small, curated set: the Read, Edit, Write, Glob, and
Grep tools, plus the Bash allow-list. Any construct that hides the
actual command from that surface — shell string-eval, interpreter
one-liners, command-construction wrappers, network bridges, or
marker-only carve-outs — is forbidden, even when the underlying
operation is itself legitimate.

## The Principle

The permission model and the dedicated tools exist so every action
is reviewable, gated, and recoverable. An escape hatch sidesteps
that model by:

- Wrapping a command in an interpreter that evaluates a string
  (`bash -c '<cmd>'`, `python -c '<cmd>'`, `eval '<cmd>'`).
- Routing commands through a launcher that obscures the effective
  program (`xargs <cmd>`, `rtk proxy <cmd>`, `env <cmd>`,
  `time <cmd>`).
- Reaching across the network or to another session
  (`nc`, `ssh`, `tmux send-keys`, `screen -X`).
- Relying on a state-file marker to authorize a sensitive action
  without the surrounding skill choreography (raw
  `bin/flow finalize-commit` invocations during an active flow).

In every case the right move is to identify the underlying task
(read a file, write a file, run an allow-listed program, commit a
change) and reach for the sanctioned tool that performs it. The
permission model already covers every legitimate operation; an
escape hatch is the model's surface being routed around, not a
gap in the surface itself.

## Canonical Escape-Hatch Shapes

The following program/flag combinations are escape hatches. Each
row names the construct, what it routes around, and the sanctioned
tool that performs the same task.

| Category | Shape | Sanctioned alternative |
|---|---|---|
| Shell-eval | `bash -c '<cmd>'`, `sh -c '<cmd>'`, `zsh -c '<cmd>'`, `eval '<cmd>'` | Separate Bash tool calls per command; the Bash tool already accepts allow-listed programs directly. |
| Interpreter-eval | `perl -e/-E '<code>'`, `python -c '<code>'`, `python3 -c '<code>'`, `ruby -e '<code>'`, `node -e/-p '<code>'`, `osascript -e '<code>'`, `tclsh -c '<code>'`, `lua -e '<code>'` | Read tool to view files; Write tool to create files; a proper script file plus the project's `bin/*` runners when computation is needed. |
| Command-wrapper | `xargs <cmd>`, `rtk proxy <cmd>` | Issue separate Bash calls per argument; invoke the underlying command directly through the sanctioned allow list. |
| Wrapper-launcher | `env <cmd>`, `time <cmd>`, `nice <cmd>`, `nohup <cmd>`, `taskset <cmd>`, `ionice <cmd>` | These wrap an inner program and obscure the effective basename. Always invoke the inner program directly; the structural layer strips the wrapper so the inner shape is caught. |
| Network-bridge | `nc <host> <port>`, direct `ssh <host>` | Use the dedicated network tool surface; use the approved ssh wrapper script when remote access is genuinely required. |
| Inter-process | `tmux send-keys ...` (with any global flags such as `-L socket`, `-S path`, `-f config`), `screen -X ...` | Use direct Bash invocations, not multiplexer key injection. The session running the action is the session that should run it. |

Indirect forms route around glob-based deny patterns and are
treated the same as the direct forms. Examples:

- Absolute path prefixes — `/usr/bin/bash -c '...'`,
  `/bin/sh -c '...'`.
- Environment-variable prefixes — `FOO=bar bash -c '...'`.
- Long flags before the trigger — `bash --norc -c '...'`,
  `bash --login -c '...'`.
- Combined short-flag tokens — `bash -lc '...'`, `bash -ic '...'`,
  `bash -xc '...'`, `node -ep '...'`. The structural check
  iterates short-flag characters within each `-<chars>` token, so
  any token containing the trigger character matches.
- Wrapper launchers — `env bash -c '...'`, `time bash -c '...'`,
  `nice python -c '...'`, `/usr/bin/env bash -c '...'`. The
  structural layer strips one or more wrapper tokens before
  checking the basename.

The structural escape-hatch layer in `validate-pretool` strips
env-var prefixes (`KEY=VAL `), wrapper launchers (`env`, `time`,
`nice`, `nohup`, `taskset`, `ionice`), and the path prefix;
basenames the first token; and matches against the program set
with trigger-flag awareness using `has_flag_char` (per-character
scan of short-flag tokens). Legitimate uses that pass an escape-
hatch program WITHOUT a string-eval trigger (`bash -n script.sh`
for syntax checking, `ssh-keygen` because the basename is
`ssh-keygen` rather than `ssh`, `tmux ls`, `screen -ls`,
`rtk discover`) remain allowed.

### Known v1 Boundaries

The structural layer covers the canonical shapes above. The
following bypass shapes are documented v1 boundaries — they slip
past the current implementation and a future tightening is a
deliberate decision rather than an accident:

- **`awk` with `system()`** — `awk` is in `UNIVERSAL_ALLOW` for
  routine text processing; a script containing `system("cmd")` is
  a shell-eval shape but blocking `awk` entirely would break
  every legitimate awk one-liner. A smarter content-aware check
  could be added but carries high false-positive risk.
- **`env` with flag arguments** — `env -u VAR bash -c '...'`
  passes the `-u VAR` tokens through `strip_wrapper_launchers`
  without consuming them. The next basename check sees `-u` as
  the first token (not in the program set) and returns None.
- **Recursive wrapper nesting** — `env time bash -c '...'`
  consumes both wrappers in sequence; more deeply nested forms
  (`env nice ionice bash -c`) also strip correctly because the
  wrapper loop iterates until the first non-wrapper basename.
- **Alternative interpreters not in the program set** — `racket`,
  `swift`, `R`, `julia`, and other less-common interpreters with
  eval flags are not enumerated. Adding them carries a small
  prose-table maintenance cost; weigh on demand.

## Canonical Bypass-Shortcut Shapes

A bypass shortcut is the inverse pattern: the program is
sanctioned, but the surrounding choreography that gives the
sanctioned program its meaning is skipped. The canonical example
is the active-flow commit gate.

`/flow:flow-commit` invokes `bin/flow finalize-commit` from inside
the skill so CI runs, the diff is reviewed, and the commit message
is composed under the skill's review prompts. The active-flow gate
on `bin/flow finalize-commit` carries a carve-out that allows the
invocation through when the state file's `_continue_pending` field
equals `"commit"` — the marker every commit-invoking skill sets
before calling the commit skill.

A bypass shortcut is a Bash call that writes the marker directly
and then invokes `bin/flow finalize-commit` without going through
`/flow:flow-commit`. The marker is present, the carve-out lets the
call through, CI still runs inside `finalize-commit` — but the
diff review, message composition, and surrounding skill steps are
skipped. The mechanical CI gate survives; the choreography that
makes the gate's product reviewable does not.

The protection against this shape is the transcript-walker check
on the skill-commit carve-out: the carve-out applies only when the
most recent assistant Skill tool_use call since the most recent
user turn is `flow:flow-commit`. The marker is belt-and-suspenders
for a fresh-session resume window; the walker is the load-bearing
predicate that ensures the surrounding skill actually ran.

## The Three Enforcement Layers

Three independent mechanical layers back this rule, each addressing
a different bypass surface.

### Layer A — Deny list (catches direct forms)

`FLOW_DENY` in `src/prime_check.rs` lists the program/flag
combinations from the Canonical Escape-Hatch Shapes table. The
glob patterns reach target projects through `/flow:flow-prime`
writing to `.claude/settings.json`, and the global `validate-pretool`
hook honors the merged deny list ahead of the allow list. This
layer catches direct shapes: `bash -c 'rm /'`, `rtk proxy ls`,
`python -c '...'`, and so on.

The deny list operates on the raw command string, so absolute
paths, env-var prefixes, combined-flag tokens, and wrapper
launchers that change the textual shape of the invocation can
route around it. Layer B closes those gaps.

### Layer B — Structural hook layer (catches indirect forms)

The structural escape-hatch layer in
`src/hooks/validate_pretool.rs::validate()` slots between the
existing deny-list check and the whitelist check. It strips
env-var prefixes (`KEY=VAL `), wrapper launchers (`env`, `time`,
`nice`, `nohup`, `taskset`, `ionice`), and the path prefix;
basenames the first token; and checks the basename against the
escape-hatch program set with `has_flag_char` trigger-character
matching.

The layer is settings-independent — it fires regardless of whether
`.claude/settings.json` is primed, so pre-prime sessions inherit
the protection. Reference patterns: the Layer 4 `find` token-walk
in the same file is the canonical structural-check shape.

### Layer C — Transcript-walker gate (closes bypass shortcuts)

The Layer 9 commit gate carries two carve-outs for legitimate
skill-driven commit paths. Each carve-out AND-combines three
conditions; the third condition in both is a transcript-walker
check that proves the surrounding skill choreography actually
ran.

**Active-flow carve-out** at
`src/hooks/validate_pretool.rs::check_active_flow_at`:

1. The command shape is `bin/flow ... finalize-commit`.
2. The state file has `_continue_pending == "commit"`.
3. `transcript_shows_commit_window_skill(transcript_path, home)`
   returns true — the most recent assistant Skill since the most
   recent user turn names a sanctioned commit-window skill
   (`flow:flow-commit` or `flow-release`). In practice every
   active-flow commit names `flow:flow-commit`; the release path
   runs on the integration trunk under the bootstrap carve-out.

Only when all three hold does the active-flow gate allow the
invocation through.

**Bootstrap-skill carve-out** at
`src/hooks/validate_pretool.rs::bootstrap_carveout_applies`,
wired into the cwd's `match_branch_at` call site only (NOT
the `-C` target — see "cwd-only scope" below):

1. The command shape is `bin/flow ... finalize-commit`.
2. `transcript_shows_commit_window_skill(transcript_path, home)`
   returns true — the shared two-arm predicate accepts either
   `flow:flow-commit` (delegated commit path used by
   `/flow:flow-start` and `/flow:flow-prime`) or
   `flow-release` (direct commit path that calls
   `bin/flow finalize-commit` without delegating to
   `/flow:flow-commit`). See
   `.claude/rules/concurrency-model.md` "Bootstrap-skill
   carve-out (integration-branch context)" for the per-skill
   trust contract.
3. `any_skill_in_set_since_user(transcript_path, home,
   BOOTSTRAP_SKILLS)` returns true, where `BOOTSTRAP_SKILLS` is
   the closed set `{"flow:flow-start", "flow:flow-prime",
   "flow-release"}`. The third entry is the bare name because
   `flow-release` is a project-local maintainer skill at
   `.claude/skills/flow-release/`; Claude Code emits the bare
   form when the user types `/flow-release`, while the first
   two stay namespaced because the corresponding skills live at
   `skills/<name>/`.

`BOOTSTRAP_SKILLS` is exactly these three skills because they are
the only FLOW skills that commit on the integration branch by
design: `flow:flow-start` Step 2 lands a `ci-fixer`
dependency-repair commit before the user's feature work begins;
`flow:flow-prime` Step 6 lands permission and stub-script setup
that must reach the integration branch before any flow can
start; and `flow-release` publishes a version-bump commit
on the integration trunk (there is no feature branch where a
release tag could live). Every other FLOW skill commits from a
feature-branch worktree, where the active-flow carve-out applies
instead. Extending the set requires naming a new skill that
legitimately needs to commit on the integration branch.

The integration-branch context has no per-branch state file at
the integration trunk, so the bootstrap carve-out uses a SECOND
walker condition in place of the state-file marker — both walker
conditions are load-bearing. The active-flow carve-out's marker
is belt-and-suspenders for a fresh-session resume window; the
bootstrap carve-out has no analogous marker because there is
nothing on the integration trunk to write to.

The carve-out names no branch — `default_branch_in()` resolves
the actual integration branch from `git symbolic-ref --short
refs/remotes/origin/HEAD` (fallback `"main"`), so the carve-out
applies identically to `main`, `staging`, `master`, `develop`,
and any other configured trunk.

cwd-only scope: `check_commit_during_flow` does NOT consult
`bootstrap_carveout_applies` at the `-C` target's
`match_branch_at(target)` callsite. The transcript walker is
session-scoped (the persisted transcript records all session
activity regardless of which repo the work targeted), so a
bootstrap chain accrued in one repo's session could otherwise
authorize a commit redirected via `git -C <other-repo>` to a
different repo's integration branch. All three legitimate
bootstrap windows (flow-start Step 2, flow-prime Step 6, and
flow-release's commit step) run with cwd ON the integration
branch by design — none uses `-C` to shift git's effective cwd
— so restricting the carve-out to the cwd callsite has no
production consumer cost while preserving cross-repo safety.

Window closure: the walker stops at the most recent real user
turn going backward. A user message after `/flow:flow-prime` (or
`/flow:flow-start` or `/flow-release`) completes — followed
by a direct `/flow:flow-commit` invocation — puts the
sanctioned-parent Skill OUTSIDE the carve-out window, so
`any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` returns false
and the block fires. Historical authorization cannot carry
forward.

Both walker checks share infrastructure with `validate-skill`
and `validate-ask-user`; reads are capped at the documented
`TRANSCRIPT_BYTE_CAP` per
`.claude/rules/external-input-path-construction.md`. See
`.claude/rules/concurrency-model.md` "Skill-commit carve-out
(active-flow context)" and "Bootstrap-skill carve-out
(integration-branch context)" for the full
substitution-of-trust analysis and the sanctioned-parent
enumeration.

## How to Apply

When tempted to reach for an escape hatch, identify the underlying
task and find the sanctioned tool that performs it:

- **Reading file content** → Read tool (single read) or Grep tool
  (pattern match).
- **Listing directory entries** → Glob tool with a pattern.
- **Writing a file** → Write tool; or, for FLOW-managed artifacts,
  `bin/flow write-rule` via the canonicalization gate per
  `.claude/rules/file-tool-preflights.md`.
- **Running a sanctioned program** → a single Bash call to the
  program directly; the allow list already covers it.
- **Running the same program N times** → N separate Bash calls per
  `.claude/rules/permission-blocked-workarounds.md`. Never wrap in
  `xargs` or a shell loop.
- **Committing during a flow** → invoke `/flow:flow-commit`. Never
  call `bin/flow finalize-commit` directly during an active flow,
  even with the marker present.
- **Remote access** → the approved ssh wrapper script; the network
  tool surface for service interactions.

A construct that does not map to one of those tools is not a tool
the model is authorized to use, even when the underlying intent
is legitimate.

## Cross-References

- `.claude/rules/permissions.md` — the deny-list and allow-list
  discipline that Layer A operationalizes.
- `.claude/rules/concurrency-model.md` — the active-flow and
  integration-branch commit gates plus their carve-outs (Layer 9
  in `validate-pretool`).
- `.claude/rules/user-only-skills.md` — the sibling enforcement
  pattern for direct user invocation; the transcript walker is
  shared infrastructure.
- `.claude/rules/permission-blocked-workarounds.md` — the
  "never create a script to batch operations the permission
  model blocks" rule, which closes the wrapper-script class of
  escape hatch.
