//! Pre-filing validator for issue bodies that the role-based
//! planning skills are about to write.
//!
//! `bin/flow validate-issue-body --mode <vanilla|decomposed>
//! --body-file <path>` reads the body file from disk and runs the
//! validation branch matching the requested mode. The decomposed
//! branch runs the same sentinel-extraction logic that
//! `bin/flow plan-from-issue` will later apply at flow-start and
//! rejects bodies whose plan section is malformed, empty, missing
//! the canonical heading, or empty of tasks. The vanilla branch
//! requires the `## What` / `## Why` / `## Acceptance Criteria`
//! triad and forbids both FLOW-PLAN sentinels and an `##
//! Implementation Plan` heading.
//!
//! The validator's named consumers are the `### Validate + File`
//! step in `skills/flow-explore/SKILL.md` (vanilla mode) and the
//! `### Validate + File + Link` step in `skills/flow-plan/SKILL.md`
//! (decomposed mode). Each skill invokes this subcommand BEFORE
//! `bin/flow issue` so a misformatted body never reaches the filed
//! issue — it routes back to the skill's Revise loop with the
//! validator's `message` field instead.
//!
//! The validator deliberately re-uses `crate::plan_from_issue`'s
//! constants and helpers (`BEGIN_MARKER`, `END_MARKER`,
//! `extract_plan`, `count_tasks`, `PLAN_BODY_BYTE_CAP`) rather than
//! restating the sentinel literals or the task-counting walker. A
//! drift between this module's accept conditions and `plan-from-issue`'s
//! extract conditions would let a body pass the validator and then
//! fail extraction at flow-start — exactly the failure mode the
//! validator exists to prevent.
//!
//! Path-construction discipline per
//! `.claude/rules/external-input-path-construction.md`: the
//! `--body-file` argument is caller-supplied and may name a path
//! anywhere on the filesystem. The validator uses
//! `fs::symlink_metadata` to inspect the path without following
//! symlinks, rejects non-regular-file inputs (directories, symlinks,
//! sockets, FIFOs), and caps the read at `PLAN_BODY_BYTE_CAP` via
//! `BufReader::new(file.take(...))` so a runaway or hostile input
//! cannot exhaust memory.
//!
//! Tests live at `tests/validate_issue_body.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::plan_from_issue::{
    count_tasks, extract_plan, BEGIN_MARKER, END_MARKER, PLAN_BODY_BYTE_CAP,
};

/// CLI arguments for `bin/flow validate-issue-body`.
#[derive(clap::Parser, Debug)]
#[command(name = "validate-issue-body")]
pub struct Args {
    /// Body file to validate. Relative paths resolve against the
    /// process cwd. The validator reads at most `PLAN_BODY_BYTE_CAP`
    /// bytes from this path; longer files are rejected with
    /// `body_too_large`.
    #[arg(long)]
    pub body_file: PathBuf,
    /// Validation mode. `decomposed` (default) requires FLOW-PLAN
    /// sentinels and an `## Implementation Plan` heading with at
    /// least one `#### Task ` entry. `vanilla` requires the
    /// problem-statement triad (`## What`, `## Why`,
    /// `## Acceptance Criteria`) and forbids both sentinels and the
    /// `## Implementation Plan` heading. Other values are rejected
    /// with `{"status":"error","reason":"invalid_mode"}` so callers
    /// see a JSON envelope instead of a clap exit-2 surface.
    #[arg(long, default_value = "decomposed")]
    pub mode: String,
}

