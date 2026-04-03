use serde_json::{json, Value};
use std::path::Path;
use std::process;

use crate::format_status;
use crate::git::{project_root, resolve_branch};
use crate::output::json_error;
use crate::phase_config::{self, find_state_files, load_phase_config};
use crate::utils::{detect_dev_mode, derive_worktree, read_version};

/// Build continue-context JSON for /flow:flow-continue Path B.
///
/// Output (JSON to stdout):
///   Success: {"status": "ok", "panel": "...", "branch": "...",
///             "worktree": "...", "current_phase": "...",
///             "phase_name": "...", "phase_command": "..."}
///   No state: {"status": "no_state", "branch": "..."}
///   Multiple: {"status": "multiple_features", "features": [...]}
///   Error:    {"status": "error", "message": "..."}
pub fn run(branch_override: Option<&str>) {
    let root = project_root();
    let (branch, candidates) = resolve_branch(branch_override, &root);

    let effective_branch = match branch {
        Some(b) => b,
        None => {
            if !candidates.is_empty() {
                // Ambiguous — fall through to find_state_files with empty branch
                String::new()
            } else {
                json_error("Could not determine current branch", &[]);
                process::exit(1);
            }
        }
    };

    let (result, exit_code) = build_context_with_branch(&root, &effective_branch);
    println!("{}", serde_json::to_string(&result).unwrap());
    process::exit(exit_code);
}

/// Build the continue-context result for a given root and branch override.
/// Extracted for testability (no process::exit, no project_root() detection).
pub fn build_context(
    root: &Path,
    branch_override: Option<&str>,
) -> (Value, i32) {
    let branch = branch_override.unwrap_or("");
    build_context_with_branch(root, branch)
}

