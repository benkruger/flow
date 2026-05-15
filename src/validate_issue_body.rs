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
//! Code-reference soft-warnings (vanilla mode only). After the
//! structural triad checks succeed, the vanilla branch walks the
//! body line-by-line and emits a `warnings` array of
//! `{pattern, line, snippet}` entries for `src/` paths, `tests/`
//! paths, and `identifier::identifier` symbol references. Every
//! occurrence on a line is emitted (not just the first per
//! pattern), the matched `pattern` is stripped of trailing prose
//! punctuation, `snippet` preserves leading indentation, and
//! word boundaries are ASCII-only so a non-ASCII letter directly
//! adjacent to a path does not suppress the match. The field is
//! always present on vanilla success envelopes (empty array when
//! no matches). Decomposed success envelopes never carry the
//! field. Per `.claude/rules/gate-consumer-enumeration.md`, the
//! consumer of the new field is `skills/flow-explore/SKILL.md`
//! Step 5, which renders entries inline when non-empty, revises
//! the body once in working memory, and re-validates before
//! filing.
//!
//! Tests live at `tests/validate_issue_body.rs` per
//! `.claude/rules/test-placement.md`.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use regex::Regex;
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
    let warnings = scan_for_code_references(body);
    (json!({"status": "ok", "warnings": warnings}), 0)
}

/// Walk the body line-by-line and return code-reference warnings.
///
/// Three regex patterns flag the most common shapes a participant
/// reaches for when grounding a problem statement in observed code
/// behavior:
///
/// - `(?-u:\b)src/\S+` — `src/foo.rs`-style production-path
///   references
/// - `(?-u:\b)tests/\S+` — `tests/bar.rs`-style test-path
///   references
/// - `(?-u:\b)[A-Za-z_]\w*::[A-Za-z_]\w*(?-u:\b)` —
///   `module::function` symbol references
///
/// Word boundaries are ASCII-only (`(?-u:\b)`) so a non-ASCII
/// letter directly adjacent to the path does not suppress the
/// match — a Unicode-aware `\b` would treat α and s as both
/// alphabetic, masking `αsrc/foo.rs` from the scan.
///
/// Emission order: line-ascending; within a single line,
/// matches appear in pattern-declaration order (`src/` paths
/// first, `tests/` paths second, `module::function` third); within
/// a single pattern on a single line, every occurrence is emitted
/// via `find_iter` so a line like `"see src/foo.rs and src/bar.rs"`
/// produces two `src/` warnings instead of one.
///
/// The matched substring is trimmed of trailing prose punctuation
/// (`.,;:)"'\`]`) before being recorded in the `pattern` field so a
/// sentence-terminal period (`"in src/foo.rs."`) is not absorbed
/// into the reported reference. The `snippet` field trims
/// trailing-only whitespace via `trim_end`, preserving leading
/// indentation so a code-block reference is distinguishable from a
/// flush-left prose reference.
///
/// The scan is intentionally narrow. Config paths (`.flow.json`,
/// `CLAUDE.md`) and rule-file references do not trigger; backtick
/// quoting around an otherwise-matching token does NOT exempt it
/// (the acceptance criteria forbid code references in any
/// presentation). English prose `::` between identifier-shaped
/// tokens is a low-rate false positive the soft-warning posture
/// absorbs.
fn scan_for_code_references(body: &str) -> Vec<Value> {
    let patterns: [Regex; 3] = [
        Regex::new(r"(?-u:\b)src/\S+").expect("static src/ pattern compiles"),
        Regex::new(r"(?-u:\b)tests/\S+").expect("static tests/ pattern compiles"),
        Regex::new(r"(?-u:\b)[A-Za-z_]\w*::[A-Za-z_]\w*(?-u:\b)")
            .expect("static double-colon pattern compiles"),
    ];
    let mut warnings: Vec<Value> = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let line_no = idx + 1;
        for pattern in &patterns {
            for m in pattern.find_iter(line) {
                warnings.push(json!({
                    "pattern": trim_trailing_punctuation(m.as_str()),
                    "line": line_no,
                    "snippet": line.trim_end(),
                }));
            }
        }
    }
    warnings
}

/// Strip prose-terminal punctuation from a matched reference so a
/// sentence-ending period or list-separating comma does not bleed
/// into the reported pattern.
///
/// Trims any trailing `.,;:)"'\`]` characters. The closing bracket
/// `]` is included so a path inside `[src/foo.rs]` (Markdown link
/// or array notation) reports the clean path. The set is closed —
/// no greedy stripping of `?` or `!` because those characters
/// rarely follow paths in prose and stripping them would obscure
/// genuine identifier shapes like `traits::Iter!`.
fn trim_trailing_punctuation(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'', '`', ']'])
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
