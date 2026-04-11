//! `bin/flow lint` subcommand.
//!
//! Delegates to the repo-local `./bin/lint` script in the current
//! working directory. The user's `bin/lint` owns the actual linter
//! invocation (cargo clippy, ruff, eslint, etc.); FLOW only provides
//! the entry point, exit code propagation, and the `FLOW_CI_RUNNING`
//! recursion guard.
//!
//! Output (JSON to stdout):
//!   `{"status": "ok"}`
//!   `{"status": "error", "message": "..."}`

use std::path::Path;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "lint", about = "Run repo-local bin/lint")]
pub struct Args {
    /// Reserved for future use; currently ignored.
    #[arg(long)]
    pub branch: Option<String>,
}

/// Testable entry point.
pub fn run_impl(_args: &Args, cwd: &Path, _root: &Path) -> (Value, i32) {
    let bin_lint = cwd.join("bin").join("lint");
    if !bin_lint.is_file() {
        return (
            json!({
                "status": "error",
                "message": format!("./bin/lint not found in {}", cwd.display()),
            }),
            1,
        );
    }

    let status = Command::new(&bin_lint)
        .current_dir(cwd)
        .env("FLOW_CI_RUNNING", "1")
        .status();

    match status {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": "./bin/lint failed"}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run ./bin/lint: {}", e)}),
            1,
        ),
    }
}

pub fn run(args: Args) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = crate::git::project_root();
    let (result, code) = run_impl(&args, &cwd, &root);
    println!("{}", serde_json::to_string(&result).unwrap());
    std::process::exit(code);
}
