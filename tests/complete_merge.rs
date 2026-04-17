//! Subprocess integration tests for `bin/flow complete-merge`.
//!
//! Covers the CLI entry (`run`) and the `complete_merge` production
//! wrapper. The child reads `FLOW_BIN_PATH` env override in
//! `bin_flow_path()` so each test points it at a per-tempdir stub
//! that responds to `check-freshness` via `STUB_FRESHNESS_JSON`.
//! `gh pr merge` is stubbed via PATH with `STUB_GH_MERGE_EXIT`.
//! The inline tests in `src/complete_merge.rs::tests` cover every
//! internal match arm via mock runners; these subprocess tests
//! prove the CLI entry's `status != "merged" → exit 1` dispatch.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

mod common;

/// Write the `bin/flow` stub script at `path`. Handles the
/// `check-freshness` subcommand via `$STUB_FRESHNESS_JSON` and
/// exits 0 for any other subcommand. Each test owns its own path
/// so parallel tests do not race.
fn write_bin_flow_stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let script = "#!/bin/sh\n\
        case \"$1\" in\n\
          check-freshness) printf '%s\\n' \"$STUB_FRESHNESS_JSON\" ;;\n\
          *) exit 0 ;;\n\
        esac\n";
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Build the `gh` stub at `<stubs_dir>/gh`. Handles `gh pr merge`
/// per `$STUB_GH_MERGE_EXIT`; returns the stubs dir for PATH use.
fn build_path_stub_dir(parent: &Path) -> PathBuf {
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_script = "#!/bin/sh\n\
        if [ \"$1 $2\" = \"pr merge\" ]; then\n\
          exit \"${STUB_GH_MERGE_EXIT:-0}\"\n\
        fi\n\
        exit 0\n";
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();
    stubs
}

#[allow(clippy::too_many_arguments)]
fn run_complete_merge(
    cwd: &Path,
    pr: &str,
    state_file: &str,
    path_stub_dir: &Path,
    flow_bin_path: &Path,
    freshness_json: &str,
    gh_merge_exit: i32,
) -> (i32, String, String) {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", path_stub_dir.display(), current_path);
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["complete-merge", "--pr", pr, "--state-file", state_file])
        .current_dir(cwd)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", flow_bin_path)
        .env("STUB_FRESHNESS_JSON", freshness_json)
        .env("STUB_GH_MERGE_EXIT", gh_merge_exit.to_string())
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("spawn flow-rs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn last_json_line(stdout: &str) -> Value {
    let last = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("failed to parse JSON line '{}': {}", last, e))
}

/// Stubbed `check-freshness` returns `up_to_date` and stubbed
/// `gh pr merge` exits 0 → `complete_merge_inner` returns
/// `status == "merged"` → `run` exits 0.
#[test]
fn merge_run_merged_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "{\"branch\": \"feat\"}").unwrap();

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "up_to_date"}"#,
        0,
    );

    assert_eq!(code, 0, "merged status must exit 0; stdout={}", stdout);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "merged");
    assert_eq!(json["pr_number"], 42);
}

/// Stubbed `check-freshness` returns `max_retries` →
/// `complete_merge_inner` returns `status == "max_retries"` →
/// `run` exits 1 (status != "merged").
#[test]
fn merge_run_non_merged_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let state_path = parent.join("state.json");
    fs::write(&state_path, "{\"branch\": \"feat\"}").unwrap();

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        state_path.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "max_retries", "retries": 3}"#,
        0,
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "max_retries");
}

/// Missing state file + stubbed `check-freshness` returning an error
/// → `complete_merge_inner` surfaces the freshness error through
/// the result → `run` exits 1. Proves the `complete_merge` wrapper
/// delegates to `complete_merge_inner` end-to-end.
#[test]
fn merge_wrapper_returns_error_on_missing_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let flow_bin = parent.join("bin-flow-stub").join("flow");
    write_bin_flow_stub(&flow_bin);
    let path_stub = build_path_stub_dir(&parent);
    let missing_state = parent.join("does-not-exist.json");

    let (code, stdout, _) = run_complete_merge(
        &parent,
        "42",
        missing_state.to_string_lossy().as_ref(),
        &path_stub,
        &flow_bin,
        r#"{"status": "error", "message": "state file missing"}"#,
        0,
    );

    assert_eq!(code, 1);
    let json = last_json_line(&stdout);
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("state file missing"),
        "wrapper must surface freshness error via the result; got: {}",
        json
    );
}
