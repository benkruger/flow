//! `bin/flow test` subcommand.
//!
//! Detects the project framework, resolves the test command via
//! [`framework_tools::tool_command`], and spawns it with inherited stdio.
//! Trailing arguments are passed through to the test runner.
//! `--file <path>` runs a single test file.
//!
//! Output (JSON to stdout):
//!   `{"status": "ok"}`
//!   `{"status": "skipped", "reason": "..."}`
//!   `{"status": "error", "message": "..."}`

use std::path::Path;
use std::process::Command;

use clap::Parser;
use serde_json::{json, Value};

use crate::framework_tools::{self, ToolType};

#[derive(Parser, Debug)]
#[command(name = "test", about = "Run framework test tool")]
pub struct Args {
    /// Override branch for state file framework lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// Run a single test file
    #[arg(long)]
    pub file: Option<String>,

    /// Additional arguments passed through to the test runner
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra: Vec<String>,
}

/// Testable entry point.
pub fn run_impl(args: &Args, cwd: &Path, root: &Path) -> (Value, i32) {
    let branch = crate::git::resolve_branch_in(args.branch.as_deref(), cwd, root);
    let framework =
        match framework_tools::detect_framework_for_project(cwd, root, branch.as_deref()) {
            Ok(fw) => fw,
            Err(msg) => return (json!({"status": "error", "message": msg}), 1),
        };

    let tool_cmd = match framework_tools::tool_command(&framework, ToolType::Test) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => {
            return (
                json!({
                    "status": "skipped",
                    "reason": format!("test is a no-op for {}", framework),
                }),
                0,
            );
        }
        Err(msg) => return (json!({"status": "error", "message": msg}), 1),
    };

    // Strip leading "--" from extra args (clap trailing_var_arg includes it)
    let extra: Vec<&str> = args
        .extra
        .iter()
        .skip_while(|s| s.as_str() == "--")
        .map(|s| s.as_str())
        .collect();

    // --file mode: framework-specific single-file test execution
    if let Some(ref file_path) = args.file {
        return run_single_file(&framework, file_path, &extra, cwd);
    }

    // Normal mode: run the full test suite with extra args
    let mut cmd = Command::new(&tool_cmd.program);
    cmd.args(&tool_cmd.args).current_dir(cwd);
    for arg in &extra {
        cmd.arg(arg);
    }

    match cmd.status() {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": format!("{} tests failed", framework)}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run {}: {}", tool_cmd.program, e)}),
            1,
        ),
    }
}

/// Run a single test file using framework-specific mechanisms.
fn run_single_file(framework: &str, file_path: &str, extra: &[&str], cwd: &Path) -> (Value, i32) {
    match framework {
        "rust" => run_single_file_rust(file_path, extra, cwd),
        "python" => {
            let mut cmd = Command::new("python3");
            cmd.args(["-m", "pytest", file_path, "-v"]).current_dir(cwd);
            for arg in extra {
                cmd.arg(arg);
            }
            run_command(cmd, framework)
        }
        "rails" => {
            let mut cmd = Command::new("bundle");
            cmd.args(["exec", "ruby", "-Ilib", "-Itest", file_path])
                .current_dir(cwd);
            for arg in extra {
                cmd.arg(arg);
            }
            run_command(cmd, framework)
        }
        "go" => {
            let mut cmd = Command::new("go");
            cmd.args(["test", file_path, "-v"]).current_dir(cwd);
            for arg in extra {
                cmd.arg(arg);
            }
            run_command(cmd, framework)
        }
        "ios" => (
            json!({
                "status": "error",
                "message": "Single-file test execution is not supported for iOS",
            }),
            1,
        ),
        _ => (
            json!({"status": "error", "message": format!("Unknown framework: {}", framework)}),
            1,
        ),
    }
}

/// Rust single-file test: compile with `rustc --test`, run the binary, clean up.
fn run_single_file_rust(file_path: &str, extra: &[&str], cwd: &Path) -> (Value, i32) {
    let temp_bin = cwd.join(".flow-test-binary");

    let compile = Command::new("rustc")
        .args(["--test", file_path, "-o"])
        .arg(&temp_bin)
        .current_dir(cwd)
        .status();

    match compile {
        Ok(s) if s.success() => {}
        Ok(_) => {
            let _ = std::fs::remove_file(&temp_bin);
            return (
                json!({"status": "error", "message": "rustc --test compilation failed"}),
                1,
            );
        }
        Err(e) => {
            return (
                json!({"status": "error", "message": format!("failed to run rustc: {}", e)}),
                1,
            );
        }
    }

    let mut run = Command::new(&temp_bin);
    run.current_dir(cwd);
    for arg in extra {
        run.arg(arg);
    }

    let result = match run.status() {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": "rust single-file tests failed"}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run test binary: {}", e)}),
            1,
        ),
    };

    let _ = std::fs::remove_file(&temp_bin);
    result
}

/// Helper: run a Command and return JSON status.
fn run_command(mut cmd: Command, framework: &str) -> (Value, i32) {
    match cmd.status() {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": format!("{} tests failed", framework)}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run test command: {}", e)}),
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
