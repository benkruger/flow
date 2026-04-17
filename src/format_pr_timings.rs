use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::phase_config::{self, PHASE_ORDER};
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

    for &key in PHASE_ORDER {
        let phase = phases.get(key);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let mut phases = serde_json::Map::new();
        let phase_names = crate::phase_config::phase_names();
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
            let name = phase_names.get(p).cloned().unwrap_or_default();
            phases.insert(
                p.to_string(),
                json!({
                    "name": name,
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
    fn test_cumulative_seconds_as_string() {
        // tolerant_i64 accepts string-numeric counter values, so a state
        // file with cumulative_seconds stored as "945" (e.g. from an
        // external edit or legacy writer) must render the same timing
        // as the integer 945.
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
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!("945");

        let result = format_timings_table(&state, false);
        // 945 seconds = 15m
        assert!(result.contains("15m"), "Result:\n{}", result);
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

    #[test]
    fn run_impl_write_error_returns_err() {
        // run_impl's fs::write error branch (wrapping the OS error
        // into "Failed to write output: ..."). Point the output
        // path at a child of an existing regular file: create_dir_all
        // silently no-ops on a file, then fs::write fails with
        // NotADirectory — triggering the Err branch.
        let dir = tempfile::tempdir().unwrap();
        let parent_as_file = dir.path().join("not-a-dir");
        std::fs::write(&parent_as_file, "blocker").unwrap();
        let output_path = parent_as_file.join("out.md");

        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let state = make_state("flow-complete", &statuses);
        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();

        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            output: output_path.to_string_lossy().to_string(),
            started_only: false,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Failed to write output"),
            "Unexpected err msg: {}",
            msg
        );
    }

    /// Exercises the read_to_string Err arm (line 89 closure). Make the
    /// state-file path a directory: `Path::exists()` returns true so
    /// the early-return guard passes, but read_to_string fails with
    /// EISDIR — triggering the map_err message.
    #[test]
    fn run_impl_read_error_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        // Create as a directory, not a file — exists() is true.
        std::fs::create_dir(&state_path).unwrap();
        let output_path = dir.path().join("out.md");

        let args = Args {
            state_file: state_path.to_string_lossy().to_string(),
            output: output_path.to_string_lossy().to_string(),
            started_only: false,
        };
        let result = run_impl(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Failed to read state file"),
            "Unexpected err msg: {}",
            msg
        );
    }

    // --- run_impl_main (main.rs entry point) ---

    #[test]
    fn run_impl_main_happy_path_ok_with_json_value() {
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
        let state = make_state("flow-complete", &statuses);
        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
        let output = dir.path().join("t.md");
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            output: output.to_string_lossy().to_string(),
            started_only: false,
        };
        let (value, code) = run_impl_main(&args);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert!(value["table"]
            .as_str()
            .unwrap()
            .contains("| Phase | Duration |"));
        assert!(output.exists());
    }

    #[test]
    fn run_impl_main_missing_state_err_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let args = Args {
            state_file: dir
                .path()
                .join("missing.json")
                .to_string_lossy()
                .to_string(),
            output: dir.path().join("t.md").to_string_lossy().to_string(),
            started_only: false,
        };
        let (value, code) = run_impl_main(&args);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
    }

    #[test]
    fn run_impl_main_write_error_err_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let parent_as_file = dir.path().join("blocker");
        std::fs::write(&parent_as_file, "block").unwrap();
        let state = make_state("flow-complete", &[]);
        let state_file = dir.path().join("state.json");
        std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();
        let args = Args {
            state_file: state_file.to_string_lossy().to_string(),
            output: parent_as_file.join("t.md").to_string_lossy().to_string(),
            started_only: false,
        };
        let (value, code) = run_impl_main(&args);
        assert_eq!(code, 1);
        assert_eq!(value["status"], "error");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .contains("Failed to write output"));
    }
}
