//! Tests for bin/flow — the subcommand dispatcher.
//!
//! Ports tests/test_bin_flow.py to Rust integration tests.
//! Each test validates the same invariant as its Python counterpart.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn run_flow(args: &[&str], cwd: Option<&std::path::Path>) -> std::process::Output {
    let script = common::bin_dir().join("flow");
    let repo = common::repo_root();
    Command::new("bash")
        .arg(&script)
        .args(args)
        .current_dir(cwd.unwrap_or(&repo))
        .output()
        .unwrap()
}

// --- Direct dispatcher tests (use the real repo's bin/flow) ---

/// Running with no arguments returns JSON error and exit 1.
#[test]
fn no_subcommand_returns_error_json() {
    let output = run_flow(&[], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap().contains("Usage"),
        "Expected 'Usage' in message, got: {}",
        data["message"]
    );
}

/// Running with a nonexistent subcommand returns JSON error and exit 1.
#[test]
fn unknown_subcommand_returns_error_json() {
    let output = run_flow(&["nonexistent-command"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(data["status"], "error");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("nonexistent-command"),
    );
}

/// Known subcommand dispatches to the matching .py file in lib/.
#[test]
fn dispatches_to_correct_script() {
    // extract-release-notes with no args exits 1 with usage message
    let output = run_flow(&["extract-release-notes"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage"), "Expected 'Usage' in stdout");
}

/// Arguments after the subcommand are passed to the Python script.
#[test]
fn passes_arguments_through() {
    let output = run_flow(&["extract-release-notes", "../../etc/passwd"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("invalid version format"),
        "Expected 'invalid version format' in stdout"
    );
}

/// Exit code from the Python script is preserved.
#[test]
fn exit_code_passes_through() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_flow(
        &["check-phase", "--required", "flow-plan"],
        Some(dir.path()),
    );
    assert_ne!(output.status.code(), Some(0));
}

// --- Hybrid dispatcher tests ---

/// Creates a self-contained project for hybrid dispatcher tests.
fn setup_hybrid_project(dir: &std::path::Path) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let real_script = common::bin_dir().join("flow");
    fs::write(bin_dir.join("flow"), fs::read_to_string(&real_script).unwrap()).unwrap();
    let mut perms = fs::metadata(bin_dir.join("flow")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("flow"), perms).unwrap();

    let lib_dir = dir.join("lib");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(lib_dir.join("test-cmd.py"), "print(\"python-handled\")\n").unwrap();
}

fn run_hybrid(
    project_dir: &std::path::Path,
    args: &[&str],
    extra_path: Option<&str>,
) -> std::process::Output {
    let mut cmd = Command::new("bash");
    cmd.arg(project_dir.join("bin").join("flow"))
        .args(args)
        .current_dir(project_dir);
    if let Some(path) = extra_path {
        cmd.env("PATH", path);
    }
    cmd.output().unwrap()
}

/// When Rust binary exists but exits 127, dispatcher falls back to Python.
#[test]
fn hybrid_falls_back_when_rust_exits_127() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("flow-rs"), "#!/usr/bin/env bash\nexit 127\n").unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("python-handled"));
}

/// When Rust binary handles the command (exit != 127), use its result.
#[test]
fn hybrid_passes_through_rust_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"rust-handled\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("rust-handled"));
    assert!(!stdout.contains("python-handled"));
}

/// Non-127 non-zero Rust exit code passes through without Python fallback.
#[test]
fn hybrid_passes_through_nonzero_rust_exit() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"rust-error\"\nexit 42\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(42));
    assert!(stdout.contains("rust-error"));
    assert!(!stdout.contains("python-handled"));
}

/// When no Rust binary exists, commands route to Python.
#[test]
fn dispatcher_works_without_rust_binary() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("python-handled"));
}

/// When both release and debug binaries exist, release is preferred.
#[test]
fn hybrid_prefers_release_over_debug() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    for variant in &["debug", "release"] {
        let target_dir = dir.path().join("target").join(variant);
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(
            target_dir.join("flow-rs"),
            format!(
                "#!/usr/bin/env bash\necho \"{}-handled\"\nexit 0\n",
                variant
            ),
        )
        .unwrap();
        let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();
    }

    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("release-handled"));
}

