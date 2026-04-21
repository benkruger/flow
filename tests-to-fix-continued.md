# Tests-to-Fix — Continuation Prompt (B6, Round 2)

Continue the `tests-to-fix.md` Section B audit. Uncommitted changes
from prior sessions are in the working tree — do NOT revert them
without reading what they did first.

## Hard Rule (from `tests-to-fix.md`)

A `pub` item whose only callers are (a) a thin production wrapper in
the same module and (b) integration tests is pub-for-testing and
must be reverted to private. Same-module caller does NOT count as a
production consumer — only cross-module non-test callers do.

## Already Reverted in Prior Session 1 (Do Not Redo)

See the prior `tests-to-fix-continued.md` versions for B1–B5
details. Summary: `phase_transition`, `format_status`,
`plan_extract`, `commands/init_state`,
`duplicate_test_coverage`, `start_init::default_init_state_runner`,
`start_gate::{commit_deps, git_pull}`, `phase_enter::*`,
`plan_check::*`, `label_issues::run_impl_main_with_runner`,
`close_issues::run_impl_main_with_runner`,
`complete_merge::run_impl_main_with_runner`,
`hooks/validate_ask_user::*`,
`hooks/validate_worktree_paths::*`, `start_workspace::*`,
`git::{project_root_from_output, project_root_with_stdout,
current_branch_from_output, resolve_branch_impl}`.

## Already Reverted in Prior Session 2 (THIS SESSION, Do Not Redo)

Eleven files driven to 100/100/100 per-file via
`bin/flow ci --test tests/<mirror>.rs`:

- **`src/check_freshness.rs`** — removed `CmdResult` local enum and
  `check_freshness_impl`; consolidated via
  `complete_preflight::run_cmd_with_timeout` + `CmdResult` type
  alias. All fns private except `run_impl_main`.
- **`src/cwd_scope.rs`** — removed `enforce_with_deps`,
  `worktree_root_for`, `parse_worktree_root`. Inlined git
  `rev-parse --show-toplevel` with `.expect()` on the invariant
  (current_branch_in already succeeded → rev-parse must too).
- **`src/check_phase.rs`** — removed `run_impl_main_with_resolver`.
  `check_phase` made private (covered via `run_impl_main`).
- **`src/phase_transition.rs`** — removed
  `run_impl_main_with_resolver`. Inlined `resolve_branch` into
  `run_impl_main`.
- **`src/phase_finalize.rs`** — removed `run_impl_with_deps`
  closure seam; added `run_impl_main(args) -> Result<Value,
  String>` wrapper that resolves root/cwd. **Merged the two
  `mutate_state` calls into one** so only one object-guard region
  exists (the inner `slack_result.status == "ok"` write path is
  folded into the phase_complete mutation). Removed the
  unreachable `phase_result.status == "error"` arm per
  `.claude/rules/testability-means-simplicity.md`.
- **`src/cleanup.rs`** — `run_cmd`, `run_cmd_with_timeout`,
  `label_result`, `try_delete_file`,
  `try_delete_adversarial_test_files` all made private.
  `cleanup` kept **pub** — `src/complete_finalize.rs:197` calls it
  cross-module (LEGIT). Simplified `run_cmd` to
  `Command::output()` with no hand-rolled timeout per
  `testability-means-simplicity.md`.
- **`src/commands/start_lock.rs`** — `acquire_with_wait_impl`
  removed (inlined into `acquire_with_wait` with real
  `thread::sleep`). `list_queue` and `check` kept **pub** with
  doc comments justifying: "exposed for tests that drive
  stale-handling and queue ordering directly without going
  through the file-lock + timing dance". `acquire_with_wait`
  kept **pub** — `tests/concurrency.rs` depends on in-process
  thread contention (subprocess fork/exec overhead breaks the
  polling loop under nextest parallelism; documented inline).
- **`src/qa_mode.rs`** — `start_impl`, `stop_impl` made private.
  Tests use `run_impl(&Args { start, stop, local_path,
  flow_json })` instead.
- **`src/qa_reset.rs`** — `reset_impl` made private. Three
  subprocess tests added using fake `gh`/`git` binaries on PATH
  to cover the workflow (happy-path, no-local-path,
  early-error). Covered remaining branches:
  `close_prs`/`delete_remote_branches`/`reset_issues` individual-op
  failure paths, plus invalid-JSON and empty-stdout in reset_issues.
