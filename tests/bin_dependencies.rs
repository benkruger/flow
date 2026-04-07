//! Tests for bin/dependencies — the framework dependency updater.
//!
//! Ports tests/test_bin_dependencies.py to Rust integration tests.
//! Each test validates the same invariant as its Python counterpart.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// bin/dependencies must exist and be executable.
#[test]
fn script_is_executable() {
    let dep = common::bin_dir().join("dependencies");
    assert!(dep.exists(), "bin/dependencies must exist");
    let meta = fs::metadata(&dep).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/dependencies must be executable"
    );
}

/// bin/dependencies must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let dep = common::bin_dir().join("dependencies");
    let output = Command::new("bash")
        .arg("-n")
        .arg(&dep)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Syntax error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Creates a minimal project layout for bin/dependencies testing.
///
/// bin/dependencies computes REPO_ROOT from $(dirname "$0")/.., so placing it at
/// <tmp>/bin/dependencies makes it look for .venv at <tmp>/.venv/.
/// Includes a .venv/bin/pip wrapper that echoes a marker and exits.
///
/// IMPORTANT: Uses a wrapper script, NOT a symlink. write_text() on a
/// symlink follows it and overwrites the target — which would corrupt
/// the real pip binary.
fn setup_dep_project(dir: &std::path::Path) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Copy the real bin/dependencies script content
    let real_script = common::bin_dir().join("dependencies");
    let script_content = fs::read_to_string(&real_script).unwrap();
    fs::write(bin_dir.join("dependencies"), &script_content).unwrap();
    let mut perms = fs::metadata(bin_dir.join("dependencies")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("dependencies"), perms).unwrap();

    // Create requirements.txt
    fs::write(dir.join("requirements.txt"), "# test requirements\n").unwrap();

    // Create fake pip wrapper
    let venv_bin = dir.join(".venv").join("bin");
    fs::create_dir_all(&venv_bin).unwrap();
    fs::write(venv_bin.join("pip"), "#!/usr/bin/env bash\necho VENV_MARKER\n").unwrap();
    let mut perms = fs::metadata(venv_bin.join("pip")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(venv_bin.join("pip"), perms).unwrap();
}

fn run_dep(project_dir: &std::path::Path) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("dependencies"))
        .current_dir(project_dir)
        .env_remove("COVERAGE_PROCESS_START")
        .output()
        .unwrap()
}

/// bin/dependencies must use the venv pip and call it once for install.
#[test]
fn uses_venv_pip() {
    let dir = tempfile::tempdir().unwrap();
    setup_dep_project(dir.path());
    let output = run_dep(dir.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let marker_count = stdout.matches("VENV_MARKER").count();
    assert_eq!(
        marker_count, 1,
        "pip should be called once (install), got {} calls",
        marker_count
    );
}

/// bin/dependencies must fail when .venv/bin/pip is missing.
#[test]
fn fails_when_no_venv() {
    let dir = tempfile::tempdir().unwrap();
    setup_dep_project(dir.path());
    // Remove the venv
    fs::remove_dir_all(dir.path().join(".venv")).unwrap();
    let output = run_dep(dir.path());
    assert!(
        !output.status.success(),
        "Should fail when .venv/bin/pip is missing"
    );
}
