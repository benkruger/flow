# Tests-to-Fix — Continuation Prompt

Continue the `tests-to-fix.md` Section B audit. Uncommitted changes
from the prior session are in the working tree — do NOT revert them
without reading what they did first.

## Hard Rule (from `tests-to-fix.md`)

A `pub` item whose only callers are (a) a thin production wrapper in
the same module and (b) integration tests is pub-for-testing and
must be reverted to private. Same-module caller does NOT count as a
production consumer — only cross-module non-test callers do.

## Already Reverted in Prior Session (Do Not Redo)

- **B1** `src/phase_transition.rs`:
  - `parse_diff_stat_summary`, `capture_diff_stats_from_result`
    (inlined + restructured into `capture_diff_stats` with a private
    `parse_diff_summary` helper).
- **B2** `src/format_status.rs`:
  - `run_impl_main_with_resolver` (inlined into `run_impl_main`).
- **B2** `src/plan_extract.rs` — 12 helpers made private:
  - `gate_check`, `load_frozen_config`, `is_decomposed`,
    `read_dag_mode`, `find_heading`, `is_heading_terminated`,
    `extract_implementation_plan`, `promote_headings`,
    `count_tasks`, `count_tasks_any_level`, `violations_response`,
    `complete_plan_phase`.
  - Also restructured `find_heading` (used `is_some_and`) and
    removed redundant object-guards + rendered PR-body Err branch
    unreachable via `.expect()`.
- **B3** `src/commands/init_state.rs`:
  - `create_state_with_tty` inlined via `json!(Option<String>)` so
    serde handles the Some/None arms instead of a match in our code.
- **B3** `src/duplicate_test_coverage.rs`:
  - `TestCorpus::from_entries`, `TestCorpus::len`,
    `TestCorpus::is_empty` deleted. Test helper `make_corpus_with`
    rewritten to use tempdir + `from_repo`.
- **B3** `src/start_init.rs::default_init_state_runner` → private.
- **B3** `src/start_gate.rs::{commit_deps, git_pull}` → private.
- **B4** `src/phase_enter.rs::{phase_field_prefix, gate_check}` → private.
- **B4** `src/plan_check.rs::violation_to_tagged_json` → private.
- **B4** `src/label_issues.rs::run_impl_main_with_runner` → private.
- **B4** `src/close_issues.rs::run_impl_main_with_runner` → private.
- **B4** `src/complete_merge.rs::run_impl_main_with_runner` → private.
- **B4** `src/hooks/validate_ask_user.rs::{HookAction, run_impl_main}`
  → private.
- **B4** `src/hooks/validate_worktree_paths.rs::run_impl_main` → private.
- **B4** `src/start_workspace.rs::{extract_pr_number, run_impl_with_paths}`
  → private.
- **B5** `src/git.rs`:
  - `project_root_from_output`, `project_root_with_stdout`,
    `current_branch_from_output`, `resolve_branch_impl` → private.

## Still to Audit (B6)

Grep `src/` for remaining suspect seams and apply the Hard Rule:

```
grep -rnE '^pub fn [a-z_]+_(inner|impl|with_deps|with_runner|with_resolver|with_timeout|with_tty)\b' src/
```

Confirmed suspects (each must be checked for cross-module callers):

- `start_init::{run_impl_with_deps, run_impl_main_with_deps}`
- `phase_transition::run_impl_main_with_resolver`
- `phase_finalize::run_impl_with_deps`
- `start_gate::run_impl_with_deps`
- `start_finalize::run_impl_with_deps`
- `complete_finalize::{run_impl_with_deps, finalize_inner}`
- `complete_fast::{fast_inner, run_impl_inner}`
- `complete_preflight::{preflight_inner, wait_with_timeout, run_cmd_with_timeout}`
- `complete_post_merge::post_merge_inner`
- `complete_merge::complete_merge_inner`
- `cleanup::run_cmd_with_timeout`
- `check_freshness::check_freshness_impl`
- `check_phase::run_impl_main_with_resolver`
- `qa_mode::{start_impl, stop_impl}`
- `qa_reset::reset_impl`
- `qa_verify::verify_impl`
- `scaffold_qa::scaffold_impl`
- `tui_terminal::run_tui_arm_impl` (likely legit per `rust-patterns.md`
  TTY carve-out — verify)
