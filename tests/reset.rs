//! Subprocess tests for `bin/flow reset` — the Rust shim that exec's
//! the existing `${CLAUDE_PLUGIN_ROOT}/bin/reset` bash script.
//!
//! These tests drive the dispatcher path end-to-end: spawn the compiled
//! `flow-rs` binary with `reset` as the subcommand and assert the bash
//! script's exit code, stderr, and `.flow-states/` filesystem effect
//! match the script's documented behavior. Coverage of the
//! `src/reset.rs` Rust shim's branches (None plugin root, exec failure)
//! lives in this same file as a sibling subprocess test using a
//! `CLAUDE_PLUGIN_ROOT` fixture that lacks `bin/reset`.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use flow_rs::reset::run_impl_main;

/// Build a no-recursion Command pointing at the compiled `flow-rs`
/// binary with the `FLOW_CI_RUNNING` env var stripped so the binary's
/// recursion guard does not fire when the test harness itself runs
/// inside `bin/flow ci`. See
/// `.claude/rules/subprocess-test-hygiene.md`.
fn flow_rs_no_recursion() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd
}

/// Initialize a git repo with a default committer config and one empty
/// commit so worktree and submodule operations are possible.
fn init_repo(dir: &Path, branch: &str) {
    Command::new("git")
        .args(["init", "-b", branch])
        .current_dir(dir)
        .output()
        .expect("git init");
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(dir)
            .output()
            .expect("git config");
    }
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .expect("git commit");
}

/// Populate `.flow-states/<branch>/` with a state.json and a sibling
/// queue file so the assertion can verify the whole directory was
/// removed, not just a single file.
fn seed_flow_states(project_root: &Path) {
    let states = project_root.join(".flow-states");
    let branch_dir = states.join("test-branch");
    fs::create_dir_all(&branch_dir).expect("create branch dir");
    fs::write(branch_dir.join("state.json"), "{}").expect("write state.json");
    fs::write(states.join("orchestrate-queue.json"), "{}").expect("write queue");
}

/// Happy path: invoking `bin/flow reset` from inside a normal repo
/// exec's the bash script, which removes `<project_root>/.flow-states/`
/// and exits 0.
#[test]
fn bin_flow_reset_dispatches_to_bash_script() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    init_repo(&root, "main");
    seed_flow_states(&root);
    assert!(root.join(".flow-states").is_dir(), "fixture precondition");

    let output = flow_rs_no_recursion()
        .args(["reset"])
        .current_dir(&root)
        .output()
        .expect("spawn flow-rs reset");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !root.join(".flow-states").exists(),
        ".flow-states/ should be removed"
    );
}

/// Inside a bare repository, the bash script's bare-repo guard rejects
/// the call: exit 1, stderr names the rejection. The `bin/flow reset`
/// dispatcher must surface the same exit code and stderr unchanged.
#[test]
fn bin_flow_reset_rejects_bare_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let bare = root.join("bare.git");
    fs::create_dir_all(&bare).expect("create bare dir");
    let init = Command::new("git")
        .args(["init", "--bare", "-b", "main"])
        .current_dir(&bare)
        .output()
        .expect("git init --bare");
    assert!(
        init.status.success(),
        "git init --bare failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    seed_flow_states(&root);

    let output = flow_rs_no_recursion()
        .args(["reset"])
        .current_dir(&bare)
        .output()
        .expect("spawn flow-rs reset");

    assert!(
        !output.status.success(),
        "expected non-zero exit on bare repo, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing to operate inside a bare repository"),
        "expected bare-repo rejection message, got stderr: {}",
        stderr
    );
    assert!(
        root.join(".flow-states").is_dir(),
        "bare repo's parent .flow-states/ must not be touched"
    );
}

/// From inside a linked worktree, the bash script's git-common-dir
/// resolution finds the MAIN repo's `.flow-states/`, not the worktree's.
/// Invoking `bin/flow reset` with cwd set to the worktree must remove
/// the main repo's `.flow-states/`.
#[test]
fn bin_flow_reset_resolves_main_root_from_worktree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    init_repo(&root, "main");
    seed_flow_states(&root);

    let worktree_path = root.join(".worktrees").join("feat");
    fs::create_dir_all(worktree_path.parent().unwrap()).expect("create .worktrees");
    let wt = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            "feat",
            &worktree_path.to_string_lossy(),
        ])
        .current_dir(&root)
        .output()
        .expect("git worktree add");
    assert!(
        wt.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&wt.stderr)
    );

    let worktree = worktree_path.canonicalize().expect("canonicalize worktree");

    let output = flow_rs_no_recursion()
        .args(["reset"])
        .current_dir(&worktree)
        .output()
        .expect("spawn flow-rs reset");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !root.join(".flow-states").exists(),
        "main repo's .flow-states/ should be removed when invoked from worktree"
    );
}

