//! Tests for `bin/setup` — the one-time install-flow build script.
//!
//! The script is invoked by users from their plain terminal after
//! `/plugin install` and before `/flow:flow-prime`. It checks for
//! `cargo` and `cc` prereqs and runs `cargo build --release` when
//! both are present. Passed `--stage-binary`, it additionally copies
//! the fresh release binary to `bin/flow-rs-darwin-arm64` so the
//! committed prebuilt binary never lags the source.
//! The first three tests assert structural
//! contracts (existence, executable bit, bash syntax, content
//! snippets, shebang, strict-mode preamble, active success echo)
//! so an accidental edit that drops a prereq check, the build
//! invocation, the success message, the executable bit, the
//! bash-specific shebang, or the strict-mode preamble fails CI
//! immediately. The last two tests exercise the script's runtime
//! behavior against mocked PATHs so a regression that breaks the
//! prereq-missing exit code or stderr routing is caught — the
//! script's full build path (`cargo build --release`) is not
//! exercised here because it would take minutes.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn script_path() -> std::path::PathBuf {
    common::bin_dir().join("setup")
}

/// bin/setup must exist and be executable.
#[test]
fn script_is_executable() {
    let script = script_path();
    assert!(script.exists(), "bin/setup must exist");
    let meta = fs::metadata(&script).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/setup must be executable"
    );
}

/// bin/setup must contain valid bash syntax.
#[test]
fn script_is_valid_bash() {
    let script = script_path();
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

/// bin/setup must contain the prereq checks, install hints, build
/// invocation, success message, and the `--stage-binary` staging
/// logic that flow-release Step 6 depends on. Guards against
/// accidental edits that drop any of these.
#[test]
fn script_contains_expected_install_flow() {
    let script = script_path();
    let content = fs::read_to_string(&script).expect("bin/setup must be readable");
    let required = [
        "command -v cargo",
        "command -v cc",
        "brew install rust",
        "xcode-select --install",
        "cargo build --release",
        "Setup complete",
        "--stage-binary",
        "bin/flow-rs-darwin-arm64",
    ];
    for snippet in required {
        assert!(
            content.contains(snippet),
            "bin/setup must contain '{}'",
            snippet
        );
    }
}

/// bin/setup depends on `set -euo pipefail` semantics (`pipefail`
/// is a bashism `/bin/sh` does not honor on every platform), so
/// the shebang must invoke bash either via `/usr/bin/env bash` or
/// `/bin/bash`. A regression that flips the shebang to `/bin/sh`
/// would silently disable `pipefail` and the unset-var guard.
#[test]
fn script_shebang_invokes_bash() {
    let content = fs::read_to_string(script_path()).expect("read bin/setup");
    let first_line = content.lines().next().expect("script must be non-empty");
    assert!(
        first_line == "#!/usr/bin/env bash" || first_line == "#!/bin/bash",
        "bin/setup shebang must invoke bash so set -euo pipefail applies; got: {:?}",
        first_line
    );
}

/// bin/setup must declare `set -euo pipefail` so the prereq checks
/// stop the script on failure and an unset `$0` derivative cannot
/// silently produce a wrong `REPO_ROOT`. The content scan in
/// `script_contains_expected_install_flow` does not cover the
/// preamble — a regression that drops `set -euo pipefail` would
/// pass that test.
#[test]
fn script_uses_strict_mode() {
    let content = fs::read_to_string(script_path()).expect("read bin/setup");
    assert!(
        content.contains("set -euo pipefail"),
        "bin/setup must declare `set -euo pipefail` so error handling and unset-var detection are active; content was:\n{}",
        content
    );
}

/// The "Setup complete" success message must appear on an active
/// `echo` line, not just somewhere in a comment. The content scan
/// in `script_contains_expected_install_flow` would pass if a
/// future edit moved the literal into a comment block while
/// removing the `echo` — leaving the script silent on success.
#[test]
fn script_success_message_is_actively_echoed() {
    let content = fs::read_to_string(script_path()).expect("read bin/setup");
    let echoed = content.lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#')
            && trimmed.starts_with("echo")
            && trimmed.contains("Setup complete")
    });
    assert!(
        echoed,
        "bin/setup must contain an active `echo` of 'Setup complete' \
         (not just the substring in a comment); content was:\n{}",
        content
    );
}

/// When `cargo` is absent from PATH the script must exit non-zero
/// AND emit the `brew install rust` hint to stderr (not stdout) so
/// a wrapping pipeline can distinguish data from diagnostics. The
/// content scan only proves the string appears in the file; this
/// runtime test proves it reaches stderr on the prereq-missing
/// path.
#[test]
fn script_missing_cargo_exits_nonzero_with_stderr_hint() {
    let output = Command::new("bash")
        .arg(script_path())
        .env("PATH", "/usr/bin:/bin") // strip any cargo on developer PATH
        .output()
        .expect("spawn bin/setup");

    assert!(
        !output.status.success(),
        "bin/setup must exit non-zero when cargo is missing; got status {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("brew install rust"),
        "bin/setup must route the cargo install hint to stderr; stderr was: {:?}",
        stderr
    );
    assert!(
        !stdout.contains("brew install rust"),
        "bin/setup must NOT print the cargo install hint to stdout; stdout was: {:?}",
        stdout
    );
}

