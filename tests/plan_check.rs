//! Integration tests for `bin/flow plan-check`.
//!
//! Drives the scanner + subcommand + state-file integration end-to-end.
//! Each test spawns `flow-rs plan-check` in a temp git repo with a
//! state file pointing at a plan file, then asserts on the JSON output.
//!
//! The scanner rejects plan files that use universal-coverage language
//! ("every subcommand", "all runners", etc.) without a named
//! enumeration of the concrete siblings nearby. See
//! `src/scope_enumeration.rs` for the trigger vocabulary and the
//! enumeration-present heuristic.
//!
//! **Exit code convention.** Infrastructure errors (unreadable state
//! file, corrupt JSON) return `Err(String)` from `run_impl` and exit
//! the process with code 1. Business responses (clean plan,
//! violations found, missing state, missing plan file) return
//! `Ok(Value)` with a `status` field and exit 0 — the skill consumer
//! branches on the JSON, not the shell exit code.

use std::fs;
use std::path::{Path, PathBuf};

use flow_rs::duplicate_test_coverage;
use flow_rs::plan_check::{
    build_violation_message, duplicate_violation_to_tagged_json, resolve_plan_file_from_state,
    resolve_plan_file_override, run_impl, Args,
};
use std::process::Command;

mod common;

use common::flow_states_dir;

/// Initialize a bare git repo in `dir` with a `main` branch and a dummy
/// commit. Plan-check only needs the state file, but `project_root()`
/// requires `.git/` to exist.
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

/// Write the state file under `.flow-states/<branch>/state.json`.
fn write_state(dir: &std::path::Path, branch: &str, plan_rel: Option<&str>) {
    let branch_dir = flow_states_dir(dir).join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let state = serde_json::json!({
        "branch": branch,
        "current_phase": "flow-plan",
        "files": {
            "plan": plan_rel.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
            "dag": serde_json::Value::Null,
        },
        "phases": {
            "flow-start": {
                "name": "Start",
                "status": "complete",
                "cumulative_seconds": 60,
                "visit_count": 1
            },
            "flow-plan": {
                "name": "Plan",
                "status": "in_progress",
                "cumulative_seconds": 0,
                "visit_count": 1
            }
        }
    });
    fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();
}

/// Write a plan file at the given relative path under `dir`.
fn write_plan(dir: &std::path::Path, plan_rel: &str, content: &str) {
    let abs = dir.join(plan_rel);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&abs, content).unwrap();
}

/// Run `flow-rs plan-check` in the given directory and return the
/// (exit code, parsed JSON) tuple.
fn run_plan_check(dir: &std::path::Path, extra_args: &[&str]) -> (i32, serde_json::Value) {
    let mut args = vec!["plan-check"];
    args.extend_from_slice(extra_args);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(&args)
        .current_dir(dir)
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let code = output.status.code().unwrap_or(-1);
    let last = stdout.trim().lines().last().unwrap_or("");
    let json: serde_json::Value =
        serde_json::from_str(last).unwrap_or(serde_json::json!({"raw": stdout.trim()}));
    (code, json)
}

// --- OK path: enumerated or empty ---

#[test]
fn plan_check_passes_on_inline_parenthetical_enumeration() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n\
        `cwd_scope::enforce` runs on every subcommand that mutates state \
        (`phase-enter`, `phase-finalize`, `phase-transition`, `set-timestamp`, \
        `add-finding`).\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0, "expected exit 0 for enumerated plan, got {}", code);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_passes_on_forward_bullet_list_enumeration() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n\
        The same guard must be added to every sibling entry point in the family:\n\n\
        - `ci::run` — the CI runner\n\
        - `build::run` — the build runner\n\
        - `lint::run` — the lint runner\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_passes_on_backward_bullet_list_enumeration() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n\
        The empty-tools branch exists in two places:\n\n\
        - `ci::run_once` — non-retry path\n\
        - `ci::run_with_retry` — retry path\n\n\
        A test at each callsite should exercise the empty-tools path.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_passes_on_empty_plan() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", "");
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_passes_on_plan_without_universal_prose() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n### Task 1\n\nAdd a function `foo`.\n\n### Task 2\n\nAdd a test.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

// --- OK path: negation and opt-out skips ---

#[test]
fn plan_check_skips_negation() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\nDo not trace every caller of the function.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_skips_fenced_code_block() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n```text\nevery state mutator enforces the guard\n```\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_skips_with_open_ended_optout_comment() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n\
        <!-- scope-enumeration: open-ended -->\n\
        Test against every supported git version.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

#[test]
fn plan_check_skips_with_imperative_optout_comment() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\n\
        <!-- scope-enumeration: imperative -->\n\
        Grep for every caller of the function.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

// --- Error path: unenumerated universal claim ---

