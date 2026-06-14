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
same file, same thresholds** — just 30× faster.

## The Rule

When the task touches exactly one mirrored src/test pair and the
iteration goal is "read coverage, edit, re-measure":

1. Run `bin/test tests/<name>.rs` for each measurement cycle.
2. Use `bin/test --show src/<name>.rs` to inspect uncovered
   regions/lines between iterations (same tool, no test run).
3. Cross-file regressions and format/lint stages are caught at
   commit time inside `/flow:flow-commit`'s `finalize-commit` call
   (`ci::run_impl()` runs full CI before `git commit` lands). Do
   not run `bin/flow ci` manually during Code phase — Layer 11
   blocks it per the Enforcement section below. The `--clean`
   variant remains available as the documented phantom-misses
   recovery path.

## When Full CI Is Warranted

Outside of Code phase (during Plan, Review, Complete, or
on main when no flow is active), full CI (or `bin/flow ci --test`)
is the right tool when:

- A change touches multiple src files and cross-file coverage
  interactions matter (e.g. shared helpers pulled into new callers).
- A refactor removes/renames pub surfaces other files depend on —
  the only way to catch consumers is to compile everything.
- A skill-level contract test (`tests/skill_contracts.rs`,
  `tests/structural.rs`, `tests/permissions.rs`, `tests/docs_sync.rs`)
  might be affected by the change.

During Code phase, `/flow:flow-commit`'s internal `ci::run_impl()`
call covers the same cross-file sanity-pass surface every commit;
no separate Bash invocation is needed.

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
   confirm stale binaries.
2. Run `bin/flow ci --clean`. This is the user-facing reset:
   removes `target/llvm-cov-target/debug/deps/`, the
   `incremental/` dir, and every `*.profraw`. Layer 11's
   `--clean` carve-out lets this run during Code phase. The
   next test run rebuilds fresh instrumentation with one crate
   hash per binary and the phantom misses disappear.
3. Re-run `bin/test tests/<name>.rs`. The reported coverage now
   reflects the actual code state.

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

## Enforcement

The Rule above is enforced mechanically by Layer 11 of
`validate-pretool`. During an active Code phase
(`current_phase == "flow-code"` AND
`phases.flow-code.status == "in_progress"` in the state file),
the hook rejects every `bin/flow ci` invocation with exit 2 and a
message naming the per-file gate as the redirect target. The
single carve-out is `bin/flow ci --clean` — the documented
phantom-misses recovery path above. Case variants (`--CLEAN`)
and the equals form (`--clean=true`) both reach the carve-out per
`.claude/rules/security-gates.md` "Normalize Before Comparing".

The gate is in `src/hooks/validate_pretool.rs::check_ci_during_code_phase`,
called from `run()` after Layer 10's commit gate.
`finalize_commit::run_impl` calls `ci::run_impl()` as a Rust
function from inside the same process — it never reaches the
Bash hook, so the commit-time CI gate is structurally
unaffected. Cross-file regressions are still caught at the
commit boundary.

The gate's posture is fail-closed-as-NO-BLOCK (inverted from
Layer 10's fail-closed-by-blocking). Every state-file read or
parse error returns "no block" because mis-blocking a legitimate
`bin/flow ci` in a wrongly-detected non-Code phase would surprise
the user. Layer 11 is friction-prevention, not a security gate.

The block fires only when ALL of the following hold:

1. `is_flow_ci_invocation(command)` — the command shape is
   `bin/flow ... ci` where `ci` appears as the FIRST non-flag
   token after the launcher. Global flags between launcher and
   subcommand (`bin/flow --log-level info ci`,
   `bin/flow --log-level=info ci`) are skipped; sibling
   subcommands whose args happen to contain the literal `ci`
   token (`bin/flow phase-enter --phase ci`,
   `bin/flow log feat "Phase 2 ci notes"`) do NOT match.
2. `!has_clean_flag(command)` — neither `--clean`, `--CLEAN`,
   nor `--clean=<value>` is present. Case-folded comparison
   ensures the documented recovery path remains available in
   every variant.
3. An active flow exists at the resolved
   `<main_root>/.flow-states/<branch>/state.json`.
4. `state_is_in_code_phase(branch, main_root)` returns true.
   `current_phase` and `phases.flow-code.status` are normalized
   via `normalize_gate_input` (NUL strip + trim + ASCII
   lowercase) so hand-edited state files with case- or
   whitespace-variant values still trigger the gate. The state
   file read is bounded at `STATE_FILE_BYTE_CAP` (8 MB) per
   `.claude/rules/external-input-path-construction.md`.

The integration test matrix in `tests/hooks/validate_pretool.rs`
under the `--- layer_11_ci_during_code_phase ---` marker covers
the full decision surface: every blocking shape, the `--clean`
carve-out (bare, with branch arg, uppercase, equals form),
subcommand-position discipline (phase-enter, phase-transition,
set-timestamp, log all pass through), every passing phase
context (no active flow; flow-start / flow-review /
flow-complete; flow-code status pending / complete; non-ci
subcommands), and every fail-closed shape (unparseable JSON,
invalid UTF-8, missing `current_phase`, wrong-type `phases`,
wrong-type `phases.flow-code`, wrong-type
`phases.flow-code.status`, unreadable state file via chmod 000,
case-variant phase values, and a no-project-root cwd).
