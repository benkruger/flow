//! `bin/flow status` — presentation wrapper around `format_status`.
//!
//! Adds a banner header and a fenced `text` envelope around the panel
//! content so consumer skills (the four phase transition gates) can
//! invoke a single bash command and print its stdout verbatim. The
//! "no FLOW feature in progress" message is hoisted into the binary
//! and emitted on stdout at exit 0 — callers no longer need to
//! distinguish exit 1 from exit 0 to render the no-flow notice.

use std::path::Path;

use crate::format_status;
use crate::utils::read_version;

/// Render the banner-and-fence wrapper around a `format_status` panel.
///
/// Returns `Result<(stdout_text, code), (stderr_text, code)>`:
///
/// - `Ok((wrapped, 0))` — banner header, fenced `text` opener, panel
///   content (single or multi), and fenced closer concatenated as
///   stdout.
/// - `Ok((no_flow, 0))` — when `format_status::run_impl_main` returns
///   `Ok(("", 1))` (no state file for any branch), this wraps the
///   "No FLOW feature in progress" message in a fenced text block
///   and returns exit 0. The exit-1-empty-stdout contract from
///   `format-status` is hoisted into a user-visible message here.
/// - `Err((msg, 2))` — branch resolution failed; the CLI arm emits
///   `msg` to stderr and exits 2.
pub fn run_impl_main(
    branch_override: Option<&str>,
    root: &Path,
) -> Result<(String, i32), (String, i32)> {
    let version = read_version();
    // format_status::run_impl_main returns exactly three shapes per
    // its contract: Ok((panel, 0)) for rendered output, Ok(("", 1))
    // for no-state, and Err(("...", 2)) for branch-resolution failure.
    // The Ok(_) arm captures the no-state case; no defensive third
    // arm is needed.
    match format_status::run_impl_main(branch_override, root) {
        Err(err) => Err(err),
        Ok((panel, 0)) => Ok((wrap_with_banner_and_fence(&version, &panel), 0)),
        Ok(_) => Ok((no_flow_message_fenced(&version), 0)),
    }
}

/// Wrap a panel string in the `flow:status` banner header and a
/// fenced `text` code block. The banner uses U+2500 box-drawing
/// glyphs to match other FLOW phase banners.
fn wrap_with_banner_and_fence(version: &str, panel: &str) -> String {
    let mut out = String::new();
    out.push_str("──────────────────────────────────────────────────\n");
    out.push_str(&format!("  FLOW v{} — flow:status — STARTING\n", version));
    out.push_str("──────────────────────────────────────────────────\n");
    out.push('\n');
    out.push_str("```text\n");
    out.push_str(panel);
    // Panels from format_status are produced via `lines.join("\n")`
    // and never carry a trailing newline. Add one so the fence closer
    // lands on its own line.
    out.push('\n');
    out.push_str("```\n");
    out
}

/// Render the "no FLOW feature in progress" message wrapped in the
/// banner header and a fenced `text` block. Used when no state file
/// exists for any branch — formerly the consumer skill's exit-1 prose
/// path; now emitted by the binary directly.
fn no_flow_message_fenced(version: &str) -> String {
    let mut out = String::new();
    out.push_str("──────────────────────────────────────────────────\n");
    out.push_str(&format!("  FLOW v{} — flow:status — STARTING\n", version));
    out.push_str("──────────────────────────────────────────────────\n");
    out.push('\n');
    out.push_str("```text\n");
    out.push_str("No FLOW feature in progress on this branch.\n");
    out.push_str("Start one with /flow:flow-start <feature name>.\n");
    out.push_str("```\n");
    out
}
