//! Tests for bin/test — the pytest wrapper script.
//!
//! Ports tests/test_bin_test.py to Rust integration tests.
//! Each test validates the same invariant as its Python counterpart.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Creates a minimal project layout that bin/test can run against.
///
/// bin/test computes REPO_ROOT from $(dirname "$0")/.., so placing it at
/// <tmp>/bin/test makes it resolve .venv relative to the temp dir.
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

    // Create tests/ directory
    fs::create_dir_all(dir.join("tests")).unwrap();

    // Create venv python3 wrapper that delegates to the repo's venv python
    // (which has pytest installed). Mirrors the Python test's use of sys.executable.
    let venv_bin = dir.join(".venv").join("bin");
    fs::create_dir_all(&venv_bin).unwrap();
    let repo_python = common::repo_root().join(".venv").join("bin").join("python3");
    fs::write(
        venv_bin.join("python3"),
        format!(
            "#!/usr/bin/env bash\nexec {} \"$@\"\n",
            repo_python.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(venv_bin.join("python3")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(venv_bin.join("python3"), perms).unwrap();
}

fn run_test(project_dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("test"))
        .args(args)
        .current_dir(project_dir)
        .env_remove("COVERAGE_PROCESS_START")
        .output()
        .unwrap()
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
        .env_remove("COVERAGE_PROCESS_START")
        .env(key, value)
        .output()
        .unwrap()
}

/// bin/test exits 0 when pytest passes.
#[test]
fn exits_0_when_pytest_passes() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok(): assert True\n",
    )
    .unwrap();
    let output = run_test(dir.path(), &["tests/"]);
    assert!(
        output.status.success(),
        "Expected exit 0, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/test exits non-zero when pytest fails.
#[test]
fn exits_nonzero_when_pytest_fails() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_fail.py"),
        "def test_bad(): assert False\n",
    )
    .unwrap();
    let output = run_test(dir.path(), &["tests/"]);
    assert!(!output.status.success(), "Expected non-zero exit code");
}

/// bin/test passes arguments through to pytest.
#[test]
fn passes_arguments_through() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok(): assert True\ndef test_also(): assert True\n",
    )
    .unwrap();
    let output = run_test(dir.path(), &["tests/", "-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("test_ok"), "stdout should contain test_ok");
    assert!(
        stdout.contains("test_also"),
        "stdout should contain test_also"
    );
}

/// bin/test --rust runs cargo test instead of pytest.
#[test]
fn rust_flag_runs_cargo_test() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());

    // Create mock cargo that echoes a marker
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_RUST_MARKER: $*\"\nexit 0\n",
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
        stdout.contains("CARGO_RUST_MARKER: test"),
        "Expected cargo test invocation, got: {}",
        stdout
    );
}

/// bin/test --rust passes remaining args to cargo test.
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
        stdout.contains("CARGO_ARGS: test my_test_name -- --nocapture"),
        "Expected full args, got: {}",
        stdout
    );
}

/// bin/test must always pass --no-cov so coverage is skipped.
#[test]
fn passes_no_cov_flag() {
    let script = fs::read_to_string(common::bin_dir().join("test")).unwrap();
    assert!(
        script.contains("--no-cov"),
        "bin/test must contain --no-cov flag"
    );
}
