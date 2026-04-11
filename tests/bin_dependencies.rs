//! Tests for `bin/dependencies` — the project's dependency updater script.

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
    let output = Command::new("bash").arg("-n").arg(&dep).output().unwrap();
    assert!(
        output.status.success(),
        "Syntax error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Creates a minimal project layout for bin/dependencies testing.
fn setup_dep_project(dir: &std::path::Path) -> std::path::PathBuf {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Copy the real bin/dependencies script content
    let real_script = common::bin_dir().join("dependencies");
    let script_content = fs::read_to_string(&real_script).unwrap();
    fs::write(bin_dir.join("dependencies"), &script_content).unwrap();
    let mut perms = fs::metadata(bin_dir.join("dependencies"))
        .unwrap()
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("dependencies"), perms).unwrap();

    // Create mock cargo
    let mock_bin = dir.join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    let log_file = dir.join("cargo_log");
    fs::write(
        mock_bin.join("cargo"),
        format!(
            "#!/usr/bin/env bash\necho \"$*\" >> \"{}\"\nexit 0\n",
            log_file.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    mock_bin
}

fn run_dep(project_dir: &std::path::Path, extra_path: &str) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("dependencies"))
        .current_dir(project_dir)
        .env("PATH", extra_path)
        .output()
        .unwrap()
}

/// bin/dependencies runs cargo update.
#[test]
fn runs_cargo_update() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin = setup_dep_project(dir.path());
    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_dep(dir.path(), &path);
    assert!(
        output.status.success(),
        "Expected exit 0\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let log = fs::read_to_string(dir.path().join("cargo_log")).unwrap();
    assert!(
        log.contains("update"),
        "Should run cargo update, got: {}",
        log
    );
}

/// bin/dependencies fails when cargo is not found.
#[test]
fn fails_when_cargo_update_fails() {
    let dir = tempfile::tempdir().unwrap();
    let _mock_bin = setup_dep_project(dir.path());
    // Replace with a mock cargo that fails
    let mock_bin = dir.path().join("fail_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(mock_bin.join("cargo"), "#!/usr/bin/env bash\nexit 1\n").unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_dep(dir.path(), &path);
    assert!(
        !output.status.success(),
        "Should fail when cargo update fails"
    );
}
