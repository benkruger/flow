//! Tests for `bin/test` — the FLOW dogfood test runner.
//!
//! `bin/test` has two modes:
//!   - Default: forwards trailing args to `cargo nextest run`
//!   - `--file <path>`: compiles the file with `rustc --test` and runs it

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// bin/test must exist and be executable.
#[test]
fn script_is_executable() {
    let script = common::bin_dir().join("test");
    assert!(script.exists(), "bin/test must exist");
    let meta = fs::metadata(&script).unwrap();
    assert!(
        meta.permissions().mode() & 0o111 != 0,
        "bin/test must be executable"
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

/// `bin/test` (no args) invokes `cargo llvm-cov nextest`.
#[test]
fn invokes_cargo_llvm_cov_nextest_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("test");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

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
        logged.contains("llvm-cov"),
        "expected cargo llvm-cov wrapper, got: {}",
        logged
    );
    assert!(
        logged.contains("nextest"),
        "expected cargo llvm-cov nextest, got: {}",
        logged
    );
}

/// `bin/test` forwards trailing args to cargo nextest.
#[test]
fn forwards_trailing_args_to_nextest() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("test");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

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
        .arg("my_test_filter")
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
        logged.contains("my_test_filter"),
        "expected filter forwarded, got: {}",
        logged
    );
}

/// `bin/test --file <path>` invokes rustc --test on the file.
#[test]
fn file_mode_invokes_rustc_test() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("test");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

    // Mock rustc that "compiles" by creating a no-op binary at the -o path
    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    let log_file = dir.path().join("rustc_log");
    fs::write(
        mock_bin.join("rustc"),
        format!(
            "#!/usr/bin/env bash\necho \"$*\" > \"{}\"\n# Find the -o argument and write a runnable script there\nout=\"\"\nfor i in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$i\"; break; fi\n  prev=\"$i\"\ndone\nif [ -n \"$out\" ]; then\n  echo '#!/usr/bin/env bash' > \"$out\"\n  echo 'exit 0' >> \"$out\"\n  chmod +x \"$out\"\nfi\nexit 0\n",
            log_file.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("rustc")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("rustc"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(&target)
        .args(["--file", "tests/foo.rs"])
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
        logged.contains("--test"),
        "expected --test, got: {}",
        logged
    );
    assert!(
        logged.contains("tests/foo.rs"),
        "expected file path, got: {}",
        logged
    );
}

/// `bin/test tests/<subdir>/<name>.rs` resolves to one binary per
/// directory: `--test <subdir>` plus a `test(/^<name>::/)` module
/// filter. Without the per-directory `main.rs` layout, sibling files
/// in `tests/hooks/` and `tests/commands/` are not auto-discovered by
/// Cargo unless they are also listed as `[[test]]` stanzas in
/// `Cargo.toml`. The migration replaces the stanzas with a per-
/// directory `main.rs` declaring siblings as `mod`, so the per-file
/// runner must address them via the directory-level binary name and
/// scope test execution to the requested module.
#[test]
fn bin_test_per_file_handles_subdirectory_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("test");
    fs::write(&target, &script_content).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target, perms).unwrap();

    let test_subdir = dir.path().join("tests/hooks");
    fs::create_dir_all(&test_subdir).unwrap();
    fs::write(test_subdir.join("stop_continue.rs"), "// fixture").unwrap();

    let src_subdir = dir.path().join("src/hooks");
    fs::create_dir_all(&src_subdir).unwrap();
    fs::write(src_subdir.join("stop_continue.rs"), "// fixture").unwrap();

    let mock_bin = dir.path().join("mock_bin");
    fs::create_dir_all(&mock_bin).unwrap();
    let log_file = dir.path().join("cargo_log");
    fs::write(
        mock_bin.join("cargo"),
        format!(
            "#!/usr/bin/env bash\necho \"$*\" >> \"{}\"\nif [ \"$1\" = \"llvm-cov\" ] && [ \"${{2:-}}\" = \"report\" ]; then\n  echo 'stop_continue.rs   10   0   100.00%   5   0   100.00%   20   0   100.00%   0   0   -'\nfi\nexit 0\n",
            log_file.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(mock_bin.join("cargo")).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mock_bin.join("cargo"), perms).unwrap();

    let path = format!("{}:{}", mock_bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(&target)
        .arg("tests/hooks/stop_continue.rs")
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
        logged.contains("--test hooks"),
        "expected --test hooks (subdirectory binary name), got: {}",
        logged
    );
    assert!(
        logged.contains("test(/^stop_continue::/)"),
        "expected nextest module filter test(/^stop_continue::/), got: {}",
        logged
    );
}

/// bin/test propagates a nonzero exit code from cargo nextest.
#[test]
fn propagates_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let real_script = common::bin_dir().join("test");
    let script_content = fs::read_to_string(&real_script).unwrap();
    let target = bin_dir.join("test");
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
