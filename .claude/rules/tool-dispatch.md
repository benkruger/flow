# Tool Dispatch

FLOW's `bin/flow ci` (and its single-phase variants
`--format`/`--lint`/`--build`/`--test`) delegate to repo-local
`./bin/<tool>` scripts. The user owns the actual tool commands and
FLOW provides the orchestration layer. This rule covers the invariants
the orchestration layer must maintain.

## Empty Tool List Is a Failure, Not a Skip

When the tool runner is invoked and NO executable `bin/{format,lint,build,test}`
scripts are present in the cwd, the runner must return
`{"status": "error"}` with exit code 1 and a message pointing at
`/flow:flow-prime`. Silent "ok skipped" behavior causes CI to pass in
unprimed projects or in subdirectories where stubs were not installed,
and `finalize-commit` then commits without any real gate.

The empty-tools branch exists in two places and they must stay in
sync:

- `ci::run_once` — non-retry path
- `ci::run_with_retry` — retry path

Both must return the same error shape. When adding a third dispatch
path in the future (e.g. a new `--parallel` mode), copy the same guard.
A test at each callsite should exercise the empty-tools path to prove
the error is produced.

The only exception is a runner that is invoked by a parent that has
already accounted for missing tools (e.g. `format-status` inspecting
metadata only). CI-family runners never have this exemption.

## Stub Marker and Sentinel Suppression

`assets/bin-stubs/*.sh` are the fallback scripts that
`install_bin_stubs` copies into a project when the user has not yet
created their own. Each stub exits 0 with a stderr reminder so a
fresh prime never blocks CI. Without additional protection, the
sentinel-skip optimization locks in a "passing" sentinel after the
first run and the stderr reminder becomes invisible — users can ship
code with no real gate.

Every stub file MUST contain the literal comment
`# FLOW-STUB-UNCONFIGURED` on its own line (ideally right after the
shebang). `ci.rs::any_tool_is_stub` reads each tool script's source
and checks for this marker. When any marker is present, the CI
runner still reports `status: ok` but sets `stubs_detected: true`
and refuses to write the sentinel. This way the stderr reminder
surfaces on every CI run until the user removes the marker and
configures a real command.

When adding a new stub template or a new auto-installed script:

1. Include `# FLOW-STUB-UNCONFIGURED` in the source of every stub
   variant (including any new tool beyond the current four).
2. If the stub is consumed by a new dispatcher, the dispatcher must
   call `any_tool_is_stub` (or an equivalent marker scan) before
   writing any success sentinel for that dispatcher.
3. Never move the marker outside the script source (e.g. into a
   sibling metadata file). The marker must live with the script so
   it is preserved through manual edits and file moves.

## Full-Suite `bin/test` Runs Clean First

`bin/test` is invoked in two modes: full-suite (no filter args) and
filtered (`bin/flow ci --test -- <filter>`). Full-suite runs are the CI
gate used by `start-gate`, `finalize-commit`, and manual `bin/flow ci`
invocations. Filtered runs are for iterative development.

The full-suite mode MUST run `cargo clean -p flow-rs --target-dir
target/llvm-cov-target` before invoking `cargo llvm-cov nextest`.
Filtered mode MUST NOT — it depends on cargo-llvm-cov's `--no-clean`
flag preserving accumulated instrumented binaries for speed.

### Why

cargo-llvm-cov embeds each compiled binary's coverage map (`.covmap`
section) describing the source layout the binary was compiled against.
When `--no-clean` preserves stale binaries across source changes —
which happens routinely on main's long-lived `target/` as PRs merge
— llvm-cov discovers ALL matching binaries and merges their covmaps.
A pre-refactor binary describing a larger source layout silently
poisons the current binary's measurement, producing Frankenstein
coverage numbers that look like a regression but aren't. The failure
mode is invisible (no warning, no error — just wrong numbers) and
manifests most sharply on main, where `start-gate` runs CI under the
start lock (see `.claude/rules/concurrency-model.md` and CLAUDE.md
"Start-Gate CI on Main as Serialization Point").

Cleaning at the `flow-rs` package scope removes the stale binaries
while preserving the dep cache (serde, regex, chrono, etc.) under
`target/llvm-cov-target/debug/deps/`. Rebuild is package-scoped,
not from-zero.

### Invariant

- The clean invocation appears in the `if [ $# -eq 0 ]` branch of
  `bin/test`, before the existing `*.profraw` sweep.
- The clean is scoped to the `flow-rs` package (`-p flow-rs`) and
  the `llvm-cov-target` dir (`--target-dir target/llvm-cov-target`).
  A workspace-wide clean would nuke the dep cache and slow every
  full-suite run by ~30s.
- A contract test in `tests/bin_test.rs` asserts the clean precedes
  the nextest invocation and uses the correct flags. If that test
  is deleted or its assertions loosened, the invariant loses its
  mechanical guard.

When adding a new tool that writes coverage-like artifacts under
`target/llvm-cov-target/` on a long-lived target dir (main's), the
same coherence discipline applies: either the tool must clean its
stale artifacts before measuring, or it must not be invoked on main.

## Stub Lifecycle Integration Tests

Any plan that adds a new stub template or new auto-installed script
must include a test task that exercises the full lifecycle end to end:

1. Prime the project fresh — verify the stub is installed at the
   expected path and carries the unconfigured marker.
2. Run the CI-family runner — verify it returns
   `status:ok stubs_detected:true` and no sentinel is written.
3. Run the CI-family runner a second time — verify the stderr
   reminder still appears (sentinel skip must NOT kick in).
4. Simulate a user removing the marker and adding a real command —
   verify the next run writes the sentinel.
5. Run a subsequent CI — verify the sentinel is respected and no
   tools re-execute.

These cases catch the class of bugs where sentinel, retry, or
skip-path optimizations interact incorrectly with placeholder scripts.
Unit tests of the marker scanner alone are insufficient — the failure
mode only manifests across the priming ↔ runner boundary.