#[test]
fn plan_check_fails_on_unenumerated_universal_claim() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\nAdd the drift guard to every state mutator.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(
        code, 0,
        "business errors exit 0 with status=error, got code={} json={}",
        code, json
    );
    assert_eq!(json["status"], "error");
    let violations = json["violations"]
        .as_array()
        .expect("violations[] expected");
    assert!(!violations.is_empty(), "expected at least one violation");
    let first = &violations[0];
    assert!(first["phrase"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("every"));
    assert!(first["phrase"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("state mutator"));
    assert!(first["line"].as_u64().is_some());
    assert!(first["context"].as_str().is_some());
}

#[test]
fn plan_check_fails_on_all_runners_without_list() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\nApply FLOW_CI_RUNNING to all runners in the codebase.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "error");
}

#[test]
fn plan_check_fails_on_each_entry_point_without_list() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let plan = "## Tasks\n\nGate each CLI entry point with the permission check.\n";
    write_plan(dir.path(), ".flow-states/test-feature-plan.md", plan);
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "error");
}

// --- Error path: business error (missing state / plan file) ---

#[test]
fn plan_check_errors_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0, "business errors exit 0, got code={}", code);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("state file"));
}

#[test]
fn plan_check_errors_when_files_plan_null() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_state(dir.path(), "test-feature", None);

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("plan"));
}

#[test]
fn plan_check_errors_when_plan_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 0);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("not found"));
}

// --- --plan-file override ---

