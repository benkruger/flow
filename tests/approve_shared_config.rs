//! Tests for `bin/flow approve-shared-config` — the user-driven
//! "proceed" subcommand that writes a single-use shared-config
//! approval marker after self-gating on the persisted transcript.
//!
//! Forgery model (same anchor as `clear-halt`): the marker is
//! forgeable (any Bash call can invoke this subcommand), so the
//! subcommand self-gates via
//! `transcript_walker::user_approved_shared_config_edit` — it
//! refuses unless the most recent real user turn typed the fixed
//! `approve shared-config: <path>` phrase AND a genuine
//! system-emitted shared-config block named the target in the same
//! exchange. Neither signal is model-forgeable.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{create_git_repo_with_remote, parse_output};
use flow_rs::approve_shared_config::{run_impl_main, Args};
use serde_json::{json, Value};

fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

fn encode_project_root(root: &Path) -> String {
    root.to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn write_transcript_for_session(
    home: &Path,
    project_root: &Path,
    session_id: &str,
    jsonl: &str,
) -> PathBuf {
    let encoded = encode_project_root(project_root);
    let dir = home.join(".claude").join("projects").join(&encoded);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.jsonl", session_id));
    fs::write(&path, jsonl).unwrap();
    path
}

fn write_state(repo: &Path, branch: &str, state: &Value) -> PathBuf {
    let branch_dir = repo.join(".flow-states").join(branch);
    fs::create_dir_all(&branch_dir).unwrap();
    let path = branch_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    path
}

/// An approving exchange: a real user request, an Edit, the
/// system shared-config BLOCK, then the user's fixed approval
/// phrase for `<target>` as the most recent real user turn. The
/// BLOCK content mirrors the production message
/// (`validate_worktree_paths::validate_shared_config`): the
/// leading clause names `<basename>` and the FULL `<target>` path
/// appears in the phrase and the `--path` argument, because
/// `user_approved_shared_config_edit` corroborates on the full
/// path (a same-basename sibling block must not cross-corroborate).
fn approving_jsonl(basename: &str, target: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"edit the manifest\"}}}}\n\
{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"Edit\",\"id\":\"e1\",\"input\":{{\"file_path\":\"{target}\"}}}}]}}}}\n\
{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"e1\",\"content\":\"BLOCKED: {basename} is a shared configuration file that affects every engineer in the repository. To authorize this single edit, the USER must reply with the exact line `approve shared-config: {target}`, then run `bin/flow approve-shared-config --path {target}` and retry the edit.\",\"is_error\":true}}]}}}}\n\
{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"approve shared-config: {target}\"}}}}\n"
    )
}

fn run_subcmd(repo: &Path, home: &Path, args: &[&str]) -> Output {
    flow_rs_no_recursion()
        .arg("approve-shared-config")
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("GH_TOKEN", "invalid")
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

// --- happy path (subprocess) ---

#[test]
fn approves_and_writes_marker_when_user_granted() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical_repo = repo.canonicalize().unwrap();
    let target = format!("{}/Cargo.toml", canonical_repo.display());
    let session_id = "asc-happy-001";
    write_transcript_for_session(
        dir.path(),
        &canonical_repo,
        session_id,
        &approving_jsonl("Cargo.toml", &target),
    );
    write_state(
        &repo,
        "b",
        &json!({ "branch": "b", "session_id": session_id, "current_phase": "flow-code" }),
    );

    let output = run_subcmd(&repo, dir.path(), &["--path", &target, "--branch", "b"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    // Marker consumable exactly once afterward.
    assert!(flow_rs::shared_config_approval::check_and_consume_approval(
        &repo, "b", &target
    ));
}

// --- forgery defense: no approving transcript ---

#[test]
fn rejects_when_transcript_lacks_approval() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical_repo = repo.canonicalize().unwrap();
    let target = format!("{}/Cargo.toml", canonical_repo.display());
    let session_id = "asc-forge-001";
    // BLOCK present but no user approval phrase.
    let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"edit it\"}}\n\
{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"e1\",\"content\":\"BLOCKED: Cargo.toml is a shared configuration file that affects every engineer in the repository.\",\"is_error\":true}]}}\n";
    write_transcript_for_session(dir.path(), &canonical_repo, session_id, jsonl);
    write_state(
        &repo,
        "b",
        &json!({ "branch": "b", "session_id": session_id, "current_phase": "flow-code" }),
    );

    let output = run_subcmd(&repo, dir.path(), &["--path", &target, "--branch", "b"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "not_user_approved");
    assert!(!flow_rs::shared_config_approval::check_and_consume_approval(&repo, "b", &target));
}

// --- --path validation (library, exhaustive) ---

fn lib_args(path: &str) -> Args {
    Args {
        path: path.to_string(),
        branch: Some("b".to_string()),
    }
}

#[test]
fn rejects_empty_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let home = dir.path();
    let (v, code) = run_impl_main(&lib_args(""), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "invalid_path");
}

#[test]
fn rejects_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let home = dir.path();
    let (v, code) = run_impl_main(&lib_args("Cargo.toml"), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "invalid_path");
}

