# Per-File Coverage Iteration

When the current task is scoped to a single `src/<name>.rs` file
(adding coverage, fixing a test, refactoring one module), default
to the per-file gate:

```bash
bin/test tests/<name>.rs
```

Not the full CI:

```bash
# Avoid for single-file iteration:
bin/flow ci          # ~3 minutes
bin/flow ci --test   # ~3 minutes
```

## Why

The per-file gate and the full-CI gate enforce identical 100/100/100
thresholds against the mirrored src file — the only difference is
scope:

- `bin/test tests/<name>.rs`:
  - Compiles one test binary (`--test <name>`), not all ~117
  - Runs only that binary's tests
  - Extracts the coverage row for `src/<name>.rs` and asserts
    `Regions == Functions == Lines == 100.00%`
  - Completes in seconds on a warm build, ~30s on cold compile
- `bin/flow ci --test`:
  - Compiles every test binary in the workspace
  - Runs all ~3700 tests
  - Applies `--fail-under-*` 100/100/100 against the aggregate total
  - Completes in ~3 minutes

For iterating on one file, the per-file gate is the **same gate,
same file, same thresholds** — just 30× faster. Running full CI
between iterations wastes ~2m55s per attempt; across a dozen
iterations that's 35 minutes that should have been 1.

## The Rule

When the task touches exactly one mirrored src/test pair and the
iteration goal is "read coverage, edit, re-measure":

1. Run `bin/test tests/<name>.rs` for each measurement cycle.
2. Use `bin/test --show src/<name>.rs` to inspect uncovered
   regions/lines between iterations (same tool, no test run).
3. Run `bin/flow ci` **once** at the end, before handing off for
   commit, to catch any cross-file regressions and verify the
   format/lint/build stages.

## When Full CI Is Warranted

Full CI (or `bin/flow ci --test`) is the right tool when:

- A change touches multiple src files and cross-file coverage
  interactions matter (e.g. shared helpers pulled into new callers).
- A refactor removes/renames pub surfaces other files depend on —
  the only way to catch consumers is to compile everything.
- A skill-level contract test (`tests/skill_contracts.rs`,
  `tests/structural.rs`, `tests/permissions.rs`, `tests/docs_sync.rs`)
  might be affected by the change.
- Pre-commit verification — `/flow:flow-commit`'s
  `finalize-commit` already runs full CI; the per-file loop above
  has verified the target file, but the commit still needs the
  cross-file sanity pass.

The trigger "change crosses file boundaries" is on the author to
decide. When uncertain, `bin/test tests/<affected-file>.rs` for
each affected file is still faster than one full-CI run.

## Cross-References

- `CLAUDE.md` "Development Environment" — names the three `bin/test`
  invocation shapes (full suite, single-phase, per-file).
- `.claude/rules/ci-is-a-gate.md` — explains why `bin/flow ci` is the
  pre-commit gate; this rule is about iteration, not the gate.
- `.claude/rules/no-waivers.md` — defines the 100/100/100 threshold
  both the per-file and full-suite gates enforce.
- `bin/test` — the script that implements both modes; per-file
  dispatch is the `tests/<path>/<name>.rs` argument form.
