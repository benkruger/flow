//! Tests for `crate::hooks::agent_prompt_scan` — parent-side Agent
//! tool prompt-body scanning per issue #1704 (branch B + C).

use flow_rs::hooks::agent_prompt_scan::{extract_path_candidates, is_safe_path_candidate};

// --- extract_path_candidates ---

#[test]
fn extract_paths_returns_empty_for_no_input() {
    assert_eq!(extract_path_candidates(""), Vec::<String>::new());
}

#[test]
fn extract_paths_returns_empty_for_no_paths() {
    let prompt = "Read the surrounding context and summarize it";
    assert_eq!(extract_path_candidates(prompt), Vec::<String>::new());
}

#[test]
fn extract_paths_finds_single_absolute_path() {
    let prompt = "Read /Users/alice/notes.md and summarize.";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter().any(|s| s == "/Users/alice/notes.md"),
        "expected /Users/alice/notes.md in {:?}",
        got
    );
}

#[test]
fn extract_paths_finds_multiple_absolute_paths() {
    let prompt = "Read /tmp/a.txt then /var/log/b.log and report.";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter().any(|s| s == "/tmp/a.txt"),
        "expected /tmp/a.txt in {:?}",
        got
    );
    assert!(
        got.iter().any(|s| s == "/var/log/b.log"),
        "expected /var/log/b.log in {:?}",
        got
    );
}

#[test]
fn extract_paths_finds_dotvenv_relative_path() {
    let prompt = "Inspect .venv/lib/python3.11/site-packages/foo.py";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter()
            .any(|s| s == ".venv/lib/python3.11/site-packages/foo.py"),
        "expected .venv/... in {:?}",
        got
    );
}

#[test]
fn extract_paths_finds_path_inside_backticks() {
    let prompt = "Open `/etc/hosts` for inspection.";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter().any(|s| s == "/etc/hosts"),
        "expected /etc/hosts in {:?}",
        got
    );
}

#[test]
fn extract_paths_finds_path_inside_fenced_code_block() {
    let prompt = "```bash\ncat /opt/data/cfg.yaml\n```";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter().any(|s| s == "/opt/data/cfg.yaml"),
        "expected /opt/data/cfg.yaml in {:?}",
        got
    );
}

#[test]
fn extract_paths_ignores_url_fragments() {
    let prompt = "See https://example.com/path/to/page for details.";
    let got = extract_path_candidates(prompt);
    assert!(
        !got.iter().any(|s| s.contains("example.com")),
        "should not extract URL host: {:?}",
        got
    );
    assert!(
        !got.iter().any(|s| s == "/path/to/page"),
        "should not extract URL path fragment: {:?}",
        got
    );
}

#[test]
fn extract_paths_ignores_option_flag_pairs() {
    let prompt = "Use -l/--long for the long form.";
    let got = extract_path_candidates(prompt);
    assert!(
        !got.iter().any(|s| s.contains("--long")),
        "should not extract option-flag pair: {:?}",
        got
    );
}

#[test]
fn extract_paths_handles_path_at_start_of_input() {
    let prompt = "/Users/alice/notes.md is the file";
    let got = extract_path_candidates(prompt);
    assert!(
        got.iter().any(|s| s == "/Users/alice/notes.md"),
        "expected leading path captured with no preceding byte in {:?}",
        got
    );
}

// --- is_safe_path_candidate ---

#[test]
fn validator_rejects_empty() {
    assert!(!is_safe_path_candidate(""));
}

#[test]
fn validator_rejects_nul_byte() {
    assert!(!is_safe_path_candidate("foo\0bar"));
}

#[test]
fn validator_rejects_leading_double_dot() {
    assert!(!is_safe_path_candidate("../etc/passwd"));
}

#[test]
fn validator_rejects_interior_traversal() {
    assert!(!is_safe_path_candidate("/Users/alice/../bob/notes.md"));
}

#[test]
fn validator_accepts_normal_path_token() {
    assert!(is_safe_path_candidate("src/hooks/agent_prompt_scan.rs"));
}

#[test]
fn validator_accepts_absolute_path_token() {
    assert!(is_safe_path_candidate("/Users/alice/notes.md"));
}

#[test]
fn validator_normalizes_input_per_security_gates() {
    // After trim, the input is non-empty, no NULs, no traversal — accept.
    assert!(is_safe_path_candidate("  /Users/alice/notes.md  "));
    // After trim, the input is empty — reject.
    assert!(!is_safe_path_candidate("   "));
}
