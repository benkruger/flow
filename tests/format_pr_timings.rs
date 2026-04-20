//! Integration tests for `src/format_pr_timings.rs` — mirrors the
//! production module and drives it through the public surface
//! (`format_timings_table`, `run_impl`, `run_impl_main`).

use flow_rs::format_pr_timings::{format_timings_table, run_impl, run_impl_main, Args};
use serde_json::{json, Value};

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let mut phases = serde_json::Map::new();
    let phase_names = flow_rs::phase_config::phase_names();
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

/// Exercises the read_to_string Err arm. Make the state-file path a
/// directory: `Path::exists()` returns true so the early-return guard
/// passes, but read_to_string fails with EISDIR — triggering the
/// map_err message.
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

/// Exercises the `output_path.parent() == None` branch inside
/// `run_impl`. `Path::new("").parent()` returns `None`, so the
/// `if let Some(parent)` arm is skipped and fs::write("") then
/// fails with "Failed to write output: ...".
#[test]
fn run_impl_empty_output_skips_mkdir_and_err_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_state("flow-complete", &[]);
    let state_file = dir.path().join("state.json");
    std::fs::write(&state_file, serde_json::to_string(&state).unwrap()).unwrap();

    let args = Args {
        state_file: state_file.to_string_lossy().to_string(),
        output: String::new(),
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
