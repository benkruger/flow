//! `bin/flow complete-finalize` — consolidated post-merge + cleanup.
//!
//! Combines complete-post-merge and cleanup into a single process,
//! eliminating the `cd <project_root>` step between them. Both
//! post_merge_inner() and cleanup() use explicit paths, so they
//! compose naturally without changing the shell working directory.
//!
//! Usage: bin/flow complete-finalize --pr <N> --state-file <path>
//!        --branch <name> --worktree <path> [--pull]
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "formatted_time": "...", "summary": "...",
//!    "issues_links": "...", "banner_line": "...", "cleanup": {...}}

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "complete-finalize", about = "FLOW Complete phase post-merge + cleanup")]
pub struct Args {
    /// PR number
    #[arg(long, required = true)]
    pub pr: i64,
    /// Path to state file
    #[arg(long = "state-file", required = true)]
    pub state_file: String,
    /// Branch name
    #[arg(long, required = true)]
    pub branch: String,
    /// Worktree path (relative)
    #[arg(long, required = true)]
    pub worktree: String,
    /// Run git pull origin main after cleanup
    #[arg(long)]
    pub pull: bool,
}

/// Core complete-finalize logic. Runs post-merge then cleanup,
/// continuing cleanup even if post-merge fails.
///
/// Returns Ok(json) with merged results from both operations,
/// Err(string) only for catastrophic failures that prevent any output.
pub fn run_impl(args: &Args) -> Result<Value, String> {
    let _ = args;
    Err("not implemented".to_string())
}

/// CLI entry point.
pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            println!("{}", json!({"status": "error", "message": e}));
            std::process::exit(1);
        }
    }
}
