//! Tests for bin/ci — the project CI runner.
//!
//! Ports tests/test_bin_ci.py to Rust integration tests.
//! Each test validates the same invariant as its Python counterpart.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Creates a minimal project layout that bin/ci can run against.
///
/// bin/ci computes REPO_ROOT from $(dirname "$0")/.., so placing it at
/// <tmp>/bin/ci makes it run pytest against <tmp>/tests/.
/// Includes a .venv/bin/python3 wrapper that delegates to the repo's venv
/// python so pytest/ruff/pymarkdown are available.
///
/// IMPORTANT: Uses a wrapper script, NOT a symlink.
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

    // Create README.md for pymarkdown
    fs::write(dir.join("README.md"), "# Test\n").unwrap();

    // Copy pymarkdown and ruff configs from the real repo
    let repo = common::repo_root();
    fs::copy(repo.join(".pymarkdown.yml"), dir.join(".pymarkdown.yml")).unwrap();
    fs::copy(repo.join("ruff.toml"), dir.join("ruff.toml")).unwrap();

    // Create lib/ and tests/ directories
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();

    // Create venv python3 wrapper that delegates to the repo's venv python
    let venv_bin = dir.join(".venv").join("bin");
    fs::create_dir_all(&venv_bin).unwrap();
    let repo_python = repo.join(".venv").join("bin").join("python3");
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

fn run_ci(project_dir: &std::path::Path) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("ci"))
        .current_dir(project_dir)
        .env_remove("COVERAGE_PROCESS_START")
        .output()
        .unwrap()
}

fn run_ci_with_env(
    project_dir: &std::path::Path,
    key: &str,
    value: &str,
) -> std::process::Output {
    Command::new("bash")
        .arg(project_dir.join("bin").join("ci"))
        .current_dir(project_dir)
        .env_remove("COVERAGE_PROCESS_START")
        .env(key, value)
        .output()
        .unwrap()
}

/// bin/ci exits 0 when pytest passes.
#[test]
fn exits_0_when_pytest_passes() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let output = run_ci(dir.path());
    assert!(
        output.status.success(),
        "Expected exit 0, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/ci exits non-zero when pytest fails.
#[test]
fn exits_nonzero_when_pytest_fails() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_fail.py"),
        "def test_bad():\n    assert False\n",
    )
    .unwrap();
    let output = run_ci(dir.path());
    assert!(!output.status.success(), "Expected non-zero exit code");
}

/// bin/ci uses venv python when available.
#[test]
fn uses_venv_python_when_available() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    // Replace the venv python with a marker script
    let fake_python = dir.path().join(".venv").join("bin").join("python3");
    fs::write(&fake_python, "#!/usr/bin/env bash\necho VENV_MARKER\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&fake_python).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_python, perms).unwrap();
    let output = run_ci(dir.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VENV_MARKER"),
        "Should use venv python, stdout: {}",
        stdout
    );
}

/// bin/ci runs ruff check and ruff format --check before pytest.
#[test]
fn runs_ruff_check_and_format() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    // Replace venv python with a marker script
    let fake_python = dir.path().join(".venv").join("bin").join("python3");
    fs::write(
        &fake_python,
        "#!/usr/bin/env bash\necho \"RUFF_MARKER: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_python).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_python, perms).unwrap();
    let output = run_ci(dir.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("RUFF_MARKER: -m ruff check lib/ tests/"),
        "Should run ruff check, stdout: {}",
        stdout
    );
    assert!(
        stdout.contains("RUFF_MARKER: -m ruff format --check lib/ tests/"),
        "Should run ruff format --check, stdout: {}",
        stdout
    );
}

/// bin/ci runs cargo test when Cargo.toml exists.
#[test]
fn runs_cargo_test_when_cargo_toml_exists() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    // Create mock cargo
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_TEST_MARKER: $*\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci_with_env(dir.path(), "PATH", &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("CARGO_TEST_MARKER: test"),
        "Should run cargo test, stdout: {}",
        stdout
    );
}

