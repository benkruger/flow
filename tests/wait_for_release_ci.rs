//! Integration tests for `bin/flow wait-for-release-ci`
//! (`src/wait_for_release_ci.rs`).
//!
//! The module reads `git rev-parse HEAD` and `gh run list` and loops
//! with a real `thread::sleep` until the latest integration-branch run
//! reaches a terminal conclusion. Both subprocesses are fixture-
//! controllable, so every test spawns the compiled binary with `PATH`
//! pointed at a stub directory holding `git` and `gh` shims (bash
//! builtins only, so `PATH` can be the stub dir alone). The retry loop
//! is driven with short `--timeout`/`--interval` values — the
//! `acquire_with_wait` test pattern, no closure seam.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::parse_output;

/// Fixed 40-char HEAD sha the `git` shim echoes for the matching cases.
const HEAD: &str = "1111111111111111111111111111111111111111";

/// Write an executable shim at `dir/<name>` with the given bash body.
fn write_shim(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Create a stub-bin directory under `root`. `git_body` is the `git`
/// shim body (always created). `gh_body`, when `Some`, creates a `gh`
/// shim; when `None`, no `gh` exists so a spawn of `gh` fails. Returns
/// the stub directory path, which the caller sets as the sole `PATH`
/// entry for the spawned binary — both shims use only bash builtins so
/// no external commands need to resolve.
fn make_stubs(root: &Path, git_body: &str, gh_body: Option<&str>) -> PathBuf {
    let stub = root.join("stub-bin");
    fs::create_dir_all(&stub).unwrap();
    write_shim(&stub, "git", git_body);
    if let Some(body) = gh_body {
        write_shim(&stub, "gh", body);
    }
    stub
}

/// A `git` shim that echoes `HEAD` on `git rev-parse HEAD`.
fn git_echo_head() -> String {
    format!("#!/bin/bash\necho {}\n", HEAD)
}

/// A `gh` shim that prints `json` and exits 0.
fn gh_print(json: &str) -> String {
    format!("#!/bin/bash\necho '{}'\n", json)
}

/// Spawn `bin/flow wait-for-release-ci` with `cwd` set to `root` and
/// `PATH` set to the stub dir only. Neutralizes GH/HOME per
/// `.claude/rules/subprocess-test-hygiene.md` (the `gh` shim ignores
/// them, but the discipline is uniform).
fn run_wait(root: &Path, stub: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("wait-for-release-ci")
        .args(["--base", "main"])
        .args(extra_args)
        .current_dir(root)
        .env("PATH", stub.to_string_lossy().to_string())
        .env("GH_TOKEN", "invalid")
        .env("HOME", root.to_string_lossy().to_string())
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env_remove("FLOW_CI_RUNNING");
    cmd.output().unwrap()
}

// --- wait_for_release_ci ---

#[test]
fn wait_for_release_ci_ready_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let gh = gh_print(&format!(
        "[{{\"headSha\":\"{}\",\"conclusion\":\"success\"}}]",
        HEAD
    ));
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    assert_eq!(data["conclusion"], "success");
}

#[test]
fn wait_for_release_ci_ready_passes_through_failure_conclusion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let gh = gh_print(&format!(
        "[{{\"headSha\":\"{}\",\"conclusion\":\"failure\"}}]",
        HEAD
    ));
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "ready");
    assert_eq!(
        data["conclusion"], "failure",
        "terminal conclusion must be passed through verbatim"
    );
}

