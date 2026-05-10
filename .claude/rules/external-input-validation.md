# External Input Validation

When a function validates its input via `assert!`, `panic!`, or any
constructor-level invariant check, callers that source the input from
outside the process must be audited. A validation that panics
downstream of an unchecked source converts a silent bug into a hard
crash in production.

## Why

An invariant check inside a constructor (`assert!`, `panic!`,
`Result`-returning with `.expect`) makes a claim about the input's
shape. If upstream callers already guarantee that shape, the check is
a logic-bug tripwire. If a caller sources the input from an external
place (git, user config, subprocess output, parsed JSON, env vars)
and does not validate upstream, the check is a denial-of-service
vector for legitimate inputs the external system permits.

A `--branch` CLI override is also an external input — `clap` accepts
any string the shell passes, including slash-containing and empty
values. The override is no more trusted than a git subprocess result.

## How to Apply

### Plan-phase audit

When the plan introduces or tightens a validation assertion on a
function parameter, the plan must include a caller audit for that
function. The audit enumerates the callsites. For every row in the
audit:

1. Record the exact source of the validated argument (e.g. state-
   file key, `current_branch()` subprocess, CLI flag, struct
   field).
2. Classify the source:
   - **Guaranteed valid** — the source is a compiled constant, a
     key that was validated at a previous boundary, or a value
     copied from state that was sanitized at write time.
   - **Trusted but external** — the source is git output, a user
     config file, a subprocess stdout, or any system command whose
     behavior is outside FLOW's control. These values may be
     structurally legal in their source system but violate the
     FLOW-side invariant.
   - **Untrusted** — direct user input, parsed untrusted JSON, etc.
3. Record the callsite's handling:
   - Sources in **Guaranteed valid** may chain
     `.expect("<boundary>")` on the fallible constructor with a
     doc-comment naming the upstream sanitizer that proves the
     input cannot fail validation.
   - Sources in **Trusted but external** or **Untrusted** must
     pattern-match on the fallible constructor's `Option`/`Result`
     and translate the invalid-input case into an expected
     control-flow branch (typically "no active flow" or "unknown
     target"), not an error.

A plan that adds a validation without this audit is incomplete.

### Codebase-wide rule

For any FLOW type that accepts a parameter from git (branch names,
tags, commit SHAs), the public API must expose a fallible
constructor as the primary surface. Callers that receive the value
from `current_branch()`, `resolve_branch()`, `resolve_branch_in()`,
or any subprocess running `git` must pattern-match on the fallible
variant's return and treat the invalid case as an expected
control-flow branch. **A `--branch` CLI override is also an
external input** — callers that accept `--branch` must use the
same fallible variant.

Callers that hold a branch already validated upstream (state-file
keyspace, `branch_name()` output, upstream `try_new` success) chain
`.expect("<boundary>")` on the fallible constructor with a
doc-comment naming the sanitizer. The `.expect` is documentation,
not a panic vector — the boundary message records *which* upstream
guarantee makes the call infallible.

The reference implementation is `FlowPaths::try_new`:

- `FlowPaths::try_new(root, branch)` returns `Option<Self>`, `None`
  when the branch fails `FlowPaths::is_valid_branch`. Callers that
  receive a branch from git or a CLI override pattern-match on the
  `Option` and treat `None` as "no active flow on this branch".
  Callers holding a known-valid branch chain
  `.expect("<boundary message naming the upstream sanitizer>")`.
- `FlowPaths::is_valid_branch(branch)` is the public predicate —
  use it for pre-validation when a caller needs to fork on
  validity before constructing.

### Hook callsite discipline

FLOW hooks (`src/hooks/*.rs`) run under Claude Code's session
lifecycle. A panic inside a hook crashes the session's tool call
and surfaces as a user-visible failure. Hooks default to
`FlowPaths::try_new` with a pattern-match on the `Option` — `None`
maps to "no active flow on this branch" (early return, or `exit 0`
for standalone hook binaries).

**Structurally-provable carve-out.** When the branch reaches the
hook through a value chain that the OS itself cannot violate
(e.g., `Path::file_name()` always yields a single path component
with no `/`, so a branch derived from filesystem-walk output is
structurally `/`-free), the hook may chain
`.expect("<boundary message naming the structural invariant>")`
on `try_new` instead of pattern-matching. The `.expect` is
documentation of the OS-level guarantee, not a reachable panic
— a panic would require the kernel to violate filesystem
semantics. The boundary message must name the invariant
explicitly so a future reader can verify the proof.

The current hook inventory that receives a branch from git includes
`stop_continue.rs`, `stop_failure.rs`, `post_compact.rs`,
`validate_ask_user.rs`, and `validate_claude_paths.rs`. Any new hook
that joins this list must follow the same discipline.

### CLI subcommand entry callsite discipline

CLI subcommands that accept a `--branch` override (or any other
branch-valued CLI arg) are the same category of caller as hooks:
the string comes from outside the process and is unvalidated before
it reaches the FLOW-side invariant. A panic in a CLI subcommand
terminates the user's shell invocation with a Rust backtrace — a
user-visible failure indistinguishable from a session-lifecycle
hook panic.

The CLI subcommand entry inventory that receives a branch via
`--branch` (and therefore must guard with `FlowPaths::try_new` and
treat `None` as a structured-error path, OR pre-validate via
`FlowPaths::is_valid_branch`) includes `src/complete_fast.rs:read_state`,
`src/start_step.rs`, `src/start_finalize.rs`, `src/start_gate.rs`,
`src/start_workspace.rs`, `src/finalize_commit.rs`, and
`src/commands/init_state.rs::create_state` (when `--branch` override
is supplied). Any new CLI subcommand that accepts `--branch` and
constructs a `FlowPaths` must follow the same discipline.

### Code Review enforcement

During Code Review, the reviewer agent and adversarial agent check
for violations of this rule. The reviewer checks that new
constructors with input-validation invariants expose a fallible
variant and that git-sourced callsites pattern-match on it. The
adversarial agent writes tests that invoke the hook/entry point
with a slash-containing branch and asserts it does not panic.

### Testing discipline

Every fallible constructor (`try_new`-style) must have unit tests
covering the rejection paths (empty input, malformed input, prohibited
characters). Hooks that use the fallible variant must have an
integration test that exercises the "no active flow" branch — the
test passes a slash-containing branch or a branch with no state file
and asserts the hook exits 0 / returns early without panicking.

CLI subcommands that accept `--branch` must include a regression
test that exercises the slash-branch path and asserts a structured
error (not a panic).
