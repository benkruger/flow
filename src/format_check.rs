//! `bin/flow format` subcommand.
//!
//! Detects the project framework, resolves the format-check command via
//! [`framework_tools::tool_command`], and spawns it with inherited stdio.
//! No-op frameworks (Rails, iOS) return a skipped status.
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
#[command(name = "format", about = "Run framework format checker")]
pub struct Args {
    /// Override branch for state file framework lookup
    #[arg(long)]
    pub branch: Option<String>,
}

/// Testable entry point.
pub fn run_impl(args: &Args, cwd: &Path, root: &Path) -> (Value, i32) {
    let branch = crate::git::resolve_branch_in(args.branch.as_deref(), cwd, root);
    let framework =
        match framework_tools::detect_framework_for_project(cwd, root, branch.as_deref()) {
            Ok(fw) => fw,
            Err(msg) => return (json!({"status": "error", "message": msg}), 1),
        };

    let tool_cmd = match framework_tools::tool_command(&framework, ToolType::Format) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => {
            return (
                json!({
                    "status": "skipped",
                    "reason": format!("format is a no-op for {}", framework),
                }),
                0,
            );
        }
        Err(msg) => return (json!({"status": "error", "message": msg}), 1),
    };

    let status = Command::new(&tool_cmd.program)
        .args(&tool_cmd.args)
        .current_dir(cwd)
        .status();

    match status {
        Ok(s) if s.success() => (json!({"status": "ok"}), 0),
        Ok(_) => (
            json!({"status": "error", "message": format!("{} format check failed", framework)}),
            1,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("failed to run {}: {}", tool_cmd.program, e)}),
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
