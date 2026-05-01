//! Consolidated start-workspace: worktree creation + PR creation + state
//! backfill + lock release in a single command.
//!
//! Replaces the old start-setup for the workspace-creation portion of
//! flow-start. Lock is released as the final action (even on error),
//! closing the race condition where another flow could commit to main
//! between lock release and worktree creation.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::{json, Value};

use std::time::Duration;

use crate::commands::log::append_log;
use crate::commands::start_lock::{queue_path, release};
use crate::commands::start_step::update_step;
use crate::flow_paths::FlowPaths;
use crate::github::detect_repo;
use crate::lock::mutate_state;
use crate::utils::{derive_feature, run_cmd, SetupError};

#[derive(Parser, Debug)]
#[command(
    name = "start-workspace",
    about = "Create worktree, PR, backfill state, release lock"
)]
pub struct Args {
    /// Human-readable feature description (for fallback prompt text)
    pub description: String,

    /// Canonical branch name (from init-state)
    #[arg(long)]
    pub branch: String,

    /// Path to file containing start prompt
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,
}

/// Extract PR number from URL like https://github.com/org/repo/pull/123.
///
/// Searches for the "pull" segment and parses the next segment as the number.
/// Returns 0 if the URL is malformed or not a PR URL.
fn extract_pr_number(pr_url: &str) -> u32 {
    pr_url
        .trim_end_matches('/')
        .split('/')
        .skip_while(|s| *s != "pull")
        .nth(1)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Create a git worktree for the feature branch.
pub(crate) fn create_worktree(
    project_root: &std::path::Path,
    branch: &str,
) -> Result<PathBuf, SetupError> {
    let wt_path = project_root.join(".worktrees").join(branch);
    run_cmd(
        &[
            "git",
            "worktree",
            "add",
            &wt_path.to_string_lossy(),
            "-b",
            branch,
        ],
        project_root,
        "worktree",
        None,
    )?;

    // Symlink .venv if it exists
    let venv_dir = project_root.join(".venv");
    if venv_dir.is_dir() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink(
                std::path::Path::new("../..").join(".venv"),
                wt_path.join(".venv"),
            );
        }
    }

    Ok(wt_path)
}

/// Make empty commit, push, and create PR. Returns (pr_url, pr_number).
///
/// `root` — the project root (main repo); `<root>/.flow-states/` is where
/// the branch-scoped commit message file lives, alongside other
/// branch-scoped state. Per `.claude/rules/file-tool-preflights.md`, the
/// commit message lives at `<root>/.flow-states/<branch>-commit-msg.txt`
/// so each worktree's flow has its own file (concurrency-safe) and abort
/// /complete cleanup deletes it deterministically with the other
/// branch-scoped state.
///
/// `base_branch` — the integration branch to target as the PR's base.
/// Read from the state file by [`run_impl_with_paths`]; written there
/// at flow-start by [`crate::commands::init_state`] via
/// [`crate::git::current_branch_in`] so it equals `git branch --show-current`
/// at the moment `/flow:flow-start` was invoked.
pub(crate) fn initial_commit_push_pr(
    root: &std::path::Path,
    wt_path: &std::path::Path,
    branch: &str,
    feature_title: &str,
    prompt: &str,
    base_branch: &str,
) -> Result<(String, u32), SetupError> {
    let commit_msg_path = FlowPaths::new(root, branch).commit_msg();
    // `init-state` ran before `start-workspace` and created
    // `<root>/.flow-states/<branch>.json`, so the directory exists. A
    // failure here would indicate disk-full or read-only filesystem —
    // neither is a FLOW-supported recovery state, so treat as an
    // invariant via `.expect()`.
    std::fs::write(&commit_msg_path, format!("Start {} branch", branch))
        .expect("commit-msg write must succeed when .flow-states/ already exists");

    let commit_msg_arg = commit_msg_path
        .to_str()
        .expect("commit-msg path is valid UTF-8 (project_root + ASCII filename)");
    let result = run_cmd(
        &["git", "commit", "--allow-empty", "-F", commit_msg_arg],
        wt_path,
        "commit",
        None,
    );
    // Always clean up the commit message file
    let _ = std::fs::remove_file(&commit_msg_path);
    result?;

    run_cmd(
        &["git", "push", "-u", "origin", branch],
        wt_path,
        "push",
        Some(Duration::from_secs(60)),
    )?;

    let pr_body = format!("## What\n\n{}.", prompt);
    let (stdout, _) = run_cmd(
        &[
            "gh",
            "pr",
            "create",
            "--title",
            feature_title,
            "--body",
            &pr_body,
            "--base",
            base_branch,
        ],
        wt_path,
        "pr_create",
        Some(Duration::from_secs(60)),
    )?;

    let pr_url = stdout.trim().to_string();
    let pr_number = extract_pr_number(&pr_url);
    Ok((pr_url, pr_number))
}