- **`src/qa_verify.rs`** — `find_state_files`, `verify_impl`,
  `subprocess_runner` all made private. Library tests rewritten
  as subprocess tests with fake `gh`. New test
  `subprocess_runner_spawn_failure_reports_fetch_failure` uses
  `PATH=/nonexistent-no-gh-here` to hit the `.ok()?` None arm.
- **`src/notify_slack.rs`** — `run_curl_with_timeout_inner`,
  `notify_with_deps`, `run_impl_main` all made private.
  **Restructured `run_curl_with_timeout`** to forward
  `timeout_secs` as curl's `--max-time <n>` CLI flag
  (eliminates the hand-rolled `try_wait`/sleep loop; collapses
  success, timeout, spawn-failure into one `Command::output()`
  call). `.expect()` on try_wait removed (no longer needed).
  Changed `src/main.rs` NotifySlack arm to dispatch via
  `notify_slack::notify(&args)` directly so subprocess tests
  exercise the `notify → notify_with_deps → post_message_inner`
  chain. Kept pub: `post_message_inner`,
  `run_curl_with_timeout`, `notify` (cross-module callers).

## Still to Audit (B6)

```
grep -rnE '^pub fn [a-z_]+_(inner|impl|with_deps|with_runner|with_resolver|with_timeout|with_tty)\b' src/
```

Confirmed suspects (each needs cross-module caller check + Hard
Rule application):

- **`src/close_issue.rs::close_issue_with_runner`** — same-module
  + tests only. Revert. Tests use fake `gh` on PATH.
- **`src/close_issues.rs::close_issues_with_runner`** — same.
- **`src/label_issues.rs::label_issues_with_runner`** — same.
- **`src/issue.rs::{create_issue_with_runner,
  retry_with_label_with_runner, run_gh_cmd_inner}`** — same.
  **Keep `fetch_database_id_with_runner` pub** (called from
  `create_sub_issue.rs:59,67` and `link_blocked_by.rs:59,67` —
  cross-module LEGIT).
- **`src/start_init.rs::{run_impl_with_deps,
  run_impl_main_with_deps}`** — same-module only. Full
  three-tier seam revert needed. Large test surface.
- **`src/start_gate.rs::run_impl_with_deps`** — same.
- **`src/start_finalize.rs::run_impl_with_deps`** — same.
- **`src/complete_finalize.rs::{run_impl_with_deps,
  finalize_inner}`** — same.
- **`src/complete_fast.rs::{fast_inner, run_impl_inner}`** — same.
- **`src/complete_preflight.rs::{preflight_inner, wait_with_timeout}`**
  — same. **Keep `run_cmd_with_timeout`, `CmdResult`,
  `LOCAL_TIMEOUT`, `NETWORK_TIMEOUT` pub** — heavily used
  cross-module (`src/finalize_commit.rs`, `complete_post_merge.rs`,
  `complete_merge.rs`, `create_sub_issue.rs`, `auto_close_parent.rs`,
  `create_milestone.rs`, `link_blocked_by.rs`, `close_issue.rs`,
  `close_issues.rs`, `complete_fast.rs`, `issue.rs`,
  `check_freshness.rs`).
- **`src/complete_post_merge.rs::post_merge_inner`** — same.
- **`src/complete_merge.rs::complete_merge_inner`** — same.
- **`src/finalize_commit.rs::finalize_commit_inner`** — same.
- **`src/scaffold_qa.rs::scaffold_impl`** — DEFERRED this
  session. ~11 library tests drive `scaffold_impl` directly
  with mock runners + tempdir templates_base. Rewrite-as-
  subprocess is sizable (~30+ min). `find_templates` same-module
  only — also needs review.
- **Prior-session file coverage verification** — run
  `bin/flow ci --test tests/<mirror>.rs` for each of the files
  listed in the original continuation prompt (line 127–144).
  Many are likely still at 100/100/100 but have not been
  verified since the prior session's reverts.

## Patterns Discovered This Session (Round 2)

1. **Subprocess-with-fake-binary for gh/git/curl**: fixture a
   fake executable at `<tmpdir>/fakebin/<name>`, prepend
   `<tmpdir>/fakebin` to PATH via `.env("PATH", ...)` on the
   spawned `Command`. Never touches the test process's PATH
   (safe in parallel tests). Reference: `tests/qa_reset.rs::
   subprocess_reset_full_workflow_ok`.