#[test]
fn wait_for_release_ci_ready_after_pending_tick() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let counter = root.join(".gh-count");
    // First invocation: conclusion null (in progress). Second: success.
    let gh = format!(
        "#!/bin/bash\nf=\"{}\"\nn=0\n[ -f \"$f\" ] && read n < \"$f\"\necho $((n+1)) > \"$f\"\nif [ \"$n\" -eq 0 ]; then\n  echo '[{{\"headSha\":\"{}\",\"conclusion\":null}}]'\nelse\n  echo '[{{\"headSha\":\"{}\",\"conclusion\":\"success\"}}]'\nfi\n",
        counter.to_string_lossy(),
        HEAD,
        HEAD
    );
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh));

    let output = run_wait(&root, &stub, &["--timeout", "10", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(
        data["status"], "ready",
        "a null conclusion on the first tick must poll again and read success"
    );
    assert_eq!(data["conclusion"], "success");
}

#[test]
fn wait_for_release_ci_still_pending_on_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let gh = gh_print(&format!(
        "[{{\"headSha\":\"{}\",\"conclusion\":null}}]",
        HEAD
    ));
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh));

    let output = run_wait(&root, &stub, &["--timeout", "0", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(
        data["status"], "still_pending",
        "a never-terminal run must report still_pending at the cap"
    );
    assert!(
        data["waited_seconds"].is_i64(),
        "still_pending must report waited_seconds, got: {}",
        data
    );
}

#[test]
fn wait_for_release_ci_error_on_head_sha_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // gh reports a run for a DIFFERENT commit than the git shim's HEAD.
    let gh = gh_print(
        "[{\"headSha\":\"2222222222222222222222222222222222222222\",\"conclusion\":\"success\"}]",
    );
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("latest commit"),
        "headSha mismatch must explain CI has not run on the latest commit, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_empty_run_list() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh_print("[]")));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("no CI runs"),
        "empty run list must report no CI runs, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_unparseable_gh_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stub = make_stubs(&root, &git_echo_head(), Some(&gh_print("not json at all")));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("parse"),
        "unparseable gh output must report a parse error, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_gh_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let gh = "#!/bin/bash\necho 'gh boom' >&2\nexit 1\n";
    let stub = make_stubs(&root, &git_echo_head(), Some(gh));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("gh boom"),
        "gh non-zero exit must surface gh stderr, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_gh_spawn_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // git shim present (HEAD resolves), no gh shim => gh spawn fails.
    let stub = make_stubs(&root, &git_echo_head(), None);

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("gh"),
        "gh spawn failure must name gh, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_git_spawn_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // gh shim present, no git shim => git rev-parse spawn fails. The
    // stub dir holds only gh, and PATH is the stub dir alone.
    let stub = root.join("stub-bin");
    fs::create_dir_all(&stub).unwrap();
    write_shim(&stub, "gh", &gh_print("[]"));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("HEAD"),
        "git spawn failure must report the rev-parse HEAD failure, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_git_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // git shim exits non-zero => head_sha is None.
    let stub = make_stubs(&root, "#!/bin/bash\nexit 1\n", Some(&gh_print("[]")));

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap_or("").contains("HEAD"),
        "git non-zero exit must report the rev-parse HEAD failure, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_empty_head_sha() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // git shim exits 0 with empty stdout — a broken/edge git. head_sha
    // must fail closed (reject empty) rather than yield an empty HEAD
    // that makes classify's headSha-mismatch gate compare "" against a
    // run with a missing headSha, pass, and report a release-ready run.
    let stub = make_stubs(
        &root,
        "#!/bin/bash\nexit 0\n",
        Some(&gh_print("[{\"conclusion\":\"success\"}]")),
    );

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(
        data["status"], "error",
        "an empty resolved HEAD must fail closed, not report ready; got: {}",
        data
    );
    assert!(
        data["message"].as_str().unwrap_or("").contains("HEAD"),
        "empty HEAD must report the rev-parse HEAD failure, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_non_hex_head_sha() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    // git shim exits 0 echoing a non-hex value. head_sha's all-hex guard
    // must reject it rather than treat garbage as a valid HEAD.
    let stub = make_stubs(
        &root,
        "#!/bin/bash\necho 'not-a-real-sha'\n",
        Some(&gh_print("[{\"conclusion\":\"success\"}]")),
    );

    let output = run_wait(&root, &stub, &["--timeout", "5", "--interval", "0"]);

    let data = parse_output(&output);
    assert_eq!(
        data["status"], "error",
        "a non-hex resolved HEAD must fail closed; got: {}",
        data
    );
    assert!(
        data["message"].as_str().unwrap_or("").contains("HEAD"),
        "non-hex HEAD must report the rev-parse HEAD failure, got: {}",
        data["message"]
    );
}

#[test]
fn wait_for_release_ci_error_on_empty_base() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stub = make_stubs(
        &root,
        &git_echo_head(),
        Some(&gh_print(&format!(
            "[{{\"headSha\":\"{}\",\"conclusion\":\"success\"}}]",
            HEAD
        ))),
    );

    // Spawn directly with an empty --base: run_wait hardcodes --base main,
    // and clap rejects a duplicate --base. An empty branch must be
    // rejected before any gh/git poll runs.
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("wait-for-release-ci")
        .args(["--base", "", "--timeout", "5", "--interval", "0"])
        .current_dir(&root)
        .env("PATH", stub.to_string_lossy().to_string())
        .env("GH_TOKEN", "invalid")
        .env("HOME", root.to_string_lossy().to_string())
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .unwrap();

    let data = parse_output(&output);
    assert_eq!(
        data["status"], "error",
        "an empty --base must be rejected, not polled; got: {}",
        data
    );
    assert!(
        data["message"]
            .as_str()
            .unwrap_or("")
            .contains("non-empty --base"),
        "empty --base must name the non-empty requirement, got: {}",
        data["message"]
    );
}