// --- Auto-rebuild tests ---

fn setup_cargo_project(dir: &std::path::Path) -> std::path::PathBuf {
    setup_hybrid_project(dir);
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"flow-rs\"\n",
    )
    .unwrap();
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("main.rs"), "fn main() {}\n").unwrap();

    let mock_bin_dir = dir.join("mock_bin");
    fs::create_dir_all(&mock_bin_dir).unwrap();
    let mock_cargo = mock_bin_dir.join("cargo");
    fs::write(
        &mock_cargo,
        "#!/usr/bin/env bash\n\
         MANIFEST_DIR=\"$(dirname \"$3\")\"\n\
         mkdir -p \"$MANIFEST_DIR/target/debug\"\n\
         cat > \"$MANIFEST_DIR/target/debug/flow-rs\" << 'SCRIPT'\n\
         #!/usr/bin/env bash\n\
         echo \"rebuilt-handled\"\n\
         exit 0\n\
         SCRIPT\n\
         chmod +x \"$MANIFEST_DIR/target/debug/flow-rs\"\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&mock_cargo).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_cargo, perms).unwrap();

    mock_bin_dir
}

/// When src/ is newer than the binary, auto-rebuild triggers.
#[test]
fn auto_rebuild_stale_binary() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());

    // Create a stale binary
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"stale-handled\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    // Make src/main.rs newer than the binary
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(
        dir.path().join("src").join("main.rs"),
        "fn main() { /* updated */ }\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_hybrid(dir.path(), &["test-cmd"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("rebuilt-handled"));
}

/// When binary is newer than src/, no rebuild occurs.
#[test]
fn auto_rebuild_skips_fresh_binary() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());

    // src/main.rs already exists from fixture
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Create binary AFTER src files
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"fresh-handled\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    // Replace mock cargo with a sentinel writer
    let sentinel = dir.path().join("cargo_was_called");
    fs::write(
        mock_bin_dir.join("cargo"),
        format!(
            "#!/usr/bin/env bash\ntouch \"{}\"\n",
            sentinel.display()
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_hybrid(dir.path(), &["test-cmd"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("fresh-handled"));
    assert!(
        !sentinel.exists(),
        "cargo should not have been called for a fresh binary"
    );
}

/// When Cargo.toml + src/ exist but no binary, auto-rebuild triggers.
#[test]
fn auto_rebuild_first_build() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_hybrid(dir.path(), &["test-cmd"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("rebuilt-handled"));
}

/// When cargo build fails, script falls back to Python without crashing.
#[test]
fn auto_rebuild_failure_falls_back_to_python() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());

    // Mock cargo that fails
    fs::write(
        mock_bin_dir.join("cargo"),
        "#!/usr/bin/env bash\nexit 1\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_hybrid(dir.path(), &["test-cmd"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("python-handled"));
}

/// When no Cargo.toml exists, auto-rebuild block is skipped entirely.
#[test]
fn auto_rebuild_skips_without_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    setup_hybrid_project(dir.path());
    // No Cargo.toml — should use Python directly
    let output = run_hybrid(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("python-handled"));
}

/// Every .py file in lib/ (except flow_utils.py) is reachable as a subcommand.
#[test]
fn every_lib_script_is_reachable() {
    let lib_dir = common::repo_root().join("lib");
    let mut scripts = Vec::new();
    for entry in fs::read_dir(&lib_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("py") {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name != "flow_utils.py" {
                scripts.push(name);
            }
        }
    }
    assert!(!scripts.is_empty(), "Expected at least one lib/*.py script");
    for script in &scripts {
        let subcmd = script.trim_end_matches(".py");
        let resolved = lib_dir.join(format!("{}.py", subcmd));
        assert!(
            resolved.is_file(),
            "bin/flow cannot find subcommand '{}' — expected {} to exist",
            subcmd,
            resolved.display()
        );
    }
}
