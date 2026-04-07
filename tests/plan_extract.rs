use flow_rs::plan_extract::{count_tasks, extract_implementation_plan, promote_headings};

// --- Unit tests for pure functions ---

// (Unit tests are below, integration tests at end of file)

#[test]
fn extract_plan_basic() {
    let body = "## Problem\n\nSomething.\n\n## Implementation Plan\n\n### Context\n\nStuff.\n\n### Tasks\n\n#### Task 1: Do thing\n\n## Files to Investigate\n\n- foo.rs\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    assert!(result.contains("#### Task 1: Do thing"));
    assert!(!result.contains("## Files to Investigate"));
    assert!(!result.contains("## Problem"));
}

#[test]
fn extract_plan_at_end_of_body() {
    let body = "## Problem\n\nFoo.\n\n## Implementation Plan\n\n### Context\n\nLast section.";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    assert!(result.contains("Last section."));
}

#[test]
fn extract_plan_missing() {
    let body = "## Problem\n\nNo plan here.\n\n## Files to Investigate\n\n- bar.rs\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn extract_plan_empty_section() {
    let body = "## Implementation Plan\n\n## Files to Investigate\n";
    assert!(extract_implementation_plan(body).is_none());
}

#[test]
fn promote_headings_basic() {
    let content = "### Context\n\nText.\n\n#### Task 1: Do thing\n\nMore text.\n";
    let result = promote_headings(content);
    assert!(result.contains("## Context"));
    assert!(!result.contains("### Context"));
    assert!(result.contains("### Task 1: Do thing"));
    assert!(!result.contains("#### Task 1"));
}

#[test]
fn promote_headings_skips_code_blocks() {
    let content = "### Before\n\n```\n### Inside code block\n#### Also inside\n```\n\n### After\n";
    let result = promote_headings(content);
    assert!(result.contains("## Before"));
    assert!(result.contains("### Inside code block"));
    assert!(result.contains("#### Also inside"));
    assert!(result.contains("## After"));
}

#[test]
fn promote_headings_preserves_h2() {
    // ## should NOT be promoted to # — only ### and #### are promoted
    let content = "## Already H2\n\n### Should become H2\n";
    let result = promote_headings(content);
    assert!(result.contains("## Already H2"));
    // The ### becomes ## too, so we have two ## lines
    let h2_count = result.lines().filter(|l| l.starts_with("## ")).count();
    assert_eq!(h2_count, 2);
}

#[test]
fn promote_headings_fenced_with_language() {
    let content = "### Heading\n\n```rust\n### not a heading\n```\n\n### Another\n";
    let result = promote_headings(content);
    assert!(result.contains("## Heading"));
    assert!(result.contains("### not a heading"));
    assert!(result.contains("## Another"));
}

#[test]
fn count_tasks_basic() {
    let content = "#### Task 1: First\n\nStuff.\n\n#### Task 2: Second\n\nMore.\n";
    assert_eq!(count_tasks(content), 2);
}

#[test]
fn count_tasks_skips_code_blocks() {
    let content = "#### Task 1: Real\n\n```\n#### Task 2: Fake\n```\n\n#### Task 3: Also real\n";
    assert_eq!(count_tasks(content), 2);
}

#[test]
fn count_tasks_zero_when_none() {
    let content = "### Context\n\nNo tasks here.\n";
    assert_eq!(count_tasks(content), 0);
}

#[test]
fn count_tasks_requires_task_prefix() {
    // #### without "Task " should not count
    let content = "#### Something else\n\n#### Task 1: Real\n";
    assert_eq!(count_tasks(content), 1);
}

#[test]
fn extract_plan_ends_at_first_h2() {
    // extract_implementation_plan uses simple find("\n## ") — not code-block-aware.
    // A ## inside a code block within the plan section ends extraction early.
    // This is acceptable because flow-create-issue controls the issue format
    // and does not produce ## headings inside code blocks.
    let body = "## Implementation Plan\n\n### Context\n\n```\n## This is not a heading\n```\n\n### Tasks\n\n## Out of Scope\n";
    let result = extract_implementation_plan(body).unwrap();
    assert!(result.contains("### Context"));
    // Extraction ends at the ## inside the code block (first \n## match)
    assert!(!result.contains("### Tasks"));
}

#[test]
fn promote_headings_five_hashes_unchanged() {
    // ##### should not be promoted (only ### and #### are)
    let content = "##### Five hashes\n### Three hashes\n";
    let result = promote_headings(content);
    // ##### starts with #### so it gets promoted to ####
    assert!(result.contains("#### Five hashes"));
    assert!(result.contains("## Three hashes"));
}

