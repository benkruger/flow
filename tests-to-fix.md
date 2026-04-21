# Tests-to-Fix Prompt

This file is the authoritative prompt for the next session. It
enumerates every outstanding `#[cfg(test)]` migration AND every
existing case where a prior session created a `pub` seam purely to
enable testing, in violation of `.claude/rules/test-placement.md`
rule #3. Both kinds of work must be completed so `bin/flow ci`
passes at 100/100/100 without the pub-for-testing anti-pattern.

## Hard Rule for This Session

**Before adding `pub` to any item**, name the non-test production
consumer outside this module. If the only callers are (a) a thin
production wrapper that exists in the same module to dispatch to
this seam and (b) integration tests, the `pub` is for testing and is
forbidden. Drive the test through the production wrapper instead —
via subprocess (`CARGO_BIN_EXE_flow-rs`) or via a fixture that makes
the real production path take the branch.

If a branch resists testing through the real production path:

1. Prefer `.expect("<rationale>")` on truly-unreachable-in-practice
   error arms per `testability-means-simplicity.md`.
2. Prefer deleting the branch if it has no production consumer.
3. Prefer restructuring the code to eliminate the hard-to-test
   branch entirely (simpler primitive, fewer seams).
4. Only as a last resort, expose a new `pub` item — and only if it
   has a genuine non-test consumer elsewhere in the crate. Document
   the non-test consumer in the `pub fn` doc comment.

A `pub` addition whose sole caller is `run()` in the same module is
the forbidden pattern. The fact that `src/hooks/validate_ask_user.rs`
already uses this pattern does NOT make it right — it is pre-existing
debt enumerated below for audit.

## Section A: Inline-test migrations still pending

These four files contain `#[cfg(test)]` blocks in `src/` and need
their tests migrated to `tests/<mirror-path>/<name>.rs` per
`.claude/rules/test-placement.md`.

### A1. `src/hooks/validate_pretool.rs`

**Status**: reverted in this session. Inline tests still present
in src. A prior in-session extraction of `pub enum HookAction` +
`pub fn run_impl_main` was reverted because it was pub-for-testing.

**Next session must**:

- Migrate inline tests to `tests/hooks/validate_pretool.rs`.
- Drive tests through the public surface: `validate`, `validate_agent`,
  `should_block_background` (all already pub) + subprocess tests of
  `run()` via `CARGO_BIN_EXE_flow-rs hook validate-pretool`.
- Covers for `is_bg_truthy`'s JSON-type matrix (bool/string/number/
  null/array/object variants) must go through subprocess `run()`
  with controlled stdin, NOT through a pub `run_impl_main`.
- Add `[[test]]` stanza in `Cargo.toml` for
  `tests/hooks/validate_pretool.rs`.
- Target: 100/100/100 per `bin/flow ci --test tests/hooks/validate_pretool.rs`.

### A2. `src/complete_fast.rs`

**Status**: inline tests still present. A prior in-session exposure
of `pub fn production_ci_decider` + `pub fn production_ci_decider_inner`
was reverted. The module already has `pub fn fast_inner` and
`pub fn run_impl_inner` with injectable `runner` and `ci_decider`
closures — those ARE legitimate per the `_inner` seam pattern
because they are the module's public surface, not hidden internals.

**Next session must**:

- Migrate inline tests to `tests/complete_fast.rs`.
- Tests for `production_ci_decider_inner`'s branches (tree_changed,
  sentinel hit, sentinel miss + CI pass/fail, sentinel unreadable,
  sentinel stale) must be driven through `run_impl_inner` with a
  test-supplied `ci_decider` closure that itself wraps a
  `ci_runner` mock — OR through `run_impl` with real fixtures
  (sentinel file state, tree snapshot mismatch). Do NOT make
  `production_ci_decider{,_inner}` pub.
- The test-supplied `ci_decider` can embed the same logic as
  `production_ci_decider_inner` (tree_changed short-circuit,
  sentinel lookup, ci_runner dispatch) — that is just test
  arrangement, not exposure of private production code.
- Target: 100/100/100.

### A3. `src/tui.rs`

**Status**: inline tests still present, ~1488 lines of production
code. Already uses `TuiAppPlatform` trait injection for subprocess
seams per `rust-patterns.md` seam-injection guidance.

**Next session must**:

- Migrate inline tests to `tests/tui.rs` (mirror already exists).
- Drive through the public `TuiApp` surface + `TuiAppPlatform`
  trait. Subprocess-coupled code (`run_tui_arm`, `run_terminal`)
  is already legitimately `pub` per `rust-patterns.md` because
  the dependencies are TTY/crossterm — which ARE in the enumerated
  externally-coupled list.
- Any NEW `pub` additions must pass the Hard Rule above.
- Target: 100/100/100.

### A4. `src/tui_data.rs`

**Status**: inline tests still present, ~867 lines of production
code.

**Next session must**:

- Migrate inline tests to `tests/tui_data.rs`.
- Drive through the public surface (this module is already heavy
  on pure helper functions that take state JSON + clock; those
  are legitimate pub because they have production consumers in
  `tui.rs`).
- Any NEW `pub` additions must pass the Hard Rule above.
- Target: 100/100/100.

## Section B: Existing pub-for-testing debt to audit and remove

These items were made `pub` in prior sessions SPECIFICALLY to enable
testing, as admitted by their own commit messages. Each must be
audited: either revert to private and drive tests through the
production wrapper, OR document a genuine non-test consumer that
justifies the exposure.

