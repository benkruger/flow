//! Tests for `bin/flow resolve-skill-mode`. Mirrors
//! `src/resolve_skill_mode.rs`.
//!
//! Library-level tests drive the pure `normalize_gate_input` / `resolve`
//! seams and the `run_impl` / `run_impl_main` entry points with a
//! `TempDir` root and a `--branch` override, so they never collide
//! with the host worktree. Subprocess tests spawn the compiled
//! `flow-rs` binary to cover the `resolve_branch` None arm (only
//! reachable when the process cwd is not on a git branch) and the
//! `src/main.rs` dispatch arm.
//!
//! Subprocess hygiene per `.claude/rules/subprocess-test-hygiene.md`:
//! every spawn neutralizes `GH_TOKEN`, `HOME`, `FLOW_CI_RUNNING`, and
//! `FLOW_SIMULATE_BRANCH` to keep the child off the host's GitHub
//! account, dotfiles, ambient CI recursion guard, and branch
//! simulation.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};

use flow_rs::resolve_skill_mode::{normalize_gate_input, resolve, run_impl, run_impl_main, Args};

/// Build an `Args` for `skill` with an explicit `--branch` override so
/// `resolve_branch` returns the override without consulting host git.
fn args(skill: &str, branch: &str) -> Args {
    Args {
        skill: skill.to_string(),
        branch: Some(branch.to_string()),
    }
}

/// Write a state file at `<root>/.flow-states/<branch>/state.json`.
fn write_state(root: &Path, branch: &str, content: &str) {
    let dir = root.join(".flow-states").join(branch);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.json"), content).unwrap();
}

/// Assert `resolve(state, skill)` returns `(commit, continue)`.
fn assert_mode(state: &Value, skill: &str, commit: &str, cont: &str) {
    assert_eq!(
        resolve(state, skill),
        (commit.to_string(), cont.to_string()),
        "resolve({skill}) mismatch"
    );
}

