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
path must reach the filesystem through one of three guards:

1. **`FlowPaths::try_new(root, branch)`** — returns `None`
   when the branch fails `is_valid_branch`. Treat `None` as "no
   active flow" (early return, structured error, or skip step).
2. **`FlowPaths::new(root, branch)`** — use ONLY when the branch
   was already validated upstream (copied from the state-file
   `branch` field written by `branch_name()` at flow-start, or
   produced by `init_state` after sanitization). The panicking
   constructor is reserved for callers holding a guaranteed-valid
   branch.
3. **`FlowPaths::is_valid_branch(&branch)` pre-validation** —
   call before any other path construction; reject the input
   with a structured error if the predicate returns false.

Direct `format!` interpolation that puts a branch into a
`.flow-states/` or `.worktrees/` path without one of these three
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

**Plan phase.** When planning a feature that introduces a new
path constructed from a branch name, add a row to the
external-input-audit table per
`.claude/rules/external-input-audit-gate.md`. <!-- scope-enumeration: imperative -->
The audit table enumerates hook callsites AND CLI subcommand
callsites that accept the same branch input — both families
flow user input into path construction.

**Code phase.** Use `FlowPaths::try_new` by default for any
external-source branch. Reserve `FlowPaths::new` for branches
that came from a known-validated source (state file's own
`branch` field, `branch_name()` output). Never write
`format!(".flow-states/{}", branch)` or
`format!(".worktrees/{}", branch)` without the guard.

**Code Review phase.** <!-- scope-enumeration: imperative -->
The reviewer agent and adversarial agent check every new
path-construction site for the guard. The adversarial agent
writes failing tests against each of the four rejected inputs
(empty, `.`, `..`, NUL byte) on every new public surface that
accepts a branch.

## Cross-References

- `.claude/rules/external-input-validation.md` — the broader
  prose discipline for fallible constructors.
- `.claude/rules/external-input-audit-gate.md` — the Plan-phase
  gate that requires a callsite audit table.
- `src/flow_paths.rs` — `FlowPaths::is_valid_branch`,
  `FlowPaths::new`, and `FlowPaths::try_new` are the canonical
  guards.
- `tests/flow_paths.rs` — coverage for every rejection class
  through every entry point.