#[test]
fn rejects_traversal_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let p = format!("{}/../escape/Cargo.toml", canonical.display());
    let (v, code) = run_impl_main(&lib_args(&p), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "invalid_path");
}

#[test]
fn rejects_nul_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let p = format!("{}/Cargo\0.toml", canonical.display());
    let (v, code) = run_impl_main(&lib_args(&p), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "invalid_path");
}

#[test]
fn rejects_path_outside_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let home = dir.path();
    // Absolute, well-formed, but outside the git worktree.
    let outside = dir.path().join("elsewhere").join("Cargo.toml");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    let (v, code) = run_impl_main(&lib_args(outside.to_str().unwrap()), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "path_outside_worktree");
}

// --- invalid branch ---

#[test]
fn rejects_invalid_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    let args = Args {
        path: target,
        branch: Some("a/b".to_string()),
    };
    let (v, code) = run_impl_main(&args, &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "invalid_branch");
}

// --- no state file ---

#[test]
fn rejects_when_no_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "no_state_file");
}

// --- no transcript path ---

#[test]
fn rejects_when_state_has_no_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    write_state(&repo, "b", &json!({ "branch": "b" }));
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "no_transcript_path");
}

// --- cwd-scope drift (state-mutator guard) ---

#[test]
fn rejects_on_cwd_drift() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    // cwd_scope::enforce resolves the branch from git (the fixture
    // clone is on `main`), so the drift state must live under the
    // git branch. relative_cwd="api" with cwd at the repo root is a
    // drift the guard rejects before any branch override is read.
    write_state(
        &repo,
        "main",
        &json!({ "branch": "main", "relative_cwd": "api", "session_id": "s" }),
    );
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "cwd_drift");
}

// --- cwd-result wrapper (main-arm fallback) ---

#[test]
fn cwd_result_ok_delegates_to_run_impl_main() {
    use flow_rs::approve_shared_config::run_impl_main_with_cwd_result;
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    let (v, code) =
        run_impl_main_with_cwd_result(&lib_args(&target), &repo, Ok(repo.clone()), home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "no_state_file");
}

#[test]
fn cwd_result_err_falls_back_to_dot() {
    use flow_rs::approve_shared_config::run_impl_main_with_cwd_result;
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    // current_dir() failure → cwd = ".". The git toplevel of "."
    // (the test process worktree) is not the fixture repo, so the
    // fixture target resolves outside it: deterministic rejection.
    let (v, code) = run_impl_main_with_cwd_result(
        &lib_args(&target),
        &repo,
        Err(std::io::Error::other("simulated current_dir failure")),
        home,
    );
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "path_outside_worktree");
}

// --- branch / path / transcript edge coverage ---

#[test]
fn rejects_when_branch_undetectable() {
    // Non-git cwd and no --branch override: resolve_branch_in
    // returns None.
    let dir = tempfile::tempdir().unwrap();
    let nongit = dir.path().join("plain");
    fs::create_dir_all(&nongit).unwrap();
    let home = dir.path();
    let args = Args {
        path: "/etc/hosts".to_string(),
        branch: None,
    };
    let (v, code) = run_impl_main(&args, dir.path(), &nongit, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "invalid_branch");
}

