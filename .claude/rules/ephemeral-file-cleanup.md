# Ephemeral File Cleanup

When a FLOW phase writes an ephemeral artifact inside the worktree
that does not survive the flow lifecycle (the Review
adversarial probe is the canonical example), the cleanup pass that
disposes of the artifact follows a small set of invariants so the
disposal is explicit, audit-trailed, and permission-safe.

## Lifecycle Ownership

An ephemeral artifact has exactly one creating phase and exactly
one disposing phase. For the adversarial probe:

- **Review (Phase 3) creates.** The adversarial agent writes
  the probe at the path resolved by `bin/test --adversarial-path`
  (a project-owned bash script that prints the canonical probe path
  for the project's language).
- **Review Step 4 may rewrite or delete.** When a Step 4 fix
  invalidates the probe's assertions, the probe is reconciled per
  `.claude/rules/adversarial-probe-lifecycle.md` — either deleted
  (default) or updated to assert the post-fix invariant.
- **Phase 4 Complete disposes.** The cleanup orchestrator
  (`src/cleanup.rs::delete_adversarial_probe`) explicitly removes
  the probe file from the worktree via `fs::remove_file` before
  the worktree directory itself is removed.

The explicit cleanup step is the disposal — not defense-in-depth
on top of `git worktree remove`. Worktree removal deletes the
worktree's filesystem tree, but it does not affect git's tracked
state on the integration branch. The only mechanism that ensures
the probe leaves no on-disk trace before the worktree is unlinked
is the explicit `fs::remove_file` call. The cleanup step's JSON
`steps` output records "deleted" / "missing" / "skipped" /
"failed" so the disposal is audit-trailed.

The disposal mechanism is sound only when the path is **untracked**
on the integration branch — a tracked stub or committed probe
would survive worktree-side cleanup because git history on the
integration branch is unaffected by working-tree deletions. See
`.claude/rules/adversarial-probe-lifecycle.md` "Untracked-Path
Invariant" for the structural test that enforces the untracked
state.

## Cleanup Ordering

Steps that remove worktree-internal artifacts must precede the
`git worktree remove` step in `src/cleanup.rs::cleanup`.

The reason is mechanical: once `git worktree remove` runs, the
worktree directory is gone — `bin/test --adversarial-path` no
longer resolves, the probe path no longer exists, and any
worktree-internal subprocess fails. Pre-removal disposal makes the
discovery step (`bin/test --adversarial-path`) deterministic and
gives each artifact its own JSON status entry.

The constraint cross-references
`.claude/rules/skill-authoring.md` "Cleanup Script Step Ordering",
which carries the same ordering invariant for SKILL.md changes.

## Permission-Safe Deletion via `fs::remove_file`

The deletion call must be `fs::remove_file` from inside Rust. Two
alternatives are insufficient and one is unsafe:

- **`rm <path>`** is rejected by the FLOW permission allow-list
  during an active flow per `.claude/rules/permissions.md`. The
  Bash tool refuses the call and the cleanup step has no fallback.
- **`git rm <path>`** fails when the artifact is in
  `.git/info/exclude` (the canonical EXCLUDE_ENTRIES location for
  ephemeral test files per `src/prime_check.rs`). Untracked files
  are not removable via `git rm`.
- **Write tool overwriting the file with a stub** is a workaround
  the model has reached for in the past, but it leaves a stub file
  on disk rather than removing the artifact. Cleanup is not an
  overwrite primitive.

`fs::remove_file` (called from the Rust process) bypasses the
permission allow-list (the Rust process is already authorized to
mutate worktree files) and works regardless of whether the file is
tracked or excluded.

## Idempotent Missing-File Handling

The cleanup pass may run twice (abort-then-complete in adjacent
sessions, or a retry after a partial failure). The deletion call
must tolerate `io::ErrorKind::NotFound` as a non-error outcome.

The canonical match in `src/cleanup.rs` is:

```rust
match fs::remove_file(&probe_path) {
    Ok(()) => "deleted".to_string(),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
    Err(e) => format!("failed: {}", e),
}
```

`"missing"` is distinct from `"skipped"` so the user can tell from
the JSON whether the path was resolvable but empty (the agent never
wrote a probe — Step 4 reconciled it) versus resolution failed
upstream (`bin/test` exited non-zero, worktree absent).

## Update-vs-Delete When Step 4 Invalidates Assertions

When Review Step 4 fixes a finding the adversarial probe
surfaced, the probe's assertions become stale. Per
`.claude/rules/adversarial-probe-lifecycle.md` "How to Apply", the
default reconciliation is **delete** — remove the file from the
worktree entirely (or empty its contents). The probe path must
not be left in a tracked state on the integration branch (see
`.claude/rules/adversarial-probe-lifecycle.md` "Untracked-Path
Invariant").

The exception is **update**: when the new behavior itself needs a
regression guard AND the guard belongs in the probe rather than in
a properly named test file (rare). The Step 4 author rewrites the
probe's assertion to lock in the post-fix invariant.

The cleanup step in `src/cleanup.rs` is unaffected by the
update-vs-delete decision: it disposes of whatever file is at the
resolved probe path, regardless of whether Step 4 left a stub, an
updated assertion, or nothing.

## Adding New Ephemeral Artifacts

When a future PR introduces a new ephemeral worktree-internal
artifact (a profiler trace, a benchmark output, a generated test
fixture), the cleanup hook for that artifact MUST follow this
rule:

1. **Add a per-artifact step** to `src/cleanup.rs::cleanup`,
   inserted BEFORE the worktree-removal step.
2. **Discovery via project-owned hook.** The artifact's path is
   resolved through a project-owned bash script (`bin/test
   --adversarial-path` is the reference) so the path remains a
   project decision rather than a FLOW hardcode.
3. **`fs::remove_file`** with `NotFound` tolerance per the canonical
   match above.
4. **JSON outcome contract** — `"deleted"`, `"missing"`,
   `"skipped"`, or `"failed: <reason>"` so the user can audit.
5. **Per-artifact tests** in `tests/cleanup.rs` covering each
   outcome branch.

## Cross-References

- `.claude/rules/adversarial-probe-lifecycle.md` — the Code
  Review-side discipline for probe creation and Step 4
  reconciliation.
- `.claude/rules/skill-authoring.md` "Cleanup Script Step
  Ordering" — the SKILL.md-side ordering constraint that mirrors
  this rule's "Cleanup Ordering" subsection.
- `.claude/rules/permissions.md` — the allow-list discipline that
  forbids `rm` mid-flow and motivates the `fs::remove_file`
  requirement.
- `src/cleanup.rs::delete_adversarial_probe` — the reference
  implementation.
- `tests/cleanup.rs::cleanup_deletes_adversarial_probe_when_present`
  and sibling tests — the per-outcome coverage.
