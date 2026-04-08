//! Consolidated start-workspace: worktree creation + PR creation + state
//! backfill + lock release in a single command.
//!
//! Replaces the old start-setup for the workspace-creation portion of
//! flow-start. Lock is released as the final action (even on error),
//! closing the race condition where another flow could commit to main
//! between lock release and worktree creation.

use std::path::PathBuf;
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::commands::log::append_log;
use crate::commands::start_lock::{queue_path, release};
use crate::commands::start_step::update_step;
use crate::git::project_root;
use crate::github::detect_repo;
use crate::lock::mutate_state;
use crate::output::json_error;
use crate::start_setup::{create_worktree, initial_commit_push_pr};
use crate::utils::derive_feature;

#[derive(Parser, Debug)]
#[command(
    name = "start-workspace",
    about = "Create worktree, PR, backfill state, release lock"
)]
pub struct Args {
    /// Feature name (for lock release)
    pub feature_name: String,

    /// Canonical branch name (from init-state)
    #[arg(long)]
    pub branch: String,

    /// Path to file containing start prompt
    #[arg(long = "prompt-file")]
    pub prompt_file: Option<String>,
}

/// Testable entry point.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let root = project_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let branch = &args.branch;
    let feature_title = derive_feature(branch);

    // Update TUI step counter
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));
    update_step(&state_path, 3);

    let queue_dir = queue_path(&root);

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
                release_lock(&args.feature_name);
                return Ok(json!({
                    "status": "error",
                    "step": "prompt_file",
                    "message": format!("Could not read prompt file: {}", e),
                }));
            }
        }
    } else {
        args.feature_name.clone()
    };

    // Step 1: Create worktree
    let wt_path = match create_worktree(&root, branch) {
        Ok(p) => p,
        Err(e) => {
            let _ = append_log(
                &root,
                branch,
                &format!("[Phase 1] start-workspace — worktree failed: {}", e.message),
            );
            release_lock(&args.feature_name);
            return Ok(json!({
                "status": "error",
                "step": e.step,
                "message": e.message,
            }));
        }
    };
    let _ = append_log(
        &root,
        branch,
        &format!(
            "[Phase 1] start-workspace — worktree .worktrees/{} (ok)",
            branch
        ),
    );

    // Step 2: Commit, push, create PR
    let (pr_url, pr_number) =
        match initial_commit_push_pr(&wt_path, branch, &feature_title, &prompt) {
            Ok(r) => r,
            Err(e) => {
                let _ = append_log(
                    &root,
                    branch,
                    &format!(
                        "[Phase 1] start-workspace — PR creation failed: {}",
                        e.message
                    ),
                );
                release_lock(&args.feature_name);
                return Ok(json!({
                    "status": "error",
                    "step": e.step,
                    "message": e.message,
                }));
            }
        };
    let _ = append_log(
        &root,
        branch,
        "[Phase 1] start-workspace — commit + push + PR create (ok)",
    );

    // Step 3: Backfill state file
    let repo = detect_repo(Some(cwd.as_path()));
    let pr_url_clone = pr_url.clone();
    let prompt_clone = prompt.clone();
    let repo_clone = repo.clone();

    if state_path.exists() {
        match mutate_state(&state_path, move |state| {
            if !(state.is_object() || state.is_null()) {
                return;
            }
            state["pr_number"] = json!(pr_number);
            state["pr_url"] = json!(pr_url_clone);
            state["repo"] = match &repo_clone {
                Some(r) => json!(r),
                None => json!(null),
            };
            state["prompt"] = json!(prompt_clone);
        }) {
            Ok(_) => {}
            Err(e) => {
                let _ = append_log(
                    &root,
                    branch,
                    &format!("[Phase 1] start-workspace — backfill failed: {}", e),
                );
                release_lock(&args.feature_name);
                return Ok(json!({
                    "status": "error",
                    "step": "backfill",
                    "message": format!("Failed to backfill state: {}", e),
                }));
            }
        }
        let _ = append_log(
            &root,
            branch,
            "[Phase 1] start-workspace — state backfill (ok)",
        );
    }

    // Step 4: Release lock (final action)
    release_lock(&args.feature_name);
    let _ = append_log(
        &root,
        branch,
        "[Phase 1] start-workspace — lock released (ok)",
    );

    let wt_relative = format!(".worktrees/{}", branch);
    Ok(json!({
        "status": "ok",
        "worktree": wt_relative,
        "pr_url": pr_url,
        "pr_number": pr_number,
        "feature": feature_title,
        "branch": branch,
    }))
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&e, &[]);
            process::exit(1);
        }
    }
}