/// Initialize a git repo on the named branch with one empty commit.
fn init_git_repo(dir: &Path, branch: &str) {
    let run = |a: &[&str]| {
        let output = Command::new("git")
            .args(a)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", a);
    };
    run(&["init", "--initial-branch", branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

/// Run `flow-rs resolve-skill-mode` in `repo` with `--skill` and an
/// optional `--branch` override. Returns the captured Output.
fn run_subcommand(repo: &Path, skill: &str, branch_override: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("resolve-skill-mode").arg("--skill").arg(skill);
    if let Some(b) = branch_override {
        cmd.arg("--branch").arg(b);
    }
    cmd.current_dir(repo)
        .env("GH_TOKEN", "invalid")
        .env("HOME", repo)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .expect("spawn flow-rs resolve-skill-mode")
}

// --- normalize_gate_input ---

#[test]
fn normalize_gate_input_strips_nul() {
    assert_eq!(normalize_gate_input("flow-complete\0"), "flow-complete");
}

#[test]
fn normalize_gate_input_trims_whitespace() {
    assert_eq!(normalize_gate_input("  flow-abort  "), "flow-abort");
}

#[test]
fn normalize_gate_input_lowercases() {
    assert_eq!(normalize_gate_input("FLOW-COMPLETE"), "flow-complete");
}

// --- resolve ---

#[test]
fn resolve_block_commit_and_continue() {
    let state = json!({"skills": {"flow-complete": {"commit": "auto", "continue": "auto"}}});
    assert_mode(&state, "flow-complete", "auto", "auto");
}

#[test]
fn resolve_block_mixed_axes() {
    let state = json!({"skills": {"flow-complete": {"commit": "manual", "continue": "auto"}}});
    assert_mode(&state, "flow-complete", "manual", "auto");
}

#[test]
fn resolve_block_continue_only_uses_commit_default() {
    // `flow-complete` commit default is `manual`.
    let state = json!({"skills": {"flow-complete": {"continue": "auto"}}});
    assert_mode(&state, "flow-complete", "manual", "auto");
}

#[test]
fn resolve_block_commit_only_uses_continue_default() {
    // `flow-complete` continue default is `manual`.
    let state = json!({"skills": {"flow-complete": {"commit": "auto"}}});
    assert_mode(&state, "flow-complete", "auto", "manual");
}

#[test]
fn resolve_block_empty_object_falls_back() {
    let state = json!({"skills": {"flow-complete": {}}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_block_continue_non_string_falls_back() {
    let state = json!({"skills": {"flow-complete": {"commit": "auto", "continue": 5}}});
    assert_mode(&state, "flow-complete", "auto", "manual");
}

/// A bare-string `skills.<skill>` entry is no longer parsed as a mode
/// value — the block-shape-only resolver clamps it to the per-skill
/// default. Guards the resolver tombstone
/// `test_resolve_skill_mode_no_bare_string_branch`.
#[test]
fn resolve_bare_string_not_parsed() {
    let state = json!({"skills": {"flow-complete": "auto"}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_bare_string_manual_not_parsed() {
    let state = json!({"skills": {"flow-review": "manual"}});
    assert_mode(&state, "flow-review", "manual", "manual");
}

#[test]
fn resolve_skills_key_absent_falls_back() {
    let state = json!({});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_entry_absent_falls_back() {
    let state = json!({"skills": {}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_entry_null_falls_back() {
    let state = json!({"skills": {"flow-complete": null}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_entry_number_falls_back() {
    let state = json!({"skills": {"flow-complete": 42}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

#[test]
fn resolve_entry_array_falls_back() {
    let state = json!({"skills": {"flow-complete": []}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

/// Every skill resolves from its own `skills.<skill>` block entry.
#[test]
fn resolve_each_skill_reads_its_own_entry() {
    for skill in [
        "flow-start",
        "flow-code",
        "flow-review",
        "flow-complete",
        "flow-abort",
    ] {
        let state = json!({"skills": {skill: {"commit": "manual", "continue": "auto"}}});
        assert_mode(&state, skill, "manual", "auto");
    }
}

/// A garbage value on the `commit` axis clamps to the per-skill
/// default rather than passing through verbatim.
#[test]
fn resolve_garbage_commit_axis_clamps_to_default() {
    let state = json!({"skills": {"flow-complete": {"commit": "xyzzy", "continue": "auto"}}});
    assert_mode(&state, "flow-complete", "manual", "auto");
}

/// A garbage value on the `continue` axis clamps to the per-skill
/// default.
#[test]
fn resolve_garbage_continue_axis_clamps_to_default() {
    let state = json!({"skills": {"flow-complete": {"continue": "banana"}}});
    assert_mode(&state, "flow-complete", "manual", "manual");
}

/// A case-variant axis value normalizes — `"AUTO"` resolves to
/// `"auto"`.
#[test]
fn resolve_uppercase_value_normalized() {
    let state = json!({"skills": {"flow-complete": {"commit": "AUTO", "continue": "MANUAL"}}});
    assert_mode(&state, "flow-complete", "auto", "manual");
}

/// A whitespace-padded axis value is trimmed before the allowlist
/// check.
#[test]
fn resolve_whitespace_padded_value_normalized() {
    let state = json!({"skills": {"flow-complete": {"continue": " auto "}}});
    assert_mode(&state, "flow-complete", "manual", "auto");
}

/// A NUL byte in the axis value is stripped before the allowlist
/// check.
#[test]
fn resolve_nul_in_value_normalized() {
    let state = json!({"skills": {"flow-complete": {"continue": "auto\0"}}});
    assert_mode(&state, "flow-complete", "manual", "auto");
}

/// A hand-edited `.flow.json` may carry a mixed-case skill key. The
/// resolver matches the `skills` object key case-insensitively, so
/// the configured mode is read rather than silently falling to the
/// per-skill default. Regression guard for the asymmetric-
/// normalization finding (`security-gates.md` "Normalize Before
/// Comparing"): the `--skill` arg was normalized but the state-file
/// key lookup was case-sensitive. The state object also carries a
/// non-matching sibling key so the case-insensitive match is
/// exercised against both a miss and a hit.
#[test]
fn resolve_mixed_case_skills_key_matches() {
    let state = json!({
        "skills": {
            "flow-start": {"commit": "manual", "continue": "manual"},
            "Flow-Complete": {"commit": "auto", "continue": "manual"}
        }
    });
    assert_mode(&state, "flow-complete", "auto", "manual");
}

// --- run_impl ---

#[test]
fn run_impl_invalid_skill_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-prime", "feature"), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_skill");
}

#[test]
fn run_impl_invalid_branch_dotdot_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", ".."), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}

#[test]
fn run_impl_invalid_branch_empty_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", ""), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}

#[test]
fn run_impl_invalid_branch_slash_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", "feature/foo"), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}

/// `.claude/rules/branch-path-safety.md` mandates a rejection test
/// for each of the four invalid branch inputs (empty, `.`, `..`,
/// NUL) on every new public surface accepting a `--branch` override.
/// A single-dot branch resolves the per-branch `.flow-states` path to
/// the directory itself.
#[test]
fn run_impl_invalid_branch_dot_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", "."), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}

/// `.claude/rules/branch-path-safety.md`: a NUL byte in the branch
/// name must be rejected before any `.flow-states` path is built.
#[test]
fn run_impl_invalid_branch_nul_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", "feat\0ure"), tmp.path());
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}

#[test]
fn run_impl_missing_state_file_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run_impl(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "manual");
}

#[test]
fn run_impl_empty_state_file_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(tmp.path(), "feature", "");
    let v = run_impl(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "manual");
}

#[test]
fn run_impl_non_json_state_file_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(tmp.path(), "feature", "{ not json");
    let v = run_impl(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "manual");
}

#[test]
fn run_impl_wrong_root_state_file_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(tmp.path(), "feature", "[]");
    let v = run_impl(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "manual");
}

#[test]
fn run_impl_valid_state_resolves_both_axes() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(
        tmp.path(),
        "feature",
        r#"{"skills": {"flow-complete": {"commit": "manual", "continue": "auto"}}}"#,
    );
    let v = run_impl(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "auto");
}

#[test]
fn run_impl_skill_normalization_maps_to_state_key() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(
        tmp.path(),
        "feature",
        r#"{"skills": {"flow-complete": {"commit": "auto", "continue": "auto"}}}"#,
    );
    let v = run_impl(&args("  FLOW-COMPLETE  ", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "auto");
    assert_eq!(v["continue"], "auto");
}

#[test]
fn run_impl_skill_nul_normalized_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(
        tmp.path(),
        "feature",
        r#"{"skills": {"flow-abort": {"continue": "auto"}}}"#,
    );
    let v = run_impl(&args("flow-abort\0", "feature"), tmp.path());
    assert_eq!(v["status"], "ok");
    assert_eq!(v["continue"], "auto");
}

// --- run_impl_main ---

#[test]
fn run_impl_main_returns_value_and_zero() {
    let tmp = tempfile::tempdir().unwrap();
    write_state(
        tmp.path(),
        "feature",
        r#"{"skills": {"flow-complete": {"commit": "auto", "continue": "auto"}}}"#,
    );
    let (v, code) = run_impl_main(&args("flow-complete", "feature"), tmp.path());
    assert_eq!(code, 0);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "auto");
    assert_eq!(v["continue"], "auto");
}

#[test]
fn run_impl_main_invalid_skill_still_exit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let (v, code) = run_impl_main(&args("flow-prime", "feature"), tmp.path());
    assert_eq!(code, 0);
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_skill");
}

// --- subprocess ---

#[test]
fn subcommand_resolves_from_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");
    write_state(
        &repo,
        "feature",
        r#"{"skills": {"flow-complete": {"commit": "manual", "continue": "auto"}}}"#,
    );

    let output = run_subcommand(&repo, "flow-complete", None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "auto");
}

#[test]
fn subcommand_no_current_branch_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    // No `git init` — `resolve_branch` returns None with no override.

    let output = run_subcommand(&repo, "flow-complete", None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["commit"], "manual");
    assert_eq!(v["continue"], "manual");
}

#[test]
fn subcommand_invalid_skill_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");

    let output = run_subcommand(&repo, "bogus-skill", None);
    let v: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_skill");
}

#[test]
fn subcommand_invalid_branch_dotdot_errors_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    init_git_repo(&repo, "feature");

    let output = run_subcommand(&repo, "flow-complete", Some(".."));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "'--branch ..' must not panic; stderr: {}",
        stderr
    );
    let v: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_branch");
}
