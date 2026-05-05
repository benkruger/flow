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

## Phantom Misses (Stale Instrumented Binaries)

`bin/test` runs with `cargo llvm-cov --no-clean` so test binaries
are kept warm across runs for fast incremental rebuilds. The
profraw sweep at the start of every invocation purges stale
profdata, but it does NOT purge the instrumented binaries
themselves under `target/llvm-cov-target/debug/deps/`. Those
binaries' instrumentation maps stay in cargo-llvm-cov's "expected
function" set even when their profdata is empty.

The result: a file can read e.g. `92.31% / 95.54% / 96.15%` with
mysterious "missed functions" that resist every test you add.
The "missed" counts are 3 different stale crate hashes' empty
function entries, not real source-level gaps. Adding tests does
nothing because the executed instantiation is already counted
once; the stale instantiations remain unexecuted forever.

**Diagnostic.** When per-file coverage looks impossibly stuck
(adds tests pass, coverage doesn't move):

1. Run `bin/test --funcs <basename>.rs` — lists every function
   instantiation with its execution count. Multiple entries for
   the same demangled name with different mangled crate hashes
   (e.g., `_RNvNtCs8fXSiUa7bCM_*`, `_RNvNtCsaO9B8DlJywj_*`,
   `_RNvNtCslT5c56zUrC1_*` all alongside the live
   `_RNvNtCscjLNWQIh9gP_*`) confirm stale binaries.
2. Run `bin/flow ci --clean`. This is the user-facing reset:
   removes `target/llvm-cov-target/debug/deps/`, the
   `incremental/` dir, and every `*.profraw`. The next test run
   rebuilds fresh instrumentation with one crate hash per binary
   and the phantom misses disappear.
3. Re-run `bin/test tests/<name>.rs`. The reported coverage now
   reflects the actual code state. If the file is still <100%,
   the remaining gap is real and addressable via tests or
   refactor.

The cleanup is a ~12-second one-shot followed by a ~45-second
fresh compile on the first subsequent test run. Cheap relative
to the cost of chasing phantom misses for hours.

**When to suspect phantom misses.** Symptoms:

- Adding tests doesn't move coverage at all (same numbers
  repeatedly).
- "Missed functions" count exceeds the count of named functions
  + closures you can actually find in the source.
- `bin/test --show <file>` shows execution counts > 0 on every
  source line but the coverage row still flags "missed regions"
  / "missed functions".
- `bin/test --funcs <file>` shows the same demangled name three
  or four times with different mangled hashes, only one of which
  has count > 0.

Any one of those is sufficient — clean and re-measure before
spending more time on test design.

## Cross-References

- `CLAUDE.md` "Development Environment" — names the three `bin/test`
  invocation shapes (full suite, single-phase, per-file).
- `.claude/rules/ci-is-a-gate.md` — explains why `bin/flow ci` is the
  pre-commit gate; this rule is about iteration, not the gate.
- `.claude/rules/no-waivers.md` — defines the 100/100/100 threshold
  both the per-file and full-suite gates enforce.
- `bin/test` — the script that implements both modes; per-file
  dispatch is the `tests/<path>/<name>.rs` argument form.
