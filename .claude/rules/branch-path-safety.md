# Branch Path Safety

When a branch name comes from outside the process (a `--branch`
CLI flag, `current_branch()`/`resolve_branch()` git output, a
state-file read, an environment variable), it must be validated
through `FlowPaths::is_valid_branch` before being interpolated
into any filesystem path. The validator rejects strings that
would escape the per-branch subdirectory: empty, `.`, `..`, any
string containing `/`, and any string containing `\0`.

`FlowPaths::branch_dir()` joins the branch onto `.flow-states/`,
and cleanup runs `fs::remove_dir_all(branch_dir())`. A
path-traversal segment (`.` or `..`) joined onto the directory
resolves outside the per-branch scope: `--branch ..` targets the
project root, `--branch .` targets `.flow-states/` itself
(every concurrent flow's state). A NUL byte truncates the path
in syscalls in implementation-defined ways. A slash creates a
nested subdirectory the discovery scanners cannot see.

## The Rule

Branch names that flow into a `.flow-states/` or `.worktrees/`
path must reach the filesystem through one of two guards:

1. **`FlowPaths::try_new(root, branch)`** — returns `None`
   when the branch fails `is_valid_branch`. Pattern-match on
   the `Option`: callers that source the branch from outside
   the process (CLI `--branch`, git output, hooks) treat
   `None` as "no active flow" (early return, structured
   error, or skip step). Callers that hold a branch already
   validated upstream may chain `.expect("<boundary>")` with
   a doc-comment naming the upstream sanitizer
   (`branch_name()`, state-file keyspace, prior `try_new`
   pattern-match) — the `.expect` is documentation, not a
   panic vector.
2. **`FlowPaths::is_valid_branch(&branch)` pre-validation** —
   call before any other path construction; reject the input
   with a structured error if the predicate returns false.

Direct `format!` interpolation that puts a branch into a
`.flow-states/` or `.worktrees/` path without one of these
guards is forbidden. The path escape is silent and the cleanup
blast radius is unbounded.

## Why

The CLI accepts any string a shell can pass — including `..`,
`.`, slash-containing values, and embedded NULs. Git permits
many of those as branch names too. State files can be hand-edited
or corrupted to contain malicious branch values. Without
validation at the path-construction boundary, a branch that
flows into a path becomes a candidate vector for
arbitrary-directory deletion (cleanup) or arbitrary-file
write/read (state mutators).

The validator runs at the boundary so that downstream code
(cleanup, discovery scanners, hooks, state mutators) can assume
the branch is safe without re-validating. A single guard at
the constructor is more reliable than per-callsite checks.

## How to Apply

**Code phase, before writing the implementation.** Enumerate
every hook callsite and CLI subcommand callsite that accepts
the same branch input — both families flow user input into
path construction. Use `FlowPaths::try_new` by default for any
external-source branch and pattern-match on the `Option`.
Callers that hold a branch already validated upstream chain
`.expect("<boundary>")` with a doc-comment naming the
sanitizer. Never write `format!(".flow-states/{}", branch)` or
`format!(".worktrees/{}", branch)` without the guard.

**Code Review phase.**
The reviewer agent and adversarial agent check every new
path-construction site for the guard. The adversarial agent
writes failing tests against each of the four rejected inputs
(empty, `.`, `..`, NUL byte) on every new public surface that
accepts a branch.

## Cross-References

- `.claude/rules/external-input-validation.md` — the broader
  prose discipline for fallible constructors.
- `src/flow_paths.rs` — `FlowPaths::is_valid_branch` and
  `FlowPaths::try_new` are the canonical guards.
- `tests/flow_paths.rs` — coverage for every rejection class
  through every entry point.
