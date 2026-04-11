//! `bin/flow test` subcommand.
//!
//! Delegates to the repo-local `./bin/test` script in the current
//! working directory. The user's `bin/test` owns the actual test
//! runner (cargo nextest, pytest, go test, etc.) and any
//! single-file compile path; FLOW only provides the entry point,
//! exit code propagation, the `FLOW_CI_RUNNING` recursion guard,
//! and `--file` / trailing-arg passthrough.
//!
//! Output (JSON to stdout):
//!   `{"status": "ok"}`
//!   `{"status": "error", "message": "..."}`

use std::path::Path;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(name = "test", about = "Run repo-local bin/test")]
pub struct Args {
    /// Reserved for future use; currently ignored.
    #[arg(long)]
    pub branch: Option<String>,

    /// Run a single test file (forwarded as `--file <path>` to bin/test)
    #[arg(long)]
    pub file: Option<String>,

    /// Additional arguments passed through to bin/test
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra: Vec<String>,
}

/// Testable entry point.
pub fn run_impl(args: &Args, cwd: &Path, root: &Path) -> (Value, i32) {
    if let Err(msg) = crate::cwd_scope::enforce(cwd, root) {
        return (json!({"status": "error", "message": msg}), 1);
    }

    let bin_test = cwd.join("bin").join("test");
    if !bin_test.is_file() {
        return (
            json!({
                "status": "error",
                "message": format!("./bin/test not found in {}", cwd.display()),
            }),
            1,
        );
    }

    // Strip leading "--" from extra args (clap trailing_var_arg includes it)
    let extra: Vec<&str> = args
        .extra
        .iter()
        .skip_while(|s| s.as_str() == "--")
        .map(|s| s.as_str())
        .collect();

    let mut cmd = Command::new(&bin_test);
    cmd.current_dir(cwd).env("FLOW_CI_RUNNING", "1");

    if let Some(ref file_path) = args.file {
        cmd.arg("--file").arg(file_path);
    }
    for arg in &extra {
        cmd.arg(arg);
    }

    match cmd.status() {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": "./bin/test failed"}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run ./bin/test: {}", e)}),
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
