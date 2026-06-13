//! Commit, cleanup, pull, push.
//!
//! Routing. Every git operation inside `run_impl` (working-tree-dirty
//! check, CI sub-invocation, plan-deviation staged diff, the commit-
//! pull-push sequence, and the tree-snapshot used to refresh the CI
//! sentinel) runs against `commit_cwd`, which is resolved from git's
//! actual checkout location for the explicit `<branch>` argument via
//! `crate::git::resolve_worktree_for_branch` (`git worktree list
//! --porcelain`). This decouples the commit destination from the
//! caller's process cwd AND from the branch name: a branch checked
//! out at the project root commits at the root, a branch in a linked
//! worktree commits in that worktree, and a branch checked out
//! nowhere (or a git failure) returns a `resolve_cwd` error before
//! any other git operation runs rather than committing to a guessed
//! `<project_root>/.worktrees/<branch>/` path that may not exist.
//!
//! Two gates and a post-CI re-stage run before committing:
//!
//! 1. **CI gate** — calls [`ci::run_impl`]. If CI fails, returns an error
//!    and commits nothing. When the CI sentinel is fresh (CI already passed
//!    for this tree state), the check noops instantly.
//! 2. **Post-CI re-stage** — runs `git add -u` after CI completes. Project
//!    `bin/*` tools run in their default auto-fix mode during the commit-
//!    time CI gate (CI=true is not set), so formatters like `ruff format`
//!    and `prettier --write` modify tracked files in place. Re-staging
//!    captures those modifications in the index so the commit records the
//!    same bytes CI validated, and the plan-deviation gate inspects identical
//!    content. `git add -u` updates already-tracked files only — it does
//!    NOT sweep untracked files (commit-message files, scratch artifacts,
//!    CI outputs the user has not yet `.gitignore`d), so the commit's
//!    scope stays bounded to what the user staged in `/flow:flow-commit`
//!    Round 3 plus any in-place modifications CI made to those tracked
//!    files. Returns `step = "restage"` on `git add` failure.
//! 3. **Plan-deviation gate** — calls [`plan_deviation::run_impl`] to
//!    cross-reference plan-named test fixture values against the staged
//!    diff. If an unacknowledged drift is detected, returns an error with
//!    `step = "plan_deviation"` and a structured stderr message.
//!
//! Usage:
//!   bin/flow finalize-commit <branch>
//!
//! The commit-message file is NOT a caller-supplied argument. After
//! `commit_cwd` resolves, the message file is always
//! `<commit_cwd>/.flow-commit-msg` — `/flow:flow-commit` (and the
//! flow-start / flow-release bootstrap paths) write it there before
//! invoking this subcommand. A missing or empty file at that path
//! returns a `message_file_missing` error. The file is deleted on
//! every exit reached after `commit_cwd` resolution.
//!
//! Output (JSON to stdout):
//!   Success:   {"status": "ok", "sha": "<commit-hash>", "pull_merged": <bool>}
//!   Conflict:  {"status": "conflict", "files": ["file1.py", ...]}
//!   Error:     {"status": "error", "step": "resolve_cwd|message_file_missing|working_tree_dirty|ci|restage|plan_deviation|commit|pull|push", "message": "..."}
//!              ("resolve_cwd" also carries "reason": "branch_not_checked_out" when the branch is checked out nowhere)

use std::fs;
use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::complete_preflight::{run_cmd_with_timeout, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::flow_paths::FlowPaths;
use crate::lock::mutate_state;
use crate::phase_config::phase_number;
use crate::plan_deviation::Deviation;
use crate::utils::parse_conflict_files;

#[derive(Parser, Debug)]
#[command(
    name = "finalize-commit",
    about = "Finalize a commit: commit, cleanup, pull, push"
)]
pub struct Args {
    /// Branch name — resolves the commit destination and the
    /// `<commit_cwd>/.flow-commit-msg` message-file path.
    pub branch: String,
}

