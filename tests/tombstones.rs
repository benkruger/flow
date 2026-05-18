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

// --- flow-plan parent-issue closure ---
//
// The decomposed-child issue supersedes the vanilla parent's
// problem-statement surface. Closing the parent at plan time
// (with a comment naming the child via
// `bin/flow close-issue --comment`) is what makes the decomposed
// issue the single open artifact for the problem. The prior
// `bin/flow link-blocked-by` invocation in flow-plan Step 6 is
// removed; this tombstone catches a merge conflict or accidental
// edit that re-introduces the invocation in `skills/flow-plan/SKILL.md`.

/// Tombstone: removed in PR #1492. The `bin/flow link-blocked-by`
/// invocation in `skills/flow-plan/SKILL.md` is replaced by a
/// `bin/flow close-issue --comment` call so the parent vanilla
/// issue closes at plan time. Must not return to the SKILL.md.
///
/// Stability argument: the protected target is markdown prose,
/// not Rust source. The byte literal `link-blocked-by` cannot be
/// reassembled at runtime — Markdown is a flat byte stream with
/// no `concat!` macro, no `format!` interpolation, and no named
/// constant references. A merge conflict can only resurrect the
/// exact bytes, which this scanner catches.
#[test]
fn test_flow_plan_skill_no_link_blocked_by() {
    let path = common::skills_dir().join("flow-plan").join("SKILL.md");
    let content = fs::read_to_string(&path).expect("flow-plan SKILL.md must exist");
    assert!(
        !content.contains("link-blocked-by"),
        "skills/flow-plan/SKILL.md must not reference `link-blocked-by` — \
         flow-plan now closes the vanilla parent with `bin/flow close-issue --comment` \
         so the decomposed child is the single open artifact for the problem."
    );
}

// --- exhausted-retry note path ---
//
// The `agent_exhausted_retries` state-note path is gone. The
// flow-review and flow-learn retry loops no longer call
// `bin/flow append-note` with a `--kind` flag, and the dead
// `agent_exhausted_retries` note kind no longer exists. Exhausted
// and skipped agents surface through `phases.<phase>.agents_skipped`
// and the Complete Done banner's Skipped Agents section. These
// per-file tombstones catch a merge conflict or accidental edit
// that re-introduces the dead invocation in either SKILL.md.

/// Tombstone: removed in PR #1584. The `agent_exhausted_retries`
/// state-note path is gone from `skills/flow-learn/SKILL.md` — the
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
fn test_flow_learn_skill_no_exhausted_retry_note() {
    let path = common::skills_dir().join("flow-learn").join("SKILL.md");
    let content = fs::read_to_string(&path).expect("flow-learn SKILL.md must exist");
    assert!(
        !content.contains("agent_exhausted_retries"),
        "skills/flow-learn/SKILL.md must not reference `agent_exhausted_retries` — \
         the dead state-note path is replaced by the Complete Done banner's \
         Skipped Agents section sourced from `phases.<phase>.agents_skipped`."
    );
    assert!(
        !content.contains("--kind"),
        "skills/flow-learn/SKILL.md must not invoke `bin/flow append-note --kind` — \
         the real append-note interface is `--note`/`--type`/`--branch`; the \
         `--kind` flag belonged only to the removed exhausted-retry note path."
    );
}

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
/// Source-content scan over skills/flow-triage-issue/SKILL.md and
/// agents/issue-triage.md for the literal `keep-open`. The literal is
/// stable per the four-question checklist in
/// `.claude/rules/tombstone-tests.md`:
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
fn test_flow_triage_no_keep_open_disposition() {
    let root = common::repo_root();
    let skill_path = root
        .join("skills")
        .join("flow-triage-issue")
        .join("SKILL.md");
    let agent_path = root.join("agents").join("issue-triage.md");
    let skill =
        fs::read_to_string(&skill_path).expect("skills/flow-triage-issue/SKILL.md must exist");
    let agent = fs::read_to_string(&agent_path).expect("agents/issue-triage.md must exist");
    assert!(
        !skill.contains("keep-open"),
        "skills/flow-triage-issue/SKILL.md must not contain `keep-open` — disposition removed in PR #1401"
    );
    assert!(
        !agent.contains("keep-open"),
        "agents/issue-triage.md must not contain `keep-open` — disposition removed in PR #1401"
    );
}

