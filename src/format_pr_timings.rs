use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::phase_config;
use crate::utils::{format_time, tolerant_i64};

/// Build a markdown timings table from state dict.
///
/// When `started_only` is true, phases with no `started_at` and 0
/// cumulative_seconds are excluded from the table.
pub fn format_timings_table(state: &Value, started_only: bool) -> String {
    let names = phase_config::phase_names();
    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => {
            return "| Phase | Duration |\n|-------|----------|\n| **Total** | **<1m** |"
                .to_string();
        }
    };

    let mut lines = vec![
        "| Phase | Duration |".to_string(),
        "|-------|----------|".to_string(),
    ];

    let mut total_seconds: i64 = 0;

    for (key, name) in &names {
        let phase = phases.get(key.as_str());
        let started = phase
            .and_then(|p| p.get("started_at"))
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty());
        let seconds = phase
            .and_then(|p| p.get("cumulative_seconds"))
            .map(tolerant_i64)
            .unwrap_or(0);

        if started_only && started.is_none() && seconds == 0 {
            continue;
        }

        total_seconds += seconds;
        lines.push(format!("| {} | {} |", name, format_time(seconds)));
    }

    lines.push(format!(
        "| **Total** | **{}** |",
        format_time(total_seconds)
    ));

    lines.join("\n")
}

#[derive(Parser, Debug)]
#[command(
    name = "format-pr-timings",
    about = "Format phase timings as a markdown table for PR body"
)]
pub struct Args {
    /// Path to state JSON file
    #[arg(long)]
    pub state_file: String,

    /// Path to write markdown output
    #[arg(long)]
    pub output: String,

    /// Only include phases that have been started
    #[arg(long)]
    pub started_only: bool,
}

/// Fallible CLI logic — returns the timings table on success or an
/// error message. `run_impl_main` wraps this into the `(Value, i32)`
/// contract that `dispatch::dispatch_json` consumes; unit tests call
/// `run_impl` directly to assert on typed results.
pub fn run_impl(args: &Args) -> Result<String, String> {
    let state_path = Path::new(&args.state_file);
    if !state_path.exists() {
        return Err(format!("State file not found: {}", args.state_file));
    }

    let content = std::fs::read_to_string(state_path)
        .map_err(|e| format!("Failed to read state file: {}", e))?;

    let state: Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse state file: {}", e))?;

    let table = format_timings_table(&state, args.started_only);

    let output_path = Path::new(&args.output);
    if let Some(parent) = output_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(output_path, &table).map_err(|e| format!("Failed to write output: {}", e))?;

    Ok(table)
}

/// Main-arm entry point: wraps `run_impl` into the `(Value, i32)`
/// contract consumed by `dispatch::dispatch_json`.
pub fn run_impl_main(args: &Args) -> (Value, i32) {
    match run_impl(args) {
        Ok(table) => (
            json!({
                "status": "ok",
                "output": args.output,
                "table": table,
            }),
            0,
        ),
        Err(msg) => (
            json!({
                "status": "error",
                "message": msg,
            }),
            1,
        ),
    }
}