/// Remove the commit message file, ignoring errors.
fn remove_message_file(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Print a user-facing block message for unacknowledged plan
/// signature deviations. Each deviation shows the plan file
/// line, the fixture key, and the plan value that is missing
/// from the staged test body. The trailing section lists the
/// `bin/flow log` template the user runs to acknowledge each
/// deviation before re-running the commit.
fn emit_deviation_stderr(branch: &str, deviations: &[Deviation]) {
    eprintln!("BLOCKED: Plan signature deviation detected.");
    eprintln!();
    for dev in deviations {
        eprintln!("Test: {}", dev.test_name);
        eprintln!(
            "  Plan value (line {}): {} = \"{}\"",
            dev.plan_line, dev.fixture_key, dev.plan_value
        );
        eprintln!(
            "  Staged diff does not contain \"{}\" in the test body.",
            dev.plan_value
        );
        eprintln!();
    }
    eprintln!("If this deviation is intentional, log it before committing:");
    eprintln!();
    for dev in deviations {
        eprintln!(
            "  bin/flow log {} \"[Phase 3] Plan signature deviation: {} drifted from {} to <new value>. Reason: <why>\"",
            branch, dev.test_name, dev.plan_value
        );
    }
    eprintln!();
    eprintln!("Then re-run the commit.");
}

/// Prepend `git -C <cwd>` to args and invoke via the shared timeout-aware
/// runner. Avoids `set_current_dir` (process-wide; races in parallel tests).
fn run_git_in_dir(
    cwd: &std::path::Path,
    args: &[&str],
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    let mut cmd_args = vec!["git", "-C", cwd.to_str().unwrap_or(".")];
    cmd_args.extend_from_slice(args);
    run_cmd_with_timeout(&cmd_args, timeout_secs)
}

/// Core commit → pull → push sequence. Private — drives through the
/// compiled `bin/flow finalize-commit` binary for testing via real git
/// fixtures and stubs. No closure-injection seam.
///
/// `commit_cwd` is the checkout path `run_impl` resolved from git for
/// the branch argument via `crate::git::resolve_worktree_for_branch`.
/// Every git operation here runs against it, never against the
/// caller's process cwd.
///
/// All git calls `.expect()` on Ok because `resolve_worktree_for_branch`
/// (the first git call in `run_impl`) already proved git is alive by
/// the time this function runs — a missing git binary would have
/// emitted `step: "resolve_cwd"` and returned before reaching here.
/// `commit` still has a non-zero exit branch (the index produces an
/// empty commit, the user's identity isn't configured, etc.); pull and
/// push retain non-zero branches for legit error cases (merge conflict,
/// push rejected) per `.claude/rules/testability-means-simplicity.md`.
///
/// The message file is NOT deleted here. `run_impl` deletes it once at
/// a tail position so every post-resolution exit (working-tree-dirty,
/// CI failure, restage failure, plan-deviation block) — none of which
/// reach this function — also disposes of the file.
fn finalize_commit(message_file: &Path, branch: &str, commit_cwd: &Path) -> Value {
    // Step 1: git commit -F <message_file>
    let message_file_str = message_file
        .to_str()
        .expect("commit-msg path is valid UTF-8 (commit_cwd + ASCII basename)");
    let (code, _, stderr) = run_git_in_dir(
        commit_cwd,
        &["commit", "-F", message_file_str],
        LOCAL_TIMEOUT,
    )
    .expect("git located by working_tree_dirty gate");
    if code != 0 {
        return json!({
            "status": "error",
            "step": "commit",
            "message": stderr.trim()
        });
    }

    // Capture post-commit SHA for pull_merged detection. After a successful
    // commit, git is alive and HEAD exists — both Err and non-zero branches
    // would be unreachable defensive code.
    let (_, post_stdout, _) = run_git_in_dir(commit_cwd, &["rev-parse", "HEAD"], LOCAL_TIMEOUT)
        .expect("git located by commit call");
    let post_commit_sha = post_stdout.trim().to_string();

    // Step 2: git pull origin <branch>
    let (pull_code, _, pull_stderr) =
        run_git_in_dir(commit_cwd, &["pull", "origin", branch], NETWORK_TIMEOUT)
            .expect("git located by commit call");
    if pull_code != 0 {
        let (_, status_stdout, _) =
            run_git_in_dir(commit_cwd, &["status", "--porcelain"], LOCAL_TIMEOUT)
                .expect("git located by commit call");
        let conflicts = parse_conflict_files(&status_stdout);
        if !conflicts.is_empty() {
            return json!({"status": "conflict", "files": conflicts});
        }
        return json!({
            "status": "error",
            "step": "pull",
            "message": pull_stderr.trim()
        });
    }

    // Step 3: git push
    let (push_code, _, push_stderr) =
        run_git_in_dir(commit_cwd, &["push"], NETWORK_TIMEOUT).expect("git located by commit call");
    if push_code != 0 {
        return json!({
            "status": "error",
            "step": "push",
            "message": push_stderr.trim()
        });
    }

    // Step 4: final rev-parse HEAD
    let (_, final_stdout, _) = run_git_in_dir(commit_cwd, &["rev-parse", "HEAD"], LOCAL_TIMEOUT)
        .expect("git located by commit call");
    let final_sha = final_stdout.trim();
    let pull_merged = post_commit_sha != final_sha;
    json!({"status": "ok", "sha": final_sha, "pull_merged": pull_merged})
}

/// Testable entry point: enforces CI, runs finalize-commit, then maintains
/// the CI sentinel (refresh on clean pull, delete on merge-pull).
///
/// `root` is the project root passed explicitly so integration tests can
/// avoid `set_current_dir` (which is process-wide and races with parallel
/// tests). The git working directory for every operation here —
/// working-tree-dirty check, CI sub-invocation, staged-diff capture,
/// commit-pull-push sequence, and the sentinel tree snapshot — is
/// `commit_cwd`, resolved from git's checkout location for `<args.branch>`
/// via `crate::git::resolve_worktree_for_branch`. The caller's process
/// cwd does not influence the commit destination.
///
/// Returns `Result<Value, String>` where `Ok` carries any JSON response
/// including status-error payloads (CI failure, commit failure, etc.) and
/// `Err` carries only infrastructure errors (empty arguments).
pub fn run_impl(args: &Args, root: &std::path::Path) -> Result<Value, String> {
    if args.branch.is_empty() {
        return Err("Usage: bin/flow finalize-commit <branch>".to_string());
    }

    // `args.branch` is clap-supplied — external input. Validate up
    // front per `.claude/rules/external-input-validation.md` "CLI
    // subcommand entry callsite discipline" so the two downstream
    // path constructions in this function can rely on a known-valid
    // branch.
    let paths = match FlowPaths::try_new(root, &args.branch) {
        Some(p) => p,
        None => {
            return Ok(json!({
                "status": "error",
                "message": format!("Invalid branch name: {:?}", &args.branch),
            }));
        }
    };

    // Commit destination resolved from git's actual checkout location,
    // not inferred from the branch name. `git worktree list --porcelain`
    // reports where `branch` is checked out: the project root for a
    // trunk or feature-at-root checkout, or a linked worktree path.
    // Resolving from git — rather than assuming
    // `<root>/.worktrees/<branch>` — handles a feature branch checked
    // out at the repo root, where the assumed worktree path does not
    // exist and the working-tree-dirty gate below would otherwise
    // refuse the commit. A git failure (`Err`) or a branch checked out
    // nowhere (`Ok(None)`) returns a structured `resolve_cwd` error
    // before any other git operation runs — never a fail-open default
    // path. Every downstream git, CI, and snapshot call binds to
    // `commit_cwd` rather than the caller's process cwd.
    //
    // The Layer 10 commit-gate hook is unaffected by this resolution:
    // it decides *whether* to block a trunk commit via a pure
    // `branch == integration` comparison
    // (`crate::flow_paths::finalize_commit_destination`), a question
    // independent of the physical commit destination. A trunk commit
    // is, per git, checked out at the root, so the binary commits
    // exactly where the hook gates it; a feature branch is never the
    // integration branch, so committing where git has it checked out
    // can never be a disguised trunk commit.
    let commit_cwd = match crate::git::resolve_worktree_for_branch(root, &args.branch) {
        Ok(Some(path)) => path,
        Ok(None) => {
            return Ok(json!({
                "status": "error",
                "step": "resolve_cwd",
                "reason": "branch_not_checked_out",
                "message": format!(
                    "Branch {:?} is not checked out in any worktree; cannot determine the commit destination.",
                    &args.branch
                ),
            }));
        }
        Err(msg) => {
            return Ok(json!({
                "status": "error",
                "step": "resolve_cwd",
                "message": msg,
            }));
        }
    };

    // The commit-message file path is derived from `commit_cwd`, not
    // supplied by the caller. `/flow:flow-commit` (and the flow-start /
    // flow-release bootstrap paths) write `<commit_cwd>/.flow-commit-msg`
    // before invoking this subcommand. A missing, empty, or
    // whitespace-only file is a skill-choreography error, surfaced as
    // `message_file_missing` BEFORE any other gate so the caller sees a
    // precise reason rather than a downstream `git commit` failure (git
    // rejects an all-whitespace message under the default
    // `--cleanup=strip`). The byte scan is encoding-agnostic — a
    // non-UTF-8 message carrying real content still counts as present.
    // This early return precedes binding any deletion target, so there
    // is nothing to clean up.
    let message_file = commit_cwd.join(".flow-commit-msg");
    let message_present = fs::read(&message_file)
        .map(|b| b.iter().any(|c| !c.is_ascii_whitespace()))
        .unwrap_or(false);
    if !message_present {
        return Ok(json!({
            "status": "error",
            "step": "message_file_missing",
            "message": format!(
                "commit message file not found (or empty/whitespace-only) at {}; \
                 /flow:flow-commit writes it before invoking finalize-commit",
                message_file.display()
            ),
        }));
    }

    // Derive phase number from state file's current_phase for log prefixes.
    let pn = {
        let state_path = paths.state_file();
        std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|c| serde_json::from_str::<Value>(&c).ok())
            .and_then(|s| {
                s.get("current_phase")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .map(|p| phase_number(&p))
            .unwrap_or(0)
    };

    // Working-tree-dirty gate. CI tools read the working tree, but
    // `git commit -F` commits the index. When working tree != index,
    // CI tests one set of bytes and `git commit` commits another.
    // Refuse the commit when `git diff` (working tree vs index) is
    // non-empty so the user makes an explicit choice: `git add` to
    // commit the working-tree state or `git restore <file>` to drop
    // unstaged edits. See `.claude/rules/plan-commit-atomicity.md`.
    //
    // `resolve_worktree_for_branch` above already proved git is alive
    // and `commit_cwd` is a real worktree git reported, so this
    // `git diff --quiet` cannot fail to spawn — `.expect()` per
    // `.claude/rules/testability-means-simplicity.md` (a missing git
    // binary surfaces as `resolve_cwd` and returns before reaching
    // here).
    let (diff_code, _, _) = run_git_in_dir(&commit_cwd, &["diff", "--quiet"], LOCAL_TIMEOUT)
        .expect("git located by resolve_worktree_for_branch");
    let working_tree_dirty = diff_code != 0;

    // Enforce CI before committing. run_impl checks the sentinel first —
    // if CI already passed for this tree state, it noops instantly.
    let result = if working_tree_dirty {
        let _ = append_log(
            root,
            &args.branch,
            &format!(
                "[Phase {}] finalize-commit — working_tree_dirty (blocked)",
                pn
            ),
        );
        json!({
            "status": "error",
            "step": "working_tree_dirty",
            "message": "Working tree has unstaged changes. Either `git add` them (commit them) or `git restore <file>` (drop them), then re-run finalize-commit. Refusing to commit code that differs from what CI tested.",
        })
    } else {
        let ci_args = crate::ci::Args {
            force: false,
            retry: 0,
            branch: Some(args.branch.clone()),
            simulate_branch: None,
            format: false,
            lint: false,
            build: false,
            test: false,
            audit: false,
            clean: false,
            trailing: Vec::new(),
            reason: Some("verifying commit before git commit".to_string()),
        };
        let (ci_result, ci_code) = crate::ci::run_impl(&ci_args, &commit_cwd, root, false);

        if ci_code != 0 {
            let msg = ci_result["message"]
                .as_str()
                .unwrap_or("bin/flow ci failed");
            let _ = append_log(
                root,
                &args.branch,
                &format!("[Phase {}] finalize-commit — ci (failed)", pn),
            );
            json!({
                "status": "error",
                "step": "ci",
                "message": msg,
            })
        } else {
            let _ = append_log(
                root,
                &args.branch,
                &format!("[Phase {}] finalize-commit — ci (ok)", pn),
            );

            // Re-stage tracked-file modifications so the commit
            // captures the bytes CI actually validated. Project `bin/*`
            // tools run in their default auto-fix mode during the
            // commit-time CI gate (CI=true is not set; see
            // `src/ci.rs`), so formatters like `ruff format` and
            // `prettier --write` modify tracked files in place. Without
            // this step, the staged-diff capture below would see the
            // pre-CI index bytes and `git commit -F` would record them
            // — diverging from what CI tested. `git add -u` updates
            // already-tracked files only: it does NOT sweep untracked
            // files. The commit-message file lives at
            // `<commit_cwd>/.flow-commit-msg` — an untracked file inside
            // the commit cwd — so `git add -u` cannot capture it, and
            // scratch files and CI artifacts the user has not yet
            // `.gitignore`d are likewise excluded. The commit's scope
            // stays bounded to what the user already staged in
            // `/flow:flow-commit` Round 3 plus any in-place
            // modifications CI made to those tracked files. The
            // working_tree_dirty gate above already proved git is
            // alive, so `.expect()` is safe here in the same sense as
            // the `git diff --cached` call below.
            let (add_code, _, add_stderr) =
                run_git_in_dir(&commit_cwd, &["add", "-u"], LOCAL_TIMEOUT)
                    .expect("git located by working_tree_dirty gate");
            if add_code != 0 {
                let _ = append_log(
                    root,
                    &args.branch,
                    &format!("[Phase {}] finalize-commit — restage (failed)", pn),
                );
                json!({
                    "status": "error",
                    "step": "restage",
                    "message": add_stderr.trim(),
                })
            } else {
                let _ = append_log(
                    root,
                    &args.branch,
                    &format!("[Phase {}] finalize-commit — restage (ok)", pn),
                );

                // Capture the staged diff for the plan-deviation gate.
                // The working_tree_dirty gate at the top of run_impl
                // already proved git is alive and the cwd is a real
                // repo, so .expect() is safe — Err and non-zero arms
                // for `git diff --cached` are unreachable in practice
                // per `.claude/rules/testability-means-simplicity.md`.
                let (_, staged_diff, _) =
                    run_git_in_dir(&commit_cwd, &["diff", "--cached"], LOCAL_TIMEOUT)
                        .expect("git located by working_tree_dirty gate");

                // Plan signature deviation gate. Blocks the commit when
                // a plan-named test's fixture value drifts without a
                // matching log acknowledgment. The gate is mechanical
                // enforcement of `.claude/rules/plan-commit-atomicity.md`
                // "Plan Signature Deviations Must Be Logged".
                match crate::plan_deviation::run_impl(root, &args.branch, &staged_diff) {
                    Ok(()) => finalize_commit(&message_file, &args.branch, &commit_cwd),
                    Err(deviations) => {
                        emit_deviation_stderr(&args.branch, &deviations);
                        let _ = append_log(
                            root,
                            &args.branch,
                            &format!(
                                "[Phase {}] finalize-commit — plan_deviation (blocked: {} deviation{})",
                                pn,
                                deviations.len(),
                                if deviations.len() == 1 { "" } else { "s" }
                            ),
                        );
                        let deviation_json: Vec<Value> = deviations
                            .iter()
                            .map(|d| {
                                json!({
                                    "test_name": d.test_name,
                                    "fixture_key": d.fixture_key,
                                    "plan_value": d.plan_value,
                                    "plan_line": d.plan_line,
                                })
                            })
                            .collect();
                        json!({
                            "status": "error",
                            "step": "plan_deviation",
                            "message": format!(
                                "{} unacknowledged plan signature deviation{}",
                                deviations.len(),
                                if deviations.len() == 1 { "" } else { "s" }
                            ),
                            "deviations": deviation_json,
                        })
                    }
                }
            }
        }
    };

    // Delete the commit-message file on every post-resolution exit.
    // Bound here (rather than inside `finalize_commit`) so the
    // working-tree-dirty, CI-failure, restage-failure, and
    // plan-deviation block paths — none of which reach
    // `finalize_commit` — also dispose of the file. The
    // `message_file_missing` early return above precedes this point, so
    // the only path that skips deletion is the one where the file does
    // not exist anyway.
    remove_message_file(&message_file);

    // Log final result
    let final_status = result["status"].as_str().unwrap_or("unknown");
    let _ = append_log(
        root,
        &args.branch,
        &format!(
            "[Phase {}] finalize-commit — done (\"{}\")",
            pn, final_status
        ),
    );

    // Clear continuation flags on error so the stop-continue hook
    // does not force-advance the parent phase after a failed commit.
    // Conflict is NOT cleared — the commit skill retries after resolving.
    if result["status"] == "error" {
        // The upstream pattern-match at the top of `run_impl`
        // already validated `args.branch` and produced `paths`;
        // reuse the same handle so the constructor runs once.
        let state_path = paths.state_file();
        if state_path.exists() {
            let _ = mutate_state(&state_path, &mut |state| {
                if !(state.is_object() || state.is_null()) {
                    return;
                }
                state["_continue_pending"] = Value::String(String::new());
                state["_continue_context"] = Value::String(String::new());
            });
        }
    }

    // Sentinel maintenance after commit:
    // - pull_merged == false: tree unchanged by pull → refresh sentinel to current snapshot.
    // - pull_merged == true: pull brought in new content → remove stale sentinel so the
    //   next CI run re-tests. (CI's run_once created the sentinel before the commit;
    //   the pull invalidated it.)
    if result["status"] == "ok" {
        let sentinel = crate::ci::sentinel_path(root, &args.branch);
        if result.get("pull_merged") == Some(&json!(false)) {
            let snapshot = crate::ci::tree_snapshot(&commit_cwd, None);
            let _ = fs::write(&sentinel, &snapshot);
        } else {
            let _ = fs::remove_file(&sentinel);
        }
    }

    Ok(result)
}

/// Main-arm dispatch: returns (value, exit code). Err wraps into JSON.
pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let root = crate::git::project_root();
    match run_impl(args, &root) {
        Err(msg) => (
            json!({"status": "error", "message": msg, "step": "args"}),
            1,
        ),
        Ok(result) => {
            let code = if result["status"] == "ok" { 0 } else { 1 };
            (result, code)
        }
    }
}