/// Main-arm dispatcher for `bin/flow validate-issue-body`.
///
/// Returns a JSON envelope and exit code. Success emits
/// `{"status":"ok","tasks_total":N}`; errors emit
/// `{"status":"error","reason":"<class>","message":"..."}`. Exit
/// code is `0` for both per
/// `.claude/rules/rust-patterns.md` "Exit code convention for
/// business errors" — callers parse the `status` field rather than
/// the shell exit code. Exit code `1` is reserved for infrastructure
/// failures that escape the JSON contract; this function does not
/// produce them.
///
/// `root` is taken as a parameter rather than read from
/// `project_root()` so integration tests can drive the function with
/// a `TempDir` fixture. The current implementation does not consume
/// `root` — `--body-file` resolves against process cwd — but the
/// parameter is preserved for parity with sibling `run_impl_main`
/// signatures and to leave room for future per-project canonicalization.
pub fn run_impl_main(args: &Args, _root: &Path) -> (Value, i32) {
    let body = match read_body(&args.body_file) {
        Ok(b) => b,
        Err(ReadError::NotRegularFile) => {
            return error_envelope(
                "body_read_failed",
                &format!(
                    "body file is not a readable regular file: {}",
                    args.body_file.display()
                ),
            );
        }
        Err(ReadError::Io(msg)) => {
            return error_envelope(
                "body_read_failed",
                &format!(
                    "failed to read body file {}: {}",
                    args.body_file.display(),
                    msg
                ),
            );
        }
        Err(ReadError::TooLarge { actual }) => {
            return error_envelope(
                "body_too_large",
                &format!(
                    "body file exceeds {}-byte cap (read at least {} bytes)",
                    PLAN_BODY_BYTE_CAP, actual
                ),
            );
        }
    };

    match normalize_mode(&args.mode).as_str() {
        "decomposed" => validate_decomposed(&body),
        "vanilla" => validate_vanilla(&body),
        _ => error_envelope(
            "invalid_mode",
            &format!(
                "unknown --mode value '{}'; expected 'vanilla' or 'decomposed' (case-insensitive, trimmed)",
                args.mode
            ),
        ),
    }
}

/// Normalize a `--mode` value before dispatch.
///
/// Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing" — every gate that decides on string input must strip
/// NULs (defeats embedded-NUL bypass from truncated writes), trim
/// whitespace (defeats shell-quoting accidents and trailing
/// newlines), and ASCII-lowercase (defeats case-variant survival).
/// The original raw value is preserved in the error envelope so the
/// caller sees what they passed; the normalized value is what
/// dispatches.
fn normalize_mode(mode: &str) -> String {
    mode.replace('\0', "").trim().to_ascii_lowercase()
}

/// Validate a decomposed-issue body — the existing FLOW-PLAN
/// sentinel + `## Implementation Plan` + `#### Task ` regime that
/// `bin/flow plan-from-issue` later extracts at flow-start.
fn validate_decomposed(body: &str) -> (Value, i32) {
    let begin_count = body.matches(BEGIN_MARKER).count();
    let end_count = body.matches(END_MARKER).count();
    if begin_count != 1 || end_count != 1 {
        return error_envelope(
            "marker_count_wrong",
            &format!(
                "body must contain exactly one BEGIN and one END FLOW-PLAN marker (got {} BEGIN, {} END)",
                begin_count, end_count
            ),
        );
    }

    let plan_content = match extract_plan(body) {
        Ok(c) => c,
        Err(e) => {
            return error_envelope(
                "plan_extraction_failed",
                &format!("extract_plan rejected the body: {}", e),
            );
        }
    };

    if !plan_starts_with_implementation_plan_heading(plan_content) {
        return error_envelope(
            "plan_missing_heading",
            "plan content between FLOW-PLAN markers must open with `## Implementation Plan`",
        );
    }

    let tasks_total = count_tasks(plan_content);
    if tasks_total == 0 {
        return error_envelope(
            "no_tasks",
            "plan content has zero `#### Task ` entries outside fenced code blocks",
        );
    }

    (
        json!({
            "status": "ok",
            "tasks_total": tasks_total,
        }),
        0,
    )
}

/// Validate a vanilla problem-statement body — the
/// `/flow:flow-explore` filing shape. The body must carry the
/// `## What` / `## Why` / `## Acceptance Criteria` triad and must
/// NOT contain FLOW-PLAN sentinels or an `## Implementation Plan`
/// heading. Forbidden constructs are checked first so a body that
/// drifts toward the decomposed shape gets a precise diagnostic
/// instead of "missing heading X" pointing at a section that was
/// deliberately omitted.
fn validate_vanilla(body: &str) -> (Value, i32) {
    if body.contains(BEGIN_MARKER) || body.contains(END_MARKER) {
        return error_envelope(
            "vanilla_has_sentinels",
            "vanilla body must not contain FLOW-PLAN sentinel markers",
        );
    }
    if has_h2_heading(body, "Implementation Plan") {
        return error_envelope(
            "vanilla_has_implementation_plan",
            "vanilla body must not contain an `## Implementation Plan` heading",
        );
    }
    if !has_h2_heading(body, "What") {
        return error_envelope(
            "vanilla_missing_section_what",
            "vanilla body must contain a `## What` heading",
        );
    }
    if !has_h2_heading(body, "Why") {
        return error_envelope(
            "vanilla_missing_section_why",
            "vanilla body must contain a `## Why` heading",
        );
    }
    if !has_h2_heading(body, "Acceptance Criteria") {
        return error_envelope(
            "vanilla_missing_section_acceptance",
            "vanilla body must contain a `## Acceptance Criteria` heading",
        );
    }
    (json!({"status": "ok"}), 0)
}

