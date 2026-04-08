//! Tests for bin/test — the Rust test runner (Rust-only since PR #953).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Creates a minimal project layout for bin/test testing.
fn setup_test_project(dir: &std::path::Path) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Copy the real bin/test script content
    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    fs::write(bin_dir.join("test"), &script_content).unwrap();
    let mut perms = fs::metadata(bin_dir.join("test")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("test"), perms).unwrap();
}

fn run_test_with_env(
    project_dir: &std::path::Path,
    args: &[&str],
    key: &str,
    value: &str,
) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("test"))
        .args(args)
        .current_dir(project_dir)
        .env(key, value)
        .output()
        .unwrap()
}

/// bin/test runs cargo nextest run.
#[test]
fn runs_cargo_nextest() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());

    // Create mock cargo
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_MARKER: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_test_with_env(dir.path(), &[], "PATH", &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("CARGO_MARKER: nextest run"),
        "Expected cargo nextest run invocation, got: {}",
        stdout
    );
}

/// bin/test passes arguments through to cargo nextest run.
#[test]
fn passes_arguments_through() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_ARGS: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_test_with_env(dir.path(), &["my_test", "--", "--nocapture"], "PATH", &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("CARGO_ARGS: nextest run my_test -- --nocapture"),
        "Expected full args, got: {}",
        stdout
    );
}

/// bin/test --rust flag is accepted (backwards compat no-op).
#[test]
fn rust_flag_runs_cargo_nextest() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_MARKER: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_test_with_env(dir.path(), &["--rust"], "PATH", &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("CARGO_MARKER: nextest run"),
        "Expected cargo nextest run invocation with --rust flag, got: {}",
        stdout
    );
}

/// bin/test --rust passes remaining args to cargo nextest run.
#[test]
fn rust_flag_passes_extra_args() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_ARGS: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_test_with_env(
        dir.path(),
        &["--rust", "my_test_name", "--", "--nocapture"],
        "PATH",
        &path,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("CARGO_ARGS: nextest run my_test_name -- --nocapture"),
        "Expected full args, got: {}",
        stdout
    );
}

/// bin/test must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let script = common::bin_dir().join("test");
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