/// Inside a submodule, the bash script's superproject-working-tree
/// discriminator selects the SUBMODULE's working tree as PROJECT_ROOT,
/// not the superproject. Invoking `bin/flow reset` from the submodule
/// must remove only the submodule's `.flow-states/` and leave the
/// superproject's `.flow-states/` untouched.
#[test]
fn bin_flow_reset_targets_submodule_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");

    // Build a submodule source repo separate from the superproject so
    // git accepts it as a submodule add target.
    let submodule_src = root.join("submodule-src");
    fs::create_dir_all(&submodule_src).expect("create submodule-src");
    init_repo(&submodule_src, "main");

    // Superproject.
    let super_root = root.join("superproject");
    fs::create_dir_all(&super_root).expect("create superproject");
    init_repo(&super_root, "main");
    seed_flow_states(&super_root);

    let add = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &submodule_src.to_string_lossy(),
            "sub",
        ])
        .current_dir(&super_root)
        .output()
        .expect("git submodule add");
    assert!(
        add.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let submodule_path = super_root.join("sub");
    seed_flow_states(&submodule_path);
    assert!(
        submodule_path.join(".flow-states").is_dir(),
        "submodule .flow-states/ precondition"
    );

    let output = flow_rs_no_recursion()
        .args(["reset"])
        .current_dir(&submodule_path)
        .output()
        .expect("spawn flow-rs reset");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !submodule_path.join(".flow-states").exists(),
        "submodule's .flow-states/ should be removed"
    );
    assert!(
        super_root.join(".flow-states").is_dir(),
        "superproject's .flow-states/ must not be touched"
    );
}

/// When `CLAUDE_PLUGIN_ROOT` points at a directory that contains
/// `flow-phases.json` (so `plugin_root()` resolves it) but no
/// `bin/reset` script, the Rust shim's `Command::exec()` fails. The
/// post-exec failure branch must emit a `status=error` JSON envelope
/// naming the missing script path and exit with code 1.
#[test]
fn bin_flow_reset_reports_exec_failure_when_script_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let fake_plugin_root = root.join("fake-plugin");
    fs::create_dir_all(&fake_plugin_root).expect("create fake plugin root");
    // flow-phases.json makes plugin_root() resolve the fake dir; the
    // absence of bin/reset is what drives the exec failure branch.
    fs::write(fake_plugin_root.join("flow-phases.json"), "{}").expect("write flow-phases.json");

    let output = flow_rs_no_recursion()
        .args(["reset"])
        .env("CLAUDE_PLUGIN_ROOT", &fake_plugin_root)
        .current_dir(&root)
        .output()
        .expect("spawn flow-rs reset");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1, got {:?}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("expected JSON on stdout, got `{}`: {}", stdout, e));
    assert_eq!(
        parsed["status"].as_str(),
        Some("error"),
        "expected status=error envelope, got: {}",
        stdout
    );
    let message = parsed["message"].as_str().unwrap_or("");
    assert!(
        message.contains("Could not exec"),
        "expected message naming the failed exec, got: {}",
        message
    );
    assert!(
        message.contains("bin/reset"),
        "expected message naming the missing script, got: {}",
        message
    );
}

/// Library-level unit test for the `None` plugin-root branch. The
/// subprocess tests cannot drive this branch because the binary's
/// `current_exe`-based plugin-root resolver finds the FLOW repo via
/// walk-up — `Some(_)` is the only reachable subprocess outcome.
/// `run_impl_main(None)` returns the documented "Plugin root not found"
/// envelope and exit 1.
#[test]
fn run_impl_main_returns_error_when_plugin_root_none() {
    let (value, code) = run_impl_main(None);
    assert_eq!(code, 1);
    assert_eq!(value["status"].as_str(), Some("error"));
    assert_eq!(
        value["message"].as_str(),
        Some("Plugin root not found"),
        "envelope must name the missing plugin root, got: {}",
        value
    );
}