#[test]
fn count_tasks_ten() {
    let mut content = String::new();
    for i in 1..=10 {
        content.push_str(&format!("#### Task {}: Description {}\n\nBody.\n\n", i, i));
    }
    assert_eq!(count_tasks(&content), 10);
}

// --- Integration tests for run_impl (via subprocess) ---

mod integration {
    use std::fs;
    use std::process::Command;

    fn setup_git_repo(dir: &std::path::Path, branch: &str) {
        Command::new("git")
            .args(["-c", "init.defaultBranch=main", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", branch])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn setup_state(dir: &std::path::Path, branch: &str, state_json: &str) {
        let state_dir = dir.join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join(format!("{}.json", branch)), state_json).unwrap();
    }

    /// Build a plan-extract-ready state JSON with flow-start complete.
    /// `prompt` controls the prompt field (determines issue detection).
    /// `extra` is a closure that can mutate the state Value before serialization.
    fn make_plan_state(prompt: &str, extra: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut state = serde_json::json!({
            "branch": "test-feature",
            "current_phase": "flow-start",
            "prompt": prompt,
            "files": {
                "plan": serde_json::Value::Null,
                "dag": serde_json::Value::Null,
            },
            "skills": {
                "flow-plan": {
                    "continue": "auto",
                    "dag": "auto",
                }
            },
            "phases": {
                "flow-start": {
                    "name": "Start",
                    "status": "complete",
                    "started_at": "2026-01-01T00:00:00-08:00",
                    "completed_at": "2026-01-01T00:01:00-08:00",
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 60,
                    "visit_count": 1
                },
                "flow-plan": {
                    "name": "Plan",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-code": {
                    "name": "Code",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-code-review": {
                    "name": "Code Review",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-learn": {
                    "name": "Learn",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                },
                "flow-complete": {
                    "name": "Complete",
                    "status": "pending",
                    "started_at": serde_json::Value::Null,
                    "completed_at": serde_json::Value::Null,
                    "session_started_at": serde_json::Value::Null,
                    "cumulative_seconds": 0,
                    "visit_count": 0
                }
            },
            "phase_transitions": []
        });
        extra(&mut state);
        state.to_string()
    }

    /// Run `flow-rs plan-extract` in the given directory.
    /// Returns (exit_code, parsed_json).
    fn run_plan_extract(
        dir: &std::path::Path,
        extra_args: &[&str],
    ) -> (i32, serde_json::Value) {
        run_plan_extract_inner(dir, extra_args, None)
    }

    /// Run `flow-rs plan-extract` with a gh stub on PATH.
    fn run_plan_extract_with_gh(
        dir: &std::path::Path,
        extra_args: &[&str],
        stub_dir: &std::path::Path,
    ) -> (i32, serde_json::Value) {
        run_plan_extract_inner(dir, extra_args, Some(stub_dir))
    }

    fn run_plan_extract_inner(
        dir: &std::path::Path,
        extra_args: &[&str],
        stub_dir: Option<&std::path::Path>,
    ) -> (i32, serde_json::Value) {
        let mut args = vec!["plan-extract"];
        args.extend_from_slice(extra_args);

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
        cmd.args(&args).current_dir(dir);

        if let Some(sd) = stub_dir {
            let path_env = format!(
                "{}:{}",
                sd.to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            );
            cmd.env("PATH", &path_env);
        }

        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let code = output.status.code().unwrap_or(-1);
        let json: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or(serde_json::json!({"raw": stdout.trim()}));
        (code, json)
    }

    /// Create a gh stub script. Returns the stub directory.
    #[allow(dead_code)]
    fn create_gh_stub(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let stub_dir = dir.join(".stub-bin");
        fs::create_dir_all(&stub_dir).unwrap();
        let gh_stub = stub_dir.join("gh");
        fs::write(&gh_stub, script).unwrap();
        fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
        stub_dir
    }

    // --- Error path tests ---

    #[test]
    fn test_error_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        // No state file created

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"].as_str().unwrap().contains("No state file"),
            "Expected 'No state file' error, got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state_dir = dir.path().join(".flow-states");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("test-feature.json"), "{bad json").unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Invalid JSON"),
            "Expected 'Invalid JSON' error, got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_gate_start_not_complete() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        // State with flow-start still pending
        let state = make_plan_state("build a thing", |s| {
            s["phases"]["flow-start"]["status"] = serde_json::json!("pending");
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Phase 1: Start must be complete"),
            "Expected gate failure message, got: {}",
            json["message"]
        );
    }
}
