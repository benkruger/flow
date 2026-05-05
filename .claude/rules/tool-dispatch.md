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

## `bin/test` Sweeps Profraws Before Every Run

`bin/test` sweeps every `*.profraw` recursively under
`target/llvm-cov-target/` at the start of every invocation —
full-suite, filtered, and forced. This is the coherence mechanism
that keeps coverage measurements bounded to a single source
generation on long-lived target directories (notably main's).

### Why

cargo-llvm-cov's `--no-clean` flag preserves accumulated
instrumented binaries across runs for incremental speed. On main's
long-lived `target/`, stale `flow_rs-*` binaries accumulate as PRs
merge and source evolves. Without a profraw sweep, old profraws
from prior runs match the stale binaries' function hashes and
contribute execution counts against old source layouts, producing
Frankenstein coverage numbers.

By sweeping all `*.profraw` at the top of every `bin/test`
invocation, llvm-cov's report is scoped to profdata produced by
THIS run only. Stale binaries remain on disk (kept warm for
incremental rebuilds) but they contribute no execution counts
without matching fresh profdata.

### Invariant

- The recursive profraw sweep (`find target/llvm-cov-target -name
  "*.profraw" -delete`) runs unconditionally at the top of
  `bin/test`, before any mode dispatch.
- A separate sweep deletes `default_*.profraw` at the worktree
  root to catch subprocess tests whose `LLVM_PROFILE_FILE`
  template resolved outside `target/llvm-cov-target/`.
- `bin/flow ci --clean` is the user-facing deep-reset that wipes
  the sentinel, all profraws, and `target/llvm-cov-target/debug/`
  when a full fresh-clone experience is wanted.

When adding a new tool that writes coverage-like artifacts under
`target/llvm-cov-target/` on a long-lived target dir (main's), the
same discipline applies: either the tool must sweep its stale
artifacts before measuring, or it must not be invoked on main.

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

## EXCLUDE_ENTRIES Per-Language Coverage

`EXCLUDE_ENTRIES` in `src/prime_check.rs` is the canonical source for
patterns that prime adds to `.git/info/exclude` at install time. When a
plan extends `EXCLUDE_ENTRIES` to cover a new family of project-managed
files (e.g. throwaway probe tests, scratch caches, generated artifacts
that share a basename convention but live inside the project's source
tree), the plan must enumerate the pattern set exhaustively across every
language whose convention the file family targets — before Code phase
begins.

### Why

`.git/info/exclude` is machine-local and write-once at prime time. Once
a pattern lands, the user has no obvious diff to review — the file is
never tracked, and the only signal that an entry is missing is "the
intended file shows up in `git status` even though it was supposed to
be excluded." The cost of a missed language is paid silently on every
project that uses that language; the cost of an over-broad pattern (a
leading wildcard that matches user-named legitimate tests) is paid
silently in lost work.

The first iteration of a `bin/test --adversarial-path` PR (PR #1333)
shipped two patterns (`test_adversarial_flow.*`,
`*_adversarial_flow_test.rb`) and missed Go, Rails Minitest, RSpec, and
Swift conventions. The Code Review reviewer + adversarial agents both
caught the gap; Step 4 replaced the patterns with five exact-basename
entries (`test_adversarial_flow.*`, `adversarial_flow_test.go`,
`adversarial_flow_test.rb`, `adversarial_flow_spec.rb`,
`AdversarialFlowTests.swift`) and bumped `CURRENT_CONFIG_HASH` a second
time within the same PR. Plan-time enumeration would have surfaced the
full pattern set in one design pass and avoided the second hash bump.

### The Rule

When a plan extends `EXCLUDE_ENTRIES` (or any analogous machine-local
exclude list FLOW maintains in the future), the plan's Tasks section
must include a per-language enumeration table:

| Language / framework | Recommended basename | Pattern that matches it |
|---|---|---|
| Rust (cargo nextest) | `tests/test_adversarial_flow.rs` | `test_adversarial_flow.*` |
| Python (pytest) | `tests/test_adversarial_flow.py` | `test_adversarial_flow.*` |
| JS/TS (jest) | `tests/test_adversarial_flow.test.ts` | `test_adversarial_flow.*` |
| Go (`go test`) | `adversarial_flow_test.go` | `adversarial_flow_test.go` |
| Rails Minitest | `test/adversarial_flow_test.rb` | `adversarial_flow_test.rb` |
| RSpec | `spec/adversarial_flow_spec.rb` | `adversarial_flow_spec.rb` |
| Swift (XCTestCase) | `Tests/AdversarialFlowTests.swift` | `AdversarialFlowTests.swift` |

The example above is the canonical reference set used by
`assets/bin-stubs/test.sh` for the adversarial-probe-path family. Plans
that introduce a new file family must produce the equivalent table for
that family before Code phase begins.

### Pattern Specificity

`EXCLUDE_ENTRIES` patterns are matched against basenames in
`.git/info/exclude` semantics: a pattern without a slash matches the
basename of every file in the working tree. Two specificity rules apply
to every entry:

- **No leading wildcards.** A pattern like `*_adversarial_flow_test.rb`
  silently excludes any user-named legitimate test ending in
  `_adversarial_flow_test.rb` (e.g.
  `authentication_adversarial_flow_test.rb`). Use exact basenames or
  trailing-only wildcards (`<exact_prefix>.*`) so user files cannot
  collide.
- **Trailing-wildcard scope.** `test_adversarial_flow.*` matches any
  file whose basename is `test_adversarial_flow.<extension>`. The `.*`
  end-of-basename wildcard is acceptable because it requires the exact
  prefix; user files named `test_adversarial_flow_local_dev.py` are
  NOT matched (the `_local_dev` segment violates the prefix anchor).

When in doubt, prefer five exact-basename patterns over one
leading-wildcard pattern. The cost of an extra entry is one line in
`.git/info/exclude`; the cost of a leading wildcard is invisible
exclusion of legitimate files.

### Hash-Bump Discipline

Every change to `EXCLUDE_ENTRIES` (and `UNIVERSAL_ALLOW`, and
`FLOW_DENY`) bumps `compute_config_hash`, which forces every primed
project to re-run `/flow:flow-prime` on the next version upgrade. This
is the intended upgrade signal — but it is expensive when iterated
within one PR. **Three or more `CURRENT_CONFIG_HASH` bumps in a single
PR is a Plan-phase signal that the design was not enumerated upfront.**
The Code Review reviewer agent should flag a third bump as a
process-gap finding and prompt the author to add a per-language
enumeration table to the plan in a follow-up.
