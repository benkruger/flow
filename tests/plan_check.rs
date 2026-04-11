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
//! **Ignore status.** These tests are currently `#[ignore]`-gated
//! because the `plan-check` subcommand does not yet exist. The
//! `#[ignore]` markers are removed in the commit that lands
//! `src/plan_check.rs`, `Commands::PlanCheck`, and the
//! `src/plan_extract.rs` integration. Until that commit, `bin/flow
//! test -- --ignored plan_check` surfaces these as the next TDD
//! target.

use std::fs;
use std::process::Command;

mod common;

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

/// Write the state file under `.flow-states/<branch>.json`.
fn write_state(dir: &std::path::Path, branch: &str, plan_rel: Option<&str>) {
    let state_dir = dir.join(".flow-states");
    fs::create_dir_all(&state_dir).unwrap();
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
    fs::write(
        state_dir.join(format!("{}.json", branch)),
        state.to_string(),
    )
    .unwrap();
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
        code, 1,
        "expected exit 1 for unenumerated plan, got {}",
        code
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
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
}

#[test]
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
}

// --- Error path: infrastructure failures ---

#[test]
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
fn plan_check_errors_when_state_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("state file"));
}

#[test]
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
fn plan_check_errors_when_files_plan_null() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_state(dir.path(), "test-feature", None);

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("plan"));
}

#[test]
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
fn plan_check_errors_when_plan_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_git_repo(dir.path(), "test-feature");
    write_state(
        dir.path(),
        "test-feature",
        Some(".flow-states/test-feature-plan.md"),
    );

    let (code, json) = run_plan_check(dir.path(), &["--branch", "test-feature"]);
    assert_eq!(code, 1);
    assert_eq!(json["status"], "error");
}

// --- --plan-file override ---

#[test]
#[ignore = "Enabled by the commit that lands src/plan_check.rs"]
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