/// Testable core with injected root and cwd. Production callers
/// binds them to [`project_root`] and `current_dir()`. Tests supply
/// a `TempDir` for both. Returns a `Value` directly — every error
/// scenario surfaces as a `status: "error"` payload with exit code 0
/// via [`run_impl_main`]. No path returns `Err` at the Rust level.
fn run_impl_with_paths(args: &Args, root: &Path, cwd: &Path) -> Value {
    let branch = &args.branch;
    let feature_title = derive_feature(branch);

    // Update TUI step counter
    let state_path = FlowPaths::new(root, branch).state_file();
    update_step(&state_path, 3);

    // Read state file once: relative_cwd routes the agent into a
    // subdirectory of the worktree (mono-repo flows); base_branch is
    // the integration branch the PR will target as `--base`. Both were
    // written by init_state before start-gate ran. Defaults preserve
    // pre-existing behavior when the state file is unreadable, parse
    // fails, or fields are absent (root-level flow against `main`).
    let state_value = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let relative_cwd = state_value
        .as_ref()
        .and_then(|v| {
            v.get("relative_cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let base_branch = state_value
        .as_ref()
        .and_then(|v| {
            v.get("base_branch")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "main".to_string());

    let queue_dir = queue_path(root);

    // Helper: release lock and return error
    let release_lock = |feature: &str| {
        release(feature, &queue_dir);
    };

    // Read prompt from file if provided. Release lock on failure.
    let prompt = if let Some(ref pf) = args.prompt_file {
        match std::fs::read_to_string(pf) {
            Ok(content) => {
                let _ = std::fs::remove_file(pf);
                content.trim().to_string()
            }
            Err(e) => {
                release_lock(&args.branch);
                return json!({
                    "status": "error",
                    "step": "prompt_file",
                    "message": format!("Could not read prompt file: {}", e),
                });
            }
        }
    } else {
        args.description.clone()
    };

    // Step 1: Create worktree
    let wt_path = match create_worktree(root, branch) {
        Ok(p) => p,
        Err(e) => {
            let _ = append_log(
                root,
                branch,
                &format!("[Phase 1] start-workspace — worktree failed: {}", e.message),
            );
            release_lock(&args.branch);
            return json!({
                "status": "error",
                "step": e.step,
                "message": e.message,
            });
        }
    };
    let _ = append_log(
        root,
        branch,
        &format!(
            "[Phase 1] start-workspace — worktree .worktrees/{} (ok)",
            branch
        ),
    );

    // Step 2: Commit, push, create PR
    let (pr_url, pr_number) = match initial_commit_push_pr(
        root,
        &wt_path,
        branch,
        &feature_title,
        &prompt,
        &base_branch,
    ) {
        Ok(r) => r,
        Err(e) => {
            let _ = append_log(
                root,
                branch,
                &format!(
                    "[Phase 1] start-workspace — PR creation failed: {}",
                    e.message
                ),
            );
            release_lock(&args.branch);
            return json!({
                "status": "error",
                "step": e.step,
                "message": e.message,
            });
        }
    };
    let _ = append_log(
        root,
        branch,
        "[Phase 1] start-workspace — commit + push + PR create (ok)",
    );

    // Step 3: Backfill state file
    let repo = detect_repo(Some(cwd));
    let pr_url_clone = pr_url.clone();
    let prompt_clone = prompt.clone();
    let repo_clone = repo.clone();

    if state_path.exists() {
        match mutate_state(&state_path, &mut |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            state["pr_number"] = json!(pr_number);
            state["pr_url"] = json!(&pr_url_clone);
            state["repo"] = match &repo_clone {
                Some(r) => json!(r),
                None => json!(null),
            };
            state["prompt"] = json!(&prompt_clone);
        }) {
            Ok(_) => {}
            Err(e) => {
                let _ = append_log(
                    root,
                    branch,
                    &format!("[Phase 1] start-workspace — backfill failed: {}", e),
                );
                release_lock(&args.branch);
                return json!({
                    "status": "error",
                    "step": "backfill",
                    "message": format!("Failed to backfill state: {}", e),
                });
            }
        }
        let _ = append_log(
            root,
            branch,
            "[Phase 1] start-workspace — state backfill (ok)",
        );
    }

    // Step 4: Release lock (final action)
    release_lock(&args.branch);
    let _ = append_log(
        root,
        branch,
        "[Phase 1] start-workspace — lock released (ok)",
    );

    // Advance TUI display to step 4 ("entering worktree") before returning
    update_step(&state_path, 4);

    let wt_relative = format!(".worktrees/{}", branch);
    // worktree_cwd is the absolute directory the agent should cd into.
    // For root-level flows it points at the worktree itself; for flows
    // started inside a mono-repo subdirectory (relative_cwd non-empty),
    // it includes that suffix so the agent lands in the same subdir
    // it started from.
    //
    // Absolute, NOT relative — the skill's Step 3 substitutes this
    // value directly into a `cd <worktree_cwd>` command, and the bash
    // tool's cwd at that moment is whatever the user invoked the flow
    // from (project root for a flat repo, or `synapse/`/`cortex/`/etc.
    // for a mono-repo subdir flow). A relative path resolves against
    // bash's current cwd and breaks for any cwd != project_root; an
    // absolute path works from any cwd.
    let wt_abs = root.join(".worktrees").join(branch);
    let worktree_cwd_path = if relative_cwd.is_empty() {
        wt_abs
    } else {
        wt_abs.join(&relative_cwd)
    };
    let worktree_cwd = worktree_cwd_path.to_string_lossy().into_owned();
    json!({
        "status": "ok",
        "worktree": wt_relative,
        "worktree_cwd": worktree_cwd,
        "relative_cwd": relative_cwd,
        "pr_url": pr_url,
        "pr_number": pr_number,
        "feature": feature_title,
        "branch": branch,
    })
}

/// Main-arm entry point: returns the `(Value, i32)` contract that
/// `dispatch::dispatch_json` consumes. Takes `root: &Path` and
/// `cwd: &Path` per `.claude/rules/rust-patterns.md` "Main-arm
/// dispatch" so inline tests can pass a `TempDir` fixture instead of
/// the host `project_root()`/`current_dir()`. `run_impl_with_paths`
/// always returns `Value` — business errors appear in the
/// `status: "error"` payload with exit code `0`.
pub fn run_impl_main(args: &Args, root: &Path, cwd: &Path) -> (Value, i32) {
    (run_impl_with_paths(args, root, cwd), 0)
}

#[cfg(any())]
mod _removed {
    use super::*;

    #[test]
    fn extract_pr_number_standard_url() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/123"),
            123
        );
    }

    #[test]
    fn extract_pr_number_trailing_slash() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/42/"),
            42
        );
    }

    #[test]
    fn extract_pr_number_malformed() {
        assert_eq!(extract_pr_number("not-a-url"), 0);
    }

    #[test]
    fn extract_pr_number_non_numeric() {
        assert_eq!(extract_pr_number("https://github.com/org/repo/pull/abc"), 0);
    }

    #[test]
    fn extract_pr_number_empty_string() {
        assert_eq!(extract_pr_number(""), 0);
    }

    #[test]
    fn extract_pr_number_pull_with_no_number() {
        // URL ends at "pull/" with nothing parseable after it
        assert_eq!(extract_pr_number("https://github.com/org/repo/pull/"), 0);
    }

    // --- run_impl_main ---

    /// Drives run_impl_main against a bare TempDir that is not a git
    /// repo — the worktree-creation subprocess fails on missing
    /// `.git`, and `run_impl_with_paths` returns a `status:"error"`
    /// `step:"worktree"` payload. run_impl_main wraps with exit 0
    /// per the business-error convention.
    #[test]
    fn start_workspace_run_impl_main_err_path() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Seed just enough state so the function reaches the
        // worktree-creation step. No .git, so create_worktree fails.
        let state_dir = root.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        let args = Args {
            description: "workspace-err-feature".to_string(),
            branch: "workspace-err-branch".to_string(),
            prompt_file: None,
        };
        let (v, code) = run_impl_main(&args, &root, &root);
        assert_eq!(code, 0, "exit code is 0 for business errors");
        assert_eq!(v["status"], "error");
    }
}