/// Tombstone: removed in PR #1401. Must not return.
///
/// Source-content scan over skills/flow-triage-issue/SKILL.md and
/// agents/issue-triage.md for the literal `fix-now`. Same stability
/// argument as the `keep-open` tombstone above: markdown files have
/// no concat/format/constant/.arg reassembly paths, and the
/// hyphenated form distinguishes the disposition token from prose
/// like "fix now."
#[test]
fn test_flow_triage_no_fix_now_disposition() {
    let root = common::repo_root();
    let skill_path = root
        .join("skills")
        .join("flow-triage-issue")
        .join("SKILL.md");
    let agent_path = root.join("agents").join("issue-triage.md");
    let skill =
        fs::read_to_string(&skill_path).expect("skills/flow-triage-issue/SKILL.md must exist");
    let agent = fs::read_to_string(&agent_path).expect("agents/issue-triage.md must exist");
    assert!(
        !skill.contains("fix-now"),
        "skills/flow-triage-issue/SKILL.md must not contain `fix-now` — disposition removed in PR #1401"
    );
    assert!(
        !agent.contains("fix-now"),
        "agents/issue-triage.md must not contain `fix-now` — disposition removed in PR #1401"
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
// (flow-explore, flow-plan, flow-decompose-project) do NOT
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

/// Tombstone: removed in PR #1427. Must not return.
///
/// Asserts `skills/flow-decompose-project/SKILL.md` does NOT
/// contain the title-cased phrase `Out of Scope` in any form.
/// Same structural shape as the sibling tombstone for
/// flow-create-issue: every templated re-introduction (bold,
/// heading, label, list entry, reordered list, singular form
/// like "Out of Scope, Context section") contains the title-
/// cased substring. The assertion catches every shape with one
/// byte check.
///
/// The byte-substring shape holds because:
///   1. `concat!` reassembly: not applicable to Markdown.
///   2. `format!` reassembly: not applicable to Markdown.
///   3. Named constant reference: not applicable to Markdown.
///   4. Method chains / split args: not applicable to Markdown.
///
/// The assertion is case-sensitive on the title-cased phrase.
/// Incidental lowercase prose is not matched and is
/// intentionally permitted.
#[test]
fn test_flow_decompose_project_skill_no_out_of_scope_instruction() {
    let content = common::read_skill("flow-decompose-project");
    assert!(
        !content.contains("Out of Scope"),
        "skills/flow-decompose-project/SKILL.md must not contain \
         the title-cased phrase `Out of Scope` in any markdown \
         shape (bold heading, plain heading, italic, label, or \
         list entry). See .claude/rules/include-bias-in-issues.md \
         for the rule the tombstone enforces."
    );
}

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

/// Tombstone: removed in PR #1489. `skills/flow-decompose-project/SKILL.md`
/// no longer presents a Step 1 DAG-review AskUserQuestion gate. The
/// user's invocation of `/flow:flow-decompose-project` is the single
/// authorization for the decompose-and-file pipeline; the
/// "Review the decomposition" prompt that used to ask for a second
/// approval between Step 1 and Step 2 broke AC#4 of issue #1488.
/// The phrase is a stable source literal (a full English sentence
/// appearing in the SKILL prose), not assembled via `concat!`,
/// `format!`, or constant composition.
#[test]
fn test_flow_decompose_project_no_dag_review_gate() {
    let content = common::read_skill("flow-decompose-project");
    let forbidden = "Review the decomposition";
    assert!(
        !content.contains(forbidden),
        "skills/flow-decompose-project/SKILL.md must not contain the Step 1 DAG-review prompt `{}` (removed in PR #1489)",
        forbidden,
    );
}

/// Tombstone: removed in PR #1489. The flow-decompose-project skill
/// no longer asks the user for a milestone due date in Step 2. The
/// `bin/flow create-milestone` subcommand and the `--milestone` flag
/// on `bin/flow issue` are removed as orphan infrastructure per
/// `.claude/rules/supersession.md` because the only consumer (this
/// skill's Step 2 + Step 3 milestone path) has been deleted. The
/// forbidden phrase is the exact AskUserQuestion prompt the removed
/// gate used.
///
/// Stability: the forbidden phrase is a stable source constant
/// (fragment of a literal English prompt string in the SKILL.md
/// prose). It is never produced by `concat!`, `format!`, or any
/// other runtime string composition — the SKILL.md corpus does
/// not programmatically build prompt strings.
#[test]
fn test_flow_decompose_project_no_due_date_prompt() {
    let content = common::read_skill("flow-decompose-project");
    let forbidden = "milestone due date (YYYY-MM-DD)";
    assert!(
        !content.contains(forbidden),
        "skills/flow-decompose-project/SKILL.md must not contain the Step 2 milestone-due-date prompt `{}` (removed in PR #1489)",
        forbidden,
    );
}

/// Tombstone: removed in PR #1489.
/// `skills/flow-decompose-project/SKILL.md` no longer invokes
/// `bin/flow create-milestone` in Step 3 and no longer passes
/// `--milestone` to `bin/flow issue` in Step 3 or Step 4. The
/// subcommand is deleted from `src/create_milestone.rs`,
/// `src/lib.rs`, and `src/main.rs` (see
/// `test_src_no_create_milestone_module`). Both forbidden
/// substrings are stable source literals — exact CLI subcommand
/// names and a flag name that appear in skill bash blocks.
///
/// Stability: the forbidden substrings are stable source constants
/// (exact CLI subcommand and flag names that appear verbatim in
/// SKILL.md bash blocks). They are never produced by `concat!`,
/// `format!`, or other runtime composition — bash blocks in
/// SKILL.md are written as literal command text, never assembled
/// programmatically.
#[test]
fn test_flow_decompose_project_no_create_milestone_invocation() {
    let content = common::read_skill("flow-decompose-project");
    assert!(
        !content.contains("create-milestone"),
        "skills/flow-decompose-project/SKILL.md must not invoke `bin/flow create-milestone` (subcommand removed in PR #1489)"
    );
    assert!(
        !content.contains("--milestone"),
        "skills/flow-decompose-project/SKILL.md must not pass `--milestone` to `bin/flow issue` (flag removed in PR #1489)"
    );
}

/// Tombstone: removed in PR #1489. The validator-failure
/// AskUserQuestion gates in Steps 3 and 4 of
/// `flow-decompose-project` are replaced by bounded auto-fix loops.
/// The two forbidden phrasings are the exact AskUserQuestion option
/// labels the removed gates used.
///
/// Stability: both forbidden phrasings are stable source constants
/// (literal English option-label strings appearing verbatim in
/// SKILL.md prose). They are never produced by `concat!`,
/// `format!`, or other runtime string composition.
#[test]
fn test_flow_decompose_project_no_validator_failure_gates() {
    let content = common::read_skill("flow-decompose-project");
    assert!(
        !content.contains("Revise the epic body and retry"),
        "skills/flow-decompose-project/SKILL.md must not contain the Step 3 validator-failure prompt option (removed in PR #1489)"
    );
    assert!(
        !content.contains("Revise this child body and retry"),
        "skills/flow-decompose-project/SKILL.md must not contain the Step 4 validator-failure prompt option (removed in PR #1489)"
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
