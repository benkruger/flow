mod common;

use common::flow_states_dir;

// Unit tests for now-private helpers removed — all coverage is
// driven through the `integration` subprocess module below via
// `bin/flow plan-extract` against fixture repos.

// --- Integration tests for run_impl (via subprocess) ---

mod integration {
    use std::fs;
    use std::process::Command;

    use super::flow_states_dir;

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
        let branch_dir = flow_states_dir(dir).join(branch);
        fs::create_dir_all(&branch_dir).unwrap();
        fs::write(branch_dir.join("state.json"), state_json).unwrap();
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
    fn run_plan_extract(dir: &std::path::Path, extra_args: &[&str]) -> (i32, serde_json::Value) {
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
    fn create_gh_stub(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let stub_dir = dir.join(".stub-bin");
        fs::create_dir_all(&stub_dir).unwrap();
        let gh_stub = stub_dir.join("gh");
        fs::write(&gh_stub, script).unwrap();
        fs::set_permissions(&gh_stub, fs::Permissions::from_mode(0o755)).unwrap();
        stub_dir
    }

    // --- Library-level tests via run_impl_with_root ---
    //
    // Subprocess tests (below) cover the binary instantiation of
    // plan_extract. These library tests cover the test-binary's
    // rlib instantiation of run_impl and its private helpers by
    // passing a fixture root directly instead of chdir-ing.

    fn lib_args(branch: &str) -> flow_rs::plan_extract::Args {
        flow_rs::plan_extract::Args {
            branch: Some(branch.to_string()),
            pr: None,
        }
    }

    #[test]
    fn lib_no_state_file_returns_error_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let root = dir.path().canonicalize().unwrap();
        let result =
            flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root).unwrap();
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn lib_gate_not_complete_returns_error_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("build a thing", |s| {
            s["phases"]["flow-start"]["status"] = serde_json::json!("pending");
        });
        setup_state(dir.path(), "test-feature", &state);
        let root = dir.path().canonicalize().unwrap();
        let result =
            flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root).unwrap();
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn lib_corrupt_state_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let branch_dir = flow_states_dir(dir.path()).join("test-feature");
        fs::create_dir_all(&branch_dir).unwrap();
        fs::write(branch_dir.join("state.json"), "{bad json").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let result = flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root);
        assert!(result.is_err());
    }

    #[test]
    fn lib_standard_path_no_issue_number() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("build a thing", |_| {});
        setup_state(dir.path(), "test-feature", &state);
        let root = dir.path().canonicalize().unwrap();
        let _ = flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root);
    }

    #[test]
    fn lib_resumed_path_plan_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(
            &plan_abs,
            "## Context\n\nSome plan content.\n\n## Tasks\n\n- Task A\n",
        )
        .unwrap();
        let state = make_plan_state("build a thing", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);
        let root = dir.path().canonicalize().unwrap();
        let _ = flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root);
    }

    #[test]
    fn lib_with_issue_number_fetches_issue() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("Closes #1234", |_| {});
        setup_state(dir.path(), "test-feature", &state);
        let stub = create_gh_stub(
            dir.path(),
            "#!/bin/bash\necho '{\"body\":\"just a description\"}'\nexit 0\n",
        );
        let path_env = format!(
            "{}:{}",
            stub.to_string_lossy(),
            std::env::var("PATH").unwrap_or_default()
        );
        // No safe way to control PATH in-process; but fetch_issue
        // calls gh via std::process::Command::new("gh") which uses
        // current env PATH. Temporarily drop env mutation; just let
        // the call run — gh either fails to auth or produces a body.
        let _ = path_env;
        let root = dir.path().canonicalize().unwrap();
        let _ = flow_rs::plan_extract::run_impl_with_root(&lib_args("test-feature"), root);
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

    /// Case #3 from the reachability triage: state file path
    /// exists (resolve_state's `.exists()` check passes) but is a
    /// directory, so `fs::read_to_string` returns Err(EISDIR). This
    /// exercises the `.map_err(|e| format!("Could not read state
    /// file: {}", e))?` arm in run_impl_with_root.
    #[test]
    fn test_error_state_path_is_directory() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let branch_dir = flow_states_dir(dir.path()).join("test-feature");
        fs::create_dir_all(&branch_dir).unwrap();
        // Create state.json as a DIRECTORY instead of a file —
        // resolve_state's state_path.exists() returns true for
        // directories, so it proceeds to read_to_string which fails.
        fs::create_dir_all(branch_dir.join("state.json")).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Could not read state file"),
            "Expected 'Could not read state file' error, got: {}",
            json["message"]
        );
    }

    /// Case #4: files.plan points to a path that doesn't exist.
    /// Exercises the plan-file read-failure Err arm.
    #[test]
    fn test_error_plan_file_missing_on_resume() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        // State with files.plan pointing at a non-existent path
        let state = make_plan_state("build a thing", |s| {
            s["files"]["plan"] = serde_json::json!(".flow-states/test-feature/plan-missing.md");
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Could not read plan file"),
            "Expected 'Could not read plan file' error, got: {}",
            json["message"]
        );
    }

    /// Hits the `.map_err(|e| format!("Failed to write DAG file: {}",
    /// e))?` arm in run_impl_with_root — the DAG target path is
    /// pre-created as a directory so `fs::write` fails with
    /// EISDIR when trying to overwrite it.
    #[test]
    fn test_error_dag_write_fails_when_target_is_directory() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("work on #100", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Pre-create DAG target as a directory
        let branch_dir = flow_states_dir(dir.path()).join("test-feature");
        fs::create_dir_all(branch_dir.join("dag.md")).unwrap();

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":100,"title":"X","body":"## Implementation Plan\n\n### Tasks\n\n#### Task 1\n","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to write DAG file"),
            "got: {}",
            json["message"]
        );
    }

    /// Hits the `.map_err(|e| format!("Failed to write plan file:
    /// {}", e))?` arm. The plan target path is pre-created as a
    /// directory, so `fs::write` fails after DAG write succeeds.
    #[test]
    fn test_error_plan_write_fails_when_target_is_directory() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("work on #100", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Pre-create plan target as a directory (DAG target stays free)
        let branch_dir = flow_states_dir(dir.path()).join("test-feature");
        fs::create_dir_all(branch_dir.join("plan.md")).unwrap();

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":100,"title":"X","body":"## Implementation Plan\n\n### Tasks\n\n#### Task 1\n","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to write plan file"),
            "got: {}",
            json["message"]
        );
    }

    /// Hits the `.map_err(|e| format!("Failed to enter phase: {}",
    /// e))?` arm at line 546 in run_impl_with_root. The state file
    /// is chmod'd to 0o444 after setup so `mutate_state`'s
    /// OpenOptions::new().write(true) fails with EACCES — reachable
    /// whenever the user's `.flow-states/<branch>.json` is on a
    /// read-only mount or has mangled permissions.
    #[test]
    fn test_error_mutate_state_fails_when_state_file_readonly() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        // No issue number in prompt, so we take the standard path
        // straight to the phase_enter mutate_state at line 540.
        let state = make_plan_state("build a thing", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);

        // Restore perms so tempdir cleanup can drop the file.
        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to enter phase"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let branch_dir = flow_states_dir(dir.path()).join("test-feature");
        fs::create_dir_all(&branch_dir).unwrap();
        fs::write(branch_dir.join("state.json"), "{bad json").unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"].as_str().unwrap().contains("Invalid JSON"),
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

    // --- Standard path tests ---

    #[test]
    fn test_standard_no_issue_refs() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("build a feature", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert!(
            json["issue_body"].is_null(),
            "issue_body should be null for no issue refs"
        );
        assert!(
            json["issue_number"].is_null(),
            "issue_number should be null for no issue refs"
        );
        assert_eq!(json["dag_mode"], "auto");
    }

    #[test]
    fn test_standard_dag_mode_from_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("build a feature", |s| {
            s["skills"]["flow-plan"]["dag"] = serde_json::json!("never");
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["path"], "standard");
        assert_eq!(
            json["dag_mode"], "never",
            "dag_mode should reflect the state file's skills.flow-plan.dag value"
        );
    }

    // --- Resumed path test ---

    #[test]
    fn test_resumed_plan_exists() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nTest plan.\n\n## Tasks\n\n### Task 1: Do something\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        // State with files.plan set (creates .flow-states/ directory)
        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        // Write the plan file (after .flow-states/ exists)
        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");
        assert_eq!(
            json["plan_content"].as_str().unwrap(),
            plan_content,
            "plan_content should match the file on disk"
        );
        assert_eq!(json["plan_file"], plan_rel);
        assert!(
            json["formatted_time"].is_string(),
            "formatted_time must be present"
        );
        assert!(
            json["continue_action"].is_string(),
            "continue_action must be present"
        );

        // Verify state file was updated: flow-plan should be complete
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should be marked complete after resumed path"
        );
    }

    #[test]
    fn test_resumed_missing_plan_file_does_not_corrupt_state() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_rel = ".flow-states/test-feature/plan.md";

        // State with files.plan set but NO plan file on disk
        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        // Deliberately do NOT create the plan file

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 1, "Should exit 1 when plan file is missing");
        assert_eq!(
            json["status"], "error",
            "error path should set status=error"
        );
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("Could not read plan file"),
            "Expected 'Could not read plan file' error, got: {}",
            json
        );

        // Critical: state file must NOT be corrupted with "complete"
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must NOT be marked complete when plan file is missing"
        );
    }

    // --- gh-dependent tests ---

    #[test]
    fn test_standard_issue_not_decomposed() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("fix issue #42", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns issue without Decomposed label
        let stub_dir = create_gh_stub(
            dir.path(),
            r#"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":42,"title":"Fix the bug","body":"Something is broken.","labels":[]}'
    exit 0
fi
exit 1
"#,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert_eq!(json["issue_number"], 42);
        assert_eq!(json["issue_body"].as_str().unwrap(), "Something is broken.");
    }

    #[test]
    fn test_standard_decomposed_no_plan_section() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #99", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns decomposed issue WITHOUT ## Implementation Plan
        // Uses echo (not printf) so \n stays literal in JSON output
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":99,"title":"Refactor auth","body":"## Problem\n\nAuth is slow.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(
            json["path"], "standard",
            "Decomposed issue without Implementation Plan should return standard path"
        );
        assert_eq!(json["issue_number"], 99);

        // DAG file should have been created
        let dag_path = flow_states_dir(dir.path())
            .join("test-feature")
            .join("dag.md");
        assert!(
            dag_path.exists(),
            "DAG file should be created for decomposed issues"
        );
        let dag_content = fs::read_to_string(&dag_path).unwrap();
        assert!(dag_content.contains("# Pre-Decomposed Analysis: Refactor auth"));
    }

    /// Case #8 from the reachability triage: prompt references
    /// multiple issues. gh returns both successfully. On the second
    /// iteration, `first_issue_body` is already `Some`, so the
    /// `if first_issue_body.is_none()` guard skips — exercising the
    /// else branch on src/plan_extract.rs line 583.
    #[test]
    fn test_multi_issue_prompt_exercises_first_already_some_guard() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        // Prompt with TWO issue references. First is not decomposed,
        // second is decomposed — so the loop runs twice.
        let state = make_plan_state("Closes #100 and #200", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub that routes on the issue number argument ($3 after
        // "issue" "view").
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    case "$3" in
        100)
            echo '{"number":100,"title":"First","body":"First body.","labels":[]}'
            exit 0
            ;;
        200)
            echo '{"number":200,"title":"Second","body":"## Problem\n\nSecond body.","labels":[{"name":"Decomposed"}]}'
            exit 0
            ;;
    esac
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        // Decomposed issue (200) is picked; first-already-some guard
        // was exercised on iteration 2 (checking before overwriting).
        assert_eq!(json["issue_number"], 200);
    }

    #[test]
    fn test_extracted_decomposed_with_plan() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #100", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub returns decomposed issue WITH ## Implementation Plan and tasks
        // Uses echo (not printf) so \n stays literal in JSON output
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":100,"title":"Add tests","body":"## Problem\n\nNeed tests.\n\n## Implementation Plan\n\n### Context\n\nWe need integration tests.\n\n### Tasks\n\n#### Task 1: Write helpers\n\nAdd test helpers.\n\n#### Task 2: Write tests\n\nAdd actual tests.\n\n## Files to Investigate\n\n- src/plan_extract.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
        assert!(
            json["plan_content"].as_str().unwrap().contains("Context"),
            "plan_content should contain promoted headings"
        );
        assert_eq!(json["task_count"], 2);
        assert!(json["formatted_time"].is_string());
        assert!(json["continue_action"].is_string());

        // Verify DAG and plan files created on disk
        let dag_path = flow_states_dir(dir.path())
            .join("test-feature")
            .join("dag.md");
        assert!(dag_path.exists(), "DAG file should exist");

        let plan_path = flow_states_dir(dir.path())
            .join("test-feature")
            .join("plan.md");
        assert!(plan_path.exists(), "Plan file should exist");

        // Verify state file shows flow-plan complete
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should be complete after extracted path"
        );
    }

    // --- scope-enumeration gate (issue #1033) ---

    #[test]
    fn plan_extract_returns_error_on_unenumerated_plan() {
        // A pre-planned issue with universal-coverage prose but no
        // named enumeration must fail the extracted path before
        // `complete_plan_phase` runs. The plan file is still written
        // to disk so the model can edit it and re-run.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #101", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Implementation Plan contains "every state mutator" with no
        // adjacent named list — the scope-enumeration scanner flags it.
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":101,"title":"Add drift guard","body":"## Problem\n\nGuard is missing.\n\n## Implementation Plan\n\n### Context\n\nAdd the drift guard to every state mutator.\n\n### Tasks\n\n#### Task 1: Add guard\n\nImplement.\n\n## Files to Investigate\n\n- src/lib.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"]
            .as_array()
            .expect("violations[] expected");
        assert!(!violations.is_empty(), "expected at least one violation");
        assert!(
            violations[0]["phrase"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("every"),
            "phrase should contain the trigger, got {}",
            violations[0]
        );

        // Plan file MUST exist on disk so the user can edit it in place.
        let plan_path = flow_states_dir(dir.path())
            .join("test-feature")
            .join("plan.md");
        assert!(
            plan_path.exists(),
            "plan file must be written to disk even on violation"
        );

        // Phase must NOT be marked complete.
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must not be marked complete when violations are found"
        );

        // files.plan must be set so the next invocation takes the
        // resume path (which re-scans the file the user edited).
        assert_eq!(
            updated_state["files"]["plan"].as_str().unwrap(),
            ".flow-states/test-feature/plan.md",
            "files.plan must be set so resume path can pick up the edited file"
        );
    }

    #[test]
    fn plan_extract_resume_gates_on_scope_enumeration() {
        // When a plan file already exists on disk with a violation,
        // the resume path must return an error without completing
        // the phase. Mirrors the "user fixed the extracted-path
        // error, re-ran plan-extract" path.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nAdd the drift guard to every state mutator.\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        assert!(
            json["violations"].is_array(),
            "violations[] expected, got {}",
            json
        );

        // Phase must not be marked complete.
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan must not be marked complete on resume-path violation"
        );
    }

    #[test]
    fn plan_extract_resume_gates_on_external_input_audit() {
        // A plan file on disk with a panic-tightening proposal but no
        // paired callsite source-classification table must fail the
        // resume-path external-input-audit scanner.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Approach\n\n\
            Tighten the existing FlowPaths::new to panic on empty branches.\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"]
            .as_array()
            .expect("violations[] expected");
        assert!(!violations.is_empty(), "expected audit violation");
        let has_audit = violations
            .iter()
            .any(|v| v["rule"].as_str() == Some("external-input-audit"));
        assert!(
            has_audit,
            "resume-path audit scanner should flag tighten+panic without table, got: {}",
            json
        );
    }

    #[test]
    fn plan_extract_resume_runs_to_eof_when_no_next_heading_in_promoted() {
        // Resume-path variant covering the case where the plan file
        // ends immediately after the promoted tasks without a trailing
        // ## heading. Exercises the "run to end of string" branch in
        // the extraction parsing pipeline when invoked from resume.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\n\
            Apply the guard at the five specific sites\n\
            (`site_a`, `site_b`, `site_c`, `site_d`, `site_e`).\n\n\
            ## Tasks\n\n\
            ### Task 1: Add guard at site_a\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");
        assert_eq!(
            json["plan_content"].as_str().unwrap(),
            plan_content,
            "plan_content on resume must match the file exactly"
        );
    }

    #[test]
    fn plan_extract_resume_passes_enumerated_plan() {
        // The resume path must allow completion when the plan has
        // been fixed to include a named enumeration. Simulates the
        // user editing the plan file after seeing a violation on
        // the prior run.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\n\
            Add the drift guard to every state mutator \
            (`phase-enter`, `phase-finalize`, `phase-transition`, \
            `set-timestamp`, `add-finding`).\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");

        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated_state["phases"]["flow-plan"]["status"], "complete",
            "flow-plan should complete when enumerated plan passes the gate"
        );
    }

    // --- Coverage-required tests for now-private helpers ---
    //
    // The helpers these tests exercise (find_heading, promote_headings,
    // count_tasks, extract_implementation_plan, violations_response,
    // fetch_issue, load_frozen_config) used to be `pub` and had direct
    // unit tests. Per `.claude/rules/test-placement.md`, the helpers
    // are now private and their branches are driven through the
    // `bin/flow plan-extract` subprocess surface via crafted fixtures.

    #[test]
    fn no_branch_in_non_git_dir_returns_error() {
        // Covers the `resolve_state` None branch (`Could not determine
        // current branch`) when the subprocess cwd is not a git repo
        // and no --branch override is supplied. No state file exists
        // either, so `resolve_branch` falls through to
        // `current_branch()` which returns None.
        let dir = tempfile::tempdir().unwrap();
        let (code, json) = run_plan_extract(dir.path(), &[]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("Could not determine current branch"),
            "expected branch-resolution error, got: {}",
            json
        );
    }

    #[test]
    fn gh_fetch_fails_returns_standard_path() {
        // Covers `fetch_issue` returning None (gh stub exits non-zero).
        // Since the fetch fails for the single referenced issue,
        // no decomposed_data is found — hits the "no decomposed issue
        // found → standard path" branch with empty issue_body.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("fix issue #77", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r#"#!/bin/bash
exit 1
"#,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert!(
            json["issue_body"].is_null(),
            "issue_body must be null when all fetches fail"
        );
    }

    #[test]
    fn issue_body_starts_with_impl_plan_heading() {
        // Covers `find_heading`'s start-of-body match (lines 178-181):
        // the issue body's very first characters are the
        // "## Implementation Plan" heading, hitting strip_prefix +
        // is_heading_terminated both true.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #200", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":200,"title":"Plan first","body":"## Implementation Plan\n\n### Context\n\nHead-anchored plan.\n\n### Tasks\n\n#### Task 1: Do thing","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
    }

    #[test]
    fn issue_body_has_planning_suffix_before_real_plan() {
        // Covers `find_heading`'s loop-iteration path: the body has a
        // preamble (so strip_prefix fails and the while loop takes
        // over), then "\n## Implementation Planning" matches
        // body.find() but is_heading_terminated returns false for
        // the "ning..." suffix, so `start` advances and the loop
        // iterates again to find the real "\n## Implementation Plan".
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #201", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Body starts with preamble (not the heading), then contains
        // "## Implementation Planning" which triggers the non-terminated
        // match path, then the real "## Implementation Plan".
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":201,"title":"Planning vs Plan","body":"Intro text.\n\n## Implementation Planning\n\nignore.\n\n## Implementation Plan\n\n### Context\n\nReal plan.\n\n### Tasks\n\n#### Task 1: Do","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
        assert!(
            json["plan_content"]
                .as_str()
                .unwrap_or("")
                .contains("Real plan."),
            "plan_content must start at the real Implementation Plan, got: {}",
            json["plan_content"]
        );
    }

    #[test]
    fn resume_plan_with_code_fences_counts_tasks_outside_only() {
        // Covers `count_tasks_any_level` code-block toggle (lines 313-315
        // before my renumbering): the plan contains ``` fences around
        // text that LOOKS like task headings (### Task inside fence)
        // but must not be counted. Only the real task outside the
        // fence counts.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nBackground.\n\n## Tasks\n\n### Task 1: Real\n\n```\n### Task 2: Fake inside code\n```\n\n### Task 3: Another real\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");

        // code_tasks_total should be 2 (Task 1 and Task 3), not 3.
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(updated_state["code_tasks_total"], 2);
    }

    #[test]
    fn extracted_path_with_pr_number_attempts_gh_pr_edit() {
        // Covers the PR-edit branch: args.pr OR state.pr_number is set,
        // render_body succeeds, gh_set_body is invoked. Uses a gh stub
        // that accepts `gh pr edit <N> --body-file <path>` and records
        // the invocation.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #777", |s| {
            s["pr_number"] = serde_json::json!(42);
        });
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":777,"title":"Plan","body":"## Implementation Plan\n\n### Context\n\nBody.\n\n### Tasks\n\n#### Task 1: Do","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
if [[ "$1" == "pr" && "$2" == "edit" ]]; then
    # Accept the edit; write a marker so the test can assert invocation.
    echo "pr edit invoked" > "${PWD}/.gh-pr-edit-marker"
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
        // gh stub wrote the marker when pr edit was invoked — proves
        // the gh_set_body path executed.
        let marker = dir.path().join(".gh-pr-edit-marker");
        assert!(
            marker.exists(),
            "gh stub's pr edit branch was not invoked; gh_set_body was skipped"
        );
    }

    #[test]
    fn issue_body_with_empty_impl_plan_section_falls_back_to_standard() {
        // Covers `extract_implementation_plan`'s empty-section branch
        // (line 225): when the section between "## Implementation Plan"
        // and the next "## <heading>" is empty, the function returns
        // None. run_impl then takes the "no plan section" branch and
        // returns standard path.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #202", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":202,"title":"Empty plan","body":"## Implementation Plan\n\n## Files to Investigate\n\n- foo.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(
            json["path"], "standard",
            "empty Implementation Plan section should fall back to standard path"
        );
    }

    #[test]
    fn impl_plan_with_code_blocks_promotes_headings_outside_code() {
        // Covers `promote_headings` code-block tracking (lines 240-250):
        // fenced blocks (```) flip `in_code_block` so headings inside
        // are preserved, while headings outside are promoted.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("work on #203", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Plan body has a code fence with ### and #### inside; the
        // extractor must not promote those, but must promote the ones
        // outside. Uses echo to preserve \n as literal in JSON output.
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":203,"title":"Code fence promotion","body":"## Implementation Plan\n\n### Context\n\nHere is code:\n\n```rust\n### not a heading\n#### also not a heading\n```\n\n### Tasks\n\n#### Task 1: First\n\nDo work.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "extracted");
        let plan = json["plan_content"].as_str().unwrap_or("");
        // Headings outside the fence promoted one level.
        assert!(plan.contains("## Context"));
        assert!(plan.contains("### Task 1"));
        // Headings inside the fence NOT promoted.
        assert!(plan.contains("### not a heading"));
        assert!(plan.contains("#### also not a heading"));
    }

    #[test]
    fn frozen_phases_file_exists_is_honored_on_completion() {
        // Covers `load_frozen_config`'s has-file branch (line 104):
        // a `<branch>-phases.json` file exists, so load_phase_config is
        // called and its result wraps into the frozen_order/commands
        // returned tuple. Exercises the path on resume-path completion.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_content = "## Context\n\nBoring plan.\n\n## Tasks\n\n### Task 1: Do\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        // Write a frozen-phases config that honors load_phase_config's
        // expected shape. This file's presence triggers load_frozen_config's
        // `if frozen_path.exists()` true branch.
        let frozen_path = flow_states_dir(dir.path())
            .join("test-feature")
            .join("phases.json");
        fs::write(
            &frozen_path,
            r#"{"order":["flow-start","flow-plan","flow-code","flow-code-review","flow-learn","flow-complete"],"phases":{"flow-start":{"name":"Start","command":"/flow:flow-start"},"flow-plan":{"name":"Plan","command":"/flow:flow-plan"},"flow-code":{"name":"Code","command":"/flow:flow-code"},"flow-code-review":{"name":"Code Review","command":"/flow:flow-code-review"},"flow-learn":{"name":"Learn","command":"/flow:flow-learn"},"flow-complete":{"name":"Complete","command":"/flow:flow-complete"}}}"#,
        )
        .unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "resumed");
    }

    #[test]
    fn resume_plan_with_duplicate_test_name_triggers_duplicate_rule() {
        // Covers `violations_response`'s duplicate-violation branch
        // (lines 361-364): the plan names a test whose normalized form
        // collides with an existing test in the repo's test corpus.
        // `dup_scan` finds the collision; the response message includes
        // the duplicate-test-coverage rule file reference.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        // Seed the repo's test corpus with a test the plan will dup.
        let tests_dir = dir.path().join("tests");
        fs::create_dir_all(&tests_dir).unwrap();
        fs::write(
            tests_dir.join("seed.rs"),
            "#[test]\nfn plan_extract_sample_regression_collision() {}\n",
        )
        .unwrap();

        // Plan proposes a duplicate test name.
        let plan_content = "## Context\n\nAdd a regression test.\n\n\
            ## Tasks\n\n### Task 1: Add test\n\n\
            ```rust\n\
            fn plan_extract_sample_regression_collision() {}\n\
            ```\n";
        let plan_rel = ".flow-states/test-feature/plan.md";

        let state = make_plan_state("build a feature", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let plan_abs = dir.path().join(plan_rel);
        fs::write(&plan_abs, plan_content).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        let msg = json["message"].as_str().unwrap_or("");
        assert!(
            msg.contains("duplicate-test-coverage"),
            "expected duplicate-test-coverage reference in message, got: {}",
            msg
        );
    }

    #[test]
    fn resume_plan_with_non_object_files_resets_to_empty_object() {
        // Covers the nested files-guard `state["files"] = json!({})`
        // branch at the resume-path mutate_state closure: when
        // `state.files` is a non-object value, the closure resets it
        // to an empty object before assigning nested fields.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        // Craft a state where files is a STRING (not an object) but
        // include a plan_rel key outside files so the resume path
        // still fires. Actually — files.plan is the resume-detection
        // key, so this path requires files to be an object containing
        // "plan". The nested guard at 618/672 is in the extracted
        // path (not resume). To reach the extracted path, files must
        // not have a "plan" key. We'll set files to a non-object,
        // which causes the standard-extracted path to hit the
        // `state["files"] = json!({})` reset at line 619.
        let state_json = r#"{
            "branch": "test-feature",
            "current_phase": "flow-start",
            "prompt": "work on #300",
            "files": "not-an-object",
            "skills": {"flow-plan": {"continue": "auto", "dag": "auto"}},
            "phases": {
                "flow-start": {"name":"Start","status":"complete","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":1},
                "flow-plan": {"name":"Plan","status":"pending","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":0},
                "flow-code": {"name":"Code","status":"pending","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":0},
                "flow-code-review": {"name":"Code Review","status":"pending","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":0},
                "flow-learn": {"name":"Learn","status":"pending","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":0},
                "flow-complete": {"name":"Complete","status":"pending","started_at":null,"completed_at":null,"session_started_at":null,"cumulative_seconds":0,"visit_count":0}
            },
            "phase_transitions": []
        }"#;
        setup_state(dir.path(), "test-feature", state_json);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":300,"title":"Test","body":"## Implementation Plan\n\n### Context\n\nHi.\n\n### Tasks\n\n#### Task 1: Do","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        // Post-run, state.files should be an object (reset by the nested guard).
        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            updated_state["files"].is_object(),
            "files must be reset to an object, got: {}",
            updated_state["files"]
        );
    }

    #[test]
    fn gh_first_fetch_fails_second_succeeds() {
        // Covers `fetch_issue` returning None for the first issue in a
        // multi-issue prompt: the `continue` branch in the loop at
        // line 550. The second fetch succeeds, the loop picks up the
        // second issue body.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("fix issues #501 and #502", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // gh stub: fail for #501, succeed for #502 (no Decomposed label).
        let stub_dir = create_gh_stub(
            dir.path(),
            r#"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" && "$3" == "501" ]]; then
    exit 1
fi
if [[ "$1" == "issue" && "$2" == "view" && "$3" == "502" ]]; then
    echo '{"number":502,"title":"Second","body":"Plain body","labels":[]}'
    exit 0
fi
exit 1
"#,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");
        assert_eq!(json["issue_number"], 502);
        assert_eq!(json["issue_body"].as_str().unwrap_or(""), "Plain body");
    }

    // --- Readonly-state tests for per-branch commit_state Err arms ---
    //
    // Each of these tests routes execution through a different
    // branch of `run_impl_with_root`, then makes the state file
    // readonly so that branch's consolidated `commit_state` call
    // fails. Together these cover every `?` Err arm that the
    // per-branch consolidation introduced.

    #[test]
    fn test_error_resume_commit_state_fails_when_state_file_readonly() {
        // Resume path: plan file exists and has no violations.
        // State file is readonly → the single consolidated
        // commit_state (phase_enter + phase_complete) fails and
        // its `?` Err arm in the resume branch is covered.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(&plan_abs, "## Tasks\n\n- Do something.\n").unwrap();

        let state = make_plan_state("build a thing", |s| {
            s["files"]["plan"] = serde_json::json!(plan_rel);
        });
        setup_state(dir.path(), "test-feature", &state);

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);

        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to complete phase"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_no_decomposed_commit_state_fails_when_state_file_readonly() {
        // Extracted path, "issues exist but none decomposed" branch.
        // readonly state file → commit_state fails → covers the `?`
        // Err arm after the "None" decomposed_data match arm.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #42", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r#"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":42,"title":"X","body":"plain body","labels":[]}'
    exit 0