/// bin/ci does not run cargo when no Cargo.toml exists.
#[test]
fn skips_cargo_when_no_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    fs::write(
        mock_bin.join("cargo"),
        "#!/usr/bin/env bash\necho \"CARGO_SHOULD_NOT_RUN\"\nexit 1\n",
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci_with_env(dir.path(), "PATH", &path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        !stdout.contains("CARGO_SHOULD_NOT_RUN"),
        "Should not run cargo without Cargo.toml"
    );
}

/// bin/ci falls back to system python when no venv.
#[test]
fn falls_back_to_system_python_when_no_venv() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    // Remove the venv
    fs::remove_dir_all(dir.path().join(".venv")).unwrap();

    // Create a local_bin with a python3 wrapper pointing to the real venv python
    let local_bin = dir.path().join("local_bin");
    fs::create_dir_all(&local_bin).unwrap();
    let repo_python = common::repo_root().join(".venv").join("bin").join("python3");
    fs::write(
        local_bin.join("python3"),
        format!(
            "#!/usr/bin/env bash\nexec {} \"$@\"\n",
            repo_python.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(local_bin.join("python3")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(local_bin.join("python3"), perms).unwrap();

    let path = format!("{}:{}", local_bin.display(), std::env::var("PATH").unwrap());
    let output = run_ci_with_env(dir.path(), "PATH", &path);
    assert!(
        output.status.success(),
        "Expected exit 0 with system python fallback\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// bin/ci removes stale __pycache__ dirs (excl. .venv) before running pytest.
#[test]
fn cleans_stale_pycache_before_pytest() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();

    // Create stale .pyc in lib/__pycache__/
    let pycache = dir.path().join("lib").join("__pycache__");
    fs::create_dir_all(&pycache).unwrap();
    let stale_pyc = pycache.join("deleted_module.cpython-314.pyc");
    fs::write(&stale_pyc, &[0u8]).unwrap();

    // Create stale .pyc in tests/__pycache__/
    let test_pycache = dir.path().join("tests").join("__pycache__");
    fs::create_dir_all(&test_pycache).unwrap();
    let stale_test_pyc = test_pycache.join("test_deleted.cpython-314-pytest-9.0.2.pyc");
    fs::write(&stale_test_pyc, &[0u8]).unwrap();

    let output = run_ci(dir.path());
    assert!(output.status.success());
    assert!(
        !stale_pyc.exists(),
        "stale lib .pyc should be cleaned by bin/ci"
    );
    assert!(
        !stale_test_pyc.exists(),
        "stale test .pyc should be cleaned by bin/ci"
    );
}

/// bin/ci must NOT clean __pycache__ inside .venv/.
#[test]
fn pycache_cleanup_preserves_venv() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();

    let venv_pycache = dir
        .path()
        .join(".venv")
        .join("lib")
        .join("python3")
        .join("__pycache__");
    fs::create_dir_all(&venv_pycache).unwrap();
    let venv_marker = venv_pycache.join("venv_module.cpython-314.pyc");
    fs::write(&venv_marker, &[0u8]).unwrap();

    let output = run_ci(dir.path());
    assert!(output.status.success());
    assert!(
        venv_marker.exists(),
        ".venv __pycache__ must be preserved"
    );
}

/// bin/ci preserves .venv/__pycache__ even when invoked from a subdirectory.
#[test]
fn pycache_cleanup_preserves_venv_from_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    setup_ci_project(dir.path());
    fs::write(
        dir.path().join("tests").join("test_pass.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();

    let venv_pycache = dir
        .path()
        .join(".venv")
        .join("lib")
        .join("python3")
        .join("__pycache__");
    fs::create_dir_all(&venv_pycache).unwrap();
    let venv_marker = venv_pycache.join("venv_pkg.cpython-314.pyc");
    fs::write(&venv_marker, &[0u8]).unwrap();

    // Invoke from the lib/ subdirectory
    let output = Command::new("bash")
        .arg(dir.path().join("bin").join("ci"))
        .current_dir(dir.path().join("lib"))
        .env_remove("COVERAGE_PROCESS_START")
        .output()
        .unwrap();

    // bin/ci may fail for other reasons from a subdirectory. The assertion
    // is that .venv/__pycache__ survives regardless of CWD.
    assert!(
        venv_marker.exists(),
        ".venv __pycache__ was deleted when bin/ci ran from a subdirectory. \
         find must use absolute $REPO_ROOT. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
