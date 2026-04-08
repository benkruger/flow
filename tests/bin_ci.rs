//! Tests for bin/ci — the project CI runner (Rust-only since PR #953).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Creates a minimal Rust project layout that bin/ci can run against.
///
/// bin/ci computes REPO_ROOT from $(dirname "$0")/.., so placing it at
/// <tmp>/bin/ci makes it resolve Cargo.toml at <tmp>/Cargo.toml.
/// Uses mock cargo to avoid real compilation.
fn setup_ci_project(dir: &std::path::Path) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Copy the real bin/ci script content
    let real_script = common::bin_dir().join("ci");
    let script_content = fs::read_to_string(&real_script).unwrap();
    fs::write(bin_dir.join("ci"), &script_content).unwrap();
    let mut perms = fs::metadata(bin_dir.join("ci")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("ci"), perms).unwrap();

    // Create Cargo.toml for cargo build detection
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
}

fn run_ci(project_dir: &std::path::Path, extra_path: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new("bash");
    cmd.arg(project_dir.join("bin").join("ci"))
        .current_dir(project_dir)
        .env_remove("COVERAGE_PROCESS_START");
    if let Some(path) = extra_path {
        cmd.env("PATH", path);
    }
    cmd.output().unwrap()
}

/// Creates a mock cargo that logs its invocations and exits 0.
fn setup_mock_cargo(dir: &std::path::Path) -> std::path::PathBuf {
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

/// bin/ci runs cargo build, cargo test, cargo clippy, and cargo fmt.
#[test]
fn runs_all_cargo_commands() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    let mock_bin = setup_mock_cargo(dir.path());
    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci(dir.path(), Some(&path));
    assert!(
        output.status.success(),
        "Expected exit 0\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let log = fs::read_to_string(dir.path().join("cargo_log")).unwrap();
    assert!(
        log.contains("build --quiet"),
        "Should run cargo build, got: {}",
        log
    );
    assert!(
        log.contains("test --quiet"),
        "Should run cargo test, got: {}",
        log
    );
    assert!(
        log.contains("clippy --quiet"),
        "Should run cargo clippy, got: {}",
        log
    );
    assert!(
        log.contains("fmt --check"),
        "Should run cargo fmt --check, got: {}",
        log
    );
}

/// bin/ci exits non-zero when cargo test fails.
#[test]
fn exits_nonzero_when_cargo_test_fails() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    // Mock cargo that fails on "test" subcommand
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\nif [[ \"$1\" == \"test\" ]]; then exit 1; fi\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci(dir.path(), Some(&path));
    assert!(
        !output.status.success(),
        "Expected non-zero exit code when cargo test fails"
    );
}

/// bin/ci exits non-zero when cargo clippy fails.
#[test]
fn exits_nonzero_when_cargo_clippy_fails() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    // Mock cargo that fails on "clippy" subcommand
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\nif [[ \"$1\" == \"clippy\" ]]; then exit 1; fi\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci(dir.path(), Some(&path));
    assert!(
        !output.status.success(),
        "Expected non-zero exit code when cargo clippy fails"
    );
}

/// bin/ci exits non-zero when cargo fmt --check fails.
#[test]
fn exits_nonzero_when_cargo_fmt_fails() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    // Mock cargo that fails on "fmt" subcommand
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\nif [[ \"$1\" == \"fmt\" ]]; then exit 1; fi\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci(dir.path(), Some(&path));
    assert!(
        !output.status.success(),
        "Expected non-zero exit code when cargo fmt fails"
    );
}

/// bin/ci skips cargo build when no Cargo.toml exists.
#[test]
fn skips_build_when_no_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    // Remove Cargo.toml
    fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
    let mock_bin = setup_mock_cargo(dir.path());
    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci(dir.path(), Some(&path));
    assert!(
        output.status.success(),
        "Expected exit 0\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let log = fs::read_to_string(dir.path().join("cargo_log")).unwrap();
    assert!(
        !log.contains("build"),
        "Should not run cargo build without Cargo.toml, got: {}",
        log
    );
    assert!(
        log.contains("test"),
        "Should still run cargo test, got: {}",
        log
    );
}
