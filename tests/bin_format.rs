//! Tests for `bin/format` — the FLOW dogfood format checker.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// bin/format must exist and be executable.
#[test]
fn script_is_executable() {
    let script = common::bin_dir().join("format");
    assert!(script.exists(), "bin/format must exist");
    let meta = fs::metadata(&script).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/format must be executable"
    );
}

/// bin/format must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let script = common::bin_dir().join("format");
    let output = Command::new("bash")
        .arg("-n")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Syntax error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/format invokes `cargo fmt --check`.
#[test]
fn invokes_cargo_fmt_check() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Copy the real bin/format into the fixture
    let real_script = common::bin_dir().join("format");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("format");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

    // Mock cargo that logs its arguments
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    let log_file = dir.path().join("cargo_log");
    fs::write(
        mock_bin.join("cargo"),
        format!(
            "#!/usr/bin/env bash\necho \"$*\" > \"{}\"\nexit 0\n",
            log_file.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(&target)
        .current_dir(dir.path())
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let logged = fs::read_to_string(&log_file).unwrap();
    assert!(
        logged.contains("fmt"),
        "expected cargo fmt, got: {}",
        logged
    );
    assert!(
        logged.contains("--check"),
        "expected --check, got: {}",
        logged
    );
}

/// bin/format propagates a nonzero exit code from cargo.
#[test]
fn propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("format");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("format");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(mock_bin.join("cargo"), "#!/usr/bin/env bash\nexit 1\n").unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(&target)
        .current_dir(dir.path())
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(!output.status.success(), "should propagate cargo failure");
}