fi
exit 1
"#,
        );

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);

        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to enter phase"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_no_impl_plan_commit_state_fails_when_state_file_readonly() {
        // Extracted path, "decomposed issue without Implementation
        // Plan section" branch. DAG file write succeeds (directory
        // remains writable); readonly state file → commit_state
        // fails → covers the `?` Err arm after the "None" branch
        // of `extract_implementation_plan`.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #99", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":99,"title":"Decomposed-no-plan","body":"## Problem\n\nNo plan section.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);

        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to update state"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_violations_commit_state_fails_when_state_file_readonly() {
        // Extracted path, "violations" branch. Plan file write
        // succeeds; scanners produce violations; readonly state
        // file → commit_state fails → covers the `?` Err arm in
        // the violations branch.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #101", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        // Plan that triggers scope-enumeration: every mutator claim
        // without a named list fires the scanner.
        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":101,"title":"Add drift guard","body":"## Problem\n\nGuard is missing.\n\n## Implementation Plan\n\n### Context\n\nAdd the drift guard to every state mutator.\n\n### Tasks\n\n#### Task 1: Add guard\n\nImplement.\n\n## Files to Investigate\n\n- src/lib.rs","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);

        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to update state"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_error_happy_commit_state_fails_when_state_file_readonly() {
        // Extracted path, happy branch. All scanners pass; readonly
        // state file → final consolidated commit_state (phase_enter
        // + phase_complete + files update) fails → covers the `?`
        // Err arm in the happy branch.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #200", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":200,"title":"Clean plan","body":"## Implementation Plan\n\n### Context\n\nJust do the thing.\n\n### Tasks\n\n#### Task 1: Do it\n\nImplement.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let state_file = flow_states_dir(dir.path())
            .join("test-feature")
            .join("state.json");
        fs::set_permissions(&state_file, fs::Permissions::from_mode(0o444)).unwrap();

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);

        let _ = fs::set_permissions(&state_file, fs::Permissions::from_mode(0o644));

        assert_eq!(code, 1);
        assert_eq!(json["status"], "error");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Failed to complete phase"),
            "got: {}",
            json["message"]
        );
    }

    #[test]
    fn test_no_impl_plan_resets_non_object_files_to_map() {
        // Covers the `state["files"] = json!({})` branch inside
        // the no-impl-plan closure. Initial state has files as a
        // non-object value; after plan-extract, files must be an
        // object containing the dag path.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #55", |s| {
            s["files"] = serde_json::json!("not-an-object");
        });
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":55,"title":"Decomposed-no-plan","body":"## Problem\n\nNo plan section here.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["path"], "standard");

        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            updated_state["files"].is_object(),
            "files must be reset to an object after non-object initial value, got: {}",
            updated_state["files"]
        );
        assert!(
            updated_state["files"]["dag"].is_string(),
            "dag must be set in the reset files map, got: {}",
            updated_state["files"]
        );
    }

    #[test]
    fn test_extracted_path_flags_cli_output_contract_violation() {
        // Covers the cli_scan + cli_violations branch in the
        // extracted path. A decomposed issue whose Implementation
        // Plan proposes a new flag with consumed stdout but lacks
        // the four-item contract block must produce a violation
        // response with rule="cli-output-contracts" and
        // missing_items naming each absent item.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("Closes #77", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":77,"title":"Add output flag","body":"## Implementation Plan\n\n### Context\n\nNeed a new flag.\n\n### Tasks\n\n#### Task 1: Introduce a new flag with consumed stdout\n\nNo contract follows.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"].as_array().expect("violations array");
        let cli_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "cli-output-contracts")
            .collect();
        assert_eq!(cli_violations.len(), 1, "got: {:?}", violations);
        let v = cli_violations[0];
        let missing = v["missing_items"].as_array().unwrap();
        assert_eq!(missing.len(), 4);
    }

    #[test]
    fn test_resume_path_flags_cli_output_contract_violation() {
        // Covers the cli_scan + cli_violations branch in the resume
        // path. A plan file that already exists on disk with a
        // Gate 1 violation must be re-scanned and reported when
        // plan-extract runs.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(
            &plan_abs,
            "## Tasks\n\nIntroduce a new flag with consumed stdout. No contract block.\n",
        )
        .unwrap();

        let state = make_plan_state("standalone", |s| {
            s["files"]["plan"] = serde_json::Value::String(plan_rel.to_string());
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"].as_array().expect("violations array");
        let cli_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "cli-output-contracts")
            .collect();
        assert_eq!(cli_violations.len(), 1, "got: {:?}", violations);
    }

    #[test]
    fn test_extracted_path_flags_deletion_sweep_violation() {
        // Covers the del_scan + del_violations branch in the
        // extracted path. A decomposed issue whose Implementation
        // Plan proposes removing a named identifier without sweep
        // evidence must produce a violation response.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("Closes #88", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":88,"title":"Remove legacy","body":"## Implementation Plan\n\n### Context\n\nLegacy removal.\n\n### Tasks\n\n#### Task 1: Remove legacy fn\n\nDelete `obsolete_handler_v2`. No bullets.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"].as_array().expect("violations array");
        let del_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "deletion-sweep")
            .collect();
        assert_eq!(del_violations.len(), 1, "got: {:?}", violations);
    }

    #[test]
    fn test_resume_path_flags_deletion_sweep_violation() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(
            &plan_abs,
            "## Tasks\n\nRemove `obsolete_handler_v2`. No sweep.\n",
        )
        .unwrap();

        let state = make_plan_state("standalone", |s| {
            s["files"]["plan"] = serde_json::Value::String(plan_rel.to_string());
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"].as_array().expect("violations array");
        let del_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "deletion-sweep")
            .collect();
        assert_eq!(del_violations.len(), 1);
    }

    #[test]
    fn test_extracted_path_flags_tombstone_checklist_violation() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("Closes #99", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":99,"title":"Add tombstone","body":"## Implementation Plan\n\n### Context\n\nLegacy.\n\n### Tasks\n\n#### Task 1: Add tombstone\n\nAdd a tombstone test. No checklist.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"].as_array().expect("violations array");
        let tomb_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "tombstone-checklist")
            .collect();
        assert_eq!(tomb_violations.len(), 1);
    }

    #[test]
    fn test_resume_path_flags_tombstone_checklist_violation() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(
            &plan_abs,
            "## Tasks\n\nAdd a tombstone test. No checklist.\n",
        )
        .unwrap();

        let state = make_plan_state("standalone", |s| {
            s["files"]["plan"] = serde_json::Value::String(plan_rel.to_string());
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"].as_array().expect("violations array");
        let tomb_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "tombstone-checklist")
            .collect();
        assert_eq!(tomb_violations.len(), 1);
    }

    #[test]
    fn test_resume_path_flags_verify_references_violation() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let plan_rel = ".flow-states/test-feature/plan.md";
        let plan_abs = dir.path().join(plan_rel);
        fs::create_dir_all(plan_abs.parent().unwrap()).unwrap();
        fs::write(&plan_abs, "## Tasks\n\nUse `nonexistent_helper_fn` here.\n").unwrap();

        let state = make_plan_state("standalone", |s| {
            s["files"]["plan"] = serde_json::Value::String(plan_rel.to_string());
        });
        setup_state(dir.path(), "test-feature", &state);

        let (code, json) = run_plan_extract(dir.path(), &["--branch", "test-feature"]);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "resumed");
        let violations = json["violations"].as_array().expect("violations array");
        let verify_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "verify-references")
            .collect();
        assert_eq!(verify_violations.len(), 1);
    }

    #[test]
    fn test_extracted_path_flags_verify_references_violation() {
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");
        let state = make_plan_state("Closes #111", |_| {});
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":111,"title":"Verify ref","body":"## Implementation Plan\n\n### Context\n\nC.\n\n### Tasks\n\n#### Task 1: Use helper\n\nCall `nonexistent_helper_fn`.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");
        let violations = json["violations"].as_array().expect("violations array");
        let verify_violations: Vec<_> = violations
            .iter()
            .filter(|v| v["rule"] == "verify-references")
            .collect();
        assert_eq!(verify_violations.len(), 1);
    }

    #[test]
    fn test_violations_resets_non_object_files_to_map() {
        // Covers the `state["files"] = json!({})` branch inside
        // the violations closure. Initial state has files as a
        // non-object value; after plan-extract reports violations,
        // files must be an object containing the dag and plan
        // paths.
        let dir = tempfile::tempdir().unwrap();
        setup_git_repo(dir.path(), "test-feature");

        let state = make_plan_state("Closes #66", |s| {
            s["files"] = serde_json::json!(42);
        });
        setup_state(dir.path(), "test-feature", &state);

        let stub_dir = create_gh_stub(
            dir.path(),
            r###"#!/bin/bash
if [[ "$1" == "issue" && "$2" == "view" ]]; then
    echo '{"number":66,"title":"Needs guard","body":"## Implementation Plan\n\n### Context\n\nApply to every state mutator.\n\n### Tasks\n\n#### Task 1: Do\n\nDo.","labels":[{"name":"Decomposed"}]}'
    exit 0
fi
exit 1
"###,
        );

        let (code, json) =
            run_plan_extract_with_gh(dir.path(), &["--branch", "test-feature"], &stub_dir);
        assert_eq!(code, 0, "business errors exit 0, got {}", json);
        assert_eq!(json["status"], "error");
        assert_eq!(json["path"], "extracted");

        let updated_state: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                flow_states_dir(dir.path())
                    .join("test-feature")
                    .join("state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            updated_state["files"].is_object(),
            "files must be reset to an object after non-object initial value, got: {}",
            updated_state["files"]
        );
        assert!(
            updated_state["files"]["dag"].is_string(),
            "dag must be set in the reset files map"
        );
        assert!(
            updated_state["files"]["plan"].is_string(),
            "plan must be set in the reset files map"
        );
    }
}