fn build_context_with_branch(root: &Path, branch: &str) -> (Value, i32) {
    let results = find_state_files(root, branch);

    if results.is_empty() {
        return (json!({"status": "no_state", "branch": branch}), 0);
    }

    if results.len() > 1 {
        let names = phase_config::phase_names();
        let mut features = Vec::new();
        for (_path, state, matched_branch) in &results {
            let current_phase = state
                .get("current_phase")
                .and_then(|c| c.as_str())
                .unwrap_or("flow-start");
            let phase_name = names
                .get(current_phase)
                .map(|s| s.as_str())
                .unwrap_or(current_phase);
            features.push(json!({
                "feature": crate::utils::derive_feature(matched_branch),
                "branch": matched_branch,
                "current_phase": current_phase,
                "phase_name": phase_name,
                "worktree": derive_worktree(matched_branch),
            }));
        }
        return (
            json!({
                "status": "multiple_features",
                "features": features,
            }),
            0,
        );
    }

    let (_state_path, state, matched_branch) = &results[0];

    let version = read_version();

    // Load frozen phase config if available
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", matched_branch));
    let phase_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    // Detect dev mode from .flow.json
    let dev_mode = detect_dev_mode(root);

    let panel = format_status::format_panel(state, &version, None, dev_mode, phase_config.as_ref());

    let current_phase = state
        .get("current_phase")
        .and_then(|c| c.as_str())
        .unwrap_or("flow-start");

    let names = phase_config::phase_names();
    let commands = phase_config::commands();

    let phase_name = names
        .get(current_phase)
        .map(|s| s.as_str())
        .unwrap_or(current_phase);
    let default_cmd = format!("/flow:{}", current_phase);
    let phase_command = commands
        .get(current_phase)
        .map(|s| s.as_str())
        .unwrap_or(&default_cmd);

    (
        json!({
            "status": "ok",
            "panel": panel,
            "branch": matched_branch,
            "worktree": derive_worktree(matched_branch),
            "current_phase": current_phase,
            "phase_name": phase_name,
            "phase_command": phase_command,
        }),
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

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
            "schema_version": 1,
            "branch": "test-feature",
            "repo": null,
            "pr_number": null,
            "pr_url": "https://github.com/test/test/pull/1",
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": current_phase,
            "notes": [],
            "phases": phases,
        })
    }

    fn setup_state_dir(tmp: &TempDir) -> std::path::PathBuf {
        let state_dir = tmp.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        state_dir
    }

    #[test]
    fn test_no_state_returns_no_state() {
        let tmp = TempDir::new().unwrap();
        // Create a minimal git repo so resolve_branch can work
        let state_dir = setup_state_dir(&tmp);
        // No state files exist
        assert!(fs::read_dir(&state_dir).unwrap().count() == 0);

        let (result, exit_code) = build_context(tmp.path(), Some("test-branch"));
        assert_eq!(result["status"], "no_state");
        assert_eq!(result["branch"], "test-branch");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_happy_path_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);

        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        fs::write(
            state_dir.join("test-feature.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();

        let (result, exit_code) = build_context(tmp.path(), Some("test-feature"));
        assert_eq!(result["status"], "ok");
        assert_eq!(result["current_phase"], "flow-plan");
        assert_eq!(result["phase_name"], "Plan");
        assert_eq!(result["phase_command"], "/flow:flow-plan");
        assert_eq!(result["worktree"], ".worktrees/test-feature");
        assert_eq!(result["branch"], "test-feature");
        assert!(result["panel"].as_str().unwrap().contains("FLOW v"));
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_multiple_features_returns_multiple() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);

        for name in &["feature-a", "feature-b"] {
            let mut state = make_state(
                "flow-plan",
                &[("flow-start", "complete"), ("flow-plan", "in_progress")],
            );
            state["branch"] = json!(name);
            fs::write(
                state_dir.join(format!("{}.json", name)),
                serde_json::to_string_pretty(&state).unwrap(),
            )
            .unwrap();
        }

        // Pass a branch that doesn't match any state file
        let (result, exit_code) = build_context(tmp.path(), Some("nonexistent"));
        assert_eq!(result["status"], "multiple_features");
        let features = result["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_branch_flag_overrides() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);

        let state = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        fs::write(
            state_dir.join("other-feature.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();

        let (result, exit_code) = build_context(tmp.path(), Some("other-feature"));
        assert_eq!(result["status"], "ok");
        assert_eq!(result["current_phase"], "flow-code");
        assert_eq!(result["branch"], "other-feature");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_ok_includes_panel_and_metadata() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);

        let mut state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(60);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(600);
        state["notes"] = json!([{"text": "note 1"}, {"text": "note 2"}]);
        fs::write(
            state_dir.join("test-feature.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();

        let (result, exit_code) = build_context(tmp.path(), Some("test-feature"));
        assert_eq!(result["status"], "ok");
        let panel = result["panel"].as_str().unwrap();
        assert!(panel.contains("Phase 4"), "Panel should contain Phase 4: {}", panel);
        assert!(panel.contains("Notes   : 2"), "Panel should contain notes count: {}", panel);
        assert_eq!(result["phase_name"], "Code Review");
        assert_eq!(result["phase_command"], "/flow:flow-code-review");
        assert_eq!(result["worktree"], ".worktrees/test-feature");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_commands_match_phase_config() {
        let commands = phase_config::commands();
        let phases_json_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("flow-phases.json");
        let content = fs::read_to_string(&phases_json_path).unwrap();
        let config: Value = serde_json::from_str(&content).unwrap();
        let phases = config["phases"].as_object().unwrap();
        for (key, phase_data) in phases {
            let expected_cmd = phase_data["command"].as_str().unwrap();
            let actual_cmd = commands.get(key.as_str()).unwrap();
            assert_eq!(
                actual_cmd, expected_cmd,
                "Command mismatch for phase {}: expected {}, got {}",
                key, expected_cmd, actual_cmd
            );
        }
    }

    #[test]
    fn test_corrupt_json_returns_no_state() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);
        fs::write(state_dir.join("test-feature.json"), "{bad json").unwrap();

        let (result, exit_code) = build_context(tmp.path(), Some("test-feature"));
        assert_eq!(result["status"], "no_state");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn test_multiple_features_include_metadata() {
        let tmp = TempDir::new().unwrap();
        let state_dir = setup_state_dir(&tmp);

        let mut state_a = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state_a["branch"] = json!("feature-a");
        fs::write(
            state_dir.join("feature-a.json"),
            serde_json::to_string_pretty(&state_a).unwrap(),
        )
        .unwrap();

        let mut state_b = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state_b["branch"] = json!("feature-b");
        fs::write(
            state_dir.join("feature-b.json"),
            serde_json::to_string_pretty(&state_b).unwrap(),
        )
        .unwrap();

        let (result, exit_code) = build_context(tmp.path(), Some("nonexistent"));
        assert_eq!(result["status"], "multiple_features");
        let features = result["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);

        // Each feature should have expected metadata
        for f in features {
            assert!(f["feature"].as_str().is_some());
            assert!(f["branch"].as_str().is_some());
            assert!(f["current_phase"].as_str().is_some());
            assert!(f["phase_name"].as_str().is_some());
            assert!(f["worktree"].as_str().is_some());
        }
        assert_eq!(exit_code, 0);
    }
}
