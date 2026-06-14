//! Consolidated tombstone tests.
//!
//! Tombstone tests assert that intentionally removed features, files,
//! and code patterns do not return. If a merge conflict resolution
//! re-introduces deleted content, the corresponding test fails.
//!
//! Standalone tombstones (file-existence, source-content) live here.
//! Topical tombstones that are integral to a test domain (e.g.
//! skill_contracts, structural) stay in their respective test files.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Substring patterns whose presence in a `.rs` source line indicates a
/// backward-facing comment per `.claude/rules/comment-quality.md`. Each
/// entry is checked case-sensitively against every line in `src/**/*.rs`
/// and `tests/**/*.rs` (except `tests/tombstones.rs` itself, which must
/// contain these strings as search input).
///
/// Lines protected by the tombstone exception (lines that match
/// `Tombstone:.*?PR #`) are skipped before this list is consulted, so
/// tombstone fixtures, tombstone assertion messages, and the
/// `tombstone-audit` source remain valid even when they reference the
/// `removed in PR` substring as fixture or documentation content.
///
/// The list is curated rather than regex-based: it captures every
/// phrasing the rule explicitly prohibits, plus the phrasings observed
/// in this repo at the time the rule was enforced. New phrasings
/// introduced by future commits will not be caught automatically — the
/// rule itself is the primary instrument, and this scanner is the
/// merge-conflict trip-wire that locks in the cleanup.
const PROHIBITED: &[&str] = &[
    // Parity references to a deleted Python codebase.
    "Python parity",
    "Python-parity",
    "TypeError parity",
    "matches Python",
    "match Python",
    "matching Python",
    "matching the Python",
    "the Python original",
    "Python original",
    "the Python script",
    "Python script",
    "the Python implementation",
    "Python implementation",
    "the Python source",
    "Python source",
    "Python's",
    "Python-era",
    "Python integration tests",
    "Python test suite",
    "Python `",
    "Python:",
    "Python Path",
    "Python timeout",
    "Python behavior",
    "Python truthy",
    "Python falsy",
    "Python semantics",
    "Python writes",
    "Python ignores",
    "Python matches",
    "Python takes",
    "Python used",
    "Python prints",
    "Python swallows",
    "Python fallback",
    "Python key ordering",
    "Python output",
    "Python-only",
    "older Python",
    "Older Python",
    // Origin / port references.
    "ported to Rust",
    "was ported",
    "Ports Python",
    "Port Python",
    "Port of ",
    "Rust port",
    "mirror Python",
    "based on the old",
    // Historical PR / before-the-fix narratives.
    "Adversarial regression (PR",
    "Before the fix",
    "Before this fix",
    "Rust since PR",
    "Fixed in PR #",
    "Removed in PR #",
    "removed in PR ",
];

/// Walk a directory recursively, appending every `.rs` file path to `out`.
/// Skips `target/` build artifact directories.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == "target" {
                    continue;
                }
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// Source-content scanner enforcing `.claude/rules/comment-quality.md`.
///
/// Walks every `*.rs` file under `src/` and `tests/` and asserts that no
/// line contains a backward-facing parity reference, historical-PR
/// provenance, or "Before the fix" narrative. Lines that match the
/// tombstone exception (`Tombstone:.*?PR #`) are skipped — they are
/// intentional per the rule. The exception regex matches any line where
/// `Tombstone:` is followed (lazily) by `PR #`, regardless of whether
/// the next characters are literal digits, a `{}` format placeholder,
/// or the regex literal `(\d+)` itself. This keeps tombstone fixture
/// generators in `tests/tombstone_audit.rs` and the parsing source in
/// `src/tombstone_audit.rs` valid without requiring per-file
/// exclusions.
///
/// The scanner self-excludes `tests/tombstones.rs` (this file) by
/// canonicalized-path comparison, because the prohibited pattern strings
/// must appear here as search input.
///
/// On any violation, the test panics with a single message listing every
/// `path:line — phrase` triple discovered in one scan, so a developer
/// gets the full inventory in one CI run instead of fixing one violation
/// at a time.
#[test]
fn test_rust_source_no_backward_facing_comments() {
    let root = common::repo_root();
    let scanner_path = root
        .join("tests")
        .join("tombstones.rs")
        .canonicalize()
        .expect("scanner path must canonicalize");

    let tombstone_re = Regex::new(r"Tombstone:.*?PR #").unwrap();

    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    collect_rs_files(&root.join("tests"), &mut files);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        // Self-exclude the scanner file (it must contain the search patterns).
        if file
            .canonicalize()
            .map(|p| p == scanner_path)
            .unwrap_or(false)
        {
            continue;
        }

        let content = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let rel = file.strip_prefix(&root).unwrap_or(file);

        for (idx, line) in content.lines().enumerate() {
            // Tombstone exception: skip lines that intentionally reference a PR.
            if tombstone_re.is_match(line) {
                continue;
            }
            for phrase in PROHIBITED {
                if line.contains(phrase) {
                    violations.push(format!("{}:{} — {}", rel.display(), idx + 1, phrase));
                }
            }
            // Paired check: "Mirrors the" + "Python" on the same line.
            // The single-pattern list cannot capture this safely because
            // "Mirrors the" appears in legitimate same-codebase parity
            // references (e.g. mirroring a guard in a sibling function).
            if line.contains("Mirrors the") && line.contains("Python") {
                violations.push(format!(
                    "{}:{} — Mirrors the .. Python",
                    rel.display(),
                    idx + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Backward-facing comments found (see .claude/rules/comment-quality.md):\n\n{}",
        violations.join("\n")
    );
}

// --- Coverage waiver loophole closure ---
//
// Coverage waivers are forbidden. The `test_coverage.md` file, the
// Waiver Discipline section in `.claude/rules/docs-with-behavior.md`,
// and any reference to `test_coverage.md` from `CLAUDE.md` are the
// three surfaces that, taken together, authorized future sessions to
// classify inconvenient code as "uncoverable" and ship a justification
// instead of a refactor. All three are removed; these tombstones fail
// CI if a merge resolution or a future edit re-introduces any of them.

#[test]
fn test_root_no_test_coverage_md_file() {
    let root = common::repo_root();
    let path = root.join("test_coverage.md");
    assert!(
        !path.exists(),
        "test_coverage.md must not exist — coverage waivers are forbidden. \
         Refactor the uncovered code instead (extract `process::exit` into \
         a return-code wrapper, inject subprocess callers as `&dyn Fn` \
         seams, split helpers until each branch is independently testable)."
    );
}

/// Tombstone: removed in PR #1549. Stability argument: the
/// `"in_progress"` literal was a JSON object key inside
/// `serde_json::json!({...})` in `analyze_issues` — a stable
/// source-string with no `concat!`/`format!`/constant
/// reassembly path that would produce the same runtime JSON
/// key without the literal appearing in source. Bypasses
/// considered and rejected: macro-concatenation (would
/// produce a different runtime expression shape that
/// `serde_json::json!` cannot accept as a key); split-string
/// reassembly (json! requires identifier-shaped or
/// string-literal keys, not runtime String concatenation);
/// hex escapes (`"\x69n_progress"` still appears in
/// source). The byte-substring check uses
/// `concat!("in_pro", "gress")` so this test file does not
/// self-trip when reading itself.
#[test]
fn test_analyze_issues_no_in_progress_json_key() {
    let root = common::repo_root();
    let path = root.join("src").join("analyze_issues.rs");
    let content = fs::read_to_string(&path).expect("src/analyze_issues.rs must exist");
    let forbidden = concat!("\"in_pro", "gress\"");
    assert!(
        !content.contains(forbidden),
        "src/analyze_issues.rs must not emit the {} JSON key — \
         the schema collapse in PR #1549 routes Flow-In-Progress rows \
         into the single `issues` array with per-row `flow_in_progress`.",
        forbidden,
    );
}

#[test]
fn test_docs_with_behavior_no_waiver_discipline_section() {
    let root = common::repo_root();
    let path = root.join(".claude/rules/docs-with-behavior.md");
    let content = fs::read_to_string(&path).expect("docs-with-behavior.md must exist");
    assert!(
        !content.contains("Waiver Discipline"),
        ".claude/rules/docs-with-behavior.md must not contain a 'Waiver Discipline' \
         section — coverage waivers are forbidden. Refactor the code instead."
    );
    assert!(
        !content.contains("test_coverage.md"),
        ".claude/rules/docs-with-behavior.md must not reference test_coverage.md — \
         the file is gone and waivers are forbidden."
    );
}

#[test]
fn test_claude_md_no_test_coverage_references() {
    let root = common::repo_root();
    let path = root.join("CLAUDE.md");
    let content = fs::read_to_string(&path).expect("CLAUDE.md must exist");
    assert!(
        !content.contains("test_coverage.md"),
        "CLAUDE.md must not reference test_coverage.md — coverage waivers are forbidden."
    );
    assert!(
        !content.contains("architecturally-unreachable code"),
        "CLAUDE.md must not contain the 'architecturally-unreachable code' waiver \
         bullet — coverage waivers are forbidden."
    );
}

// --- Module split: window_snapshot.rs ---
//
// `src/window_snapshot.rs` mixed three concerns (token capture,
// rate-limit reads, cost-file reads) behind one entry point.
// Splitting into `session_metrics.rs`, `session_cost.rs`, and
// `per_flow_capture.rs` makes the cost/metrics decoupling
// structural. The file-existence tombstone prevents merge
// conflicts from resurrecting the old module under its original
// path; the directory walk in `test_rust_source_no_backward_facing_comments`
// would otherwise still pass even if the file came back.

/// Tombstone: removed in PR #1456. `src/window_snapshot.rs` was
/// split into `session_metrics.rs`, `session_cost.rs`, and
/// `per_flow_capture.rs`. Must not return.
#[test]
fn test_src_no_window_snapshot_file() {
    let root = common::repo_root();
    let path = root.join("src").join("window_snapshot.rs");
    assert!(
        !path.exists(),
        "src/window_snapshot.rs must not exist — the module was \
         split into src/session_metrics.rs (tokens + rate limits), \
         src/session_cost.rs (cost reads), and \
         src/per_flow_capture.rs (per-flow orchestrator)."
    );
}

/// Tombstone: removed in PR #1730. The `session_cost_usd` field was
/// removed from `WindowSnapshot` in `src/state.rs` when per-phase cost
/// and month-to-date became token-derived (priced from `by_model` via
/// `src/pricing.rs`), orphaning the statusline cost-file read the field
/// held. Must not return.
///
/// Literal byte-substring scan of `src/state.rs`. Stability checklist,
/// all four answers "no": a `#[derive(Serialize, Deserialize)]` struct
/// field is a Rust identifier the compiler requires verbatim — it
/// cannot be assembled by `concat!` or `format!`, cannot be a named
/// `constant` substituted in later, and is not a split method chain.
/// The `#[serde(rename = "...")]` / `#[serde(alias = "...")]` bypass
/// still re-introduces the literal `session_cost_usd` (as the rename
/// or alias string), so the substring scan catches every resurrection
/// shape. File-resurrection pair: none — `src/state.rs` is not deleted.
#[test]
fn test_state_no_session_cost_usd_field() {
    let root = common::repo_root();
    let content =
        fs::read_to_string(root.join("src").join("state.rs")).expect("src/state.rs must exist");
    assert!(
        !content.contains("session_cost_usd"),
        "src/state.rs must not declare session_cost_usd — per-phase cost \
         and month-to-date are token-derived (priced from by_model via \
         src/pricing.rs); the snapshot cost field was removed."
    );
}

/// Tombstone: removed in PR #1777. The `dag` field of `StateFiles`
/// (`src/state.rs`) is deleted — the DAG now lives inside the plan in
/// the GitHub issue body, so no subcommand writes a `dag.md` artifact
/// or a `files.dag` pointer. Must not return.
///
/// Scoped structurally to the `StateFiles` struct body because the bare
/// substring `dag` is high-collision across the file. The struct-field
/// shape is byte-stable within that scope:
///   1. `concat!` reassembly: a struct field name is a literal
///      identifier (`dag:`), not a runtime-assembled string — re-adding
///      the field requires the literal `dag` token inside the struct.
///   2. `format!` reassembly: struct declarations are not produced by
///      `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot supply a struct
///      field name; the field declaration itself trips the scoped check.
#[test]
fn test_state_no_files_dag_field() {
    let root = common::repo_root();
    let content =
        fs::read_to_string(root.join("src").join("state.rs")).expect("src/state.rs must exist");
    let tail = content
        .split_once("pub struct StateFiles {")
        .map(|(_, t)| t)
        .expect("StateFiles struct must exist");
    let body = tail
        .split_once('}')
        .map(|(b, _)| b)
        .expect("StateFiles struct must close");
    let forbidden = "dag";
    assert!(
        !body.contains(forbidden),
        "StateFiles in src/state.rs must not declare a `dag` field — the \
         DAG-on-disk lane was retired; the DAG lives in the issue-body plan."
    );
}

/// Tombstone: removed in PR #1777. The `FlowPaths::dag_file()` method
/// (`src/flow_paths.rs`) is deleted alongside the `files.dag` state
/// field — no caller computes a `dag.md` path. Must not return.
///
/// Byte-substring on `fn dag_file` scoped to `src/flow_paths.rs`. The
/// shape is byte-stable:
///   1. `concat!` reassembly: a Rust method name cannot be assembled by
///      `concat!` — re-adding the method requires the literal
///      `fn dag_file` in source.
///   2. `format!` reassembly: method declarations are not produced by
///      `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot declare a method;
///      the `fn dag_file` declaration itself trips the check.
#[test]
fn test_flow_paths_no_dag_file_method() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("flow_paths.rs"))
        .expect("src/flow_paths.rs must exist");
    assert!(
        !content.contains("fn dag_file"),
        "src/flow_paths.rs must not declare `fn dag_file` — the dag.md \
         path helper was removed with the DAG-on-disk lane."
    );
}

/// Tombstone: removed in PR #1777. The `ManagedArtifact::DagMd` variant
/// and its `classify_path`/`canonical_path` arms (`src/write_rule.rs`)
/// are deleted — `dag.md` is no longer a write-rule-managed artifact.
/// Must not return.
///
/// Byte-substring on `DagMd` scoped to `src/write_rule.rs`. The
/// PascalCase enum-variant identifier is byte-stable:
///   1. `concat!` reassembly: a variant name cannot be assembled by
///      `concat!` — re-adding it requires the literal `DagMd` token.
///   2. `format!` reassembly: enum declarations and match arms are not
///      produced by `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot supply an enum
///      variant; the variant declaration itself trips the check.
#[test]
fn test_write_rule_no_dag_md_variant() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("write_rule.rs"))
        .expect("src/write_rule.rs must exist");
    assert!(
        !content.contains("DagMd"),
        "src/write_rule.rs must not contain `DagMd` — dag.md is no longer \
         a write-rule-managed artifact."
    );
}

/// Tombstone: removed in PR #1894. The `FlowPaths::commit_msg()` and
/// `FlowPaths::commit_msg_content()` methods (`src/flow_paths.rs`) are
/// deleted — the commit-message file is now `<commit_cwd>/.flow-commit-msg`,
/// derived inside `finalize-commit`, with no `.flow-states/<branch>/`
/// path helper. Must not return.
///
/// Byte-substring on `fn commit_msg` scoped to `src/flow_paths.rs`. The
/// shape is byte-stable:
///   1. `concat!` reassembly: a Rust method name cannot be assembled by
///      `concat!` — re-adding the method requires the literal
///      `fn commit_msg` in source.
///   2. `format!` reassembly: method declarations are not produced by
///      `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot declare a method;
///      the `fn commit_msg` declaration itself trips the check.
#[test]
fn test_flow_paths_no_commit_msg_method() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("flow_paths.rs"))
        .expect("src/flow_paths.rs must exist");
    assert!(
        !content.contains("fn commit_msg"),
        "src/flow_paths.rs must not declare `fn commit_msg` (covers \
         commit_msg and commit_msg_content) — the commit-message file is \
         now <commit_cwd>/.flow-commit-msg, derived in finalize-commit."
    );
}

/// Tombstone: removed in PR #1894. The `ManagedArtifact::CommitMsgTxt`
/// variant and its `classify_path`/`canonical_path` arms
/// (`src/write_rule.rs`) are deleted — `commit-msg.txt` is no longer a
/// write-rule-managed artifact because finalize-commit derives its own
/// `.flow-commit-msg` path. Must not return.
///
/// Byte-substring on `CommitMsgTxt` scoped to `src/write_rule.rs`. The
/// PascalCase enum-variant identifier is byte-stable:
///   1. `concat!` reassembly: a variant name cannot be assembled by
///      `concat!` — re-adding it requires the literal `CommitMsgTxt` token.
///   2. `format!` reassembly: enum declarations and match arms are not
///      produced by `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot supply an enum
///      variant; the variant declaration itself trips the check.
#[test]
fn test_write_rule_no_commit_msg_txt_variant() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("write_rule.rs"))
        .expect("src/write_rule.rs must exist");
    assert!(
        !content.contains("CommitMsgTxt"),
        "src/write_rule.rs must not contain `CommitMsgTxt` — commit-msg.txt \
         is no longer a write-rule-managed artifact."
    );
}

/// Tombstone: removed in PR #1777. The legacy `plan_file` top-level
/// state field (`src/state.rs`) is deleted — `files.plan` is the sole
/// plan-path pointer. Must not return.
///
/// Byte-substring on `plan_file` scoped to `src/state.rs`. The
/// surviving `FlowPaths::plan_file()` method lives in
/// `src/flow_paths.rs`, not `state.rs`, so a file-scoped check on
/// `state.rs` is unambiguous and byte-stable:
///   1. `concat!` reassembly: a struct field name is a literal
///      identifier — re-adding `pub plan_file` requires the literal
///      `plan_file` token in `state.rs`.
///   2. `format!` reassembly: struct declarations are not produced by
///      `format!` interpolation.
///   3. Named `constant` reference: a `const` cannot supply a struct
///      field name; the field declaration itself trips the check.
#[test]
fn test_state_no_plan_file_field() {
    let root = common::repo_root();
    let content =
        fs::read_to_string(root.join("src").join("state.rs")).expect("src/state.rs must exist");
    let forbidden = "plan_file";
    assert!(
        !content.contains(forbidden),
        "src/state.rs must not declare a `plan_file` field — files.plan \
         is the sole plan-path pointer."
    );
}

/// Tombstone: removed in PR #1777. The legacy `state.get("plan_file")`
/// fallback reads (phase_enter, render_pr_body, tui_data,
/// plan_deviation) are deleted — every consumer reads `files.plan`
/// directly. Must not return.
///
/// Byte-substring on the literal call shape `get("plan_file")` scoped
/// to `src/`. The surviving `response["plan_file"]` output key (an
/// `IndexMut` assignment, not a `.get`) and the `plan_file()` method
/// call do not match this shape. The shape is byte-stable:
///   1. `concat!` reassembly: re-adding the fallback requires the
///      literal `"plan_file"` argument to `get(...)`.
///   2. `format!` reassembly: a `get(...)` call argument is a literal
///      string, not a `format!` product.
///   3. Named `constant` reference: a `const KEY = "plan_file"` aliased
///      into `get(KEY)` would not match, but the legacy fallback shape
///      this guards always passed the literal — the documented v1
///      contract is the literal call shape.
#[test]
fn test_src_no_plan_file_legacy_fallback() {
    let root = common::repo_root();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    let forbidden = "get(\"plan_file\")";
    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if content.contains(forbidden) {
            let rel = file.strip_prefix(&root).unwrap_or(file);
            violations.push(rel.display().to_string());
        }
    }
    assert!(
        violations.is_empty(),
        "src must not read the legacy `get(\"plan_file\")` fallback — \
         every consumer reads files.plan directly:\n{}",
        violations.join("\n")
    );
}

/// Tombstone: removed in PR #1567. The `bump_install_path` function and
/// its two `run_impl` callsites are deleted from `src/bump_version.rs` —
/// the `flow-marketplace/flow/<version>/bin/setup` install-path
/// references it rewrote at version-bump time were dropped from
/// README.md and docs/index.html when the marketplace install stopped
/// requiring a build step. Must not return.
///
/// Asserts `src/bump_version.rs` does NOT contain the identifier
/// `bump_install_path`. The byte-substring shape holds because:
///   1. `concat!` reassembly: a Rust function name cannot be assembled
///      by `concat!` — re-introducing the function requires the literal
///      `fn bump_install_path` in source.
///   2. `format!` reassembly: function declarations are not produced by
///      `format!` interpolation.
///   3. Named constant reference: a `const` aliasing the string would
///      still place the literal `bump_install_path` in source, and the
///      function declaration itself trips the byte check regardless.
///   4. Method chains / split args: not applicable — the target is a
///      function identifier, not a CLI argument passed via `.arg()`.
#[test]
fn test_src_no_bump_install_path() {
    let path = common::repo_root().join("src/bump_version.rs");
    let content = fs::read_to_string(&path).expect("src/bump_version.rs must exist");
    assert!(
        !content.contains("bump_install_path"),
        "src/bump_version.rs must not contain `bump_install_path` — the \
         function and its run_impl callsites were deleted when the \
         marketplace install-path doc references were removed."
    );
}