#[test]
fn plan_check_accepts_plan_file_override() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_state(dir.path(), "test-feature", None);
    let plan = "## Tasks\n\nAdd a test for `foo`.\n";
    let plan_path = dir.path().join("custom-plan.md");
    fs::write(&plan_path, plan).unwrap();

    let (code, json) = run_plan_check(
        dir.path(),
        &[
            "--branch",
            "test-feature",
            "--plan-file",
            plan_path.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(json["status"], "ok");
}

// --- Infrastructure error (exit 1) ---

/// When the state file contains invalid JSON, `run_impl` returns
/// `Err(String)` and `run()` hits the `json_error` + `exit(1)` branch
/// (lines 57-59). The error is written to stderr, not stdout.
#[test]
fn plan_check_invalid_json_state_exits_with_code_1() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    let branch_dir = flow_states_dir(dir.path()).join("test-feature");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{not valid json at all").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["plan-check", "--branch", "test-feature"])
        .current_dir(dir.path())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let code = output.status.code().unwrap_or(-1);
    assert_eq!(
        code, 1,
        "expected exit 1 for corrupt state file, got {}",
        code
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Invalid JSON"),
        "stdout should mention JSON parse failure, got: {}",
        stdout
    );
}

// --- library-level tests (migrated from inline) ---

#[test]
fn absolute_override_passes_through() {
    let root = Path::new("/tmp/fake-root");
    let resolved = resolve_plan_file_override(root, "/tmp/other/plan.md");
    assert_eq!(resolved, PathBuf::from("/tmp/other/plan.md"));
}

#[test]
fn relative_override_joins_project_root() {
    let root = Path::new("/tmp/fake-root");
    let resolved = resolve_plan_file_override(root, "custom/plan.md");
    assert_eq!(resolved, PathBuf::from("/tmp/fake-root/custom/plan.md"));
}

// Direct `violation_to_tagged_json` tests removed — the helper is
// now private. Its output shape is covered via `run_impl` tests
// that assert on the violation JSON returned by the full plan-check
// pipeline.

#[test]
fn message_names_only_scope_when_audit_count_is_zero() {
    let m = build_violation_message(2, 0, 0, 2);
    assert!(m.contains("2 universal-coverage"));
    assert!(m.contains("scope-enumeration.md"));
    assert!(!m.contains("panic/assert"));
    assert!(!m.contains("duplicate-test-coverage"));
}

#[test]
fn message_names_only_audit_when_scope_count_is_zero() {
    let m = build_violation_message(0, 3, 0, 3);
    assert!(m.contains("3 panic/assert"));
    assert!(m.contains("external-input-audit-gate.md"));
    assert!(!m.contains("universal-coverage"));
}

#[test]
fn message_names_only_duplicate_when_others_are_zero() {
    let m = build_violation_message(0, 0, 2, 2);
    assert!(m.contains("2 duplicate-test-coverage"));
    assert!(m.contains("duplicate-test-coverage.md"));
    assert!(!m.contains("universal-coverage"));
    assert!(!m.contains("panic/assert"));
}

#[test]
fn message_names_all_three_rules_when_all_have_violations() {
    let m = build_violation_message(2, 1, 3, 6);
    assert!(m.contains("2 universal-coverage"));
    assert!(m.contains("1 panic/assert"));
    assert!(m.contains("3 duplicate-test-coverage"));
    assert!(m.contains("scope-enumeration.md"));
    assert!(m.contains("external-input-audit-gate.md"));
    assert!(m.contains("duplicate-test-coverage.md"));
    assert!(m.contains("6 plan-check violation"));
}

#[test]
fn run_impl_aggregates_violations_from_both_scanners() {
    let tmp = std::env::temp_dir().join(format!("plan-check-lib-dual-{}.md", std::process::id()));
    let plan_content = "## Approach\n\n\
        Add the drift guard to every state mutator.\n\n\
        tighten FlowPaths::new to panic on empty branches.\n";
    std::fs::write(&tmp, plan_content).expect("write fixture plan");

    let args = Args {
        branch: None,
        plan_file: Some(tmp.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).expect("run_impl returns business response");
    let _ = std::fs::remove_file(&tmp);

    assert_eq!(result["status"], "error");
    let violations = result["violations"]
        .as_array()
        .expect("violations is an array");
    let rules: Vec<String> = violations
        .iter()
        .map(|v| v["rule"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(rules.iter().any(|r| r == "scope-enumeration"));
    assert!(rules.iter().any(|r| r == "external-input-audit"));
}

#[test]
fn duplicate_violation_json_shape_has_all_required_fields() {
    let v = duplicate_test_coverage::Violation {
        file: PathBuf::from("/tmp/plan.md"),
        line: 42,
        phrase: "foo_bar_baz_quux".to_string(),
        context: "Plan names `foo_bar_baz_quux` as a new test.".to_string(),
        existing_test: "test_foo_bar_baz_quux".to_string(),
        existing_file: "tests/hooks.rs:1499".to_string(),
    };
    let json = duplicate_violation_to_tagged_json(&v);
    assert_eq!(json["file"], "/tmp/plan.md");
    assert_eq!(json["line"], 42);
    assert_eq!(json["phrase"], "foo_bar_baz_quux");
    assert_eq!(json["rule"], "duplicate-test-coverage");
    assert_eq!(json["existing_test"], "test_foo_bar_baz_quux");
    assert_eq!(json["existing_file"], "tests/hooks.rs:1499");
}

#[test]
fn run_impl_does_not_panic_on_slash_branch() {
    let args = Args {
        branch: Some("feature/foo".to_string()),
        plan_file: None,
    };
    let result = run_impl(&args).expect("run_impl returns business response");
    assert_eq!(result["status"], "error");
}

#[test]
fn run_impl_plan_file_is_directory_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        branch: None,
        plan_file: Some(dir.path().to_string_lossy().to_string()),
    };
    let err = run_impl(&args).unwrap_err();
    assert!(err.contains("Could not read plan file"));
}

#[test]
fn run_impl_triggers_dup_violations() {
    let tmp = std::env::temp_dir().join(format!("plan-check-lib-dup-{}.md", std::process::id()));
    let plan_content =
        "## Tasks\n\n```rust\nfn claude_md_has_no_unenumerated_universal_claims() {\n}\n```\n";
    std::fs::write(&tmp, plan_content).expect("write fixture plan");

    let args = Args {
        branch: None,
        plan_file: Some(tmp.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).expect("run_impl returns business response");
    let _ = std::fs::remove_file(&tmp);

    assert_eq!(result["status"], "error");
    let violations = result["violations"]
        .as_array()
        .expect("violations is an array");
    let rules: Vec<String> = violations
        .iter()
        .map(|v| v["rule"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(rules.iter().any(|r| r == "duplicate-test-coverage"));
}

#[test]
fn resolve_plan_file_from_state_legacy_plan_file_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch = "test-legacy-fallback-lib";
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    std::fs::write(&state_path, r#"{"plan_file": "legacy-plan.md"}"#).unwrap();

    let result = resolve_plan_file_from_state(root, Some(branch));
    let path = result
        .expect("outer Result should be Ok")
        .expect("inner Result should be Ok");
    assert_eq!(path, root.join("legacy-plan.md"));
}

#[test]
fn resolve_plan_file_from_state_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch = "test-invalid-json-lib";
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    std::fs::write(&state_path, "{not valid json").unwrap();

    let result = resolve_plan_file_from_state(root, Some(branch));
    let err = result.unwrap_err();
    assert!(err.contains("Invalid JSON in state file"));
}

#[test]
fn resolve_plan_file_from_state_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let branch = "test-unreadable-lib";
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let state_path = branch_dir.join("state.json");
    std::fs::write(&state_path, r#"{"valid": true}"#).unwrap();

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&state_path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = resolve_plan_file_from_state(root, Some(branch));
    let _ = std::fs::set_permissions(&state_path, std::fs::Permissions::from_mode(0o644));

    let err = result.unwrap_err();
    assert!(err.contains("Could not read state file"));
}

#[test]
fn run_impl_returns_ok_when_both_scanners_clean() {
    let tmp = std::env::temp_dir().join(format!("plan-check-lib-clean-{}.md", std::process::id()));
    std::fs::write(&tmp, "## Approach\n\nA plain plan with no triggers.\n")
        .expect("write fixture plan");

    let args = Args {
        branch: None,
        plan_file: Some(tmp.to_string_lossy().to_string()),
    };
    let result = run_impl(&args).expect("run_impl returns business response");
    let _ = std::fs::remove_file(&tmp);

    assert_eq!(result["status"], "ok");
}