#[test]
fn rejects_when_cwd_not_git_managed() {
    // worktree_root returns None (git rev-parse fails in a non-git
    // cwd) → path_outside_worktree at the callsite.
    let dir = tempfile::tempdir().unwrap();
    let nongit = dir.path().join("plain");
    fs::create_dir_all(&nongit).unwrap();
    let home = dir.path();
    let p = nongit.join("Cargo.toml");
    let args = Args {
        path: p.to_str().unwrap().to_string(),
        branch: Some("b".to_string()),
    };
    let (v, code) = run_impl_main(&args, dir.path(), &nongit, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "path_outside_worktree");
}

#[test]
fn rejects_rootless_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let home = dir.path();
    // "/" is absolute with no `..` (passes path_shape_ok) but has
    // no parent → path_inside_worktree returns false.
    let (v, code) = run_impl_main(&lib_args("/"), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "path_outside_worktree");
}

#[test]
fn rejects_when_parent_dir_absent() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    // Parent directory does not exist → parent.canonicalize() Err
    // → path_inside_worktree false.
    let p = format!("{}/no_such_dir/Cargo.toml", canonical.display());
    let (v, code) = run_impl_main(&lib_args(&p), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "path_outside_worktree");
}

#[test]
fn rejects_when_state_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    let branch_dir = repo.join(".flow-states").join("b");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(branch_dir.join("state.json"), "{not json").unwrap();
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "no_state_file");
}

#[test]
fn rejects_when_session_id_unsafe_and_no_transcript_path() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical.display());
    // session_id fails is_safe_session_id (slash) → filtered out;
    // no transcript_path → resolve_transcript_path None.
    write_state(&repo, "b", &json!({ "branch": "b", "session_id": "a/b" }));
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "no_transcript_path");
}

#[test]
fn approves_via_explicit_transcript_path_field() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical_repo = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical_repo.display());
    // State carries an explicit transcript_path (not session_id):
    // exercises resolve_transcript_path's transcript_path branch.
    let proj = home.join(".claude").join("projects").join("pj");
    fs::create_dir_all(&proj).unwrap();
    let tpath = proj.join("explicit.jsonl");
    fs::write(&tpath, approving_jsonl("Cargo.toml", &target)).unwrap();
    write_state(
        &repo,
        "b",
        &json!({ "branch": "b", "transcript_path": tpath.to_str().unwrap() }),
    );
    let (v, code) = run_impl_main(&lib_args(&target), &repo, &repo, home);
    assert_eq!(code, 0, "value: {v}");
    assert_eq!(v["status"], "ok");
}

#[test]
fn write_failed_when_marker_dir_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical_repo = repo.canonicalize().unwrap();
    let home = dir.path();
    let target = format!("{}/Cargo.toml", canonical_repo.display());
    let session_id = "asc-wf-001";
    write_transcript_for_session(
        dir.path(),
        &canonical_repo,
        session_id,
        &approving_jsonl("Cargo.toml", &target),
    );
    // root must be the canonical repo so derive_transcript_path
    // encodes the same project-root path the transcript was
    // written under.
    let branch_dir = canonical_repo.join(".flow-states").join("b");
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&json!({ "branch": "b", "session_id": session_id })).unwrap(),
    )
    .unwrap();
    // A regular file where the approvals directory must be created
    // makes write_approval's create_dir_all fail.
    fs::write(branch_dir.join("shared-config-approvals"), "x").unwrap();
    let (v, code) = run_impl_main(&lib_args(&target), &canonical_repo, &canonical_repo, home);
    assert_eq!(code, 1);
    assert_eq!(v["reason"], "write_failed");
}

// --- main-dispatch arm reachable (subprocess) ---

#[test]
fn main_dispatch_arm_is_reachable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let canonical = repo.canonicalize().unwrap();
    let target = format!("{}/Cargo.toml", canonical.display());
    // No state file → structured error through the real binary,
    // proving Commands::ApproveSharedConfig dispatches.
    let output = run_subcmd(&repo, dir.path(), &["--path", &target, "--branch", "b"]);
    assert_eq!(output.status.code(), Some(1));
    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert_eq!(data["reason"], "no_state_file");
}
