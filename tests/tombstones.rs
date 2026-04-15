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
fn test_no_backward_facing_comments_in_rust_source() {
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

/// Four inline dispatch helpers were removed from `src/main.rs` in
/// favor of module-level `run_impl_main` functions so the CLI
/// dispatch paths are testable in-process. If a merge conflict
/// resolution reintroduces any of them, this test fails.
///
/// Tombstone: removed in PR #1156. Dispatch lives in
/// `check_phase::run_impl_main`, `phase_transition::run_impl_main`,
/// `format_status::run_impl_main`, and `tui_data::run_impl_main`.
#[test]
fn test_main_no_inline_dispatch_fns() {
    let root = common::repo_root();
    let main_rs = root.join("src").join("main.rs");
    let content = fs::read_to_string(&main_rs).expect("src/main.rs must be readable");

    const REMOVED_FNS: &[&str] = &[
        "fn run_check_phase(",
        "fn run_phase_transition(",
        "fn run_format_status(",
        "fn run_tui_data(",
    ];

    let mut violations: Vec<&str> = Vec::new();
    for needle in REMOVED_FNS {
        if content.contains(needle) {
            violations.push(needle);
        }
    }

    assert!(
        violations.is_empty(),
        "Inline dispatch fn(s) returned to src/main.rs: {:?}. Each was replaced by a module-level run_impl_main. See PR #1156.",
        violations
    );
}

#[test]
fn test_notify_slack_no_post_message_wrapper() {
    // Tombstone: removed in PR #1157. The three-line `post_message`
    // closure-binder wrapper is superseded by `notify_with_deps`, which
    // takes a `poster` closure and delegates directly to
    // `post_message_inner`. Resurrection via merge conflict must fail.
    let root = common::repo_root();
    let path = root.join("src").join("notify_slack.rs");
    let content = fs::read_to_string(&path).expect("notify_slack.rs must exist");
    assert!(
        !content.contains("pub fn post_message("),
        "post_message wrapper must not return; callers use notify_with_deps + post_message_inner directly"
    );
}

#[test]
fn test_concurrency_no_subprocess_start_lock() {
    // Tombstone: removed in PR #1166. The two thundering-herd lock
    // tests `thundering_herd_zero_delay` and `start_lock_serialization`
    // in `tests/concurrency.rs` call
    // `flow_rs::commands::start_lock::{acquire_with_wait, release}`
    // directly instead of spawning `flow-rs start-lock` subprocesses.
    // Subprocess fork/exec contention under nextest full-suite
    // parallelism inflated the lock-holder's release latency past the
    // worker polling timeout; the library-call shape removes that
    // variability while still exercising the queue, mtime ordering,
    // polling loop, and stale detection. Functional CLI surface
    // verification for the start-lock command lives in
    // `tests/main_dispatch.rs::start_lock_cli_roundtrip`, which
    // exercises `--acquire`, `--check`, and `--release` via real
    // subprocess dispatch.
    //
    // The assertion walks the function body of each converted test
    // (bounded by the next `#[test]` attribute) and fails if
    // `Command::new(FLOW_RS)` appears anywhere in the body — regardless
    // of how the subprocess arguments are constructed. This catches
    // every regression pattern that a byte-substring check on the file
    // as a whole would miss: `concat!`, `format!`, `.join("")`, split
    // constants, `String::push_str`, hex-escape prefixes, chained
    // `.arg()` calls, etc. The bounded scope follows the
    // subsection-local assertion pattern from
    // `.claude/rules/testing-gotchas.md` — walk to the function with
    // `split_once("fn <name>(")`, then walk to the next `#[test]`
    // attribute (or EOF for the last test) to get the body.
    let root = common::repo_root();
    let path = root.join("tests").join("concurrency.rs");
    let content = fs::read_to_string(&path).expect("tests/concurrency.rs must exist");

    const FORBIDDEN: &str = "Command::new(FLOW_RS)";
    const PROTECTED_FNS: &[&str] = &["start_lock_serialization", "thundering_herd_zero_delay"];

    for fn_name in PROTECTED_FNS {
        let marker = format!("fn {}(", fn_name);
        let tail = content
            .split_once(&marker)
            .map(|(_, t)| t)
            .unwrap_or_else(|| {
                panic!(
                    "tests/concurrency.rs is missing `fn {}(` — the tombstone \
                     protects a test that no longer exists. See PR #1166.",
                    fn_name
                )
            });
        let body = tail.split_once("#[test]").map(|(b, _)| b).unwrap_or(tail);
        assert!(
            !body.contains(FORBIDDEN),
            "tests/concurrency.rs::{} must not spawn `flow-rs` subprocesses; \
             use acquire_with_wait() and release() from \
             flow_rs::commands::start_lock directly. Found `{}` in the function \
             body — the library-call shape was reverted. See PR #1166 and \
             tests/main_dispatch.rs::start_lock_cli_roundtrip for CLI surface \
             verification.",
            fn_name,
            FORBIDDEN
        );
    }
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
fn test_coverage_md_must_not_exist() {
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

#[test]
fn docs_with_behavior_no_waiver_discipline_section() {
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
fn claude_md_no_test_coverage_references() {
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

/// Scan a Rust source file for a `pub fn run` wrapper whose body
/// calls `process::exit`. Returns `true` when the forbidden construct
/// is present. The scan is structural — it tolerates whitespace
/// variants (space before paren, newline before paren), generic
/// parameters (`pub fn run<T>`), reference parameters
/// (`pub fn run(args: &Args)`), and renamed arg types
/// (`pub fn run(args: RunArgs)`). A literal byte-substring check
/// against a fixed signature cannot catch these bypasses; the
/// structural scan locates every `pub fn run` token not followed by
/// an identifier character, finds its braced body, and inspects the
/// body for `process::exit`.
fn source_contains_pub_fn_run_with_process_exit(content: &str) -> bool {
    let needle = "pub fn run";
    let mut search_from = 0usize;
    while let Some(rel) = content[search_from..].find(needle) {
        let abs = search_from + rel;
        let after_needle = abs + needle.len();
        search_from = after_needle;

        // Reject matches that extend `run` into a longer identifier
        // (e.g. `pub fn run_impl`, `pub fn run_impl_main`).
        match content[after_needle..].chars().next() {
            Some(c) if c.is_ascii_alphanumeric() || c == '_' => continue,
            Some(_) => {}
            None => continue,
        }

        // Locate the opening brace of the function body.
        let tail = &content[after_needle..];
        let brace_idx = tail.find('{');
        let Some(bi) = brace_idx else {
            continue;
        };

        // Scan the braced body with depth tracking so nested blocks
        // don't terminate the scan early.
        let body_slice = &tail[bi..];
        let mut depth: i32 = 0;
        let mut body_end: Option<usize> = None;
        for (i, ch) in body_slice.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(be) = body_end else {
            continue;
        };
        let body = &body_slice[..=be];

        // Look for `process::exit` in any non-comment span. A naive
        // `line.contains("process::exit")` misidentifies inline
        // comments (e.g. `let x = 1; // process::exit ...`) as
        // matches. Strip everything from `//` onward on each line
        // before searching — this handles both leading and inline
        // comments. It doesn't handle block comments (/* ... */),
        // but those are rare in wrapper bodies and a stricter parser
        // would exceed the tombstone's risk/complexity budget.
        for line in body.lines() {
            let code = match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            };
            if code.contains("process::exit") {
                return true;
            }
        }
    }
    false
}

/// Structural tombstone: scan `format_complete_summary.rs` for any
/// `pub fn run` wrapper whose body calls `process::exit`. The refactor
/// replaced the wrapper with `pub fn run_impl_main(&Args) -> (Value, i32)`
/// so that `process::exit` lives in `dispatch::dispatch_json` instead
/// of the formatter. A merge resolver that reintroduces the wrapper —
/// in any signature variant the adversarial agent proved could bypass
/// a literal check (space before paren, newline before paren, generic
/// parameters, reference parameter, renamed Args type) — regresses
/// the module's coverage by terminating the subprocess before
/// cargo-llvm-cov flushes its profdata.
#[test]
fn test_format_complete_summary_no_pub_fn_run_wrapper() {
    // Tombstone: removed in PR #1176. Must not return.
    let root = common::repo_root();
    let path = root.join("src/format_complete_summary.rs");
    let content = fs::read_to_string(&path).expect("format_complete_summary.rs must exist");
    assert!(
        !source_contains_pub_fn_run_with_process_exit(&content),
        "src/format_complete_summary.rs must not contain a \
         `pub fn run` wrapper whose body calls `process::exit` — \
         use `run_impl_main` + `dispatch::dispatch_json` so \
         `process::exit` is isolated to the dispatcher."
    );
}

/// Structural tombstone: see `test_format_complete_summary_no_pub_fn_run_wrapper`.
#[test]
fn test_format_issues_summary_no_pub_fn_run_wrapper() {
    // Tombstone: removed in PR #1176. Must not return.
    let root = common::repo_root();
    let path = root.join("src/format_issues_summary.rs");
    let content = fs::read_to_string(&path).expect("format_issues_summary.rs must exist");
    assert!(
        !source_contains_pub_fn_run_with_process_exit(&content),
        "src/format_issues_summary.rs must not contain a \
         `pub fn run` wrapper whose body calls `process::exit` — \
         use `run_impl_main` + `dispatch::dispatch_json` so \
         `process::exit` is isolated to the dispatcher."
    );
}

/// Structural tombstone: see `test_format_complete_summary_no_pub_fn_run_wrapper`.
#[test]
fn test_format_pr_timings_no_pub_fn_run_wrapper() {
    // Tombstone: removed in PR #1176. Must not return.
    let root = common::repo_root();
    let path = root.join("src/format_pr_timings.rs");
    let content = fs::read_to_string(&path).expect("format_pr_timings.rs must exist");
    assert!(
        !source_contains_pub_fn_run_with_process_exit(&content),
        "src/format_pr_timings.rs must not contain a \
         `pub fn run` wrapper whose body calls `process::exit` — \
         use `run_impl_main` + `dispatch::dispatch_json` so \
         `process::exit` is isolated to the dispatcher."
    );
}

#[cfg(test)]
mod source_scanner_tests {
    // Unit tests for the structural scanner — ensures it catches
    // every adversarial bypass the literal byte-substring check
    // would miss.
    use super::source_contains_pub_fn_run_with_process_exit as scan;

    #[test]
    fn scanner_catches_canonical_wrapper() {
        let src = "pub fn run(args: Args) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_catches_space_before_paren() {
        let src = "pub fn run (args: Args) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_catches_newline_before_paren() {
        let src = "pub fn run\n    (args: Args) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_catches_generic_parameter() {
        let src = "pub fn run<T>(args: Args) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_catches_reference_parameter() {
        let src = "pub fn run(args: &Args) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_catches_renamed_args_type() {
        let src = "pub fn run(args: RunArgs) { process::exit(1); }\n";
        assert!(scan(src));
    }

    #[test]
    fn scanner_accepts_run_impl_main_without_process_exit() {
        let src = "pub fn run_impl_main(args: &Args) -> (Value, i32) { (Value::Null, 0) }\n";
        assert!(!scan(src));
    }

    #[test]
    fn scanner_accepts_run_impl_fallible() {
        let src = "pub fn run_impl(args: &Args) -> Result<(), String> { Ok(()) }\n";
        assert!(!scan(src));
    }

    #[test]
    fn scanner_ignores_process_exit_in_comment() {
        let src =
            "pub fn run(args: Args) { // process::exit used to be here\n    let _ = args;\n}\n";
        assert!(!scan(src));
    }

    #[test]
    fn scanner_accepts_empty_file() {
        assert!(!scan(""));
    }

    #[test]
    fn scanner_catches_process_exit_inside_nested_block() {
        let src = "pub fn run(args: Args) { if args.foo { process::exit(1); } }\n";
        assert!(scan(src));
    }
}

// --- TUI refactor removals ---
//
// PR #1154 (issue #1135) extracted the TUI's subprocess surface into a
// `TuiAppPlatform` struct and moved the crossterm terminal lifecycle into
// `src/main.rs::run_tui_terminal`. The public API surface lost several
// identifiers: `flow_rs::tui::run` (the module-level entry point),
// `TuiApp::run_terminal` (the method variant that owned the crossterm
// setup+cleanup), and the free functions `open_url` and
// `activate_iterm_tab`. These tombstones fail CI if a merge conflict
// resolution resurrects any of them.

#[test]
fn test_tui_no_free_fn_run_terminal() {
    // Tombstone: removed in PR #1154. Method was superseded by the
    // draw/events-closure seam in `TuiApp::run_event_loop` plus the
    // crossterm glue in `src/main.rs::run_tui_terminal`.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    assert!(
        !content.contains("pub fn run_terminal"),
        "src/tui.rs must not define pub fn run_terminal — superseded by \
         TuiApp::run_event_loop + src/main.rs::run_tui_terminal in PR #1154"
    );
}

#[test]
fn test_tui_no_free_fn_activate_iterm_tab() {
    // Tombstone: removed in PR #1154. The free function became a
    // method on `TuiApp` that reads `self.platform.osascript_binary`
    // so tests can inject a /bin/true stub and cover the spawn path.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    // The method form `fn activate_iterm_tab(&self, ...)` is allowed;
    // the free-fn form `fn activate_iterm_tab(session_tty` (no &self)
    // is the removed shape.
    assert!(
        !content.contains("fn activate_iterm_tab(session_tty"),
        "src/tui.rs must not define the free-fn activate_iterm_tab(session_tty: ...) — \
         superseded by TuiApp::activate_iterm_tab in PR #1154"
    );
}

#[test]
fn test_tui_no_free_fn_open_url() {
    // Tombstone: removed in PR #1154. Free fn was superseded by
    // `TuiApp::open_url` which reads `self.platform.open_binary`.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    assert!(
        !content.contains("fn open_url(url: &str)"),
        "src/tui.rs must not define the free-fn open_url(url: &str) — \
         superseded by TuiApp::open_url in PR #1154"
    );
}

#[test]
fn test_tui_no_free_fn_find_bin_flow() {
    // Tombstone: removed in PR #1154. Binary-path resolution now
    // happens once at `TuiAppPlatform::production()` via
    // `derive_bin_flow_path`, then is cached on the platform struct.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    assert!(
        !content.contains("fn find_bin_flow"),
        "src/tui.rs must not define find_bin_flow — superseded by \
         TuiAppPlatform::production() + derive_bin_flow_path in PR #1154"
    );
}

#[test]
fn test_tui_no_module_level_run_fn() {
    // Tombstone: removed in PR #1154. `flow_rs::tui::run(root,
    // version, repo)` was the pre-refactor entry point that wrapped
    // construction + terminal setup + event loop in one. It is
    // superseded by `TuiApp::new(root, version, repo, platform)` +
    // `run_tui_terminal(&mut app)` in `src/main.rs`.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    assert!(
        !content.contains("pub fn run(root: PathBuf"),
        "src/tui.rs must not define pub fn run(root: PathBuf, ...) — \
         superseded by TuiApp::new + run_tui_terminal in PR #1154"
    );
}

#[test]
fn test_tui_no_module_level_atty_check() {
    // Tombstone: removed in PR #1154. The atty check was inlined
    // into the TUI dispatch arm in `src/main.rs` using libc::isatty
    // directly — the free function form is gone.
    let root = common::repo_root();
    let path = root.join("src/tui.rs");
    let content = fs::read_to_string(&path).expect("src/tui.rs must exist");
    assert!(
        !content.contains("fn atty_check"),
        "src/tui.rs must not define atty_check — the tty check lives \
         inline in src/main.rs after PR #1154"
    );
}