/// Tombstone: removed in PR #1689. `phase_config::auto_skills` and the
/// `init_state::run` branch that substituted it for the `.flow.json`
/// skills block are deleted — the state file's skills section is
/// always seeded from `.flow.json` so a `flow-start --auto` flag can
/// no longer wholesale-override a configured `manual` mode. Must not
/// return.
///
/// Asserts neither `src/phase_config.rs` nor `src/commands/init_state.rs`
/// contains the identifier `auto_skills`. The byte-substring shape
/// holds because:
///   1. `concat!` reassembly: a Rust function name cannot be assembled
///      by `concat!` — re-introducing the function requires the literal
///      `fn auto_skills` in source.
///   2. `format!` reassembly: function declarations and direct calls
///      are not produced by `format!` interpolation.
///   3. Named constant reference: a `const` aliasing the string would
///      still place the literal `auto_skills` in source, and the
///      declaration / call site trips the byte check regardless.
///   4. Method chains / split args: not applicable — the target is a
///      function identifier, not a CLI argument passed via `.arg()`.
#[test]
fn test_phase_config_no_auto_skills() {
    let root = common::repo_root();
    for rel in ["src/phase_config.rs", "src/commands/init_state.rs"] {
        let content =
            fs::read_to_string(root.join(rel)).unwrap_or_else(|_| panic!("{rel} must exist"));
        assert!(
            !content.contains("auto_skills"),
            "{rel} must not contain `auto_skills` — the wholesale \
             `flow-start --auto` skills-override was removed; the state \
             file's skills section is always seeded from `.flow.json`."
        );
    }
}

// --- resolve-skill-mode bare-string branch ---
//
// `resolve_skill_mode::resolve` parses ONLY the block-shape
// `skills.<skill>` config object (`{commit, continue}`). The prior
// `entry.as_str()` branch that treated a bare-string `skills.<skill>`
// entry as a mode value is removed — every non-object entry now
// clamps to the per-skill default. This tombstone catches a merge
// conflict or accidental edit that re-introduces bare-string parsing
// inside `resolve()`.

/// Tombstone: removed in PR #1691. The bare-string-parsing arm of
/// `resolve_skill_mode::resolve` — `entry.as_str()` treating a
/// `skills.<skill>` bare string as a mode value — is deleted. The
/// resolver reads only the block-shape `{commit, continue}` object.
///
/// Assertion kind: structural. The forbidden construct (consuming
/// the `skills.<skill>` entry as a bare string) can be expressed
/// many ways, so a byte-substring scan over the whole file is
/// insufficient — a `concat!` / `format!`-reassembled literal would
/// evade it, and prose elsewhere in the file legitimately mentions
/// `as_str`. The scan is therefore bounded to the body of
/// `resolve()` via `extract_fn_body` (brace-balanced from the
/// signature marker `pub fn resolve(`) and asserts the body contains
/// no `as_str(` call. The new `resolve()` delegates per-axis string
/// extraction to the separate `resolve_axis` helper — which sits
/// before `resolve` in the file and is outside the bounded slice —
/// so `resolve()`'s own body carries no `as_str(`. Re-introducing
/// `entry.as_str()` inside `resolve()` re-adds the call and trips
/// this test.
#[test]
fn test_resolve_skill_mode_no_bare_string_branch() {
    let root = common::repo_root();
    let path = root.join("src").join("resolve_skill_mode.rs");
    let content = fs::read_to_string(&path).expect("src/resolve_skill_mode.rs must exist");
    const MARKER: &str = "pub fn resolve(";
    let sig_start = content
        .find(MARKER)
        .expect("`pub fn resolve(` must exist in src/resolve_skill_mode.rs");
    let body = extract_fn_body(&content, sig_start + MARKER.len())
        .expect("resolve() body must be brace-balanced");
    assert!(
        !body.contains("as_str("),
        "src/resolve_skill_mode.rs::resolve must not contain `as_str(` — \
         the bare-string `skills.<skill>` parsing arm is removed; the \
         resolver reads only the block-shape `{{commit, continue}}` \
         object. Per-axis string extraction lives in the separate \
         `resolve_axis` helper."
    );
}

// --- complete-fast / complete-preflight mode-flag removal ---
//
// `--auto` and `--manual` clap arguments are removed from both
// `complete-fast` and `complete-preflight`. The Complete-phase
// autonomy mode is resolved purely from the state file's
// `skills.flow-complete` block via `resolve_skill_mode`. These
// tombstones catch a merge conflict or accidental edit that
// re-introduces either clap field.

/// Tombstone: removed in PR #1691. The `pub auto: bool` and
/// `pub manual: bool` clap fields are removed from
/// `src/complete_preflight.rs::Args`. The Complete-phase mode is
/// resolved from the state file's `skills.flow-complete` block, not
/// from CLI flags. Must not return.
///
/// Stability argument: the protected targets are Rust struct field
/// declarations (`pub auto: bool`, `pub manual: bool`). A field
/// declaration is Rust syntax — it cannot be assembled by `concat!`
/// or produced by `format!` (those macros yield string values, not
/// `struct` members), and it cannot be a named `constant` reference
/// (a field is a declaration, not a value). rustfmt pins the single
/// space in `pub auto: bool`, so the byte literal is canonical. A
/// merge conflict can only resurrect the exact bytes, which this
/// scan catches.
#[test]
fn test_complete_preflight_no_auto_manual_args() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("complete_preflight.rs"))
        .expect("src/complete_preflight.rs must exist");
    assert!(
        !content.contains("pub auto: bool"),
        "src/complete_preflight.rs must not contain `pub auto: bool` — \
         the `--auto` clap field is removed; mode is resolved from the \
         state file's `skills.flow-complete` block."
    );
    assert!(
        !content.contains("pub manual: bool"),
        "src/complete_preflight.rs must not contain `pub manual: bool` — \
         the `--manual` clap field is removed; mode is resolved from the \
         state file's `skills.flow-complete` block."
    );
}

/// Tombstone: removed in PR #1691. The `pub auto: bool` and
/// `pub manual: bool` clap fields are removed from
/// `src/complete_fast.rs::Args`. The Complete-phase mode is resolved
/// from the state file's `skills.flow-complete` block, not from CLI
/// flags. Must not return.
///
/// Stability argument: the protected targets are Rust struct field
/// declarations (`pub auto: bool`, `pub manual: bool`). A field
/// declaration is Rust syntax — it cannot be assembled by `concat!`
/// or produced by `format!` (those macros yield string values, not
/// `struct` members), and it cannot be a named `constant` reference
/// (a field is a declaration, not a value). rustfmt pins the single
/// space in `pub auto: bool`, so the byte literal is canonical. A
/// merge conflict can only resurrect the exact bytes, which this
/// scan catches.
#[test]
fn test_complete_fast_no_auto_manual_args() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("complete_fast.rs"))
        .expect("src/complete_fast.rs must exist");
    assert!(
        !content.contains("pub auto: bool"),
        "src/complete_fast.rs must not contain `pub auto: bool` — \
         the `--auto` clap field is removed; mode is resolved from the \
         state file's `skills.flow-complete` block."
    );
    assert!(
        !content.contains("pub manual: bool"),
        "src/complete_fast.rs must not contain `pub manual: bool` — \
         the `--manual` clap field is removed; mode is resolved from the \
         state file's `skills.flow-complete` block."
    );
}

// --- phase-enter resolve_mode removal ---
//
// `phase_enter::resolve_mode` read the autonomy mode from the state
// file's skills block and embedded a `mode` object in the
// phase-enter response. The single tested source of truth for skill
// autonomy is now `resolve_skill_mode`, which every skill's
// `## Mode Resolution` section calls directly — so phase-enter no
// longer resolves or returns a mode. The pub fn and its `run_impl`
// callsite are deleted. This tombstone catches a merge conflict or
// accidental edit that re-introduces the function in
// `src/phase_enter.rs`.

/// Tombstone: removed in PR #1691. The `pub fn resolve_mode` in
/// `src/phase_enter.rs` — which read `skills.<phase>` and embedded a
/// `mode` object in the phase-enter response — is deleted. Skill
/// autonomy is resolved exclusively through `resolve_skill_mode`.
/// Must not return.
///
/// Asserts `src/phase_enter.rs` does NOT contain the identifier
/// `resolve_mode`. The byte-substring shape holds because:
///   1. `concat!` reassembly: a Rust function name cannot be
///      assembled by `concat!` — re-introducing the function
///      requires the literal `fn resolve_mode` in source.
///   2. `format!` reassembly: function declarations and direct
///      calls are not produced by `format!` interpolation.
///   3. Named constant reference: a `const` aliasing the string
///      would still place the literal `resolve_mode` in source, and
///      the declaration / call site trips the byte check regardless.
///   4. Method chains / split args: not applicable — the target is
///      a function identifier, not a CLI argument passed via
///      `.arg()`.
///
/// Scoped to `src/phase_enter.rs` only — a distinct `resolve_mode`
/// (signature `fn resolve_mode(state: Option<&Value>) -> String`)
/// legitimately survives in `src/complete_preflight.rs`, so a
/// codebase-wide scan would false-positive.
#[test]
fn test_phase_enter_no_resolve_mode() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("phase_enter.rs"))
        .expect("src/phase_enter.rs must exist");
    assert!(
        !content.contains("resolve_mode"),
        "src/phase_enter.rs must not contain `resolve_mode` — the \
         function and its run_impl callsite were deleted; skill \
         autonomy is resolved exclusively through `resolve_skill_mode`."
    );
}

// --- Complete-phase GitHub-CI determination removal ---
//
// Phase 5 Complete no longer makes its own determination about a PR's
// GitHub CI state. `gh pr merge --squash` is the sole merge authority;
// when it refuses, the verbatim stderr surfaces as a `not_mergeable`
// stop-and-report. The deleted surface: `parse_gh_checks_output` and
// the `ci_drift` / `ci_pending` dispatch arms in the two complete
// modules, the `gh pr checks` permission, the `_drift_recovery_attempted`
// loop-guard field, and the "Check GitHub CI status" SKILL step. These
// tombstones catch a merge conflict or accidental edit re-introducing
// any of them.

/// Tombstone: removed in PR #1726. The `fn parse_gh_checks_output` in
/// `src/complete_fast.rs` — which parsed `gh pr checks` tab-separated
/// output into a status string — is deleted along with the GitHub-CI
/// determination it fed. `gh pr merge --squash` is now the sole merge
/// authority. Must not return.
///
/// Stability argument: the protected target is a Rust function
/// definition (`fn parse_gh_checks_output`). A function item cannot be
/// assembled by `concat!` or produced by `format!` (those macros yield
/// string values, not items), and it cannot be a named `constant`
/// reference (a `const` cannot be an `fn`). Re-introducing the function
/// requires the literal `parse_gh_checks_output` identifier in source,
/// which this byte scan catches.
#[test]
fn test_complete_fast_no_parse_gh_checks_output() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("complete_fast.rs"))
        .expect("src/complete_fast.rs must exist");
    assert!(
        !content.contains("parse_gh_checks_output"),
        "src/complete_fast.rs must not contain `parse_gh_checks_output` — \
         the GitHub-CI parse helper is deleted; `gh pr merge --squash` is \
         the merge authority and surfaces `not_mergeable` on refusal."
    );
}

/// Tombstone: removed in PR #1726. The `ci_drift` and `ci_pending`
/// dispatch arms are deleted from the Complete-phase merge dispatch
/// functions — `freshness_and_merge` and `run_impl` in
/// `src/complete_fast.rs`, and `complete_merge` in
/// `src/complete_merge.rs`. The base-policy arm now emits
/// `not_mergeable` carrying the verbatim `gh pr merge` stderr. Must
/// not return.
///
/// Structural fn-body scan: the assertion bounds to each dispatch
/// function's body (from `fn <name>(` to the next `\nfn `) so a
/// legitimate mention elsewhere cannot satisfy the check, and so a
/// re-introduced dispatch arm is caught regardless of whether the
/// path string is written as a literal or assembled via `concat!` /
/// `format!` — the body scan catches the dispatch arm by its location
/// in the merge functions, not only by the exact literal bytes.
#[test]
fn test_complete_dispatch_no_ci_drift_or_ci_pending() {
    let root = common::repo_root();
    let fast = fs::read_to_string(root.join("src").join("complete_fast.rs"))
        .expect("src/complete_fast.rs must exist");
    let merge = fs::read_to_string(root.join("src").join("complete_merge.rs"))
        .expect("src/complete_merge.rs must exist");

    let scan = |content: &str, marker: &str| -> String {
        let tail = content
            .split_once(marker)
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("{} must exist", marker));
        tail.split_once("\nfn ")
            .map(|(b, _)| b)
            .unwrap_or(tail)
            .to_string()
    };

    for (label, body) in [
        (
            "complete_fast::freshness_and_merge",
            scan(&fast, "fn freshness_and_merge("),
        ),
        ("complete_fast::run_impl", scan(&fast, "fn run_impl(")),
        (
            "complete_merge::complete_merge",
            scan(&merge, "fn complete_merge("),
        ),
    ] {
        assert!(
            !body.contains("ci_drift"),
            "{} must not dispatch `ci_drift` — the toolchain-drift path is \
             deleted; `gh pr merge --squash` is the merge authority.",
            label
        );
        assert!(
            !body.contains("ci_pending"),
            "{} must not dispatch `ci_pending` — the GitHub-CI-pending path is \
             deleted; `gh pr merge --squash` returns `not_mergeable` instead.",
            label
        );
    }
}

/// Tombstone: removed in PR #1726. The `_drift_recovery_attempted`
/// loop-guard field is deleted along with the `ci_drift` recovery
/// path it guarded. It must not appear in either of its former
/// documentation homes — `skills/flow-complete/SKILL.md` (the
/// SOFT-GATE clearing and Step 1 dispatch) or
/// `docs/reference/flow-state-schema.md` (the field row). Must not
/// return.
///
/// Stability argument: both targets are flat Markdown byte streams.
/// The literal `_drift_recovery_attempted` cannot be reassembled at
/// runtime — Markdown has no `concat!` macro, no `format!`
/// interpolation, and no named `constant` references. A merge
/// conflict can only resurrect the exact bytes, which this scan
/// catches in each file.
#[test]
fn test_flow_complete_no_drift_recovery_attempted() {
    let skill = fs::read_to_string(common::skills_dir().join("flow-complete").join("SKILL.md"))
        .expect("skills/flow-complete/SKILL.md must exist");
    assert!(
        !skill.contains("_drift_recovery_attempted"),
        "skills/flow-complete/SKILL.md must not reference \
         `_drift_recovery_attempted` — the ci_drift loop-guard is deleted."
    );
    let schema = fs::read_to_string(
        common::repo_root()
            .join("docs")
            .join("reference")
            .join("flow-state-schema.md"),
    )
    .expect("docs/reference/flow-state-schema.md must exist");
    assert!(
        !schema.contains("_drift_recovery_attempted"),
        "docs/reference/flow-state-schema.md must not document \
         `_drift_recovery_attempted` — the field is deleted."
    );
}

/// Tombstone: removed in PR #1726. The `Bash(gh pr checks *)` entry is
/// deleted from `UNIVERSAL_ALLOW` in `src/prime_check.rs` — the only
/// FLOW consumer (the Complete-phase GitHub-CI check) is gone, so the
/// permission is orphaned. `gh pr checks` survives only as user-facing
/// diagnostic prose the user runs manually, which needs no allow
/// entry. Must not return.
///
/// Stability argument: `UNIVERSAL_ALLOW` entries are plain `&str`
/// literals by project convention, so the byte-substring is stable. A
/// `concat!("Bash(gh pr ", "checks *)")` reassembly is non-idiomatic
/// (every other entry is a single literal), and `format!`
/// interpolation does not appear in the const array. A named
/// `constant` aliasing the string would still place the literal in
/// source. The matching prime-SKILL block is independently enforced by
/// `tests/permissions.rs`.
#[test]
fn test_prime_check_no_gh_pr_checks_permission() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("prime_check.rs"))
        .expect("src/prime_check.rs must exist");
    assert!(
        !content.contains("Bash(gh pr checks *)"),
        "src/prime_check.rs must not contain `Bash(gh pr checks *)` in \
         UNIVERSAL_ALLOW — the Complete-phase GitHub-CI check that needed \
         it is deleted; `gh pr merge --squash` is the merge authority."
    );
}