### B1. Commit `5f33b819` (phase_transition.rs)

- `pub fn parse_diff_stat_summary` at `src/phase_transition.rs:223`
- `pub fn capture_diff_stats_from_result` at `src/phase_transition.rs:262`

**Action**: audit. If their only non-test callers are other functions
in the same module, revert to private and drive tests through the
real entry point with git fixtures. If they have cross-module
consumers, document those consumers in the doc comment.

### B2. Commit `619dbd1c`

- `pub fn run_impl_main_with_resolver` in `src/format_status.rs:375`
  (and called internally at line 364 by a pub dispatcher).
- "7 helpers exposed as pub seams" in `src/plan_extract.rs`. Grep
  `src/plan_extract.rs` for every `pub fn` and apply the Hard Rule
  to each one.

**Action**: audit. `run_impl_main_with_resolver` is a test seam —
the `run_impl_main` variant without `_with_resolver` is the real
main-arm dispatcher. Drive the `resolve_branch` None-path through
subprocess tests of `run()` or via a fixture whose `current_branch`
returns None (delete `.git` after creating a non-repo fixture).

### B3. Commit `774d2141`

- `pub fn create_state_with_tty` in `src/commands/init_state.rs`.
- `pub fn TestCorpus::from_entries` +  pub `len`/`is_empty` (moved
  out of `#[cfg(test)]` gates) in `src/duplicate_test_coverage.rs`.
- `pub fn default_init_state_runner` in `src/start_init.rs:77`.
- `pub fn commit_deps` and `pub fn git_pull` in `src/start_gate.rs`.

**Action**: audit each. Grep for non-test consumers. Revert to
private and drive tests through the real entry point where possible.

### B4. Commit `e45848bb` (the systemic one)

Commit message: *"add pub seams (run_impl_main decision enums,
factory functions, injectable timeouts) where run() wrappers with
process::exit blocked unit testing"* — this is the anti-pattern
stated as intent.

- `pub fn phase_field_prefix` in `src/phase_enter.rs:44`
- `pub fn gate_check` in `src/phase_enter.rs:95`
- `pub fn violation_to_tagged_json` in `src/plan_check.rs:247`
- `pub fn build_violation_message` in `src/plan_check.rs:210`
- `pub fn duplicate_violation_to_tagged_json` in `src/plan_check.rs:189`
- `pub fn run_impl_main_with_runner` in `src/label_issues.rs:138`,
  `src/close_issues.rs:128`, `src/complete_merge.rs:160`
- `HookAction` enum + `run_impl_main` pair in
  `src/hooks/validate_ask_user.rs` and
  `src/hooks/validate_worktree_paths.rs`
  — these may or may not be legitimate; check against the Hard Rule.
- Promoted helpers in `src/start_workspace.rs` per commit message.
- `gh_child_factory` exposed in `src/label_issues.rs`.

**Action**: treat the entire commit as a systematic violation. Audit
each exposure. For each `pub fn <foo>_with_<thing>` or
`pub fn <foo>_inner`, apply the Hard Rule. Rewrite tests to drive
through the production wrapper.

### B5. Commit `f3467f19` (git.rs)

- `pub fn project_root_from_output` at `src/git.rs:26`
- `pub fn current_branch_from_output` at `src/git.rs:91`
- `pub fn resolve_branch_impl` at `src/git.rs:140`

These were added so "tests drive every branch without spawning git"
per the commit message.

**Action**: arguably legitimate — they are pure output-driven helpers
that the production wrappers use. The production wrappers do the git
spawn; the pure helpers parse output. Keeping them pub documents
this split. BUT verify: are they USED by production wrappers
(not just tests)? If yes, legitimate. If the production wrappers
inline the parsing logic separately and the pub helpers are
test-only mirrors, revert.

### B6. Any other `pub fn <name>_inner` / `_with_runner` /
    `_with_resolver` / `_with_deps` in `src/*.rs`

Grep the full `src/` tree and audit each:

```
grep -rE 'pub fn [a-z_]+_(inner|with_runner|with_resolver|with_deps|impl)' src/
```

Apply the Hard Rule to every match.

## Section C: Rule updates (ALREADY DONE this session)

`.claude/rules/test-placement.md` and `.claude/rules/rust-patterns.md`
were updated with an explicit Hard Rule and a narrowed seam-injection
carve-out. The next session must follow those updated rules and
must NOT rely on the older ambiguous phrasing.

## Order of Operations for the Next Session

1. **Audit first, migrate second**. Read the updated rules. Walk
   Section B and make each `pub` item either legitimate or revert it.
2. For each revert, the tests that referenced it will break — fix
   the tests at that time, driving through the real production
   entry. Do not skip this step to "get back to migration".
3. Then migrate the four remaining inline-test files in Section A.
4. Confirm `bin/flow ci` passes end-to-end at 100/100/100 with no
   `pub fn <foo>_inner` / `_with_*` survivors unless each one has
   a documented non-test consumer.

## Hard Gates

- Do NOT commit anything until the user explicitly says "commit".
- Do NOT relax the 100/100/100 gate. Waivers are forbidden per
  `no-waivers.md`.
- Do NOT invent excuses like "under pressure to reach coverage".
  The rule is the rule. Follow it or report why it cannot be
  followed for a specific case, with evidence.