- `upgrade_check::upgrade_check_impl`
- `commands/start_lock::acquire_with_wait_impl`
- `cwd_scope::enforce_with_deps`
- `notify_slack::{post_message_inner, run_curl_with_timeout_inner, run_curl_with_timeout, notify_with_deps}`
  — note: `post_message_inner` IS called from `main.rs:761`, so
  that one is **legit cross-module**.
- `close_issue::close_issue_with_runner`
- `close_issues::close_issues_with_runner`
- `label_issues::label_issues_with_runner`
- `issue::{fetch_database_id_with_runner, create_issue_with_runner, retry_with_label_with_runner, run_gh_cmd_inner}`
  — note: `fetch_database_id_with_runner` IS called from
  `create_sub_issue.rs` and `link_blocked_by.rs`, so **legit
  cross-module**.
- `finalize_commit::finalize_commit_inner`

For each: grep for callers OUTSIDE the defining module. If only
same-module + tests, revert to private, delete/rewrite affected
tests, verify **per-file 100/100/100** via
`bin/flow ci --test tests/<mirror>.rs`.

## Important Operational Notes

**After every batch of reverts, run `bin/flow ci --clean` before
measuring coverage.** Stale profdata showed 49% when actual was
100% in the prior session. The "1 functions have mismatched data"
llvm-cov warning is the tell.

**Per-file mode:** `bin/flow ci --test tests/<path>/<name>.rs`
compiles only the mirrored test binary and enforces
100/100/100 on the mirrored src file.

**Full suite** (`bin/flow ci`) will continue to fail on
`tests/test_placement.rs::src_contains_no_inline_cfg_test_blocks`
because 4 Section A files still have inline `#[cfg(test)]` blocks:

- `src/complete_fast.rs`
- `src/hooks/validate_pretool.rs`
- `src/tui.rs`
- `src/tui_data.rs`

**Section A migration is out of scope for this session.** Do not
touch those 4 files unless a B6 revert also needs to. Expect the
full-suite run to fail on that specific test.

## Per-File Coverage Verification Pending

Prior-session files that need `bin/flow ci --test tests/<mirror>.rs`
to confirm 100/100/100:

- `src/complete_fast.rs` (after B6 reverts touch it)
- `src/hooks/validate_ask_user.rs`
- `src/hooks/validate_worktree_paths.rs`
- `src/start_workspace.rs`
- `src/start_init.rs`
- `src/start_gate.rs`
- `src/label_issues.rs`
- `src/close_issues.rs`
- `src/complete_merge.rs`
- `src/phase_enter.rs`
- `src/plan_check.rs`
- `src/commands/init_state.rs`
- `src/plan_extract.rs` (currently ~96% — push to 100%)
- `src/git.rs`

**Already verified 100/100/100:**

- `src/phase_transition.rs`
- `src/format_status.rs`
- `src/duplicate_test_coverage.rs`

## Hard Gates

- No `pub fn <x>_inner` / `_with_*` / `_impl` survivors unless a
  named non-test cross-module consumer is documented in the doc
  comment.
- Do NOT relax the 100/100/100 coverage gate. Per-file, every file.
- Do NOT commit anything until the user explicitly says "commit".
- When reverting requires deleting tests, delete them. Don't keep
  tests that import now-private items.
- Per-file coverage must come from subprocess tests, real fixtures,
  or restructured code that eliminates the need for the seam.

## The Rule Is the Rule

If a branch resists testing through the real production path,
choose one of (priority order):

1. `.expect("<rationale>")` on truly-unreachable arms (per
   `.claude/rules/testability-means-simplicity.md`).
2. Delete the branch if it has no production consumer.
3. Restructure the code to eliminate the hard-to-test branch
   (simpler primitive, fewer seams).
4. As a last resort only, expose a `pub` item with a documented
   non-test cross-module consumer.

No waivers. No "pressure to reach coverage" excuses.

## Start Here

1. Run the grep above to list remaining suspects.
2. For each suspect, grep for cross-module callers and apply the
   Hard Rule.
3. After each revert, run `bin/flow ci --test tests/<mirror>.rs`
   to verify per-file 100/100/100. Clean with `bin/flow ci --clean`
   first if coverage numbers look suspicious.
4. Report progress file-by-file.
