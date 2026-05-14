//! Tests for bin/flow — the subcommand dispatcher.
//!
//! Validates the Rust subcommand dispatcher.

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
    assert!(data["message"]
        .as_str()
        .unwrap()
        .contains("nonexistent-command"),);
}

/// Known subcommand dispatches to the Rust binary.
#[test]
fn dispatches_to_correct_script() {
    // extract-release-notes with no args exits 1 with usage message
    let output = run_flow(&["extract-release-notes"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage"), "Expected 'Usage' in stdout");
}

/// Arguments after the subcommand are passed through.
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

/// Exit code from the Rust binary is preserved.
#[test]
fn exit_code_passes_through() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_flow(
        &["check-phase", "--required", "flow-code"],
        Some(dir.path()),
    );
    assert_ne!(output.status.code(), Some(0));
}

// --- Dispatcher tests with fixture projects ---

/// Creates a self-contained project for dispatcher tests.
fn setup_project(dir: &std::path::Path) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let real_script = common::bin_dir().join("flow");
    fs::write(
        bin_dir.join("flow"),
        fs::read_to_string(&real_script).unwrap(),
    )
    .unwrap();
    let mut perms = fs::metadata(bin_dir.join("flow")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join("flow"), perms).unwrap();
}

fn run_dispatcher(
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

/// When the underlying flow-rs binary exits 127 (subcommand not found), the
/// dispatcher must surface a structured error JSON to stdout instead of
/// silently dropping the failure or printing raw shell text.
#[test]
fn rust_exit_127_returns_error_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\nexit 127\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs"))
        .unwrap()
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_dispatcher(dir.path(), &["test-cmd"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(data["status"], "error");
    assert!(
        data["message"].as_str().unwrap().contains("test-cmd"),
        "Error message should name the unknown subcommand"
    );
}

/// When Rust binary handles the command (exit != 127), use its result.
#[test]
fn rust_passes_through_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"rust-handled\"\nexit 0\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs"))
        .unwrap()
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_dispatcher(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("rust-handled"));
}

/// Non-127 non-zero Rust exit code passes through.
#[test]
fn rust_passes_through_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    let target_dir = dir.path().join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("flow-rs"),
        "#!/usr/bin/env bash\necho \"rust-error\"\nexit 42\n",
    )
    .unwrap();
    let mut perms = fs::metadata(target_dir.join("flow-rs"))
        .unwrap()
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    let output = run_dispatcher(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(42));
    assert!(stdout.contains("rust-error"));
}

/// When no Rust binary exists and no Cargo.toml, returns error JSON.
#[test]
fn no_binary_returns_error_json() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    let output = run_dispatcher(dir.path(), &["test-cmd"], None);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(data["status"], "error");
}

/// When both release and debug binaries exist, release is preferred.
#[test]
fn prefers_release_over_debug() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
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
        let mut perms = fs::metadata(target_dir.join("flow-rs"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();
    }

    let output = run_dispatcher(dir.path(), &["test-cmd"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("release-handled"));
}

/// Hook subcommand fails closed (exit 2) when no Rust binary available.
#[test]
fn hook_subcommand_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    let output = run_dispatcher(dir.path(), &["hook", "validate-pretool"], None);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLOCKED"),
        "Hook failure should output BLOCKED message"
    );
}

// --- Auto-rebuild tests ---

fn setup_cargo_project(dir: &std::path::Path) -> std::path::PathBuf {
    setup_project(dir);
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"flow-rs\"\n").unwrap();
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
    let mut perms = fs::metadata(target_dir.join("flow-rs"))
        .unwrap()
        .permissions();
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
    let output = run_dispatcher(dir.path(), &["test-cmd"], Some(&path));
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
    let mut perms = fs::metadata(target_dir.join("flow-rs"))
        .unwrap()
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(target_dir.join("flow-rs"), perms).unwrap();

    // Replace mock cargo with a sentinel writer
    let sentinel = dir.path().join("cargo_was_called");
    fs::write(
        mock_bin_dir.join("cargo"),
        format!("#!/usr/bin/env bash\ntouch \"{}\"\n", sentinel.display()),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_dispatcher(dir.path(), &["test-cmd"], Some(&path));
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
    let output = run_dispatcher(dir.path(), &["test-cmd"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("rebuilt-handled"));
}

/// When cargo build fails and no binary exists, returns error JSON.
#[test]
fn auto_rebuild_failure_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());

    // Mock cargo that fails
    fs::write(mock_bin_dir.join("cargo"), "#!/usr/bin/env bash\nexit 1\n").unwrap();

    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_dispatcher(dir.path(), &["test-cmd"], Some(&path));
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(data["status"], "error");
}

// --- Committed-binary resolution candidate ---

/// Writes an executable bash script at `path` that echoes `identity`
/// and exits 0, so dispatcher-resolution tests can assert which
/// candidate the dispatcher selected by inspecting stdout.
fn fake_binary(path: &std::path::Path, identity: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        format!("#!/usr/bin/env bash\necho \"{}\"\nexit 0\n", identity),
    )
    .unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// When target/release, target/debug, AND the committed
/// bin/flow-rs-darwin-arm64 binary all exist, target/release wins —
/// the committed candidate must not disturb existing precedence.
#[test]
fn dispatcher_prefers_target_release_when_present() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(&dir.path().join("target/release/flow-rs"), "target-release");
    fake_binary(&dir.path().join("target/debug/flow-rs"), "target-debug");
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );
    let output = run_dispatcher(dir.path(), &["noop"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("target-release"),
        "expected target-release, got: {}",
        stdout
    );
}

