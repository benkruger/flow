//! CLI smoke test for `flow-rs upgrade-check`.
//!
//! Exercises the real binary end-to-end with a fake `gh` shell script on
//! PATH and a tempdir plugin.json via `FLOW_PLUGIN_JSON`. Uses
//! `Command::env()` (per-subprocess, safe per rust-port-parity
//! env-var rule) to avoid parent-process env races with parallel tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;

fn flow_rs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
}

#[test]
fn cli_current_version_smoke() {
    let dir = tempfile::tempdir().unwrap();

    // Write a fake plugin.json with a known version + github repository.
    let plugin_json = dir.path().join("plugin.json");
    fs::write(
        &plugin_json,
        r#"{"version":"1.0.0","repository":"https://github.com/example/test"}"#,
    )
    .unwrap();

    // Create a fake `gh` shell script that prints the same version, making
    // this a "current version" scenario.
    let gh = dir.path().join("gh");
    fs::write(&gh, "#!/usr/bin/env bash\necho 'v1.0.0'\n").unwrap();
    let mut perms = fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).unwrap();

    // Prepend the tempdir to PATH so the fake `gh` wins over any real one.
    // Command::env() applies only to the spawned child — it does not leak
    // into the parent process, so concurrent tests are safe.
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.path().display(), orig_path);

    let output = flow_rs()
        .arg("upgrade-check")
        .env("FLOW_PLUGIN_JSON", &plugin_json)
        .env("PATH", new_path)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Parse the last non-empty stdout line as JSON.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no stdout lines: {}", stdout));
    let data: Value = serde_json::from_str(last_line.trim())
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_line));

    assert_eq!(data["status"], "current");
    assert_eq!(data["installed"], "1.0.0");
}
