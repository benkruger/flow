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

## Mechanical Enforcement

The Plan-phase prose audit gate (`bin/flow plan-check`) catches
plans that propose adding a panic/assert validation without a
paired callsite source-classification table. See
`.claude/rules/external-input-audit-gate.md` for the gate's
trigger vocabulary, the required audit table format, the opt-out
grammar, and the three-callsite enforcement topology.

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
   - Sources in **Guaranteed valid** may use the panicking
     constructor directly.
   - Sources in **Trusted but external** or **Untrusted** must use
     a fallible variant (`try_new` returning `Option`, a `Result`-
     returning constructor, or an explicit validity check before
     the panicking constructor). Treat the invalid-input case as
     an expected control-flow branch (typically "no active flow"
     or "unknown target"), not an error.

A plan that adds a validation without this audit is incomplete.

### Codebase-wide rule

For any FLOW type that accepts a parameter from git (branch names,
tags, commit SHAs), the public API must expose at least one fallible
constructor alongside the panicking one. Callers that receive the
value from `current_branch()`, `resolve_branch()`, `resolve_branch_in()`,
or any subprocess running `git` must use the fallible variant. **A
`--branch` CLI override is also an external input** — callers that
accept `--branch` must use the fallible variant. The panicking
variant is reserved for callers that have already validated the
value at a prior boundary (state-file keyspace, `branch_name()`
output, upstream `try_new` success).

The reference implementation is `FlowPaths::new` / `FlowPaths::try_new`:

- `FlowPaths::new(root, branch)` panics on empty or slash-containing
  branches. Callers that hold a value already known to be a canonical
  FLOW branch name use this.
- `FlowPaths::try_new(root, branch)` returns `Option<Self>`, `None`
  when the branch fails `FlowPaths::is_valid_branch`. Callers that
  receive a branch from git use this.
- `FlowPaths::is_valid_branch(branch)` is the public predicate — use
  it for pre-validation when a caller needs to fork on validity before
  constructing.

### Hook callsite discipline

FLOW hooks (`src/hooks/*.rs`) run under Claude Code's session
lifecycle. A panic inside a hook crashes the session's tool call
and surfaces as a user-visible failure. Hooks therefore must never
construct a branch-scoped `FlowPaths` via `FlowPaths::new` — they
must use `FlowPaths::try_new` and treat `None` as "no active flow on
this branch" (early return, or `exit 0` for standalone hook
binaries).

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
`--branch` (and therefore must guard with `FlowPaths::try_new` or
pre-validate via `FlowPaths::is_valid_branch`) includes
`src/complete_fast.rs:read_state`. Any new CLI subcommand that
accepts `--branch` and constructs a `FlowPaths` must follow the
same discipline.

### Code Review enforcement

During Code Review, the reviewer agent and adversarial agent check
for violations of this rule. The reviewer checks that new panicking
constructors have a fallible sibling exposed and that git-sourced
callsites use it. The adversarial agent writes tests that invoke the
hook/entry point with a slash-containing branch and asserts it does
not panic.

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