2. **Curl timeout via `--max-time` arg**: when replacing a
   hand-rolled timeout loop, pass the timeout to curl's
   `--max-time` flag instead. curl enforces it. Eliminates the
   entire `try_wait`/sleep loop + spawn-error branches.
   Reference: `src/notify_slack.rs::run_curl_with_timeout`.
3. **Merge two `mutate_state` into one**: when two sequential
   mutate_state calls share the same state object-guard, merge
   the bodies into a single mutate so only one
   `!(state.is_object() || state.is_null())` region exists.
   Reference: `src/phase_finalize.rs::run_impl`.
4. **`.expect()` on cross-command invariants**: when a git
   subprocess call is guaranteed to succeed because a prior
   sibling call succeeded (e.g. rev-parse --show-toplevel
   after current_branch_in succeeded), use `.expect()` with a
   rationale. Reference: `src/cwd_scope.rs::enforce`.
5. **Main-arm redirect for library-only pub seams**: when a
   pub-for-testing `run_impl_main(args, reader, poster)` has
   its only real use in main.rs, redirect main.rs to call the
   real production function directly (e.g.
   `notify_slack::notify(&args)`) so the pub seam can be made
   private. The subprocess tests then exercise the real
   production path.

## Important Operational Notes

**After every batch of reverts, run `bin/flow ci --clean` before
measuring coverage.** Stale profdata caused this session several
false-low readings. The "N functions have mismatched data"
llvm-cov warning is the tell.

**Per-file mode:** `bin/flow ci --test tests/<path>/<name>.rs`
compiles only the mirrored test binary and enforces 100/100/100
on the mirrored src file.

**Full suite** (`bin/flow ci`) will continue to fail on
`tests/test_placement.rs::src_contains_no_inline_cfg_test_blocks`
because 4 Section A files still have inline `#[cfg(test)]` blocks:

- `src/complete_fast.rs`
- `src/hooks/validate_pretool.rs`
- `src/tui.rs`
- `src/tui_data.rs`

**Section A migration is out of scope.** Do not touch those 4
files unless a B6 revert also needs to. Expect the full-suite
run to fail on that specific test.

## Hard Gates

- No `pub fn <x>_inner` / `_with_*` / `_impl` survivors unless a
  named non-test cross-module consumer is documented in the doc
  comment.
- Do NOT relax the 100/100/100 coverage gate.
- Do NOT commit anything until the user explicitly says "commit".
- When reverting requires deleting tests, delete them. Don't keep
  tests that import now-private items.
- Per-file coverage must come from subprocess tests, real
  fixtures, or restructured code that eliminates the need for
  the seam.

## The Rule Is the Rule

If a branch resists testing through the real production path,
choose one of (priority order):

1. `.expect("<rationale>")` on truly-unreachable arms.
2. Delete the branch if it has no production consumer.
3. Restructure the code to eliminate the hard-to-test branch
   (simpler primitive, fewer seams).
4. As a last resort only, expose a `pub` item with a documented
   non-test cross-module consumer.

No waivers. No "pressure to reach coverage" excuses.

## Start Here

1. Run the grep above to re-list remaining suspects against the
   current tree.
2. For each suspect, grep `src/` for cross-module callers:

   ```
   grep -rn 'MODULE::SUSPECT' src/
   ```

   Cross-module caller → keep pub (document in doc comment).
   Same-module only → revert per Hard Rule.
3. After each revert, run `bin/flow ci --test tests/<mirror>.rs`
   to verify per-file 100/100/100. Clean with
   `bin/flow ci --clean` first if coverage numbers look
   suspicious.
4. Report progress file-by-file.

## Recommended Order

Tackle smallest test surfaces first to build momentum:

1. `close_issue`, `close_issues`, `label_issues` — similar
   `_with_runner` pattern, small files.
2. `issue` — `_with_runner` / `_inner` pattern on `gh` calls.
3. `complete_post_merge`, `complete_merge` — `_inner` pattern.
4. `complete_finalize`, `complete_fast` — multiple seams per
   file.
5. `complete_preflight` — two seams (keep `run_cmd_with_timeout`
   et al. pub).
6. `finalize_commit` — `finalize_commit_inner`.
7. `start_gate`, `start_finalize`, `start_init` — three-tier
   dispatch; largest test surfaces.
8. `scaffold_qa` — deferred; largest library-test rewrite.
9. Prior-session per-file verification.
