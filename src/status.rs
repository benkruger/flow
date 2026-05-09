//! `bin/flow status` — presentation wrapper around `format_status`.
//!
//! Wraps the panel content in a fenced `text` envelope so consumer
//! skills (the four phase transition gates) can invoke a single bash
//! command and print its stdout verbatim. Three cases — rendered
//! panel, no-state, and branch-resolution failure — all emit fenced
//! stdout at exit 0 so a bash block that prints stdout verbatim
//! always produces a useful message regardless of the underlying
//! state. The format_status panel content carries its own header
//! ("Current Status", "All Phases Complete!", "Multiple Features
//! Active") so this wrapper does not add an outer banner that could
//! contradict the inner panel.

use std::path::Path;

use crate::flow_paths::FlowPaths;
use crate::format_status;

/// Render the fenced wrapper around a `format_status` panel.
///
/// Returns `Result<(stdout_text, code), (stderr_text, code)>`:
///
/// - `Ok((wrapped, 0))` — fenced `text` envelope around the panel
///   content (single, multi, or all-complete).
/// - `Ok((no_flow, 0))` — fenced no-flow message, used when
///   `format_status::run_impl_main` returns `Ok(("", 1))` (no state
///   file) OR when the panel content is empty (a corrupted state
///   file with non-object `phases` produces `Ok(("", 0))` from
///   `format_status::format_panel`).
/// - `Ok((error_message, 0))` — fenced error message used when
///   `format_status::run_impl_main` returns `Err((msg, 2))`. Phase
///   skills print stdout verbatim, so emitting the error on stdout
///   at exit 0 surfaces a useful notice instead of an empty fence.
/// - `Err((msg, 1))` — the `--branch` override failed
///   `FlowPaths::is_valid_branch`. The CLI arm emits `msg` to
///   stderr and exits 1 so a programmatic caller can detect the
///   misuse.
pub fn run_impl_main(
    branch_override: Option<&str>,
    root: &Path,
) -> Result<(String, i32), (String, i32)> {
    if let Some(b) = branch_override {
        if !FlowPaths::is_valid_branch(b) {
            return Err((
                format!(
                    "Invalid --branch value: {:?}. Branch names must be \
                     non-empty and free of slashes and NUL bytes.",
                    b
                ),
                1,
            ));
        }
    }
    match format_status::run_impl_main(branch_override, root) {
        Ok((panel, 0)) if !panel.is_empty() => Ok((render_fenced(&panel), 0)),
        // No-state OR empty-panel (corrupted phases) — both indicate
        // there is nothing useful to display, so present the no-flow
        // message rather than an empty fence.
        Ok(_) => Ok((render_fenced(NO_FLOW_MESSAGE), 0)),
        Err((msg, _)) => Ok((render_fenced(&format!("Status unavailable: {}", msg)), 0)),
    }
}

/// Wrap `body` in a fenced `text` code block. Every caller passes
/// content without a trailing newline — `format_status` panels are
/// produced via `lines.join("\n")`, the no-flow message constant
/// has no trailing newline, and the error `format!` produces none —
/// so the closer always lands on its own line via the unconditional
/// `\n` push.
fn render_fenced(body: &str) -> String {
    let mut out = String::new();
    out.push_str("```text\n");
    out.push_str(body);
    out.push('\n');
    out.push_str("```\n");
    out
}

/// User-facing notice for the no-state and empty-panel cases.
const NO_FLOW_MESSAGE: &str =
    "No FLOW feature in progress on this branch.\nStart one with /flow:flow-start <feature name>.";
