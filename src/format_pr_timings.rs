use std::path::Path;
use std::process;

use clap::Parser;
use serde_json::{json, Value};

use crate::output::{json_error, json_ok};
use crate::phase_config::{self, PHASE_ORDER};
use crate::utils::format_time;

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

    for &key in PHASE_ORDER {
        let phase = phases.get(key);
        let started = phase
            .and_then(|p| p.get("started_at"))
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty());
        let seconds = phase
            .and_then(|p| p.get("cumulative_seconds"))
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
            .unwrap_or(0);

        if started_only && started.is_none() && seconds == 0 {
            continue;
        }

        let name = names.get(key).map(|s| s.as_str()).unwrap_or(key);
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

/// Fallible CLI logic — returns the timings table on success or an error message.
/// Extracted from `run()` so error paths can be unit-tested without `process::exit`.
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

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(table) => {
            json_ok(&[("output", json!(args.output)), ("table", json!(table))]);
        }
        Err(msg) => {
            json_error(&msg, &[]);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let mut phases = serde_json::Map::new();
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        for &p in &all_phases {
            let status = phase_statuses
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| *v)
                .unwrap_or("pending");
            phases.insert(
                p.to_string(),
                json!({
                    "name": match p {
                        "flow-start" => "Start",
                        "flow-plan" => "Plan",
                        "flow-code" => "Code",
                        "flow-code-review" => "Code Review",
                        "flow-learn" => "Learn",
                        "flow-complete" => "Complete",
                        _ => p,
                    },
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": null,
                    "cumulative_seconds": 0,
                    "visit_count": 0,
                }),
            );
        }
        json!({
            "branch": "test-feature",
            "pr_url": "https://github.com/test/test/pull/1",
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": current_phase,
            "phases": phases,
        })
    }

    #[test]
    fn test_all_complete() {
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let mut state = make_state("flow-complete", &statuses);
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(36);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(945);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(328);
        state["phases"]["flow-code-review"]["cumulative_seconds"] = json!(500);
        state["phases"]["flow-learn"]["cumulative_seconds"] = json!(352);
        state["phases"]["flow-complete"]["cumulative_seconds"] = json!(20);

        let result = format_timings_table(&state, false);
        assert!(
            result.contains("| Phase | Duration |"),
            "Result:\n{}",
            result
        );
        assert!(result.contains("| Start |"), "Result:\n{}", result);
        assert!(result.contains("| Plan |"), "Result:\n{}", result);
        assert!(result.contains("| Code Review |"), "Result:\n{}", result);
        assert!(result.contains("| **Total** |"), "Result:\n{}", result);
    }

    #[test]
    fn test_partial_state() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(600);

        let result = format_timings_table(&state, false);
        assert!(result.contains("| Start |"), "Result:\n{}", result);
        assert!(result.contains("| Plan |"), "Result:\n{}", result);
        assert!(result.contains("| Code |"), "Result:\n{}", result);
        // Pending phases with 0 seconds should show <1m
        assert!(result.contains("| Complete |"), "Result:\n{}", result);
    }

    #[test]
    fn test_started_only() {
        let mut state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state["phases"]["flow-start"]["started_at"] = json!("2026-01-01T00:00:00Z");
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["started_at"] = json!("2026-01-01T00:01:00Z");
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-code"]["started_at"] = json!("2026-01-01T00:06:00Z");

        let result = format_timings_table(&state, true);
        assert!(result.contains("| Start |"), "Result:\n{}", result);
        assert!(result.contains("| Plan |"), "Result:\n{}", result);
        assert!(result.contains("| Code |"), "Result:\n{}", result);
        assert!(!result.contains("| Code Review |"), "Result:\n{}", result);
        assert!(!result.contains("| Learn |"), "Result:\n{}", result);
        assert!(!result.contains("| Complete |"), "Result:\n{}", result);
        assert!(result.contains("| **Total** |"), "Result:\n{}", result);
    }

    #[test]
    fn test_uses_format_time() {
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let mut state = make_state("flow-complete", &statuses);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(3700);

        let result = format_timings_table(&state, false);
        // 3700 seconds = 1h 1m
        assert!(result.contains("1h 1m"), "Result:\n{}", result);
    }

    #[test]
    fn test_cli_writes_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let mut state = make_state("flow-complete", &statuses);
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(60);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);

        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
        let output_file = dir.path().join("timings.md");

        // Test the format function directly then verify file output
        let table = format_timings_table(&state, false);
        std::fs::write(&output_file, &table).unwrap();

        let content = std::fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("| Phase | Duration |"));
    }

    #[test]
    fn test_no_phases_key() {
        let state = json!({"branch": "test"});
        let result = format_timings_table(&state, false);
        assert!(
            result.contains("| Phase | Duration |"),
            "Result:\n{}",
            result
        );
        assert!(result.contains("| **Total** |"), "Result:\n{}", result);
    }

    #[test]
    fn test_cli_missing_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            state_file: dir
                .path()
                .join("missing.json")
                .to_string_lossy()
                .to_string(),
            output: dir.path().join("out.md").to_string_lossy().to_string(),
            started_only: false,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_cli_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("bad.json");
        std::fs::write(&state_file, "not valid json {{{").unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            output: dir.path().join("out.md").to_string_lossy().to_string(),
            started_only: false,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    #[test]
    fn test_cli_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let mut state = make_state("flow-complete", &statuses);
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(60);

        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
        let output_file = dir.path().join("timings.md");

        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            output: output_file.to_string_lossy().to_string(),
            started_only: false,
        };
        let result = run_impl(&args);
        assert!(result.is_ok());
        let table = result.unwrap();
        assert!(table.contains("| Phase | Duration |"));
        assert!(output_file.exists());
    }
}
