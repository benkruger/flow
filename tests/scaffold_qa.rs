//! Integration tests for `src/scaffold_qa.rs`.
//!
//! Covers the CLI wrapper surface that inline unit tests cannot reach:
//! `run()` process-exit paths and `find_templates`' default
//! templates_dir branch that resolves from `current_exe()`.
//!
//! Inline tests in `src/scaffold_qa.rs` already cover `scaffold_impl`'s
//! success and error branches with injected runners and explicit
//! template paths. This file covers the gaps those tests cannot
//! exercise in-process.

use std::process::Command;

use flow_rs::scaffold_qa;

/// Subprocess invocation of `bin/flow scaffold-qa` with an unknown
/// template name. Exercises `run()`'s `Ok(result)` arm when the result
/// carries `status == "error"` — the path that prints the JSON and
/// calls `process::exit(1)`. Transitively covers `run_impl`'s call
/// to `scaffold_impl` with `None` templates_dir, which in turn
/// exercises `find_templates`' default path that resolves the
/// templates directory from `current_exe()` three levels up. With the
/// unknown template, `find_templates` returns an `Err("Unknown
/// template: ...")` so the subprocess prints the error JSON and exits
/// non-zero.
#[test]
fn scaffold_qa_cli_unknown_template_exits_nonzero_with_error_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args([
            "scaffold-qa",
            "--template",
            "definitely_not_a_real_template_name",
            "--repo",
            "owner/nonexistent",
        ])
        .env_remove("FLOW_CI_RUNNING")
        .output()
        .expect("failed to spawn flow-rs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 on unknown template, got {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"status\":\"error\""),
        "expected error status in stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Unknown template"),
        "expected 'Unknown template' in stdout, got: {}",
        stdout
    );
}

/// Directly drives `run_impl` with an unknown template. This exercises
/// the `Ok(scaffold_impl(...))` wrapper inside `run_impl`, the
/// `find_templates(None)` default-path branch (via cargo test's exe
/// location), and the early-return where `scaffold_impl` emits the
/// find_templates error as JSON. The integration-test compilation
/// links against `flow_rs` as a library, so this test hits the
/// library-level surface independent of the `run()` wrapper.
#[test]
fn scaffold_qa_run_impl_unknown_template_returns_error_status() {
    let args = scaffold_qa::Args {
        template: "nonexistent_scaffold_qa_template_for_test".to_string(),
        repo: "owner/repo".to_string(),
    };

    let result = scaffold_qa::run_impl(&args).expect("run_impl returns Ok wrapping scaffold_impl");
    assert_eq!(
        result["status"], "error",
        "expected status=error on unknown template"
    );
    let message = result["message"]
        .as_str()
        .expect("error response carries message");
    assert!(
        message.contains("Unknown template"),
        "expected 'Unknown template' in message, got: {}",
        message
    );
}

/// Drives `find_templates(None)` default-path branch directly to
/// confirm the exe-based resolution returns `Err("Unknown template")`
/// rather than panicking when the template is absent. This complements
/// the inline `test_find_templates_default_dir` which takes the
/// explicit-path branch — this test takes the `None` branch and
/// asserts the error path returns a structured message.
#[test]
fn scaffold_qa_find_templates_none_dir_unknown_returns_error() {
    let result = scaffold_qa::find_templates("template_never_present_under_qa_templates", None);
    assert!(result.is_err(), "expected Err for unknown template");
    let err = result.unwrap_err();
    assert!(
        err.contains("Unknown template"),
        "expected 'Unknown template' in error, got: {}",
        err
    );
}