/// When target/release is absent but target/debug and the committed
/// binary both exist, target/debug wins over the committed candidate.
#[test]
fn dispatcher_falls_back_to_target_debug_when_release_missing() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(&dir.path().join("target/debug/flow-rs"), "target-debug");
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );
    let output = run_dispatcher(dir.path(), &["noop"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("target-debug"),
        "expected target-debug, got: {}",
        stdout
    );
}

/// When no target/* binary exists, the dispatcher resolves the
/// committed bin/flow-rs-darwin-arm64 candidate — the end-user case
/// where `/plugin install` shipped the prebuilt binary.
#[test]
fn dispatcher_falls_back_to_committed_binary_when_target_absent() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );
    let output = run_dispatcher(dir.path(), &["noop"], None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("committed-binary"),
        "expected committed-binary, got: {}",
        stdout
    );
}

/// When no target/* binary and no committed binary exist but
/// Cargo.toml + src/ are present, the dispatcher falls through to the
/// auto-rebuild path — the committed candidate does not short-circuit
/// the contributor rebuild flow.
#[test]
fn dispatcher_falls_through_to_auto_rebuild_when_no_binaries_present() {
    let dir = tempfile::tempdir().unwrap();
    let mock_bin_dir = setup_cargo_project(dir.path());
    let path = format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    );
    let output = run_dispatcher(dir.path(), &["noop"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("rebuilt-handled"),
        "expected rebuilt-handled, got: {}",
        stdout
    );
}

// --- Committed-binary auto-rebuild interaction ---

/// Writes an executable `cargo` mock at `<dir>/mock_bin/cargo` with the
/// given script body and returns a PATH value that puts the mock first.
/// Callers vary `body` to simulate a cargo that succeeds-but-builds-
/// nothing, a cargo that records its invocation, or a cargo that fails.
fn mock_cargo_path(dir: &std::path::Path, body: &str) -> String {
    let mock_bin_dir = dir.join("mock_bin");
    fs::create_dir_all(&mock_bin_dir).unwrap();
    let mock_cargo = mock_bin_dir.join("cargo");
    fs::write(&mock_cargo, body).unwrap();
    let mut perms = fs::metadata(&mock_cargo).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_cargo, perms).unwrap();
    format!(
        "{}:{}",
        mock_bin_dir.display(),
        std::env::var("PATH").unwrap()
    )
}

/// When the committed binary resolves AND Cargo.toml + src/ are present
/// with src/ newer than the committed binary, the auto-rebuild fires.
/// If `cargo build` exits 0 without producing a target/* binary (the
/// marketplace-cache layout — source ships beside the committed binary,
/// no target/ tree), the post-build re-resolution must still fall back
/// to the committed binary instead of dropping it.
#[test]
fn committed_binary_survives_when_rebuild_produces_no_target_binary() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"flow-rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    // src/main.rs newer than the committed binary → staleness check fires.
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let path = mock_cargo_path(dir.path(), "#!/usr/bin/env bash\nexit 0\n");
    let output = run_dispatcher(dir.path(), &["noop"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("committed-binary"),
        "dispatcher dropped the still-present committed binary after a \
         no-op rebuild: status={:?} stdout={:?}",
        output.status.code(),
        stdout
    );
}

/// A committed binary newer than src/ must NOT trigger a rebuild — the
/// `find ... -newer "$RUST_BIN"` staleness anchor treats the resolved
/// committed binary as a valid staleness reference.
#[test]
fn fresh_committed_binary_does_not_trigger_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"flow-rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    // Committed binary created AFTER src/ → it is fresh.
    std::thread::sleep(std::time::Duration::from_millis(50));
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );

    let sentinel = dir.path().join("cargo_was_called");
    let path = mock_cargo_path(
        dir.path(),
        &format!("#!/usr/bin/env bash\ntouch \"{}\"\n", sentinel.display()),
    );
    let output = run_dispatcher(dir.path(), &["noop"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("committed-binary"),
        "dispatcher did not resolve the fresh committed binary: stdout={:?}",
        stdout
    );
    assert!(
        !sentinel.exists(),
        "cargo was invoked even though the committed binary is newer than src/"
    );
}

/// When the committed binary resolves, src/ is newer, AND `cargo build`
/// fails, the dispatcher must keep the present, runnable committed
/// binary — the failed-build path leaves RUST_BIN untouched.
#[test]
fn committed_binary_survives_failed_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-binary",
    );
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"flow-rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let path = mock_cargo_path(dir.path(), "#!/usr/bin/env bash\nexit 1\n");
    let output = run_dispatcher(dir.path(), &["noop"], Some(&path));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("committed-binary"),
        "dispatcher errored despite a present committed binary after a \
         failed rebuild: status={:?} stdout={:?}",
        output.status.code(),
        stdout
    );
}

/// The `hook` subcommand must also survive a failed rebuild when the
/// committed binary is present — otherwise every PreToolUse hook fails
/// closed (exit 2) on a contributor checkout whose committed binary is
/// stale relative to src/ and whose cargo build fails.
#[test]
fn committed_binary_hook_survives_failed_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    setup_project(dir.path());
    fake_binary(
        &dir.path().join("bin/flow-rs-darwin-arm64"),
        "committed-hook-handled",
    );
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"flow-rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let path = mock_cargo_path(dir.path(), "#!/usr/bin/env bash\nexit 1\n");
    let output = run_dispatcher(dir.path(), &["hook", "validate-pretool"], Some(&path));
    assert!(
        output.status.success(),
        "hook subcommand failed (status={:?}) despite a present, runnable \
         committed binary after a failed rebuild",
        output.status.code()
    );
}