/// Returns true when `body` contains a line whose trimmed content
/// matches `## <title>` exactly. Trailing whitespace is tolerated;
/// leading whitespace is not (Markdown headings must start at column
/// zero to render as headings).
fn has_h2_heading(body: &str, title: &str) -> bool {
    let needle = format!("## {}", title);
    body.lines().any(|line| line.trim_end() == needle)
}

/// Reasons `read_body` rejects the body-file input.
enum ReadError {
    /// `symlink_metadata` succeeded but the entry is not a regular
    /// file — covers directories, symlinks (pre-resolution), and
    /// special files (FIFOs, sockets, devices). Symlinks are
    /// rejected without following so a hostile or accidental
    /// symlink cannot redirect the read to an arbitrary path.
    NotRegularFile,
    /// `symlink_metadata` returned `Err`, `File::open` returned
    /// `Err`, or `read_to_string` failed mid-read with a non-cap
    /// error. The wrapped string carries the OS error message.
    Io(String),
    /// Read produced more than `PLAN_BODY_BYTE_CAP` bytes (one more
    /// byte than the cap was observed, so the body exceeds the
    /// cap). `actual` reports the observed read length so the
    /// caller sees how close to the cap the input was.
    TooLarge { actual: usize },
}

/// Read the body file, capped at `PLAN_BODY_BYTE_CAP + 1` bytes.
///
/// Rejects non-regular-file paths via `symlink_metadata` so a
/// dangling or hostile symlink at the path cannot redirect the
/// read. Uses `BufReader::new(file.take(...))` with a cap of
/// `PLAN_BODY_BYTE_CAP + 1` so the function can distinguish "body
/// at the cap" (accept) from "body exceeds the cap" (reject) — when
/// the read produces `PLAN_BODY_BYTE_CAP + 1` bytes the input is
/// known to be too large.
fn read_body(path: &Path) -> Result<String, ReadError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return Err(ReadError::Io(e.to_string())),
    };
    let ft = meta.file_type();
    if !ft.is_file() {
        return Err(ReadError::NotRegularFile);
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return Err(ReadError::Io(e.to_string())),
    };
    // Read one byte beyond the cap so we can distinguish "exactly
    // at the cap" (acceptable) from "exceeds the cap" (rejected).
    let read_cap = (PLAN_BODY_BYTE_CAP as u64) + 1;
    let mut reader = BufReader::new(file.take(read_cap));
    let mut buf = String::new();
    if let Err(e) = reader.read_to_string(&mut buf) {
        return Err(ReadError::Io(e.to_string()));
    }
    if buf.len() > PLAN_BODY_BYTE_CAP {
        return Err(ReadError::TooLarge { actual: buf.len() });
    }
    Ok(buf)
}

/// Returns true when the plan content (already extracted from
/// between FLOW-PLAN markers) opens with the canonical
/// `## Implementation Plan` heading. Leading whitespace and blank
/// lines are tolerated — the first non-blank line must be the
/// heading.
///
/// `extract_plan` already rejects bodies whose marker-delimited
/// content is blank-only via `ExtractError::Empty`, so by the
/// time this helper runs the plan slice always contains at least
/// one non-blank line. `trim_start` followed by `lines().next()`
/// captures that first non-blank line directly — no loop, no
/// unreachable fall-through.
fn plan_starts_with_implementation_plan_heading(plan: &str) -> bool {
    let first_non_blank_line = plan.trim_start().lines().next().unwrap_or("").trim_end();
    first_non_blank_line == "## Implementation Plan"
}

fn error_envelope(reason: &str, message: &str) -> (Value, i32) {
    (
        json!({
            "status": "error",
            "reason": reason,
            "message": message,
        }),
        0,
    )
}