/// Stage a mock-cargo / mock-cc environment that lets bin/setup run
/// without invoking the real Rust toolchain. The mock cargo parses
/// `--manifest-path`, creates `target/release/` next to it, and
/// writes a small placeholder file to `target/release/flow-rs` with
/// mode 0755. Returns (tempdir, path_value, setup_script_path,
/// placeholder_contents) so the test can drive the script and
/// inspect the resulting tree.
fn stage_setup_fixture() -> (tempfile::TempDir, String, std::path::PathBuf, &'static [u8]) {
    const PLACEHOLDER: &[u8] = b"mock-flow-rs-binary-contents";
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    let mock_bin = root.join("mock_bin");
    fs::create_dir_all(&mock_bin).expect("create mock_bin");

    let mock_cargo_body = r#"#!/usr/bin/env bash
set -euo pipefail
manifest=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --manifest-path)
      manifest="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
repo_root="$(dirname "$manifest")"
mkdir -p "$repo_root/target/release"
printf 'mock-flow-rs-binary-contents' > "$repo_root/target/release/flow-rs"
chmod 0755 "$repo_root/target/release/flow-rs"
exit 0
"#;
    let mock_cargo = mock_bin.join("cargo");
    fs::write(&mock_cargo, mock_cargo_body).expect("write mock cargo");
    let mut perms = fs::metadata(&mock_cargo).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_cargo, perms).expect("chmod mock cargo");

    let mock_cc = mock_bin.join("cc");
    fs::write(&mock_cc, "#!/bin/sh\nexit 0\n").expect("write mock cc");
    let mut perms = fs::metadata(&mock_cc).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_cc, perms).expect("chmod mock cc");

    let staged_bin = root.join("bin");
    fs::create_dir_all(&staged_bin).expect("create staged bin");
    let staged_setup = staged_bin.join("setup");
    let setup_source = fs::read(script_path()).expect("read source bin/setup");
    fs::write(&staged_setup, &setup_source).expect("write staged setup");
    let mut perms = fs::metadata(&staged_setup).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&staged_setup, perms).expect("chmod staged setup");

    let path_value = format!("{}:/usr/bin:/bin", mock_bin.to_string_lossy());
    (tmp, path_value, staged_setup, PLACEHOLDER)
}

/// `bin/setup --stage-binary` must MOVE the freshly-built binary from
/// `target/release/flow-rs` to `bin/flow-rs-darwin-arm64`, not copy
/// it. A leftover `target/release/flow-rs` shadows source changes
/// during `--plugin-dir` QA because the dispatcher prefers the
/// release artifact when its mtime is newer than the committed
/// binary's mtime — so the second-run gate must assert the source
/// artifact is gone after staging.
#[test]
fn stage_binary_removes_target_release_artifact() {
    let (tmp, path_value, staged_setup, placeholder) = stage_setup_fixture();
    let root = tmp.path();

    let output = Command::new("/bin/bash")
        .arg(&staged_setup)
        .arg("--stage-binary")
        .env("PATH", &path_value)
        .output()
        .expect("spawn staged bin/setup");

    assert!(
        output.status.success(),
        "bin/setup --stage-binary must exit 0; got status {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let source_artifact = root.join("target/release/flow-rs");
    assert!(
        !source_artifact.exists(),
        "bin/setup --stage-binary must remove the source artifact at \
         target/release/flow-rs after staging; found it still on disk at {}",
        source_artifact.display()
    );

    let committed = root.join("bin/flow-rs-darwin-arm64");
    assert!(
        committed.exists(),
        "bin/setup --stage-binary must place the binary at \
         bin/flow-rs-darwin-arm64; missing at {}",
        committed.display()
    );
    let committed_meta = fs::metadata(&committed).expect("metadata committed");
    assert!(
        committed_meta.permissions().mode() & 0o111 != 0,
        "bin/setup --stage-binary must leave the committed binary executable; mode was {:o}",
        committed_meta.permissions().mode()
    );

    let committed_bytes = fs::read(&committed).expect("read committed binary");
    assert_eq!(
        committed_bytes.as_slice(),
        placeholder,
        "committed binary bytes must equal the mock cargo's output; \
         the move must transfer content, not just delete the source"
    );
}

/// When `cargo` is present but `cc` is absent the script must exit
/// non-zero AND emit the `xcode-select --install` hint to stderr.
/// Tests that only assert "bin/setup contains 'xcode-select
/// --install'" do not catch a regression that drops the `>&2`
/// redirect, or that flips the prereq check to a no-op.
#[test]
fn script_missing_cc_exits_nonzero_with_stderr_hint() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).expect("create fake bin dir");

    let fake_cargo = fake_bin.join("cargo");
    fs::write(&fake_cargo, "#!/bin/sh\nexit 0\n").expect("write fake cargo");
    let mut perms = fs::metadata(&fake_cargo).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_cargo, perms).expect("chmod fake cargo");

    let path_value = fake_bin.to_string_lossy().to_string();

    let output = Command::new("/bin/bash")
        .arg(script_path())
        .env("PATH", &path_value)
        .output()
        .expect("spawn bin/setup");

    assert!(
        !output.status.success(),
        "bin/setup must exit non-zero when cc is missing; got status {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("xcode-select --install"),
        "bin/setup must route the cc install hint to stderr; stderr was: {:?}\nstdout was: {:?}",
        stderr,
        stdout,
    );
    assert!(
        !stdout.contains("xcode-select --install"),
        "bin/setup must NOT print the cc install hint to stdout; stdout was: {:?}",
        stdout
    );
}