/// Tombstone: removed in PR #1726. The "Check GitHub CI status" step
/// is deleted from `skills/flow-complete/SKILL.md` — Complete makes no
/// GitHub-CI determination of its own. The literal heading must not
/// return, AND no SKILL step may dispatch on the deleted `ci_pending`
/// path value (the `gh pr merge --squash` authority returns
/// `not_mergeable` instead).
///
/// Stability argument: the protected targets are flat Markdown byte
/// strings (a step heading and a `"path": "ci_pending"` dispatch
/// marker). Markdown has no `concat!` macro, no `format!`
/// interpolation, and no named `constant` references, so neither can
/// be reassembled at runtime. The paired no-dispatch assertion covers
/// a reworded heading: a renamed GitHub-CI step is still the deleted
/// feature, and re-introducing its dispatch requires the
/// `ci_pending` path marker this scan catches.
#[test]
fn test_flow_complete_no_github_ci_status_step() {
    let content = fs::read_to_string(common::skills_dir().join("flow-complete").join("SKILL.md"))
        .expect("skills/flow-complete/SKILL.md must exist");
    assert!(
        !content.contains("Check GitHub CI status"),
        "skills/flow-complete/SKILL.md must not contain the `Check GitHub CI \
         status` step heading — Complete makes no GitHub-CI determination."
    );
    assert!(
        !content.contains(r#"`"path": "ci_pending"`"#),
        "skills/flow-complete/SKILL.md must not dispatch on the `ci_pending` \
         path — `gh pr merge --squash` returns `not_mergeable` instead."
    );
}

// --- exhausted-retry note path ---
//
// The `agent_exhausted_retries` state-note path is gone. The
// flow-review retry loop no longer calls
// `bin/flow append-note` with a `--kind` flag, and the dead
// `agent_exhausted_retries` note kind no longer exists. Exhausted
// and skipped agents surface through `phases.<phase>.agents_skipped`
// and the Complete Done banner's Skipped Agents section. This
// per-file tombstone catches a merge conflict or accidental edit
// that re-introduces the dead invocation in the SKILL.md.

/// Tombstone: removed in PR #1584. The `agent_exhausted_retries`
/// state-note path is gone from `skills/flow-review/SKILL.md` — the
/// retry loop and Done-section recovery no longer invoke
/// `bin/flow append-note` with the dead `--kind` flag or the
/// `agent_exhausted_retries` note kind. Must not return.
///
/// Stability argument: the protected target is Markdown prose, not
/// Rust source. The byte literals `agent_exhausted_retries` and
/// `--kind` cannot be reassembled at runtime — Markdown is a flat
/// byte stream with no `concat!` macro, no `format!` interpolation,
/// and no named constant references. A merge conflict can only
/// resurrect the exact bytes, which this scanner catches.
#[test]
fn test_flow_review_skill_no_exhausted_retry_note() {
    let path = common::skills_dir().join("flow-review").join("SKILL.md");
    let content = fs::read_to_string(&path).expect("flow-review SKILL.md must exist");
    assert!(
        !content.contains("agent_exhausted_retries"),
        "skills/flow-review/SKILL.md must not reference `agent_exhausted_retries` — \
         the dead state-note path is replaced by the Complete Done banner's \
         Skipped Agents section sourced from `phases.<phase>.agents_skipped`."
    );
    assert!(
        !content.contains("--kind"),
        "skills/flow-review/SKILL.md must not invoke `bin/flow append-note --kind` — \
         the real append-note interface is `--note`/`--type`/`--branch`; the \
         `--kind` flag belonged only to the removed exhausted-retry note path."
    );
}

// --- flow-reset guard prose ---
//
// `/flow:flow-reset` previously gated invocation on the integration
// branch via a `bin/flow base-branch` lookup, rejecting any cwd that
// did not match. The guard solved a non-problem — `.flow-states/` is
// at the project root, not inside any worktree — and broke in repos
// whose integration branch did not match `origin/HEAD`. The guard
// and its rejection prose are gone; the per-script
// `${CLAUDE_PLUGIN_ROOT}/bin/reset` invocation runs from any cwd.

/// Tombstone: removed in PR #1643. The integration-branch guard in
/// `skills/flow-reset/SKILL.md` is gone — the skill now invokes
/// `${CLAUDE_PLUGIN_ROOT}/bin/reset` directly after the user
/// confirmation prompt, with no `bin/flow base-branch` lookup and no
/// "Must be on" rejection message. Must not return.
///
/// Stability argument: the protected target is Markdown prose, not
/// Rust source. The byte literals `Must be on` and `base-branch`
/// cannot be reassembled at runtime — Markdown is a flat byte stream
/// with no `concat!` macro, no `format!` interpolation, and no named
/// constant references. A merge conflict can only resurrect the
/// exact bytes, which this scanner catches.
#[test]
fn test_flow_reset_no_guard_prose() {
    let path = common::skills_dir().join("flow-reset").join("SKILL.md");
    let content = fs::read_to_string(&path).expect("flow-reset SKILL.md must exist");
    assert!(
        !content.contains("Must be on"),
        "skills/flow-reset/SKILL.md must not contain `Must be on` — \
         the integration-branch rejection message belonged to the \
         deleted guard. The skill now invokes \
         `${{CLAUDE_PLUGIN_ROOT}}/bin/reset` directly."
    );
    assert!(
        !content.contains("bin/flow base-branch"),
        "skills/flow-reset/SKILL.md must not invoke `bin/flow base-branch` — \
         the deleted guard's branch lookup is gone. The script resolves \
         project root via git rev-parse internally."
    );
}

// --- start_init label-apply removal ---
//
// The `label_issues` call that applied "Flow In-Progress" to
// referenced issues was removed from `src/start_init.rs::run_impl`
// and moved to the end of `src/start_workspace.rs`'s success path.
// The label now means "a flow is live, worktree exists, PR exists"
// rather than "a flow was attempted" — failed start-gate runs no
// longer leave a sticky label that blocks the next retry.

/// Tombstone: removed in PR #1697. The Flow In-Progress label
/// apply moved from `start_init` to `start_workspace`'s
/// trailing success block so the label means "a flow is live"
/// rather than "a flow was attempted". Must not return inside
/// `src/start_init.rs::run_impl`.
///
/// Assertion kind: structural. Function-body-scoped scan of
/// `run_impl` in `src/start_init.rs`. The byte literal
/// `label_issues(` is checked inside the bounded slice
/// extracted via `extract_fn_body`. Stability argument: a Rust
/// function call requires the callee identifier to appear as a
/// literal token in source, so a `concat!`-assembled name
/// cannot resurrect the call without the literal `label_issues(`
/// returning to the body; `format!` produces strings, not call
/// expressions; a named `constant` rebinding (`let f =
/// label_issues; f(...)`) still places the identifier in source;
/// a `.arg()` split is not applicable because the target is a
/// function-call site, not a CLI argument chain. The bounded
/// slice scope prevents the module-level `use
/// crate::label_issues::LABEL;` import — which legitimately
/// stays for the pre-lock guard — from satisfying the check,
/// and prevents prose elsewhere in the file (module doc,
/// sibling helpers, error messages) from accidental matches.
#[test]
fn test_start_init_no_label_apply() {
    let root = common::repo_root();
    let path = root.join("src").join("start_init.rs");
    let content = fs::read_to_string(&path).expect("src/start_init.rs must exist");
    const MARKER: &str = "fn run_impl(";
    let sig_start = content
        .find(MARKER)
        .expect("`fn run_impl(` must exist in src/start_init.rs");
    let body = extract_fn_body(&content, sig_start + MARKER.len())
        .expect("run_impl() body must be brace-balanced");
    assert!(
        !body.contains("label_issues("),
        "src/start_init.rs::run_impl must not contain `label_issues(` — \
         the label apply moved to start_workspace per PR #1697 so the \
         Flow In-Progress label means \"a flow is live, worktree exists, \
         PR exists\" rather than \"a flow was attempted\"."
    );
}

// --- FLOW_DENY escape-hatch entries ---
//
// The deny entries listed below block the canonical escape-hatch
// program/flag combinations enumerated in
// `.claude/rules/no-escape-hatches.md` "Canonical Escape-Hatch Shapes":
// shell-eval (`bash -c`, `sh -c`, `zsh -c`, `eval`), interpreter-eval
// (`perl -e/-E`, `python -c`, `python3 -c`, `ruby -e`, `node -e/-p`),
// command-wrapper (`xargs`, `rtk proxy`), network-bridge (`nc`, direct
// `ssh`), and inter-process (`tmux send-keys`, `screen -X`). Removing
// any entry from `FLOW_DENY` re-opens the escape-hatch surface it
// blocks. The structural escape-hatch layer in `validate-pretool`
// covers indirect forms (absolute paths, env-var prefixes); the deny
// list is the first-pass filter against direct shapes that reach
// target projects' `.claude/settings.json` via `/flow:flow-prime`.

/// Canonical FLOW_DENY entries that block escape-hatch program/flag
/// combinations. Each entry must appear verbatim inside the
/// `FLOW_DENY` const slice in `src/prime_check.rs`.
const FLOW_DENY_ESCAPE_HATCH_ENTRIES: &[&str] = &[
    "Bash(bash -c *)",
    "Bash(sh -c *)",
    "Bash(zsh -c *)",
    "Bash(eval *)",
    "Bash(xargs *)",
    "Bash(perl -e *)",
    "Bash(perl -E *)",
    "Bash(python -c *)",
    "Bash(python3 -c *)",
    "Bash(ruby -e *)",
    "Bash(node -e *)",
    "Bash(node -p *)",
    "Bash(nc *)",
    "Bash(tmux send-keys *)",
    "Bash(screen -X *)",
    "Bash(ssh *)",
    "Bash(rtk proxy *)",
];

/// Tombstone: removed in PR #1495. The seventeen escape-hatch entries
/// listed in `FLOW_DENY_ESCAPE_HATCH_ENTRIES` block the canonical
/// program/flag combinations from
/// `.claude/rules/no-escape-hatches.md` "Canonical Escape-Hatch
/// Shapes". A merge conflict or accidental edit that removes any
/// entry from the `FLOW_DENY` const slice in `src/prime_check.rs`
/// re-opens the corresponding escape-hatch surface — direct
/// invocations bypass `validate-pretool`'s structural Layer 8 if
/// the settings-layer deny entry is also missing.
///
/// Stability argument (per `.claude/rules/tombstone-tests.md`
/// "Literal tombstones — stability checklist"): each entry is a
/// const &str slice element in `FLOW_DENY` — the literal cannot be
/// reassembled by `concat!`, produced by `format!`, or split across
/// constant declarations because the patterns are stored as inline
/// string literals in a const slice. The bounded-slice scan over
/// the `FLOW_DENY` region (between `pub const FLOW_DENY: &[&str] = &[`
/// and the terminating `];`) prevents prose elsewhere in
/// `src/prime_check.rs` that mentions the pattern from satisfying
/// the `.contains(...)` check. The `.arg()`-chain bypass does not
/// apply because `FLOW_DENY` is parsed by `permission_to_regex` as a
/// whole string, not as method-chain arguments.
#[test]
fn test_flow_deny_no_escape_hatch_entry_removal() {
    let root = common::repo_root();
    let path = root.join("src").join("prime_check.rs");
    let content = fs::read_to_string(&path).expect("src/prime_check.rs must exist");
    let tail = content
        .split_once("pub const FLOW_DENY: &[&str] = &[")
        .map(|(_, t)| t)
        .expect("FLOW_DENY declaration must exist in src/prime_check.rs");
    let region = tail
        .split_once("];")
        .map(|(r, _)| r)
        .expect("FLOW_DENY const must be terminated by `];`");
    let mut missing = Vec::new();
    for entry in FLOW_DENY_ESCAPE_HATCH_ENTRIES {
        let quoted = format!("\"{}\"", entry);
        if !region.contains(&quoted) {
            missing.push(entry.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "src/prime_check.rs::FLOW_DENY is missing escape-hatch entries:\n  {}\n\n\
         Each entry blocks a canonical escape-hatch shape from \
         `.claude/rules/no-escape-hatches.md`. Removing any entry re-opens \
         the corresponding direct-form bypass surface.",
        missing.join("\n  ")
    );
}

/// Tombstone: removed in PR #1650. The `Bash(*flow*/bin/reset)`
/// wildcard allow entry was a script-specific permission that
/// granted direct invocation of `${CLAUDE_PLUGIN_ROOT}/bin/reset`
/// outside the canonical `bin/flow` dispatcher. The
/// `/flow:flow-reset` skill now invokes
/// `${CLAUDE_PLUGIN_ROOT}/bin/flow reset` and the Rust shim
/// `src/reset.rs` exec's the underlying bash script — so the
/// single `Bash(*bin/flow *)` allow entry covers the call per
/// `.claude/rules/permissions.md` "bin/flow Dispatch First". A
/// merge-conflict re-introduction of the deleted entry would
/// silently widen the model's bash-invocation surface and bypass
/// the dispatcher's discoverability contract.
///
/// Stability argument (per `.claude/rules/tombstone-tests.md`
/// "Literal tombstones — stability checklist"): the entry is a
/// const &str slice element in `UNIVERSAL_ALLOW` — the literal
/// cannot be reassembled by `concat!`, produced by `format!`, or
/// split across constant declarations because the patterns are
/// stored as inline string literals in a const slice. The
/// bounded-slice scan over the `UNIVERSAL_ALLOW` region (between
/// `pub const UNIVERSAL_ALLOW: &[&str] = &[` and the terminating
/// `];`) prevents prose elsewhere in `src/prime_check.rs` that
/// mentions the pattern from satisfying the `.contains(...)`
/// check. The `.arg()`-chain bypass does not apply because
/// `UNIVERSAL_ALLOW` is consumed as whole-string entries, not as
/// method-chain arguments.
#[test]
fn test_universal_allow_no_flow_bin_reset_wildcard() {
    let root = common::repo_root();
    let path = root.join("src").join("prime_check.rs");
    let content = fs::read_to_string(&path).expect("src/prime_check.rs must exist");
    let tail = content
        .split_once("pub const UNIVERSAL_ALLOW: &[&str] = &[")
        .map(|(_, t)| t)
        .expect("UNIVERSAL_ALLOW declaration must exist in src/prime_check.rs");
    let region = tail
        .split_once("];")
        .map(|(r, _)| r)
        .expect("UNIVERSAL_ALLOW const must be terminated by `];`");
    let forbidden = "\"Bash(*flow*/bin/reset)\"";
    assert!(
        !region.contains(forbidden),
        "src/prime_check.rs::UNIVERSAL_ALLOW must not re-introduce `{}`. \
         The script is now reached via the canonical `bin/flow reset` \
         dispatcher; see `.claude/rules/permissions.md` \
         \"bin/flow Dispatch First\".",
        forbidden
    );
}

/// Tombstone: removed in PR #1743. The `commit_format` configuration
/// axis is gone — Conventional Commits is the single always-on commit
/// format. `src/prime_setup.rs` must not re-introduce the
/// `commit_format` field/key or the `--commit-format` CLI flag.
///
/// Protection target: the config-key literal `commit_format` and the
/// CLI-flag literal `--commit-format` (whose `commit-format` substring
/// the hyphen check catches). Assertion kind: literal byte-substring.
/// Stability argument — the byte check holds because:
///   1. `concat!` reassembly: a clap `#[arg]` flag name and a JSON key
///      are written as plain string literals, never assembled from
///      fragments by `concat!`.
///   2. `format!` reassembly: a flag name / config key is matched where
///      it appears verbatim in source, not produced by interpolation.
///   3. Named constant reference: a `const` aliasing the value would
///      still place the literal `commit_format` in source.
///   4. `.arg()` split: N/A — the target is a struct field / flag name,
///      not a value passed across multiple `.arg()` calls.
///
/// No file-resurrection pair: no source file was deleted (field/flag
/// removal within `src/prime_setup.rs`).
#[test]
fn test_prime_setup_no_commit_format_flag() {
    let root = common::repo_root();
    let content =
        fs::read_to_string(root.join("src/prime_setup.rs")).expect("src/prime_setup.rs must exist");
    assert!(
        !content.contains("commit_format"),
        "src/prime_setup.rs must not contain `commit_format` — the \
         configuration axis was removed; Conventional Commits is the \
         single always-on commit format."
    );
    assert!(
        !content.contains("commit-format"),
        "src/prime_setup.rs must not contain the `--commit-format` CLI \
         flag — the commit-format choice was removed."
    );
}

/// Tombstone: removed in PR #1743. `skills/flow-prime/SKILL.md` no
/// longer asks the commit-format question — the `commit_format`
/// configuration axis was removed and Conventional Commits is the
/// single always-on commit format. The prime skill must not
/// re-introduce the prompt.
///
/// Protection target: the config-key literal `commit_format` and the
/// CLI-flag literal `commit-format`. Assertion kind: literal
/// byte-substring. Stability argument — the byte check holds because:
///   1. `concat!` reassembly: a SKILL.md prose mention / bash flag is
///      written verbatim in Markdown, never assembled by `concat!`.
///   2. `format!` reassembly: Markdown prose is not produced by
///      `format!` interpolation.
///   3. Named constant reference: N/A for Markdown corpus.
///   4. `.arg()` split: N/A for Markdown corpus.
///
/// No file-resurrection pair: `skills/flow-prime/SKILL.md` survives;
/// only the commit-format prompt and the `--commit-format` invocation
/// flag were removed from it.
#[test]
fn test_prime_no_commit_format_question() {
    let c = common::read_skill("flow-prime");
    assert!(
        !c.contains("commit_format"),
        "flow-prime SKILL must not contain `commit_format` — the \
         commit-format choice was removed."
    );
    assert!(
        !c.contains("commit-format"),
        "flow-prime SKILL must not contain `--commit-format` — the \
         prime-setup invocation no longer passes the flag."
    );
}

/// Tombstone: removed in PR #1743. `skills/flow-commit/SKILL.md` no
/// longer branches on the `commit_format` axis (`full` vs
/// `title-only`) — Conventional Commits is the single always-on
/// format. The commit skill must not re-introduce the format choice.
///
/// Protection target: the format-value literal `title-only` and the
/// config-key literal `commit_format`. Assertion kind: literal
/// byte-substring. Stability argument — the byte check holds because:
///   1. `concat!` reassembly: a SKILL.md format-value mention is
///      written verbatim in Markdown, never assembled by `concat!`.
///   2. `format!` reassembly: Markdown prose is not produced by
///      `format!` interpolation.
///   3. Named constant reference: N/A for Markdown corpus.
///   4. `.arg()` split: N/A for Markdown corpus.
///
/// No file-resurrection pair: `skills/flow-commit/SKILL.md` survives;
/// only the per-project format branch was removed from it.
#[test]
fn test_commit_no_title_only_or_full_format() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("title-only"),
        "flow-commit SKILL must not contain `title-only` — the \
         per-project commit-format choice was removed in favor of \
         Conventional Commits."
    );
    assert!(
        !c.contains("commit_format"),
        "flow-commit SKILL must not contain `commit_format` — the \
         configuration axis was removed."
    );
}

// --- fs2 dependency removal (PR #1779) ---
//
// The `fs2` crate is deprecated and unmaintained. Its
// `FileExt::lock_exclusive()` calls on `&File` across src/lock.rs,
// src/commands/log.rs, and tests/concurrency.rs migrated to the
// standard library's `File::lock()` (stabilized Rust 1.89);
// tests/concurrency.rs additionally migrated its explicit
// `FileExt::unlock()` calls to `File::unlock()`, while src/lock.rs and
// src/commands/log.rs release the lock implicitly on file drop. `fs2 =
// "0.4"` was removed from Cargo.toml. This tombstone catches a merge
// conflict or accidental edit that re-introduces the dependency or any
// `use fs2` / `fs2::` reference in source.

/// Tombstone: removed in PR #1779. The `fs2` dependency and every
/// `use fs2` / `fs2::` reference are gone — file locking uses the
/// standard library's `File::lock()` / `File::unlock()`. Must not
/// return to `Cargo.toml`, `src/`, or `tests/`.
///
/// Assertion kind: literal byte-substring across `Cargo.toml` and
/// every `.rs` file under `src/` and `tests/` (self-excluding this
/// file, which carries the literal as search input). Stability
/// argument per `.claude/rules/tombstone-tests.md` "Literal
/// tombstones — stability checklist":
///   1. `concat!` reassembly: a `Cargo.toml` dependency line is TOML
///      key=value text, not Rust source, so `concat!` cannot assemble
///      it. A `use fs2::FileExt;` import is a Rust `use` item whose
///      path segment must appear verbatim — `concat!` yields a string
///      value, never a path token in a `use` declaration.
///   2. `format!` reassembly: neither a TOML manifest line nor a Rust
///      `use` path is produced by `format!` interpolation.
///   3. Named constant reference: a `const` aliasing `"fs2"` cannot
///      stand in for the crate name in a `use` path or a Cargo
///      dependency key — both require the literal token in source.
///   4. `.arg()` split: not applicable — the target is a manifest key
///      and a `use`-path segment, not a CLI argument chain.
///
/// File-resurrection pair: not applicable — no source file is
/// deleted; the change is callsite migration plus a manifest-line
/// removal.
#[test]
fn test_deps_no_fs2() {
    let root = common::repo_root();
    let scanner_path = root
        .join("tests")
        .join("tombstones.rs")
        .canonicalize()
        .expect("scanner path must canonicalize");

    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml must exist");
    assert!(
        !cargo_toml.contains("fs2"),
        "Cargo.toml must not contain `fs2` — the deprecated crate was \
         removed; file locking uses std `File::lock()` / `File::unlock()`."
    );

    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    collect_rs_files(&root.join("tests"), &mut files);

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        // Self-exclude this scanner file — it carries `fs2` as search input.
        if file
            .canonicalize()
            .map(|p| p == scanner_path)
            .unwrap_or(false)
        {
            continue;
        }
        let content = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = file.strip_prefix(&root).unwrap_or(file);
        for (idx, line) in content.lines().enumerate() {
            if line.contains("use fs2") || line.contains("fs2::") {
                violations.push(format!("{}:{}", rel.display(), idx + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "fs2 references must not appear under src/ or tests/ — \
         file locking uses std `File::lock()` / `File::unlock()`:\n  {}",
        violations.join("\n  ")
    );
}

// --- flow-decompose-project removal (issue #1590 AC#6) ---
//
// AC#6 of issue #1590 mandates the removal of the
// `flow-decompose-project` skill: the multi-track filing branch
// in `flow-plan` (AC#4) supersedes the prior six-step automated
// pipeline. The skill directory, its doc page, every catalog row,
// every `UNIVERSAL_ALLOW` / `MULTI_STEP_UTILITY_SKILLS` reference,
// and every rule-file mention are removed in the same PR. This
// tombstone catches a merge conflict or accidental edit that
// re-introduces any of the bytes.

/// Tombstone: removed in PR #1694. The `flow-decompose-project`
/// skill and every reference to it are gone — `flow-plan`'s
/// multi-track branch (AC#4) is the replacement. Must not return
/// to `skills/`, `docs/skills/`, `README.md`,
/// `skills/flow-skills/SKILL.md`, `skills/flow-prime/SKILL.md`,
/// or any `.claude/rules/` file.
///
/// Stability argument: the protected target is a SKILL.md
/// directory and its byte-string references. (a) `concat!` —
/// N/A for Markdown corpus; (b) `format!` — N/A for Markdown;
/// (c) split constant — N/A for Markdown; (d) `.arg()` split —
/// N/A for Markdown. The file-existence half of the tombstone
/// (`skills/flow-decompose-project/SKILL.md` absent) catches any
/// Rust-side resurrection via `#[path]` aliases or otherwise; the
/// byte-substring half catches Markdown prose resurrection.
#[test]
fn test_skills_no_flow_decompose_project() {
    let skill_path = common::skills_dir()
        .join("flow-decompose-project")
        .join("SKILL.md");
    assert!(
        !skill_path.exists(),
        "skills/flow-decompose-project/SKILL.md must not exist — the skill was removed in PR #1694 per AC#6 of issue #1590"
    );

    // Byte-substring scan: the literal `flow-decompose-project`
    // must not appear in any of the user-facing surfaces. The
    // `.claude/rules/`/`CLAUDE.md` corpus is excluded because rule
    // prose may legitimately reference the removed skill in
    // historical context (commit messages, change-log entries).
    let scan_paths: Vec<PathBuf> = vec![
        common::skills_dir(),
        common::docs_dir().join("skills"),
        common::repo_root().join("docs").join("phases"),
        common::repo_root().join("README.md"),
    ];
    const FORBIDDEN: &str = "flow-decompose-project";
    let mut violations: Vec<String> = Vec::new();
    for path in scan_paths {
        scan_for_substring(&path, FORBIDDEN, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Found {} reference(s) to the removed `flow-decompose-project` skill (PR #1694 removed it):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

/// Helper for the SKILL.md byte-substring tombstones — walks a path
/// (file or directory) and pushes every line that contains the
/// forbidden substring into `out`. Directories are walked
/// recursively; non-text files are skipped silently. Used by
/// `test_skills_no_flow_decompose_project` and
/// `test_skills_no_loop_token`.
fn scan_for_substring(path: &Path, needle: &str, out: &mut Vec<String>) {
    if path.is_file() {
        if let Ok(content) = fs::read_to_string(path) {
            for (lineno, line) in content.lines().enumerate() {
                if line.contains(needle) {
                    out.push(format!("{}:{} {}", path.display(), lineno + 1, line.trim()));
                }
            }
        }
        return;
    }
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            scan_for_substring(&entry.path(), needle, out);
        }
    }
}

// --- /loop skill removal (PR #1728) ---
//
// flow-start (locked→/loop), flow-release (null CI→/loop), and a
// stale flow-complete Rules note all invoked the `loop` skill to
// re-run a phase on a timer. With start-init and wait-for-release-ci
// now blocking internally on a bounded cap, the `loop`-skill external
// plugin dependency is removed from every SKILL.md. This tombstone
// catches a merge conflict or accidental edit that re-introduces the
// invocation token in any skill.

/// Tombstone: removed in PR #1728. The `loop` skill is no longer
/// invoked from any SKILL.md — start-init and wait-for-release-ci
/// block internally, and the invoking skills re-run a single line on
/// cap-exhaustion. Neither the slash-command token nor the
/// backtick-quoted skill reference must reappear in any `skills/` or
/// `.claude/skills/` SKILL.md. Bare-word "loop" (loop-guard, "Do not
/// loop or retry", "loop back through CI") carries neither token and
/// is intentionally not matched.
///
/// Stability argument: the protected targets are two literal tokens
/// in the Markdown SKILL.md corpus. (a) `concat!` — N/A for Markdown
/// prose, which has no string-assembly construct; (b) `format!` —
/// N/A for Markdown; (c) split constant — N/A for Markdown; (d)
/// `.arg()` split — N/A for Markdown. A Markdown author cannot
/// synthesize either token without writing the literal bytes, so the
/// byte-substring scan is exhaustive for this corpus. Re-introduction
/// in a NEW skill is covered because the scan walks every file under
/// both skill roots, not a fixed file list.
#[test]
fn test_skills_no_loop_token() {
    let scan_paths: Vec<PathBuf> = vec![
        common::skills_dir(),
        common::repo_root().join(".claude").join("skills"),
    ];
    // The `loop` skill is reached two ways: as a slash command and as
    // a backtick-quoted skill reference. Both tokens are forbidden;
    // bare-word "loop" is not. The constants are built via `concat!`
    // so this test file (scanned by the rust-source backward-comment
    // tombstone, not by this scan) does not itself carry the literal
    // slash-token on a source line.
    let forbidden_slash = concat!("/", "loop");
    let forbidden_backtick = concat!("`", "loop", "`");
    let mut violations: Vec<String> = Vec::new();
    for path in &scan_paths {
        scan_for_substring(path, forbidden_slash, &mut violations);
        scan_for_substring(path, forbidden_backtick, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Found {} `loop`-skill invocation token(s) in SKILL.md (PR #1728 removed every loop-skill site):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

// --- create-sub-issue removal (PR #1694 supersession) ---
//
// `bin/flow create-sub-issue` was invoked only by
// `flow-decompose-project` Step 5. With `flow-decompose-project`
// removed in this same PR (AC#6 of issue #1590), the
// `create-sub-issue` subcommand has no callers and is orphan
// infrastructure per `.claude/rules/supersession.md`. The
// `src/create_sub_issue.rs` module, its tests, its dispatch arm,
// and every doc reference are removed. This tombstone catches a
// merge conflict or accidental edit that re-introduces the module
// or the subcommand name.

/// Tombstone: removed in PR #1694. The `src/create_sub_issue.rs`
/// module and the `create-sub-issue` CLI subcommand are gone —
/// the GitHub native blocked-by dependency graph (set via
/// `bin/flow link-blocked-by`) is the surviving relationship
/// mechanism. Must not return to `src/`, `tests/`, `README.md`,
/// `docs/skills/`, or any catalog row.
///
/// Stability argument: the protected targets are a Rust source
/// file path AND a CLI subcommand name. (a) `concat!` — a Rust
/// author could `concat!("create_sub", "_issue")` but the
/// file-existence assertion (`src/create_sub_issue.rs` absent)
/// blocks the actual file from being committed; (b) `format!` —
/// same, defeated by the file-existence half; (c) split constant
/// — same; (d) `.arg()` split — clap subcommand registration
/// cannot be reassembled across multiple `.arg()` calls and the
/// file-existence half catches the underlying module file
/// regardless. The file-existence assertion is load-bearing; the
/// byte-substring assertion is the secondary guard against doc/
/// catalog resurrection.
#[test]
fn test_src_no_create_sub_issue() {
    let module_path = common::repo_root().join("src").join("create_sub_issue.rs");
    assert!(
        !module_path.exists(),
        "src/create_sub_issue.rs must not exist — the module was removed in PR #1694 (orphaned after flow-decompose-project deletion)"
    );

    let test_path = common::repo_root()
        .join("tests")
        .join("create_sub_issue.rs");
    assert!(
        !test_path.exists(),
        "tests/create_sub_issue.rs must not exist — the test module was removed in PR #1694 alongside its src/ counterpart"
    );

    // Byte-substring scan across the source/doc surfaces. The
    // module name `create_sub_issue` (Rust snake_case) and the
    // subcommand name `create-sub-issue` (CLI kebab-case) are
    // both forbidden.
    const FORBIDDEN_SUBSTRINGS: &[&str] = &["create_sub_issue", "create-sub-issue"];
    let scan_paths: Vec<PathBuf> = vec![
        common::repo_root().join("src"),
        common::docs_dir().join("skills"),
        common::repo_root().join("README.md"),
        common::skills_dir().join("flow-skills"),
    ];
    let mut violations: Vec<String> = Vec::new();
    for needle in FORBIDDEN_SUBSTRINGS {
        for path in &scan_paths {
            scan_for_substring(path, needle, &mut violations);
        }
    }
    assert!(
        violations.is_empty(),
        "Found {} reference(s) to the removed create_sub_issue module / create-sub-issue subcommand (PR #1694 removed both):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

// --- Weak-coverage prose loophole closure ---
//
// Weak-coverage language ("adequate test coverage", "adequately tested")
// is the prose surface through which a reviewer or reviewer agent could
// justify shipping below 100% coverage. The 100% gate in `bin/test`
// (`--fail-under-*` gate) and `.claude/rules/no-waivers.md` are the
// load-bearing mechanisms; this scanner prevents the prose from drifting
// back in via merge conflict or accidental edit. Scope is intentionally
// narrow: agent reports, skill instructions, and public docs — the
// surfaces where the phrases would license below-100% shipping. The
// `.claude/rules/` and `CLAUDE.md` corpus is excluded because those
// files discuss the coverage discipline and may legitimately cite the
// forbidden phrases. The `tests/` corpus is excluded because this
// scanner file contains the phrases as search input.

/// Weak-coverage phrases that must not reappear in the user-facing
/// prose corpus. Re-introducing either phrase would let a reviewer
/// agent cite "adequate"/"adequately" coverage as grounds for
/// shipping below 100%, defeating the `--fail-under-*` gate in
/// `bin/test` and the `.claude/rules/no-waivers.md` discipline.
const WEAK_COVERAGE_PHRASES: &[&str] = &["adequate test coverage", "adequately tested"];

/// Scan scope for the weak-coverage check. Only `agents/`, `skills/`,
/// and `docs/` are scanned — those are the prose surfaces where the
/// forbidden phrases would license below-100% shipping. `.claude/rules/`
/// and `CLAUDE.md` legitimately discuss the coverage discipline, and
/// `tests/tombstones.rs` contains the phrases as search input. None of
/// those paths fall under the scan directories, so the scanner cannot
/// reach its own literals.
const WEAK_COVERAGE_SCAN_DIRS: &[&str] = &["agents", "skills", "docs"];

/// Normalize prose for the weak-coverage scan: ASCII-lowercase plus
/// whitespace collapse (any run of whitespace — spaces, tabs, newlines,
/// non-breaking spaces — becomes a single ASCII space). This catches
/// case variants ("Adequate test coverage"), interior whitespace
/// variants ("adequate  test coverage", tab-separated, non-breaking
/// space), and line-spanning matches where Markdown word-wrap puts
/// the forbidden phrase on two lines. Per
/// `.claude/rules/tombstone-tests.md` "Assertion Strength" and
/// `.claude/rules/security-gates.md` "Normalize Before Comparing",
/// both sides of the comparison must be normalized.
fn normalize_for_weak_coverage_scan(s: &str) -> String {
    s.to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_prose_corpus_no_weak_coverage_language() {
    let root = common::repo_root();
    let normalized_phrases: Vec<String> = WEAK_COVERAGE_PHRASES
        .iter()
        .map(|p| normalize_for_weak_coverage_scan(p))
        .collect();
    let mut violations: Vec<String> = Vec::new();
    for dir in WEAK_COVERAGE_SCAN_DIRS {
        let dir_path = root.join(dir);
        for (rel, content) in common::collect_md_files(&dir_path) {
            let normalized = normalize_for_weak_coverage_scan(&content);
            for (orig, normalized_phrase) in
                WEAK_COVERAGE_PHRASES.iter().zip(normalized_phrases.iter())
            {
                if normalized.contains(normalized_phrase.as_str()) {
                    violations.push(format!("{}/{} — {}", dir, rel, orig));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Weak-coverage language found in prose corpus \
         (see .claude/rules/no-waivers.md and issue #1195):\n\n{}",
        violations.join("\n")
    );
}

// Stale tombstones for PR #1176, PR #1154, PR #1258, PR #1344, and
// PR #1375 removed — each PR merged before the oldest open PR was
// created, so no active branch can resurrect the deleted code via
// merge conflict. The structural scanner
// `source_contains_pub_fn_run_with_process_exit` and its unit test
// module `source_scanner_tests` were also removed as orphaned
// helpers.
//
// PR #1176: format_complete_summary, format_issues_summary,
//   format_pr_timings — pub fn run wrappers replaced by run_impl_main
// PR #1154: TUI refactor — run_terminal, activate_iterm_tab, open_url,
//   find_bin_flow, module-level run, atty_check removed
// PR #1258: branch-scoped state-file layout moved from
//   `.flow-states/<branch>-<purpose>.<ext>` to
//   `.flow-states/<branch>/<purpose>.<ext>`; the
//   `test_no_flat_layout_format_in_rust_source` scanner is no
//   longer needed once the branch-cutoff window passed
// PR #1344: flow-qa maintainer skill, the four backing Rust modules
//   (qa_mode, qa_reset, qa_verify, scaffold_qa), the qa/templates/
//   directory, the Commands::Qa* clap variants, and the
//   `Bash(rm -rf *.qa-repos*)` allow-list entry. The maintainer QAs
//   locally via --plugin-dir instead.
// PR #1383: Phase 2 (Plan) lifecycle phase, the flow-plan skill,
//   plan_extract / plan_check / scanner sources, plan_step state
//   fields, FlowPlan enum variant, scanner rule files, and the
//   Phase 2 docs page. Plans now travel inside issue bodies via
//   the FLOW-PLAN-BEGIN / FLOW-PLAN-END sentinels extracted by
//   bin/flow plan-from-issue at flow-start.
// PR #1389: flow-status skill replaced by `bin/flow status` Rust
//   subcommand. The four byte-substring + file-existence
//   tombstones guarding `flow:flow-status` invocation surface and
//   the SKILL.md / docs/skills/flow-status.md files are stale —
//   no active branch can resurrect them.

// --- scan_naming_violations ---
//
// Pure helper used by `test_tombstones_no_naming_violations` to enforce
// the tombstone naming convention from `.claude/rules/tombstone-tests.md`
// "Naming Convention". Walks `#[test] fn <name>(` declarations in the
// supplied content and flags any name that does not match the regex
// `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$` (the literal form of the
// `test_<scope>_no_<removed_thing>` pattern). Names listed in the
// `exclusions` slice are skipped — used by the contract test for the
// two contract tests themselves whose names are part of the rule's
// own implementation rather than tombstones.
//
// The walk regex tolerates zero or more intervening attributes
// between `#[test]` and `fn` so an author cannot bypass naming
// enforcement by stacking a second attribute (such as
// should_panic or other test-runner directives). The walk does
// NOT distinguish `#[test]` inside raw string literals from a
// real attribute — fixtures that need to emit the literal in
// source must use the `concat!` escape (see
// `tests/test_placement.rs::src_contains_no_inline_cfg_test_blocks`
// for the canonical pattern).

fn scan_naming_violations(content: &str, exclusions: &[&str]) -> Vec<String> {
    let test_fn_re =
        Regex::new(r"#\[test\](?:\s+#\[\w+(?:\([^)]*\))?\])*\s+fn\s+(\w+)\s*\(").unwrap();
    let name_re = Regex::new(r"^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$").unwrap();
    let mut violations = Vec::new();
    for cap in test_fn_re.captures_iter(content) {
        let m = cap.get(1).unwrap();
        let name = m.as_str();
        if exclusions.contains(&name) {
            continue;
        }
        if !name_re.is_match(name) {
            let offset = m.start();
            let line = content[..offset].matches('\n').count() + 1;
            violations.push(format!(
                "line {}: {} — must match `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$`",
                line, name
            ));
        }
    }
    violations
}

#[test]
fn test_scanner_no_violations_for_conformant_names() {
    let fixture = concat!(
        "#[",
        "test",
        "]\nfn test_foo_no_bar() {}\n",
        "#[",
        "test",
        "]\nfn test_root_no_test_coverage_md_file() {}\n",
    );
    let violations = scan_naming_violations(fixture, &[]);
    assert!(
        violations.is_empty(),
        "expected no violations for conformant names: {:?}",
        violations
    );
}

#[test]
fn test_scanner_no_false_negative_for_missing_test_prefix() {
    let fixture = concat!("#[", "test", "]\nfn missing_prefix_no_test() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(
        violations[0].contains("missing_prefix_no_test"),
        "violation should name the offender: {}",
        violations[0]
    );
}

#[test]
fn test_scanner_no_false_negative_for_missing_no_segment() {
    let fixture = concat!("#[", "test", "]\nfn test_something_must_not_exist() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(violations[0].contains("test_something_must_not_exist"));
}

#[test]
fn test_scanner_no_false_negative_for_test_no_prefix_only() {
    let fixture = concat!("#[", "test", "]\nfn test_no_scope_segment() {}\n",);
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation: {:?}",
        violations
    );
    assert!(violations[0].contains("test_no_scope_segment"));
}

#[test]
fn test_scanner_no_violations_for_excluded_names() {
    let fixture = concat!("#[", "test", "]\nfn nonconformant_excluded_name() {}\n",);
    let violations = scan_naming_violations(fixture, &["nonconformant_excluded_name"]);
    assert!(
        violations.is_empty(),
        "excluded name should be skipped: {:?}",
        violations
    );
}

#[test]
fn test_scanner_no_violations_for_non_test_fns() {
    let fixture = "fn helper_function() {}\nfn another_plain_fn() {}\n";
    let violations = scan_naming_violations(fixture, &[]);
    assert!(
        violations.is_empty(),
        "plain fn declarations without #[test] should be ignored: {:?}",
        violations
    );
}

// --- Learn phase removal ---
//
// The Learn phase (Phase 4) and its `flow-learn` skill plus the
// cognitively-isolated `learn-analyst` agent were removed. These
// tombstones lock the removal in: a merge conflict resolution that
// re-introduces any of the deleted files, the `Phase::FlowLearn`
// enum variant, the `learn_steps_total` state field, the `flow-learn`
// config axis, or the `Agent(flow:learn-analyst)` / `Skill(flow:flow-learn)`
// allow entries fails CI immediately.

/// Tombstone: removed in PR #1897. The `flow-learn` SKILL.md was
/// deleted alongside the Learn phase. Must not return. File-existence
/// check pairs with the source-content tombstones below so a
/// `#[path = "..."]` re-import cannot resurrect the skill unseen.
#[test]
fn test_skills_no_flow_learn_skill_file() {
    assert!(
        !common::skills_dir()
            .join("flow-learn")
            .join("SKILL.md")
            .exists(),
        "skills/flow-learn/SKILL.md must not exist — the Learn phase was removed"
    );
}

/// Tombstone: removed in PR #1897. The `learn-analyst` agent was
/// deleted alongside the Learn phase. Must not return.
#[test]
fn test_agents_no_learn_analyst_agent_file() {
    assert!(
        !common::agents_dir().join("learn-analyst.md").exists(),
        "agents/learn-analyst.md must not exist — the Learn phase was removed"
    );
}

/// Tombstone: removed in PR #1897. The `Phase::FlowLearn` enum variant
/// is gone from `src/state.rs`. Must not return.
///
/// Stability argument: `FlowLearn` is a Rust enum-variant identifier
/// that must appear verbatim in source to compile — it cannot be
/// produced by `concat!`, `format!`, or a named constant reference,
/// so the byte-substring check is sufficient.
#[test]
fn test_state_no_flow_learn_enum_variant() {
    let content = fs::read_to_string(common::repo_root().join("src").join("state.rs"))
        .expect("src/state.rs must exist");
    assert!(
        !content.contains("FlowLearn"),
        "src/state.rs must not declare the `Phase::FlowLearn` variant"
    );
}

/// Tombstone: removed in PR #1897. The `learn_steps_total` state field
/// is gone from `src/state.rs`. Must not return.
///
/// Stability argument: `learn_steps_total` is a Rust struct-field
/// identifier that must appear verbatim in source to compile — no
/// `concat!`, `format!`, or named constant can synthesize a struct
/// field name, so the byte-substring check is sufficient.
#[test]
fn test_state_no_learn_steps_total_field() {
    let content = fs::read_to_string(common::repo_root().join("src").join("state.rs"))
        .expect("src/state.rs must exist");
    assert!(
        !content.contains("learn_steps_total"),
        "src/state.rs must not declare the `learn_steps_total` field"
    );
}

/// Tombstone: removed in PR #1897. The `flow-learn` config axis is gone
/// from `flow-phases.json`. Must not return.
///
/// Stability argument: `flow-phases.json` is a flat JSON byte stream
/// with no `concat!` macro, no `format!` interpolation, and no named
/// constant references — the `flow-learn` key can only reappear as the
/// exact bytes a merge conflict resurrects, which this scanner catches.
#[test]
fn test_phases_json_no_flow_learn_axis() {
    let content = fs::read_to_string(common::repo_root().join("flow-phases.json"))
        .expect("flow-phases.json must exist");
    assert!(
        !content.contains("flow-learn"),
        "flow-phases.json must not reference the `flow-learn` config axis"
    );
}

/// Tombstone: removed in PR #1897. The `Agent(flow:learn-analyst)` and
/// `Skill(flow:flow-learn)` allow entries are gone from
/// `src/prime_check.rs::UNIVERSAL_ALLOW`. Must not return.
///
/// Stability argument: the two entries are string literals in a Rust
/// `&[&str]` constant. A reintroduction by `concat!` or `format!`
/// would not produce a compile-time `&'static str` array element, and
/// no named constant aliases them, so the byte-substring check over
/// the source is sufficient.
#[test]
fn test_prime_check_no_learn_analyst_allow_entry() {
    let content = fs::read_to_string(common::repo_root().join("src").join("prime_check.rs"))
        .expect("src/prime_check.rs must exist");
    assert!(
        !content.contains("Agent(flow:learn-analyst)"),
        "src/prime_check.rs must not list the `Agent(flow:learn-analyst)` allow entry"
    );
    assert!(
        !content.contains("Skill(flow:flow-learn)"),
        "src/prime_check.rs must not list the `Skill(flow:flow-learn)` allow entry"
    );
}

// --- test_tombstones_no_naming_violations ---
//
// Contract test enforcing the tombstone naming convention against
// the live `tests/tombstones.rs` source. Reads the file at runtime
// and asserts every `#[test] fn` declaration matches
// `^test_[a-z][a-z0-9_]*_no_[a-z][a-z0-9_]*$`. The two contract
// tests themselves are excluded because their names are part of the
// rule's own implementation rather than tombstones — they enforce
// the conventions but do not assert a removal.

#[test]
fn test_tombstones_no_naming_violations() {
    let root = common::repo_root();
    let path = root.join("tests").join("tombstones.rs");
    let content = fs::read_to_string(&path).expect("tests/tombstones.rs must exist");
    let exclusions: &[&str] = &[
        "test_tombstones_no_naming_violations",
        "test_tombstones_no_stability_docs_violations",
    ];
    let violations = scan_naming_violations(&content, exclusions);
    assert!(
        violations.is_empty(),
        "Tombstone naming convention violations \
         (see .claude/rules/tombstone-tests.md `Naming Convention`):\n\n{}",
        violations.join("\n")
    );
}

// --- scan_stability_docs_violations ---
//
// Pure helper used by `test_tombstones_no_stability_docs_violations` to
// enforce the literal-tombstone stability checklist from
// `.claude/rules/tombstone-tests.md` "Literal tombstones — stability
// checklist". For every `#[test] fn` whose body (between the function's
// matching braces) contains a `.contains(` call (the byte-substring
// shape) AND whose preceding `///` doc block carries one or more
// `Tombstone:.*?PR #N` markers with the highest N at or above the
// sentinel PR, the helper checks the doc block for a stability
// argument — case-insensitive match on the macro forms `concat!` or
// `format!`, or the substring `constant`. A tombstone above the
// sentinel that uses a byte-substring assertion without at least one
// of those keywords in its doc block is a violation.
//
// Edge cases handled:
//
// - Body extraction tracks brace depth so the `.contains(` check
//   only sees the function's actual body, not interstitial helpers
//   or the next test's preceding doc block.
// - The walk regex tolerates zero or more intervening attributes
//   between `#[test]` and `fn` so a stacked second attribute
//   cannot bypass enforcement.
// - The doc-block walker tolerates one or more blank lines between
//   the `///` block and the `#[test]` attribute; rustdoc still
//   attaches the doc block across one blank line.
// - When multiple `Tombstone:.*?PR #N` markers appear in the same
//   doc block, the highest PR number wins (so an in-scope marker
//   stacked second cannot be hidden by a stale below-sentinel
//   marker stacked first).
// - PR-number parse failure (overflow beyond `u32::MAX`) fails
//   closed: the marker is treated as in-scope per
//   `.claude/rules/security-gates.md` "Fail Closed When State Is
//   Unreliable".
//
// Known-fuzzy keyword: the `constant` substring may match prose
// containing words like `constant-time` or `constants`. Authors who
// invoke `concat!` and `format!` in their stability argument
// (the canonical first two checklist items) trigger the more-
// specific macro-form keywords and avoid the fuzzy substring
// surface entirely.
//
// The sentinel scopes enforcement to tombstones at or above
// `STABILITY_DOCS_SENTINEL_PR`. Tombstones below the sentinel are
// out of scope — retrofitting `///` blocks onto every existing
// byte-substring tombstone would expand the diff far past the
// rule the contract test enforces forward.

/// Sentinel PR number for `test_tombstones_no_stability_docs_violations`.
///
/// Tombstones whose `Tombstone:.*?PR #N` marker has N at or above this
/// value MUST carry a `///` doc block that mentions `concat!`,
/// `format!`, or `constant` (case-insensitive). Tombstones below the
/// sentinel predate the stability-docs requirement and are out of
/// scope; the contract test does not retroactively flag them.
///
/// When raising the sentinel — typically after a campaign that
/// retrofits `///` blocks onto older byte-substring tombstones —
/// update the value here and verify every newly-in-scope tombstone
/// passes the contract test before committing.
const STABILITY_DOCS_SENTINEL_PR: u32 = 1397;

fn scan_stability_docs_violations(
    content: &str,
    sentinel_pr: u32,
    exclusions: &[&str],
) -> Vec<String> {
    let test_fn_re =
        Regex::new(r"#\[test\](?:\s+#\[\w+(?:\([^)]*\))?\])*\s+fn\s+(\w+)\s*\(").unwrap();
    let tombstone_re = Regex::new(r"Tombstone:.*?PR #(\d+)").unwrap();
    let mut violations = Vec::new();

    for cap in test_fn_re.captures_iter(content) {
        let name = cap.get(1).unwrap().as_str();
        if exclusions.contains(&name) {
            continue;
        }

        // Extract the function body by tracking brace depth from the
        // first `{` after the signature. This narrows the .contains(
        // check to the function's actual body rather than stretching
        // through interstitial code or the next test's preceding doc
        // block.
        let after_sig = cap.get(0).unwrap().end();
        let body = match extract_fn_body(content, after_sig) {
            Some(b) => b,
            None => continue,
        };
        if !body.contains(".contains(") {
            continue;
        }

        // Walk preceding lines for the `///` doc block. Tolerate one
        // or more blank lines between the doc block and `#[test]` —
        // rustdoc still attaches the doc block.
        let test_start = cap.get(0).unwrap().start();
        let preceding = &content[..test_start];
        let mut doc_lines: Vec<&str> = Vec::new();
        let mut iter = preceding.lines().rev().peekable();
        while iter.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
            iter.next();
        }
        for line in iter {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") {
                doc_lines.push(line);
            } else {
                break;
            }
        }
        doc_lines.reverse();
        let doc_block = doc_lines.join("\n");

        // Collect all PR # markers and use the highest. A stacked
        // below-sentinel marker cannot hide a co-located above-
        // sentinel marker. Parse failure (overflow beyond u32::MAX)
        // fails closed: treat as in-scope.
        let mut max_pr: Option<u32> = None;
        for c in tombstone_re.captures_iter(&doc_block) {
            let parsed: u32 = c.get(1).unwrap().as_str().parse().unwrap_or(u32::MAX);
            max_pr = Some(max_pr.map_or(parsed, |m| m.max(parsed)));
        }
        let pr_num = match max_pr {
            Some(n) => n,
            None => continue, // no Tombstone marker in doc block
        };
        if pr_num < sentinel_pr {
            continue;
        }

        let lower = doc_block.to_lowercase();
        let has_keyword =
            lower.contains("concat!") || lower.contains("format!") || lower.contains("constant");
        if !has_keyword {
            let line = content[..test_start].matches('\n').count() + 1;
            violations.push(format!(
                "line {}: {} (PR #{}) — `///` doc block missing stability keyword (concat!/format!/constant)",
                line, name, pr_num
            ));
        }
    }
    violations
}

/// Extract the body of a `fn` declaration starting at `after_sig`
/// (the byte offset right after the closing `)` of the function
/// signature). Returns the body slice including the outer braces, or
/// `None` if no opening brace follows or the braces are unbalanced.
///
/// Brace counting does not interpret string literals or comments —
/// curly braces inside `"..."` would skew the depth. For Rust test
/// bodies in `tests/tombstones.rs` (which contain regex literals,
/// concat! fixtures, and assert! calls but not raw `}` characters in
/// strings outside macros), the simple counter is accurate.
fn extract_fn_body(content: &str, after_sig: usize) -> Option<&str> {
    let opening = after_sig + content[after_sig..].find('{')?;
    let mut depth: i32 = 0;
    for (idx, byte) in content[opening..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[opening..=opening + idx]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn test_stability_scanner_no_violations_for_doc_with_concat_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// The literal is stable per the concat! analysis: the\n",
        "/// runtime resolver reads a fixed identifier.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "doc block with `concat` keyword should pass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_missing_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "expected 1 violation for doc block missing keyword: {:?}",
        violations
    );
    assert!(violations[0].contains("test_x_no_y"));
    assert!(violations[0].contains("1500"));
}

#[test]
fn test_stability_scanner_no_violations_for_below_sentinel_pr() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #100. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "PR below sentinel is out of scope: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_test_without_contains_call() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    assert!(!path.exists());\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "test without .contains( body should be ignored: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_excluded_names() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// No stability keyword here.\n",
        "#[",
        "test",
        "]\nfn test_excluded_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &["test_excluded_no_check"]);
    assert!(
        violations.is_empty(),
        "excluded name should be skipped: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_uppercase_keyword_variants() {
    let fixture_upper = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Argued via FORMAT! reassembly check.\n",
        "#[",
        "test",
        "]\nfn test_uppercase_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture_upper, 1397, &[]);
    assert!(
        violations.is_empty(),
        "uppercase FORMAT should match case-insensitively: {:?}",
        violations
    );

    let fixture_mixed = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Argued via Constant declaration check.\n",
        "#[",
        "test",
        "]\nfn test_mixed_case_no_check() {\n",
        "    let s = \"foo\";\n",
        "    s.contains(\"foo\");\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture_mixed, 1397, &[]);
    assert!(
        violations.is_empty(),
        "mixed-case Constant should match case-insensitively: {:?}",
        violations
    );
}

#[test]
fn test_naming_scanner_no_false_negative_for_intervening_attribute() {
    let fixture = concat!(
        "#[",
        "test",
        "]\n#[",
        "ignore",
        "]\n",
        "fn nonconformant_with_ignore_attr() {}\n",
    );
    let violations = scan_naming_violations(fixture, &[]);
    assert_eq!(
        violations.len(),
        1,
        concat!(
            "intervening #[",
            "ignore",
            "] should not bypass naming check: {:?}"
        ),
        violations
    );
    assert!(violations[0].contains("nonconformant_with_ignore_attr"));
}

#[test]
fn test_stability_scanner_no_false_negative_for_overflow_pr_number() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #99999999999999999999. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "overflow PR # should fail closed (treated as in-scope): {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_blank_line_before_test() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "blank line between doc and #[test] should not bypass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_path_existence_test_adjacent_to_substring_test() {
    // Two adjacent tests: the first is a path-existence tombstone
    // (no `.contains(` in its body); the second is a byte-substring
    // tombstone with a valid stability argument. The first should
    // not be misclassified by leakage of `.contains(` from the
    // second's preceding doc block.
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "#[",
        "test",
        "]\nfn test_a_no_subdir() {\n",
        "    assert!(!path.exists());\n",
        "}\n",
        "\n",
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "/// Stable per concat! analysis.\n",
        "#[",
        "test",
        "]\nfn test_b_no_invocation() {\n",
        "    let content = read();\n",
        "    assert!(!content.contains(\"forbidden\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "path-existence tombstone should not be misclassified by adjacent test: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_violations_for_doc_with_format_macro_keyword() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// The literal is stable per format! analysis.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert!(
        violations.is_empty(),
        "doc block with `format!` macro keyword should pass: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_doc_with_format_status_only() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// Bare `flow-status` is not scanned because it is a substring of `format-status`.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "incidental `format-status` substring without `format!` macro should violate: {:?}",
        violations
    );
}

#[test]
fn test_stability_scanner_no_false_negative_for_first_marker_below_when_second_above_sentinel() {
    let fixture = concat!(
        "/// ",
        "Tombstone: removed in PR #100. Must not return.\n",
        "/// ",
        "Tombstone: removed in PR #1500. Must not return.\n",
        "///\n",
        "/// File-existence guard with no stability argument.\n",
        "#[",
        "test",
        "]\nfn test_x_no_y() {\n",
        "    let s = \"foo\";\n",
        "    assert!(s.contains(\"foo\"));\n",
        "}\n",
    );
    let violations = scan_stability_docs_violations(fixture, 1397, &[]);
    assert_eq!(
        violations.len(),
        1,
        "highest PR # in doc block should determine scope (not first): {:?}",
        violations
    );
}

// --- test_tombstones_no_stability_docs_violations ---
//
// Contract test enforcing the literal-tombstone stability checklist
// against the live `tests/tombstones.rs` source. Reads the file at
// runtime, calls `scan_stability_docs_violations` with the sentinel
// PR (`STABILITY_DOCS_SENTINEL_PR`) and the contract-test exclusion
// list, and asserts the violations vector is empty. Existing
// tombstones with PR #N below the sentinel are out of scope; new
// tombstones at or above the sentinel must carry a `///` doc block
// with at least one of the stability keywords.

#[test]
fn test_tombstones_no_stability_docs_violations() {
    let root = common::repo_root();
    let path = root.join("tests").join("tombstones.rs");
    let content = fs::read_to_string(&path).expect("tests/tombstones.rs must exist");
    let exclusions: &[&str] = &[
        "test_tombstones_no_naming_violations",
        "test_tombstones_no_stability_docs_violations",
    ];
    let violations =
        scan_stability_docs_violations(&content, STABILITY_DOCS_SENTINEL_PR, exclusions);
    assert!(
        violations.is_empty(),
        "Literal-tombstone stability checklist violations \
         (see .claude/rules/tombstone-tests.md \
         `Literal tombstones — stability checklist`):\n\n{}",
        violations.join("\n")
    );
}

// --- flow-triage-issue disposition reduction (PR #1401) ---

/// Tombstone: removed in PR #1401. Must not return.
///
/// Source-content scan over skills/flow-triage-issue/SKILL.md for the
/// literal `keep-open`. The literal is stable per the four-question
/// checklist in `.claude/rules/tombstone-tests.md`:
///
/// 1. concat!: not applicable — markdown files contain no Rust macros.
/// 2. format!: same — no runtime reassembly in markdown.
/// 3. split constants: same.
/// 4. .arg() chains: same.
///
/// The hyphenated form `keep-open` distinguishes it from prose like
/// "keep open" or "keep this open"; only the disposition token uses
/// the hyphen.
#[test]
fn test_tombstones_no_keep_open_in_skill() {
    let root = common::repo_root();
    let skill_path = root
        .join("skills")
        .join("flow-triage-issue")
        .join("SKILL.md");
    let skill =
        fs::read_to_string(&skill_path).expect("skills/flow-triage-issue/SKILL.md must exist");
    assert!(
        !skill.contains("keep-open"),
        "skills/flow-triage-issue/SKILL.md must not contain `keep-open` — disposition removed in PR #1401"
    );
}

/// Tombstone: removed in PR #1401. Must not return.
///
/// Source-content scan over skills/flow-triage-issue/SKILL.md for the
/// literal `fix-now`. Same stability argument as the `keep-open`
/// tombstone above: markdown files have no concat!/format!/constant/
/// .arg reassembly paths, and the hyphenated form distinguishes the
/// disposition token from prose like "fix now."
#[test]
fn test_tombstones_no_fix_now_in_skill() {
    let root = common::repo_root();
    let skill_path = root
        .join("skills")
        .join("flow-triage-issue")
        .join("SKILL.md");
    let skill =
        fs::read_to_string(&skill_path).expect("skills/flow-triage-issue/SKILL.md must exist");
    assert!(
        !skill.contains("fix-now"),
        "skills/flow-triage-issue/SKILL.md must not contain `fix-now` — disposition removed in PR #1401"
    );
}

/// Tombstone: agents/issue-triage.md was deleted in PR #1699 when the
/// triage Process was inlined directly into
/// skills/flow-triage-issue/SKILL.md. The agent file's content (5-step
/// Process, 10-question lens, verdict-card format, Disposition
/// Semantics, Reasoning Discipline, Framing Challenges, Hard Rules)
/// was ported into the SKILL.md as steps 3-7 plus updated Hard Rules.
///
/// This is a file-existence tombstone paired with the byte-substring
/// tombstones above, per `.claude/rules/tombstone-tests.md` "Two kinds
/// of tombstone": when the deletion target includes a source file, the
/// file-existence check catches resurrection regardless of how the
/// file is later imported (e.g. via `#[path = "..."] mod`).
#[test]
fn test_tombstones_no_issue_triage_agent_file() {
    let root = common::repo_root();
    let agent_path = root.join("agents").join("issue-triage.md");
    assert!(
        !agent_path.exists(),
        "agents/issue-triage.md must not exist — inlined into skills/flow-triage-issue/SKILL.md in PR #1699"
    );
}

// --- FlowPaths::new constructor deletion (PR #1395) ---

/// Tombstone: removed in PR #1395. Must not return.
///
/// `FlowPaths::new` was a panicking constructor (`assert!(is_valid_branch)`)
/// that produced production incidents whenever a caller drifted from
/// `try_new`. The constructor was deleted entirely; `try_new` is the
/// only constructor on `FlowPaths`. Resurrection — through merge
/// conflict, refactor, or a new author copying the deleted shape —
/// must fail CI.
///
/// Source-content scan with literal `pub fn new(` against the
/// `FlowPaths` impl block in `src/flow_paths.rs`. Stability per
/// `.claude/rules/tombstone-tests.md` "Literal tombstones —
/// stability checklist":
///
/// 1. `concat!`: would have to assemble `pub fn new(` from fragments
///    inside the same impl block — possible in theory but the only
///    runtime effect of the function is path construction, which
///    `try_new` already provides; there is no incentive for a future
///    author to assemble the string indirectly.
/// 2. `format!`: cannot produce a `fn` definition — `format!` is a
///    runtime call.
/// 3. Split constants: same as `concat!` — only meaningful inside an
///    impl block, where the bounded scan below catches the literal.
/// 4. Method chains: not applicable to `fn` definitions.
///
/// The bounded-slice pattern from `.claude/rules/testing-gotchas.md`
/// "Subsection-Local Assertions in Contract Tests" scopes the scan
/// to the `impl FlowPaths {` block so unrelated `pub fn new(`
/// definitions on other types in the same file (`FlowStatesDir::new`)
/// do not produce false positives.
#[test]
fn test_flow_paths_no_new_constructor() {
    let root = common::repo_root();
    let path = root.join("src").join("flow_paths.rs");
    let content = fs::read_to_string(&path).expect("src/flow_paths.rs must exist");

    let tail_at_impl = content
        .split_once("impl FlowPaths {")
        .map(|(_, tail)| tail)
        .expect("FlowPaths impl block must exist in src/flow_paths.rs");
    let impl_block = tail_at_impl
        .split_once("\n}\n")
        .map(|(block, _)| block)
        .unwrap_or(tail_at_impl);

    assert!(
        !impl_block.contains("pub fn new("),
        "src/flow_paths.rs::FlowPaths impl must not contain `pub fn new(` — \
         the panicking constructor was deleted in PR #1395 and replaced by \
         `try_new`. Reintroduction defeats the compile-time invariant that \
         every callsite must handle invalid branches."
    );
}

// --- Flow label removal (PR #1408) ---

/// Tombstone: removed in PR #1408. Must not return.
///
/// Issue #1405 removed the redundant `Flow` label from the
/// `benkruger/flow` issue tracker. Every issue filed there is
/// already plugin-related — the label conveyed no information.
/// The tombstone scans every prose-corpus surface named in
/// `.claude/rules/docs-with-behavior.md` "Feature-Configurable
/// Prose Generalization" so a future contributor cannot resurrect
/// the flag in any documentation surface:
///
/// 1. `--label "Flow"` arguments in skill bash blocks
///    (`skills/**/SKILL.md`).
/// 2. `--label "Flow"` arguments in rule prose
///    (`.claude/rules/*.md`).
/// 3. `--label "Flow"` arguments in agent prompts (`agents/*.md`).
/// 4. `--label "Flow"` arguments in Jekyll/marketing docs
///    (`docs/**/*.md`).
/// 5. `--label "Flow"` arguments in `README.md`.
/// 6. `--label "Flow"` arguments in `CLAUDE.md`.
/// 7. `"Flow"` as a slice element in
///    `src/analyze_issues.rs::LABEL_CATEGORIES`.
///
/// Stability per `.claude/rules/tombstone-tests.md` "Literal
/// tombstones — stability checklist":
///
/// 1. `concat!`: not applicable to markdown files (no Rust
///    macros). For `LABEL_CATEGORIES` a future author could
///    reassemble `"Flow"` via `concat!("Fl", "ow")`, but the
///    bounded scan over the slice block plus the no-incentive
///    argument (the categorization removal would have to be
///    deliberately undone) makes this implausible.
/// 2. `format!`: cannot produce a slice element at compile
///    time, and markdown files have no runtime reassembly.
/// 3. Split constants: a future author could declare
///    `const FLOW_LABEL: &str = "Flow"` and reference it inside
///    the slice. The bounded `LABEL_CATEGORIES` scan would miss
///    the named-constant resurrection — but the resurrection
///    would also require re-introducing the const definition,
///    which is reviewable code and would fail the issue's
///    intent.
/// 4. Method chains: not applicable to slice literals or to
///    `--label "Flow"` invocations (the flag name and value are
///    one shell token sequence).
///
/// Bypasses considered and rejected:
///
/// - `--label 'Flow'` (single quotes): scanned alongside the
///   double-quoted form.
/// - `--label  "Flow"` (extra whitespace): markdown convention
///   normalizes whitespace to a single space; reviewer would
///   catch the deviation.
/// - `"Flow In-Progress"` and `"Flow"` substring collision:
///   sibling labels legitimately contain `Flow`. The scanner
///   matches only the exact `--label "Flow"` and `--label
///   'Flow'` argument forms, and the LABEL_CATEGORIES check
///   uses bounded-slice scanning over the named const block to
///   match `"Flow",` (with comma) — the entry is always
///   followed by a comma in slice literals.
/// - Markdown files committed with `.markdown` extension: the
///   directory walker filters on `.md` only. The codebase uses
///   `.md` exclusively; a `.markdown` file in any of the scanned
///   surfaces would be a pre-existing convention violation that
///   reviewer-agent inspection would catch independently.
#[test]
fn test_filing_no_flow_label() {
    let root = common::repo_root();

    // The six prose-corpus surfaces. Directory entries are scanned
    // recursively for `.md` files; single-file entries are read
    // directly so `README.md` and `CLAUDE.md` (at the repo root)
    // do not require walking the whole repo.
    let dir_surfaces: &[(&str, std::path::PathBuf)] = &[
        ("skills/**/SKILL.md", root.join("skills")),
        (".claude/rules/*.md", root.join(".claude").join("rules")),
        ("agents/*.md", root.join("agents")),
        ("docs/**/*.md", root.join("docs")),
    ];
    let file_surfaces: &[(&str, std::path::PathBuf)] = &[
        ("README.md", root.join("README.md")),
        ("CLAUDE.md", root.join("CLAUDE.md")),
    ];

    for (label, dir) in dir_surfaces {
        let hits = scan_for_flow_label_arg(dir, "md");
        assert!(
            hits.is_empty(),
            "{} must not contain `--label \"Flow\"` — \
             the redundant Flow label was removed in PR #1408. \
             Offending files: {:?}",
            label,
            hits
        );
    }

    for (label, path) in file_surfaces {
        if let Ok(content) = fs::read_to_string(path) {
            assert!(
                !content.contains("--label \"Flow\"") && !content.contains("--label 'Flow'"),
                "{} must not contain `--label \"Flow\"` — \
                 the redundant Flow label was removed in PR #1408.",
                label
            );
        }
    }
}

/// Recursively scans `dir` for files with the given extension and
/// returns the paths that contain `--label "Flow"` or
/// `--label 'Flow'` as a literal substring. Used by
/// [`test_filing_no_flow_label`].
fn scan_for_flow_label_arg(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return hits,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            hits.extend(scan_for_flow_label_arg(&path, ext));
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if content.contains("--label \"Flow\"") || content.contains("--label 'Flow'") {
            hits.push(path);
        }
    }
    hits
}

// --- Phase 3 renamed Code Review -> Review corpus tombstone ---
//
// Phase 3 was renamed from `Code Review` to `Review` for naming
// consistency across the 5-phase family. The corpus tombstone below
// asserts that NO tracked file in the working tree contains any of
// the nine forbidden case variants the rename eliminates. The
// corpus tombstone is the single gate that catches every drift
// surface: phase-identifier strings, display labels, Rust symbols,
// the `--override-code-review-ban` CLI flag, prose in skills/docs/
// rules, and any future surface.

/// Per-file byte cap on the corpus tombstone walk (10 MiB).
///
/// Bounds I/O so a committed generated artifact, golden fixture, or
/// hostile commit containing a multi-megabyte tracked file cannot
/// OOM the test process. Per
/// `.claude/rules/external-input-path-construction.md` "Enforce a
/// documented size cap on every external read" — applies to
/// filesystem walks that read each yielded file. 10 MiB is generous
/// for prose corpora; any tracked file beyond it is almost certainly
/// a generated artifact and silently skipping it is the correct
/// fail-open behavior for a tombstone scanner whose job is to flag
/// legitimate prose drift.
const CORPUS_TOMBSTONE_BYTE_CAP: u64 = 10 * 1024 * 1024;

/// Tombstone: removed in PR #1430. Must not return.
///
/// Asserts the corpus of tracked files contains none of the nine
/// forbidden case variants — `Code Review`, `Code-Review`,
/// `Code-review`, `code-review`, `Code_Review`, `code_review`,
/// `CODE_REVIEW`, `CODE-REVIEW`, `CodeReview` — anywhere in the
/// working tree. The walker invokes
/// `git ls-files` via `std::process::Command` so the scan respects
/// `.gitignore` and skips untracked artifacts (build output, IDE
/// scratch files, `.flow-states/`).
///
/// The forbidden literals are built via `concat!` so the search
/// strings do not themselves appear as plain substrings in this
/// source file — otherwise the scanner would flag itself before
/// the self-exclude logic could apply. The self-exclude is a
/// canonicalized-path comparison against the test file's own
/// path, matching the pattern used by
/// `test_rust_source_no_backward_facing_comments` above.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: the only known bypass is the same
///      `concat!` technique this test itself uses internally; any
///      future code that synthesizes the forbidden phrase via
///      `concat!` would still ship the runtime string into a
///      consumer that ultimately serializes it back into prose or
///      a config file, where this scanner would catch it on the
///      next CI run.
///   2. `format!` reassembly: same as above — runtime synthesis
///      surfaces in prose or config eventually.
///   3. Named constant reference: a `const PHASE_LABEL: &str =
///      concat!("Code", " ", "Review")` would centralize the
///      literal in a constant whose value still produces the
///      forbidden substring at compile time, but the constant
///      definition itself would appear in source as a `concat!`
///      call that does NOT trip the scanner. This is the one
///      acceptable carve-out and is the same technique this
///      test uses to embed the forbidden literals.
///   4. Method chains / split args: the substring check operates
///      on file content, not on source-level syntax, so any code
///      whose runtime effect is to emit one of the forbidden
///      fragments into a tracked file (a SKILL.md, a doc, a JSON
///      value) is caught regardless of how the source assembles
///      the value.
#[test]
fn test_corpus_no_old_code_review_identifiers() {
    use std::io::Read;

    let root = common::repo_root();
    let scanner_path = root
        .join("tests")
        .join("tombstones.rs")
        .canonicalize()
        .expect("scanner path must canonicalize");

    // Build forbidden literals via concat! so the source of this
    // test file itself does not contain the substrings being
    // searched for. The eight variants cover every case shape the
    // PR sweep needs to catch:
    //   1. Title case + space     — `Code Review` (prose, banners)
    //   2. Title case + hyphen    — `Code-Review` (markdown headings)
    //   3. Mixed case + hyphen    — `Code-review` (accidental case)
    //   4. lowercase + hyphen     — `code-review` (slugs, CLI flags)
    //   5. Title case + underscore — `Code_Review` (rare but possible)
    //   6. lowercase + underscore — `code_review` (Rust snake_case)
    //   7. ALL CAPS + underscore  — `CODE_REVIEW` (Rust consts)
    //   8. ALL CAPS + hyphen      — `CODE-REVIEW` (env vars, uppercase CLI)
    //   9. PascalCase no separator — `CodeReview` (Rust enum variants)
    let forbidden: [&str; 9] = [
        concat!("Code", " ", "Review"),
        concat!("Code", "-", "Review"),
        concat!("Code", "-", "review"),
        concat!("code", "-", "review"),
        concat!("Code", "_", "Review"),
        concat!("code", "_", "review"),
        concat!("CODE", "_", "REVIEW"),
        concat!("CODE", "-", "REVIEW"),
        concat!("Code", "Review"),
    ];

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files"])
        .output()
        .expect("git ls-files must succeed");
    assert!(
        output.status.success(),
        "git ls-files exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).expect("git ls-files stdout is UTF-8");

    let mut violations: Vec<String> = Vec::new();

    for rel in listing.lines() {
        if rel.is_empty() {
            continue;
        }
        let abs = root.join(rel);

        // Self-exclude the scanner file by canonicalized-path
        // comparison — it must contain the forbidden literals as
        // `concat!` arguments to do its job.
        if abs
            .canonicalize()
            .map(|p| p == scanner_path)
            .unwrap_or(false)
        {
            continue;
        }

        // Read with a documented byte cap so a multi-megabyte
        // tracked file cannot OOM the test process. Files larger
        // than the cap are silently skipped — a tombstone scanner's
        // job is to flag prose drift, not to police generated
        // artifacts. Per
        // `.claude/rules/external-input-path-construction.md`.
        let mut content = String::new();
        let file = match std::fs::File::open(&abs) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if file
            .take(CORPUS_TOMBSTONE_BYTE_CAP)
            .read_to_string(&mut content)
            .is_err()
        {
            continue;
        }

        for phrase in &forbidden {
            if content.contains(phrase) {
                violations.push(format!("{} — {}", rel, phrase));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Phase 3 was renamed from Code Review to Review in PR #1430. \
         The following tracked files still contain forbidden fragments:\n\n{}",
        violations.join("\n")
    );
}

// --- Out of Scope template-section instruction tombstones ---
//
// The tests below assert that the surviving issue-filing skills
// (flow-explore, flow-plan) do NOT
// contain a templated "Out of Scope" section instruction in any
// markdown shape — bold heading, plain heading, italic,
// list-item label, or body-draft enumeration entry. The
// structural protection target is the title-cased phrase "Out of
// Scope" itself, because every templated form (`**Out of
// Scope**`, `## Out of Scope`, `Out of Scope:`, `Files to
// Investigate, Out of Scope, Context`) contains that exact
// substring. `.claude/rules/include-bias-in-issues.md` is the
// rule the tombstones enforce.
//
// The flow-create-issue tombstone (`Tombstone: removed in
// PR #1427`) was retired in PR #1477 alongside the flow-create-issue
// skill itself; the file-existence and prose-absence tombstones
// for the skill removal live below as
// `test_skills_no_flow_create_issue_dir`,
// `test_docs_no_flow_create_issue_md`, and
// `test_skills_no_flow_create_issue_references`.

/// Tombstone: removed in PR #1435. Must not return.
///
/// Asserts `skills/flow-code/SKILL.md` does NOT contain the
/// title-cased phrase `Flaky Test`. The flaky-test filing path was
/// removed because filed issues produced no actionable signal; the
/// retry loop and green-before-commit HARD-GATE handle intermittent
/// failures inline.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: not applicable to Markdown.
///   2. `format!` reassembly: not applicable to Markdown.
///   3. Named constant reference: not applicable to Markdown.
///   4. Method chains / split args: not applicable to Markdown.
#[test]
fn test_flow_code_skill_no_flaky_test_label() {
    let content = common::read_skill("flow-code");
    assert!(
        !content.contains("Flaky Test"),
        "skills/flow-code/SKILL.md must not contain the title-cased \
         phrase `Flaky Test` — the flaky-test filing path was removed \
         because filed issues produced no actionable signal."
    );
}

/// Tombstone: removed in PR #1435. Must not return.
///
/// Asserts `src/analyze_issues.rs` does NOT contain the title-cased
/// phrase `Flaky Test`. The string was removed from `LABEL_CATEGORIES`
/// when the flaky-test filing path was deleted; the analyzer now
/// recognizes only `Rule`, `Tech Debt`, and `Documentation Drift`.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: the string is a `&'static str` slice
///      element, not assembled at runtime.
///   2. `format!` reassembly: the constant is a compile-time slice
///      literal.
///   3. Named constant reference: a `const FLAKY: &str = "Flaky \
///      Test";` followed by use of `FLAKY` would still place the
///      literal `"Flaky Test"` in the source — the constant
///      definition itself trips the byte check.
///   4. Method chains / split args: not applicable — the value
///      lives inside a slice literal, not constructed via `.arg()`.
#[test]
fn test_analyze_issues_no_flaky_test_label() {
    let path = common::repo_root().join("src/analyze_issues.rs");
    let content = fs::read_to_string(&path).expect("src/analyze_issues.rs must exist");
    assert!(
        !content.contains("Flaky Test"),
        "src/analyze_issues.rs must not contain the title-cased \
         phrase `Flaky Test` — the label was removed from \
         LABEL_CATEGORIES alongside the flaky-test filing path."
    );
}

/// Tombstone: removed in PR #1435. Must not return.
///
/// Asserts `skills/flow-orchestrate/SKILL.md` does NOT contain the
/// title-cased phrase `Flaky Test`. The category was removed from
/// the orchestrate dashboard's categorize-by-label list because the
/// filing path no longer produces issues under that label.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: not applicable to Markdown.
///   2. `format!` reassembly: not applicable to Markdown.
///   3. Named constant reference: not applicable to Markdown.
///   4. Method chains / split args: not applicable to Markdown.
#[test]
fn test_flow_orchestrate_skill_no_flaky_test_label() {
    let content = common::read_skill("flow-orchestrate");
    assert!(
        !content.contains("Flaky Test"),
        "skills/flow-orchestrate/SKILL.md must not contain the \
         title-cased phrase `Flaky Test` — the category was removed \
         from the categorize-by-label list when the filing path \
         was deleted."
    );
}

/// Tombstone: removed in PR #1435. Must not return.
///
/// Asserts `skills/flow-issues/SKILL.md` does NOT contain the
/// phrase `flaky tests` (case-insensitive). The urgent-criteria
/// sentence was rewritten to drop the reference because there is
/// no longer a flaky-test category to surface.
///
/// The case-insensitive check is performed via `to_lowercase()`
/// before `.contains()` so capitalization variants ("Flaky Tests",
/// "FLAKY TESTS", "Flaky tests") all trip the guard.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: not applicable to Markdown.
///   2. `format!` reassembly: not applicable to Markdown.
///   3. Named constant reference: not applicable to Markdown.
///   4. Method chains / split args: not applicable to Markdown.
#[test]
fn test_flow_issues_skill_no_flaky_tests_phrase() {
    let content = common::read_skill("flow-issues");
    assert!(
        !content.to_lowercase().contains("flaky tests"),
        "skills/flow-issues/SKILL.md must not contain the phrase \
         `flaky tests` (case-insensitive) — the reference was \
         removed from the urgent-criteria sentence when the \
         filing path was deleted."
    );
}

// --- flow-create-issue skill removal (PR #1477) ---
//
// The `/flow:flow-create-issue` skill was retired alongside the
// addition of `/flow:flow-explore` (PM voice, vanilla
// problem-statement filing) and the rewrite of `/flow:flow-plan`
// into a `#N`-argument decompose-and-file pipeline. The three
// tombstones below assert that the skill directory, its docs page,
// and its references in surviving SKILL.md files do not return.
//
// Stability: each tombstone targets a stable on-disk path or a
// SKILL.md byte-substring. Markdown contains no `concat!` /
// `format!` / constant-reference reassembly, so byte-substring
// checks suffice.

/// Tombstone: removed in PR #1477. Must not return.
///
/// The `skills/flow-create-issue/` directory housed the deleted
/// skill. A merge-conflict resolution that re-introduces the
/// directory would resurrect the skill. The check is path-existence
/// only (does NOT depend on a particular SKILL.md content) so any
/// re-creation of the directory fires the tombstone.
///
/// Stability: pure path-existence assertion against a stable
/// project-relative path. No string reassembly applies.
#[test]
fn test_skills_no_flow_create_issue_dir() {
    let path = common::repo_root().join("skills").join("flow-create-issue");
    assert!(
        !path.exists(),
        "skills/flow-create-issue/ must not exist — the skill was \
         retired in PR #1477. The pipeline split into \
         /flow:flow-explore (vanilla problem statements) and \
         /flow:flow-plan #N (decomposed implementation plans)."
    );
}

/// Tombstone: removed in PR #1477. Must not return.
///
/// The `docs/skills/flow-create-issue.md` page documented the
/// deleted skill. `tests/docs_sync.rs::every_docs_skill_page_has_a_skill_dir`
/// would also fire if the docs page returned without the skill
/// directory, but the docs-sync test is a sibling-pair invariant;
/// this tombstone asserts the docs page is gone independently so a
/// merge-conflict re-introduction surfaces here even if a future
/// edit also re-adds the skill directory (which the sibling
/// tombstone above catches).
///
/// Stability: pure path-existence assertion. No reassembly applies.
#[test]
fn test_docs_no_flow_create_issue_md() {
    let path = common::repo_root()
        .join("docs")
        .join("skills")
        .join("flow-create-issue.md");
    assert!(
        !path.exists(),
        "docs/skills/flow-create-issue.md must not exist — the \
         skill was retired in PR #1477."
    );
}

/// Tombstone: removed in PR #1477. Must not return.
///
/// Surviving SKILL.md files must not reference the deleted
/// `flow-create-issue` skill. References would either:
/// (a) violate `tests/skill_contracts.rs::flow_references_point_to_existing_skills`
/// (which fails CI on `/flow:<name>` references to non-existent
/// skills), OR
/// (b) for non-slash-command prose mentions, mislead readers into
/// thinking the skill still exists.
///
/// The check scans every `skills/<name>/SKILL.md` for the literal
/// string `flow-create-issue`. The skill name is a stable kebab-case
/// identifier — it cannot be reassembled by `concat!` / `format!` /
/// constant reference at SKILL.md read time (Markdown is inert
/// text), so a byte-substring check is sufficient.
///
/// Stability: byte-substring check against a stable kebab-case
/// identifier. Markdown contains no Rust macros that could
/// reassemble the literal at read time.
#[test]
fn test_skills_no_flow_create_issue_references() {
    let skills_dir = common::skills_dir();
    let mut violations: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&skills_dir).expect("skills/ must exist") {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if content.contains("flow-create-issue") {
            let rel = skill_md
                .strip_prefix(common::repo_root())
                .unwrap_or(&skill_md);
            violations.push(rel.display().to_string());
        }
    }
    assert!(
        violations.is_empty(),
        "Surviving SKILL.md files must not reference \
         `flow-create-issue` (skill retired in PR #1477). \
         Offending files:\n  {}",
        violations.join("\n  ")
    );
}

/// Tombstone: removed in PR #1489. `skills/flow-explore/SKILL.md`
/// no longer invokes `set-utility-in-progress` or
/// `clear-utility-in-progress`. The skill is excluded from
/// `crate::commands::utility_marker::MULTI_STEP_UTILITY_SKILLS`
/// because it never invokes `decompose:decompose`, so the Stop
/// hook's decompose-return gate cannot fire on its behalf. Both
/// marker strings are stable source literals — they appear as
/// exact CLI subcommand names in skill bash blocks, are never
/// constructed via `concat!`, `format!`, or constant references in
/// the SKILL.md corpus, and are never split across `.arg()` chains.
#[test]
fn test_flow_explore_no_utility_marker_calls() {
    let content = common::read_skill("flow-explore");
    assert!(
        !content.contains("set-utility-in-progress"),
        "skills/flow-explore/SKILL.md must not contain `set-utility-in-progress` (removed in PR #1489: marker writes are dead code when MULTI_STEP_UTILITY_SKILLS excludes flow-explore)"
    );
    assert!(
        !content.contains("clear-utility-in-progress"),
        "skills/flow-explore/SKILL.md must not contain `clear-utility-in-progress` (removed in PR #1489: nothing to clear when no marker is written)"
    );
}

/// Tombstone: removed in PR #1489. `skills/flow-explore/SKILL.md`
/// no longer carries a wrap-up AskUserQuestion gate before filing.
/// The user's readiness signal in Step 3 is the single
/// authorization to file; a second confirmation question breaks
/// AC#4 (single-signal filing). The forbidden phrasing is the
/// exact verbatim prompt the removed gate used; this is a stable
/// source literal (a full English sentence appearing in the SKILL
/// prose), not assembled via `concat!`, `format!`, or constant
/// composition.
#[test]
fn test_flow_explore_no_wrap_up_ask_user_question() {
    let content = common::read_skill("flow-explore");
    let forbidden = "Review the draft above. Ready to file?";
    assert!(
        !content.contains(forbidden),
        "skills/flow-explore/SKILL.md must not contain the wrap-up AskUserQuestion prompt `{}` (removed in PR #1489: Step 5 files directly on the user's readiness signal)",
        forbidden,
    );
}

/// Tombstone: removed in PR #1489. `skills/flow-plan/SKILL.md` no
/// longer carries a wrap-up AskUserQuestion gate before filing the
/// decomposed issue. Per AC#4 of issue #1488, the user's readiness
/// signal from the Step 4 discussion is the single authorization
/// to file; the decompose + transform pipeline that precedes Step 6
/// is unattended infrastructure, not a second decision point. The
/// forbidden phrasing is the exact verbatim prompt the removed
/// gate used; this is a stable source literal (a full English
/// sentence appearing in the SKILL prose), not assembled via
/// `concat!`, `format!`, or constant composition.
#[test]
fn test_flow_plan_no_wrap_up_ask_user_question() {
    let content = common::read_skill("flow-plan");
    let forbidden = "Review the draft above. Ready to file?";
    assert!(
        !content.contains(forbidden),
        "skills/flow-plan/SKILL.md must not contain the wrap-up AskUserQuestion prompt `{}` (removed in PR #1489: Step 6 files directly after the decompose + transform pipeline)",
        forbidden,
    );
}

/// Tombstone: removed in PR #1489. `src/create_milestone.rs` was
/// the only producer the `bin/flow create-milestone` subcommand
/// depended on; with the decompose-project skill no longer
/// requesting a milestone, the subcommand became orphan
/// infrastructure and is deleted per
/// `.claude/rules/supersession.md`. This tombstone is structural:
/// a file-existence check plus byte-substring scans of `src/lib.rs`
/// and `src/main.rs` for the constant module-registration strings.
///
/// Stability: the scanned substrings (`pub mod create_milestone`,
/// `CreateMilestone`) are exact Rust identifiers and module-
/// declaration tokens that the compiler requires verbatim. They
/// cannot be assembled by `concat!` or `format!` and still resolve
/// at compile time — a future resurrection would have to write the
/// literal strings the scan catches.
#[test]
fn test_src_no_create_milestone_module() {
    let root = common::repo_root();
    assert!(
        !root.join("src").join("create_milestone.rs").exists(),
        "src/create_milestone.rs must not exist — the subcommand was deleted in PR #1489"
    );
    let lib_content =
        fs::read_to_string(root.join("src").join("lib.rs")).expect("src/lib.rs must exist");
    assert!(
        !lib_content.contains("pub mod create_milestone"),
        "src/lib.rs must not declare `pub mod create_milestone` (removed in PR #1489)"
    );
    let main_content =
        fs::read_to_string(root.join("src").join("main.rs")).expect("src/main.rs must exist");
    assert!(
        !main_content.contains("CreateMilestone"),
        "src/main.rs must not reference `CreateMilestone` (variant + dispatch arm removed in PR #1489)"
    );
}

/// Tombstone: removed in PR #1489. The `--milestone` flag on
/// `bin/flow issue` is deleted along with `bin/flow create-milestone`.
/// The scan is scoped to `src/issue.rs` because the codebase
/// legitimately mentions `--milestone` elsewhere in historical
/// contexts (release notes, changelogs); only the source file must
/// be milestone-free.
///
/// Stability: the scanned strings (`--milestone`, `milestone`) are
/// stable source constants — `--milestone` is the literal CLI flag
/// name clap requires verbatim, and `milestone` is the Rust field
/// identifier the compiler would require if the field were
/// resurrected. Neither can be produced by `concat!` / `format!` at
/// the call sites the scan covers (clap attribute strings and
/// struct field declarations require compile-time literal tokens).
#[test]
fn test_src_issue_no_milestone_flag() {
    let root = common::repo_root();
    let issue_src =
        fs::read_to_string(root.join("src").join("issue.rs")).expect("src/issue.rs must exist");
    assert!(
        !issue_src.contains("--milestone"),
        "src/issue.rs must not reference `--milestone` (flag removed in PR #1489)"
    );
    assert!(
        !issue_src.to_lowercase().contains("milestone"),
        "src/issue.rs must not reference `milestone` (field + arg removed in PR #1489)"
    );
}

// --- flow-create-issue prose corpus protection (PR #1486) ---

/// Tombstone: removed in PR #1486. Must not return.
///
/// PR #1477 retired the `flow-create-issue` skill, and PR #1486
/// completed the prose sweep across CLAUDE.md, `.claude/rules/*.md`,
/// and `.claude/skills/<name>/SKILL.md`. The sibling tombstone
/// `test_skills_no_flow_create_issue_references` covers
/// `skills/<name>/SKILL.md` (the public skill directory); this
/// tombstone covers the three other prose surfaces where the
/// identifier could resurface via merge conflict: the project
/// CLAUDE.md, the rules corpus, and the maintainer-skill corpus.
///
/// Stability: byte-substring check against the kebab-case skill
/// name `flow-create-issue`. The string is a stable identifier
/// embedded in markdown prose. Cannot be assembled by `concat!`
/// or `format!` (markdown is inert text, not Rust code). Cannot
/// be a Rust constant (the files are markdown). Cannot be split
/// into multiple `.arg()` calls (not a CLI invocation). The
/// four-question stability checklist passes for all three
/// corpora.
#[test]
fn test_prose_corpus_no_flow_create_issue_references() {
    let root = common::repo_root();
    let mut violations: Vec<String> = Vec::new();

    // CLAUDE.md (single file).
    let claude_md = root.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&claude_md) {
        if content.contains("flow-create-issue") {
            violations.push("CLAUDE.md".to_string());
        }
    }

    // .claude/rules/*.md (rules corpus — flat directory of markdown).
    let rules_dir = root.join(".claude").join("rules");
    for (rel, content) in common::collect_md_files(&rules_dir) {
        if content.contains("flow-create-issue") {
            violations.push(format!(".claude/rules/{}", rel));
        }
    }

    // .claude/skills/<name>/SKILL.md (maintainer-skill corpus —
    // each subdirectory carries one SKILL.md).
    let claude_skills_dir = root.join(".claude").join("skills");
    for (rel, content) in common::collect_md_files(&claude_skills_dir) {
        if content.contains("flow-create-issue") {
            violations.push(format!(".claude/skills/{}", rel));
        }
    }

    assert!(
        violations.is_empty(),
        "CLAUDE.md, .claude/rules/*.md, and .claude/skills/<name>/SKILL.md \
         must not reference `flow-create-issue` (skill retired in \
         PR #1477; prose corpus swept in PR #1486). Offending files:\n  {}",
        violations.join("\n  ")
    );
}

// --- acquire_with_wait_impl Rust seam citation removal (PR #1486) ---

/// Tombstone: removed in PR #1486. Must not return.
///
/// `.claude/rules/testing-gotchas.md` previously cited
/// `acquire_with_wait_impl` in `src/commands/start_lock.rs` as
/// the canonical example of an injectable `sleep_fn` seam. The
/// production function no longer carries that signature and the
/// `_impl` variant does not exist. The prose was rewritten to
/// describe the retry/timeout pattern without naming a stale
/// reference. The scan covers the full rule corpus plus
/// CLAUDE.md so a future edit that reintroduces the identifier
/// in any prose surface fails CI.
///
/// Stability: byte-substring check against the literal identifier
/// `acquire_with_wait_impl`. The string is a Rust function name
/// embedded in markdown prose, cannot be assembled by `concat!`
/// or `format!` (markdown is inert text, not Rust code), cannot
/// be a constant (the files are markdown, not Rust source), and
/// cannot be split into multiple `.arg()` calls (not a CLI
/// invocation). The four-question stability checklist passes.
#[test]
fn test_rules_no_acquire_with_wait_impl() {
    let root = common::repo_root();
    let mut violations: Vec<String> = Vec::new();

    let claude_md = root.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&claude_md) {
        if content.contains("acquire_with_wait_impl") {
            violations.push("CLAUDE.md".to_string());
        }
    }

    let rules_dir = root.join(".claude").join("rules");
    for (rel, content) in common::collect_md_files(&rules_dir) {
        if content.contains("acquire_with_wait_impl") {
            violations.push(format!(".claude/rules/{}", rel));
        }
    }

    assert!(
        violations.is_empty(),
        "CLAUDE.md and .claude/rules/*.md must not reference \
         `acquire_with_wait_impl` — the production function no \
         longer exposes that `_impl` seam; describe the \
         retry/timeout pattern without naming a stale identifier. \
         Offending files:\n  {}",
        violations.join("\n  ")
    );
}

// --- run_tui_arm_impl closure-pair seam citation removal (PR #1486) ---

/// Tombstone: removed in PR #1486. Must not return.
///
/// `.claude/rules/rust-patterns.md` previously cited
/// `run_tui_arm_impl(is_tty_fn, run_terminal_fn, root)` as the
/// canonical closure-injection seam. That signature was
/// collapsed: `run_tui_arm_impl` is intentionally non-generic
/// today (single `root: &Path` parameter) and
/// `run_terminal_body<B, C, E>` is the actual closure-injection
/// seam. The function name itself remains legitimate prose (the
/// rule cites it as the named non-generic layer above
/// `run_terminal`), so the tombstone forbids the obsolete
/// invocation SHAPE — any `run_tui_arm_impl(...)` call site in
/// prose with a comma inside the argument list. A single-arg
/// `run_tui_arm_impl(root)` call has no comma between parens and
/// is allowed.
///
/// Stability: structural check on the multi-arg call shape (per
/// `tombstone-tests.md` "Two kinds of tombstone" — when in
/// doubt, assume #2 structural). A literal byte-substring on
/// any single parameter name (e.g., `is_tty_fn`) would bypass
/// the tombstone whenever a regression renamed the closure
/// parameter (`tty_check_fn`, `tty_predicate`, etc.) — the
/// architectural construct is "multi-arg call to
/// `run_tui_arm_impl` in prose," not "the specific identifier
/// `is_tty_fn`." Macro forms `concat!` and `format!` are
/// inapplicable here (markdown is inert text, not Rust code);
/// Rust constants are inapplicable (markdown files cannot host
/// constant declarations). The forbidden shape — a function
/// call with multiple comma-separated args — is captured by the
/// structural scan, independent of parameter names.
#[test]
fn test_rules_no_run_tui_arm_impl_closure_pair() {
    let root = common::repo_root();
    let rules_dir = root.join(".claude").join("rules");
    let mut violations: Vec<String> = Vec::new();

    // For each .md file in .claude/rules/, find every occurrence
    // of "run_tui_arm_impl(" and check whether the argument list
    // (bytes between the matching parens) contains a comma. The
    // current single-arg form has no comma inside the parens; any
    // multi-arg form (the obsolete closure-pair shape) does.
    const NEEDLE: &str = "run_tui_arm_impl(";
    for (rel, content) in common::collect_md_files(&rules_dir) {
        let bytes = content.as_bytes();
        let mut search_from = 0;
        while let Some(idx) = content[search_from..].find(NEEDLE) {
            let open_paren = search_from + idx + NEEDLE.len() - 1;
            // Walk forward from the opening paren, tracking nesting,
            // until the matching closing paren. Bail at newline so a
            // multi-line code block doesn't fold into a single
            // argument list.
            let mut depth = 1usize;
            let mut found_comma = false;
            let mut cursor = open_paren + 1;
            while cursor < bytes.len() {
                let b = bytes[cursor];
                if b == b'\n' {
                    break;
                }
                if b == b'(' {
                    depth += 1;
                } else if b == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if b == b',' && depth == 1 {
                    found_comma = true;
                }
                cursor += 1;
            }
            if found_comma {
                let snippet_end = (open_paren + 60).min(content.len());
                let snippet = &content[search_from + idx..snippet_end];
                violations.push(format!(".claude/rules/{}: {}", rel, snippet));
            }
            search_from = open_paren + 1;
        }
    }

    assert!(
        violations.is_empty(),
        ".claude/rules/*.md must not reference `run_tui_arm_impl` \
         with a multi-arg call shape (the obsolete closure-pair seam \
         at the `run_tui_arm_impl` layer). The single-arg form \
         `run_tui_arm_impl(root)` is allowed; any call with a comma \
         inside the parens is the obsolete shape. Offending sites:\n  {}",
        violations.join("\n  ")
    );
}

// --- Layer 10 integration-branch carve-out removal ---
//
// PR #1514 added the bootstrap-skill carve-out to Layer 10's
// integration-branch context (previously uncarved). The old claim
// "The integration-branch context is NOT carved out — commits on
// the integration branch are blocked regardless of the marker"
// appeared in both CLAUDE.md "Permission Invariant" and
// `.claude/rules/concurrency-model.md`'s "Skill-commit carve-out"
// subsection. Both occurrences were rewritten to describe the
// new two-context (active-flow + integration-branch) carve-out
// structure. A merge resolution that re-introduces the old
// prose would silently restore a contradictory security claim:
// the rule files would assert the gate is uncarved while the
// hook code does carve out the bootstrap window. The tombstone
// fails CI on either resurrection.

/// Tombstone: removed in PR #1514. The literal phrase
/// "integration-branch context is NOT carved out" must not
/// appear in `CLAUDE.md` or `.claude/rules/concurrency-model.md`
/// — the bootstrap-skill carve-out replaced the unconditional
/// block on the integration-branch context.
///
/// Stability: this is a markdown-prose substring, not Rust code.
/// `concat!` and `format!` are macros that synthesize Rust string
/// literals at compile time — they cannot assemble markdown
/// rendered into a `.md` file. Constants are inapplicable
/// (markdown files cannot host Rust `const` declarations).
/// Method-chain splits across `.arg()` calls are inapplicable
/// for the same reason. A merge-conflict resurrection would
/// reintroduce the literal phrase verbatim in a markdown file,
/// which the byte-substring scan catches.
#[test]
fn test_rules_no_integration_branch_not_carved_out_claim() {
    let root = common::repo_root();
    const FORBIDDEN: &str = "integration-branch context is NOT carved out";
    for rel in ["CLAUDE.md", ".claude/rules/concurrency-model.md"] {
        let path = root.join(rel);
        let content = fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!("{} must exist for tombstone scan", rel);
        });
        assert!(
            !content.contains(FORBIDDEN),
            "{} must not contain '{}' — the bootstrap-skill carve-out \
             on Layer 10's integration-branch context was added in \
             PR #1514, replacing the uncarved block. A merge \
             resurrection that brings back this phrase would create \
             a contradiction between the rule prose and the hook \
             code in `src/hooks/validate_pretool.rs::bootstrap_carveout_applies`.",
            rel,
            FORBIDDEN
        );
    }
}

// --- Stop hook predicate stack rewrite (PR #1543) ---
//
// `src/hooks/stop_continue.rs` previously composed six predicates —
// `check_discussion_mode`, `check_first_stop`, `check_halt_pending`,
// `check_autonomous_in_progress`, `check_prose_pause_at_task_entry`,
// and three private helpers (`format_conditional_continue_reason`,
// `body_has_question_outside_code`, `last_assistant_text_and_tool_use`)
// plus two block-reason constants (`DISCUSSION_BLOCK_REASON`,
// `HALT_PAUSE_BLOCK_REASON`). The unified `check_autonomous_stop`
// gate consolidates these into a single three-rule predicate; the
// transcript-walker continue-token grammar (`user_message_contains_continue_token`,
// `find_token_with_boundary`, `find_two_word_token`) is replaced by
// the `/flow:flow-continue` slash-command exit path. A merge conflict
// or accidental edit that re-introduces any of these names would
// resurrect the pre-rewrite stack alongside the new predicate,
// producing contradictory behavior.

/// Deleted symbol names from the Stop hook rewrite (PR #1543).
/// Each must not appear in its previous home module.
const STOP_CONTINUE_REMOVED_NAMES: &[&str] = &[
    "check_discussion_mode",
    "check_first_stop",
    "check_halt_pending",
    "check_autonomous_in_progress",
    "check_prose_pause_at_task_entry",
    "format_conditional_continue_reason",
    "body_has_question_outside_code",
    "last_assistant_text_and_tool_use",
    "DISCUSSION_BLOCK_REASON",
    "HALT_PAUSE_BLOCK_REASON",
];

const TRANSCRIPT_WALKER_REMOVED_NAMES: &[&str] = &[
    "user_message_contains_continue_token",
    "find_token_with_boundary",
    "find_two_word_token",
];

/// Tombstone: removed in PR #1543. The six Stop-hook predicates
/// and their helper functions and constants listed in
/// `STOP_CONTINUE_REMOVED_NAMES` were consolidated into the
/// unified `check_autonomous_stop` predicate. None of the deleted
/// names may appear in `src/hooks/stop_continue.rs`.
///
/// Stability argument: each entry is a Rust identifier. Rust
/// identifiers cannot be assembled via `concat!` or `format!` at
/// definition sites — a function or constant declaration requires
/// the literal name in source. A merge-conflict resurrection
/// would land the exact bytes, which this scanner catches.
#[test]
fn test_stop_continue_no_removed_predicate_names() {
    let root = common::repo_root();
    let path = root.join("src").join("hooks").join("stop_continue.rs");
    let content = fs::read_to_string(&path).expect("stop_continue.rs must exist");
    let mut violations: Vec<&str> = Vec::new();
    for name in STOP_CONTINUE_REMOVED_NAMES {
        if content.contains(name) {
            violations.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "src/hooks/stop_continue.rs must not contain the deleted \
         predicates/helpers/constants: {:?}. The Stop hook rewrite \
         (PR #1543) replaced the pre-rewrite stack with the \
         unified `check_autonomous_stop` predicate.",
        violations
    );
}

/// Tombstone: removed in PR #1543. The continue-token grammar
/// helpers in `src/hooks/transcript_walker.rs` were removed when
/// `/flow:flow-continue` replaced the prose continue-token exit
/// path. None of the deleted names may appear in
/// `src/hooks/transcript_walker.rs`.
///
/// Stability argument: same as the Stop-hook tombstone above —
/// Rust identifier definitions require the literal name in source
/// and cannot be assembled via `concat!` or `format!`.
#[test]
fn test_transcript_walker_no_removed_continue_token_helpers() {
    let root = common::repo_root();
    let path = root.join("src").join("hooks").join("transcript_walker.rs");
    let content = fs::read_to_string(&path).expect("transcript_walker.rs must exist");
    let mut violations: Vec<&str> = Vec::new();
    for name in TRANSCRIPT_WALKER_REMOVED_NAMES {
        if content.contains(name) {
            violations.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "src/hooks/transcript_walker.rs must not contain the deleted \
         continue-token helpers: {:?}. PR #1543 replaced the prose \
         continue-token exit path with the `/flow:flow-continue` \
         slash command, making these helpers dead code.",
        violations
    );
}

// --- flow-prime Skip role option removal (PR #1552) ---

/// Tombstone: removed in PR #1552. The `/flow-prime` SKILL.md
/// previously offered "Skip — No default; choose per conversation"
/// as a fourth role option. PR #1552 drops the Skip option so the
/// role prompt presents three concrete roles only. A merge conflict
/// that re-introduces the Skip option would also reintroduce the
/// Skip-branch invocation shape in Step 4 that PR #1552 deleted.
///
/// Stability: byte-substring check against the literal Skip
/// description "No default; choose per conversation". The string
/// is markdown prose, not Rust code — `concat!` and `format!`
/// cannot synthesize markdown at compile time, and markdown files
/// cannot host Rust `constant` declarations. The phrase is not a
/// CLI invocation, so `.arg()` chain splits do not apply. The
/// four-question stability checklist passes for this byte-literal
/// scan.
#[test]
fn test_tombstones_no_flow_prime_skip_role_option() {
    let content = common::read_skill("flow-prime");
    assert!(
        !content.contains("No default; choose per conversation"),
        "skills/flow-prime/SKILL.md must not contain the Skip role-option \
         description 'No default; choose per conversation' — the Skip \
         option was removed in PR #1552 so the role prompt presents \
         three concrete roles (PM, Tech Lead, Founder / Solo Dev) only."
    );
}

/// Tombstone: removed in PR #1552. The `/flow-prime` SKILL.md
/// Step 3 Customize-autonomy section previously asked a per-skill
/// AskUserQuestion titled "Continue mode for /flow:flow-start?"
/// to let users override the Start continue mode. PR #1552 removes
/// the question entirely; the Customize branch now hardcodes
/// `flow-start: continue: auto` so users never get prompted for
/// the Start continue axis. A merge resurrection would re-expose
/// the prompt and break the "Start is never prompted" invariant.
///
/// Stability: byte-substring check against the literal question
/// heading "Continue mode for /flow:flow-start". The string is
/// markdown prose embedded in a `>` blockquote — `concat!` and
/// `format!` are Rust-only macros that cannot synthesize markdown
/// at compile time, and markdown files do not host Rust
/// `constant` declarations. The phrase is a quoted prompt
/// string, not a CLI invocation, so `.arg()` chain splits do not
/// apply. The four-question stability checklist passes.
#[test]
fn test_tombstones_no_flow_prime_customize_start_question() {
    let content = common::read_skill("flow-prime");
    assert!(
        !content.contains("Continue mode for /flow:flow-start"),
        "skills/flow-prime/SKILL.md must not contain the Customize \
         Start sub-question 'Continue mode for /flow:flow-start?' — \
         the question was removed in PR #1552; the Customize branch \
         hardcodes `flow-start: continue: auto` so Start is never \
         prompted in any autonomy path."
    );
}

/// Tombstone: removed in PR #1552. The `/flow-prime` Step 4
/// setup-script section previously branched on the user's Skip
/// choice with a "When the user chose Skip, omit `--role` entirely"
/// header and a paired bash invocation. PR #1552 deletes the Skip
/// UI option and folds Step 4 to a single user-driven invocation
/// shape (the legacy-data Reprime carry-forward path is described
/// separately as "When the Reprime path carries forward a legacy
/// `.flow.json`..."). A merge resurrection reintroducing the
/// "When the user chose Skip" header reactivates the deleted UI
/// option from Step 1 by implication.
///
/// Stability: byte-substring check against the literal header
/// "When the user chose Skip,". The string is markdown prose, not
/// Rust code — `concat!` and `format!` cannot synthesize markdown
/// at compile time, and markdown files cannot host Rust
/// `constant` declarations. The phrase is not a CLI invocation,
/// so `.arg()` chain splits do not apply. The four-question
/// stability checklist passes. The companion
/// `test_tombstones_no_flow_prime_skip_role_option` covers the
/// Step 1 prompt; this tombstone covers the Step 4 branching
/// surface independently so a partial resurrection (e.g., the
/// Step 4 branch returns without the Step 1 option, or vice
/// versa) trips CI either way.
#[test]
fn test_tombstones_no_flow_prime_step_4_skip_branch() {
    let content = common::read_skill("flow-prime");
    assert!(
        !content.contains("When the user chose Skip,"),
        "skills/flow-prime/SKILL.md must not contain the Step 4 Skip-branch \
         header 'When the user chose Skip,' — the branch was removed in \
         PR #1552 alongside the Step 1 Skip option. The legacy-data \
         Reprime carry-forward path (no role in .flow.json) is named \
         'When the Reprime path carries forward a legacy .flow.json' and \
         is intentionally distinct from the deleted Skip UI option."
    );
}

// --- cleanup_all / build_inventory removal (PR #1643) ---
//
// `cleanup_all` and `build_inventory` previously backed the deleted
// `bin/flow cleanup --all` dispatch arm in `src/cleanup.rs`. Their
// only consumer was `/flow:flow-reset`, which now invokes
// `${CLAUDE_PLUGIN_ROOT}/bin/reset` directly. Both functions are
// gone; the per-branch `cleanup()` function used by
// `/flow:flow-abort` and `/flow:flow-complete` is untouched.

/// Tombstone: removed in PR #1643. `cleanup_all` and `build_inventory`
/// are gone from `src/cleanup.rs` — `/flow:flow-reset` now invokes
/// `${CLAUDE_PLUGIN_ROOT}/bin/reset` directly. The `--all` and
/// `--dry-run` `Args` fields and the `--all` dispatch arm in
/// `run_impl_main` are gone too. The per-branch `cleanup()` function
/// stays untouched. Must not return.
///
/// Stability: byte-substring checks against the literal function
/// definition signatures `pub fn cleanup_all(` and `fn build_inventory(`.
/// Rust function declarations are emitted as source-level tokens —
/// a `concat!` reassembly cannot produce a parseable function
/// declaration (the declaration is parsed by `rustc`, not at
/// runtime), a `format!` reassembly does not apply (function
/// declarations are not runtime strings), and a `constant`
/// declaration cannot replace a function definition. The four-question
/// stability checklist passes for these byte-literal scans.
#[test]
fn test_cleanup_no_cleanup_all_or_build_inventory() {
    let content = fs::read_to_string("src/cleanup.rs").expect("src/cleanup.rs must exist");
    assert!(
        !content.contains("pub fn cleanup_all("),
        "src/cleanup.rs must not contain `pub fn cleanup_all(` — \
         the function was removed in PR #1643. /flow:flow-reset \
         now invokes ${{CLAUDE_PLUGIN_ROOT}}/bin/reset directly."
    );
    assert!(
        !content.contains("fn build_inventory("),
        "src/cleanup.rs must not contain `fn build_inventory(` — \
         the helper was removed in PR #1643 alongside cleanup_all. \
         No remaining caller needs the inventory categorization."
    );
}

/// Tombstone: removed in PR #1643. The `cleanup_all_*` test family
/// is gone from `tests/cleanup.rs` — the tests exercised the deleted
/// `cleanup_all` function and `--all` dispatch arm. Per-branch
/// `cleanup()` tests stay untouched. Must not return.
///
/// Stability: byte-substring check against the literal function-name
/// prefix `fn cleanup_all_`. Test function declarations are emitted
/// as source-level tokens parsed by rustc — a `concat!` reassembly
/// cannot produce a parseable `#[test] fn ...` declaration, a
/// `format!` reassembly does not apply (function declarations are
/// not runtime strings), and a `constant` declaration cannot
/// replace a function definition. The four-question stability
/// checklist passes for this byte-literal scan.
#[test]
fn test_cleanup_tests_no_cleanup_all_prefix() {
    let content = fs::read_to_string("tests/cleanup.rs").expect("tests/cleanup.rs must exist");
    assert!(
        !content.contains("fn cleanup_all_"),
        "tests/cleanup.rs must not contain `fn cleanup_all_*` test functions — \
         the cleanup_all test family was removed in PR #1643 alongside \
         the cleanup_all function it exercised."
    );
}

/// Tombstone: removed in PR #1643. The `--all` and `--dry-run` CLI
/// flag strings are gone from `src/cleanup.rs` — they belonged to
/// the deleted `cleanup_all` dispatch arm and its mutual-exclusion
/// reporting. The per-branch `cleanup` arm exposes only `--branch`
/// and `--worktree`. Must not return.
///
/// Stability: byte-substring checks against the literal clap-attribute
/// strings `"--all"` and `"--dry-run"`. Clap derive attributes are
/// parsed as source-level tokens by `rustc` and the proc-macro
/// expansion — a `concat!` reassembly cannot produce a parseable
/// attribute body, a `format!` reassembly does not apply (clap
/// attributes are not runtime strings), and a `constant` declaration
/// cannot replace an inline attribute argument. The four-question
/// stability checklist passes for these byte-literal scans.
#[test]
fn test_cleanup_no_all_or_dry_run_flag_strings() {
    let content = fs::read_to_string("src/cleanup.rs").expect("src/cleanup.rs must exist");
    assert!(
        !content.contains("\"--all\""),
        "src/cleanup.rs must not contain the `--all` flag string — \
         the flag was removed in PR #1643 alongside cleanup_all. \
         The per-branch cleanup arm exposes only --branch and --worktree."
    );
    assert!(
        !content.contains("\"--dry-run\""),
        "src/cleanup.rs must not contain the `--dry-run` flag string — \
         the flag was removed in PR #1643 alongside cleanup_all's \
         inventory machinery. There is no remaining dry-run consumer."
    );
}

// --- flow-plan Step 1/Step 2 dual-input rewrite (PR #1676) ---
//
// `/flow:flow-plan` accepts either `#N` (issue-input mode, which
// re-plans an existing problem statement in place) or a bare
// non-empty prompt (bare-prompt mode, which synthesizes a brief
// What/Why/AC and files one new decomposed issue). The Step 1
// Conversation Gate no longer rejects bare topics with a
// `/flow:flow-explore` redirect, and the Step 2 HARD-GATE no
// longer refuses issues that carry the `decomposed` label —
// the in-place edit IS the correct re-plan path now.
//
// Stability: each tombstone targets a SKILL.md byte-substring.
// Markdown contains no `concat!` / `format!` / constant reassembly
// surface — the file is read verbatim at runtime — so a byte
// check is sufficient.

/// Tombstone: removed in PR #1676. Must not return.
///
/// `skills/flow-plan/SKILL.md` Step 1 previously rejected
/// bare-prompt invocations with a migration message opening
/// "Topic-style invocations are no longer accepted". The current
/// Step 1 accepts both `#N` and bare prompts; the redirect
/// message is removed. A future regression might re-introduce
/// the gate with the same opening phrasing — this tombstone
/// catches the load-bearing identifier of that gate.
///
/// Stability: byte-substring check against "Topic-style
/// invocations" — a single-line Markdown phrase that uniquely
/// identified the removed gate's opening clause. Markdown is read
/// verbatim at runtime — there is no `concat!` reassembly, no
/// `format!` template, and no `constant` declaration that could
/// synthesize the prose without the literal appearing in source.
/// Plausible bypasses considered and rejected: (1) a reworded
/// rejection ("bare topics are not accepted") — different prose;
/// this scanner targets the canonical phrasing the removed gate
/// used. (2) a redirect that retains "/flow:flow-explore" prose
/// elsewhere — the slash-command name itself remains legitimate
/// in cross-references; only "Topic-style invocations" identifies
/// the removed rejection. (3) line-wrapping the phrase across
/// Markdown blockquote prefixes — Markdown line breaks don't
/// alter the contiguous bytes within a single line.
#[test]
fn test_flow_plan_no_explore_redirect_message() {
    let content = fs::read_to_string("skills/flow-plan/SKILL.md")
        .expect("skills/flow-plan/SKILL.md must exist");
    const FORBIDDEN: &str = "Topic-style invocations";
    assert!(
        !content.contains(FORBIDDEN),
        "skills/flow-plan/SKILL.md must not contain the bare-prompt \
         rejection identifier `{}` — Step 1 accepts both `#N` and \
         bare prompts as of PR #1676. Bare-prompt mode synthesizes \
         a brief What/Why/AC and files one new decomposed issue.",
        FORBIDDEN
    );
}

/// Tombstone: removed in PR #1676. Must not return.
///
/// `skills/flow-plan/SKILL.md` Step 6 previously closed the
/// vanilla parent issue after filing the decomposed child, via a
/// `bin/flow close-issue --comment` invocation. With the
/// rewrite, Step 6 has two branches: bare-prompt mode files one
/// new issue (no parent to close) and issue-input mode edits the
/// existing issue #N in place (the issue STAYS open, holding the
/// in-place plan). The close-parent path is gone entirely. A
/// future regression would re-introduce the close call via the
/// same subcommand surface.
///
/// Stability: byte-substring check against the `close-issue`
/// subcommand name. The flow-plan SKILL.md's surviving content
/// never references `close-issue` for any other purpose — the
/// subcommand exists in the FLOW CLI but flow-plan is the only
/// historical caller. Markdown contains no `concat!` reassembly,
/// no `format!` template, and no `constant` declaration that
/// could synthesize the literal without it appearing in source.
/// The sibling `gh issue close` invocation is also forbidden —
/// it was named in the close-failure recovery prose as a manual
/// fallback the user could run, but the recovery prose itself is
/// gone now that the close path is removed. Plausible bypasses:
/// (1) re-introducing the close call as `gh issue close <N>`
/// without `bin/flow close-issue` — caught by the second
/// assertion. (2) re-introducing it under a different subcommand
/// name (e.g., `gh issue update --state closed`) — not caught;
/// such a regression would require a separate skill audit. The
/// scanner targets the canonical surface the removed path used.
#[test]
fn test_flow_plan_no_close_parent_flow() {
    let content = fs::read_to_string("skills/flow-plan/SKILL.md")
        .expect("skills/flow-plan/SKILL.md must exist");
    assert!(
        !content.contains("close-issue"),
        "skills/flow-plan/SKILL.md must not contain `close-issue` — \
         the close-parent path was removed in PR #1676. Issue-input \
         mode edits the issue in place; bare-prompt mode files a \
         new issue with no parent to close."
    );
    assert!(
        !content.contains("gh issue close"),
        "skills/flow-plan/SKILL.md must not contain `gh issue close` — \
         the manual-fallback recovery prose was removed alongside the \
         close-parent path in PR #1676."
    );
}

/// Tombstone: removed in PR #1676. Must not return.
///
/// `skills/flow-plan/SKILL.md` Step 2 previously refused to plan
/// against any issue whose labels included `decomposed`, with the
/// user-facing message "already carries the `decomposed` label".
/// The current shape edits #N in place — the `decomposed` label is
/// expected (and re-applied via `gh issue edit --add-label
/// decomposed`). The label gate has been removed entirely. A future
/// regression that re-adds the gate would re-use the same
/// user-facing phrasing.
///
/// Stability: byte-substring check against the user-facing
/// rejection message. The phrase "already carries the `decomposed`
/// label" is a fixed Markdown sentence inside a Step 2 HARD-GATE
/// block; Markdown has no `concat!` reassembly, no `format!`
/// template, and no `constant` declaration that could synthesize
/// the literal without it appearing in source. Plausible bypasses
/// considered: (1) a paraphrased rejection ("issue is already
/// decomposed") — different prose; this scanner catches the
/// canonical phrasing that the removed gate emitted. (2) a Hard
/// Rules entry that re-asserts the gate's intent without the
/// user-facing message — caught by the absence of the rejection
/// flow in Step 2 (see the rewritten test
/// `flow_plan_skill_keeps_closed_issue_rejection` which keeps
/// the closed-issue arm but not the label arm). (3) a relocated
/// gate that emits the same message from a different Step —
/// still caught by this whole-file byte-substring scan.
#[test]
fn test_flow_plan_no_decomposed_label_gate() {
    let content = fs::read_to_string("skills/flow-plan/SKILL.md")
        .expect("skills/flow-plan/SKILL.md must exist");
    const FORBIDDEN: &str = "already carries the `decomposed` label";
    assert!(
        !content.contains(FORBIDDEN),
        "skills/flow-plan/SKILL.md must not contain the \
         decomposed-label rejection message `{}` — the gate was \
         removed in PR #1676. Re-planning an already-decomposed \
         issue is the in-place edit path now; the label is \
         expected, not a rejection criterion.",
        FORBIDDEN
    );
}

/// Tombstone: removed in PR #1687. Must not return.
///
/// The "Plan-Reviewer Loop" cap-exhausted branch previously halted
/// the skill with the structured error envelope
/// `{"status":"error","reason":"plan_reviewer_max_retries",...}`
/// and printed the COMPLETE-FAILED banner instead of filing the
/// issue. The reviewer is advisory now: after 3 failed reviews
/// the issue is filed with the last drafted plan and the
/// violations are surfaced as a non-blocking warning. The
/// `plan_reviewer_max_retries` error reason is gone entirely from
/// both `skills/flow-plan/SKILL.md` (the behavior) and
/// `docs/skills/flow-plan.md` (the user-facing description). A
/// future regression would re-introduce the halt-and-fail
/// envelope alongside this identifier in either file.
///
/// Stability: byte-substring check against the error-reason
/// identifier. Both scanned files are Markdown — they have no
/// `concat!` macro concatenation, no `format!` template
/// reassembly, and no `constant` declaration that could
/// synthesize the identifier without it appearing in source.
/// Plausible bypasses considered and rejected: (1)
/// `concat!("plan_reviewer", "_max_retries")` — Markdown has no
/// `concat!`. (2) a `format!` template assembling the identifier
/// — Markdown has no `format!`. (3) a Rust constant referenced by
/// name — the identifier lives in Markdown prose, not Rust
/// source, so no constant indirection is possible.
#[test]
fn test_flow_plan_no_plan_reviewer_max_retries() {
    const FORBIDDEN: &str = "plan_reviewer_max_retries";
    for path in ["skills/flow-plan/SKILL.md", "docs/skills/flow-plan.md"] {
        let content = fs::read_to_string(path).unwrap_or_else(|_| panic!("{} must exist", path));
        assert!(
            !content.contains(FORBIDDEN),
            "{} must not contain the error-reason identifier `{}` — \
             the cap-exhausted halt-and-fail envelope was removed in \
             PR #1687. The plan-reviewer is advisory now: after 3 \
             failed reviews the issue is filed with the last drafted \
             plan and the violations are surfaced as a non-blocking \
             warning.",
            path,
            FORBIDDEN
        );
    }
}

// --- Agent-run recording moved to PreToolUse:Agent hook (PR #1806) ---
//
// PR #1806 moved "a required Review/Learn sub-agent ran" from the
// model-invoked `record-agent-return` CLI (which a model could
// fabricate without launching the agent) into the `PreToolUse:Agent`
// hook (`src/hooks/agent_run_record.rs`), which records the launch as
// unforgeable evidence. `phase-finalize`'s required-agents gate now
// reads `agents_returned` alone. The same PR deleted the now-orphaned
// skip and verification machinery. These tombstones assert none of
// the deleted identifiers or files return.

/// Tombstone: removed in PR #1806. `verify_agent_returned_in_phase`
/// and `recent_phase_finalize_agent_skip` (plus their private
/// helpers) were removed from `src/hooks/transcript_walker.rs` when
/// agent-run recording moved into the `PreToolUse:Agent` hook and the
/// required-agents gate began reading `agents_returned` alone.
///
/// Stability argument: a Rust `fn` definition requires the literal
/// name in source and cannot be assembled via `concat!` or
/// `format!`; a constant rename would still surface a definition. The
/// four-question stability checklist passes for these byte-substring
/// scans.
#[test]
fn test_transcript_walker_no_agent_accounting_helpers() {
    let root = common::repo_root();
    let path = root.join("src").join("hooks").join("transcript_walker.rs");
    let content = fs::read_to_string(&path).expect("transcript_walker.rs must exist");
    for name in [
        "verify_agent_returned_in_phase",
        "recent_phase_finalize_agent_skip",
    ] {
        assert!(
            !content.contains(name),
            "src/hooks/transcript_walker.rs must not contain the \
             deleted agent-accounting helper `{}` — PR #1806 moved \
             agent-run recording into the PreToolUse:Agent hook.",
            name
        );
    }
}

/// Tombstone: removed in PR #1806. `is_known_agent` was removed from
/// `src/required_agents.rs` when agent-run recording moved into the
/// `PreToolUse:Agent` hook, which matches the launched subagent
/// against `required_agents_for_phase` directly.
///
/// Stability argument: a Rust `fn` definition requires the literal
/// name in source and cannot be produced by `concat!` or `format!`;
/// the byte-substring scan is sufficient.
#[test]
fn test_required_agents_no_is_known_agent() {
    let root = common::repo_root();
    let path = root.join("src").join("required_agents.rs");
    let content = fs::read_to_string(&path).expect("required_agents.rs must exist");
    assert!(
        !content.contains("is_known_agent"),
        "src/required_agents.rs must not contain the deleted \
         `is_known_agent` helper — removed in PR #1806."
    );
}

/// Tombstone: removed in PR #1806. `refresh_active_flow_session` was
/// removed from `src/hooks/capture_session.rs` when the orphaned
/// agent-skip machinery it fed was deleted.
///
/// Stability argument: a Rust `fn` definition requires the literal
/// name in source; `concat!`/`format!` cannot synthesize a
/// definition, so the byte-substring scan holds.
#[test]
fn test_capture_session_no_refresh_active_flow_session() {
    let root = common::repo_root();
    let path = root.join("src").join("hooks").join("capture_session.rs");
    let content = fs::read_to_string(&path).expect("capture_session.rs must exist");
    assert!(
        !content.contains("refresh_active_flow_session"),
        "src/hooks/capture_session.rs must not contain the deleted \
         `refresh_active_flow_session` helper — removed in PR #1806."
    );
}

/// Tombstone: removed in PR #1806. `skipped_agents_section` was
/// removed from `src/format_complete_summary.rs` when the skipped-
/// agents panel was dropped from the Complete summary.
///
/// Stability argument: a Rust `fn` definition requires the literal
/// name in source and cannot be assembled by `concat!` or `format!`.
#[test]
fn test_format_complete_summary_no_skipped_agents_section() {
    let root = common::repo_root();
    let path = root.join("src").join("format_complete_summary.rs");
    let content = fs::read_to_string(&path).expect("format_complete_summary.rs must exist");
    assert!(
        !content.contains("skipped_agents_section"),
        "src/format_complete_summary.rs must not contain the deleted \
         `skipped_agents_section` helper — removed in PR #1806."
    );
}

/// Tombstone: removed in PR #1806. The `record-agent-return`
/// subcommand and its module `src/record_agent_return.rs` were
/// removed when agent-run recording moved into the PreToolUse:Agent
/// hook. The file-existence check pairs with the source-content check
/// per `.claude/rules/tombstone-tests.md` "file-resurrection
/// threats": a `#[path = "record_agent_return.rs"]` re-import under a
/// renamed module would resurrect the file without the variant string
/// reappearing.
///
/// Stability argument: the clap `RecordAgentReturn` variant requires
/// the literal identifier in `src/main.rs`; it cannot be assembled by
/// `concat!` or `format!`. The paired file-existence check covers the
/// renamed-module resurrection shape the byte scan cannot.
#[test]
fn test_src_no_record_agent_return_module() {
    let root = common::repo_root();
    assert!(
        !root.join("src").join("record_agent_return.rs").exists(),
        "src/record_agent_return.rs must not exist — removed in PR #1806."
    );
    let main = fs::read_to_string(root.join("src").join("main.rs")).expect("main.rs must exist");
    assert!(
        !main.contains("RecordAgentReturn"),
        "src/main.rs must not contain the deleted `RecordAgentReturn` \
         clap variant — removed in PR #1806."
    );
}

/// Tombstone: removed in PR #1806. The `add-skipped-agent` subcommand
/// and its module `src/add_skipped_agent.rs` were removed when the
/// required-agents gate stopped reading `agents_skipped`. The
/// file-existence check pairs with the source-content check per
/// `.claude/rules/tombstone-tests.md` "file-resurrection threats".
///
/// Stability argument: the clap `AddSkippedAgent` variant requires
/// the literal identifier in `src/main.rs`; it cannot be assembled by
/// `concat!` or `format!`. The paired file-existence check covers the
/// renamed-module resurrection shape.
#[test]
fn test_src_no_add_skipped_agent_module() {
    let root = common::repo_root();
    assert!(
        !root.join("src").join("add_skipped_agent.rs").exists(),
        "src/add_skipped_agent.rs must not exist — removed in PR #1806."
    );
    let main = fs::read_to_string(root.join("src").join("main.rs")).expect("main.rs must exist");
    assert!(
        !main.contains("AddSkippedAgent"),
        "src/main.rs must not contain the deleted `AddSkippedAgent` \
         clap variant — removed in PR #1806."
    );
}

#[test]
fn test_complete_finalize_no_speculative_sentinel_write() {
    // Tombstone: removed in PR #1879. complete_finalize::run_impl must
    // not fabricate the integration-branch CI sentinel by writing a
    // `ci::tree_snapshot` result to `ci::sentinel_path` without an
    // intervening `ci::run_impl` pass. The sanctioned path is a
    // sentinel-gated `ci::run_impl` call against the base branch, which
    // runs format/lint/build/test and writes the sentinel only on a
    // real CI pass.
    //
    // Structural assertion (function-body-scoped, not a single literal):
    // the speculative write can be reassembled via concat!/format!/split
    // constants, so the scan targets the construct — a manual
    // `tree_snapshot(` paired with a manual `sentinel_path(` inside
    // run_impl's body. The sanctioned `ci::run_impl(` call reaches both
    // internally (in src/ci.rs), but complete_finalize's own body
    // references neither after the removal.
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("src").join("complete_finalize.rs"))
        .expect("src/complete_finalize.rs must exist");
    // run_impl is the only fn in the file and the last item, so the
    // body slice runs from `fn run_impl` to EOF (no subsequent `\nfn `).
    let tail = content
        .split_once("fn run_impl")
        .map(|(_, t)| t)
        .expect("complete_finalize must define run_impl");
    let body = tail.split_once("\nfn ").map(|(b, _)| b).unwrap_or(tail);
    let has_snapshot = body.contains("tree_snapshot(");
    let has_sentinel = body.contains("sentinel_path(");
    assert!(
        !(has_snapshot && has_sentinel),
        "complete_finalize::run_impl must not fabricate the base-branch \
         CI sentinel by pairing a manual tree_snapshot() with a manual \
         sentinel_path() write — use sentinel-gated ci::run_impl instead \
         (removed in PR #1879)."
    );
}
