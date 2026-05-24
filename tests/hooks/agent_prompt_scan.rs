//! Tests for `crate::hooks::agent_prompt_scan` — parent-side Agent
//! tool prompt-body scanning per issue #1704 (branch B + C).

use flow_rs::hooks::agent_prompt_scan::{
    extract_path_candidates, is_safe_path_candidate, validate_agent_prompt, AGENT_PROMPT_BYTE_CAP,
};
use std::path::Path;

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

// --- validate_agent_prompt ---

const WORKTREE: &str = "/Users/alice/.worktrees/feat";

#[test]
fn validate_agent_prompt_silent_outside_active_flow() {
    let (allowed, msg) = validate_agent_prompt(
        Some("Read /etc/hosts for inspection."),
        Path::new(WORKTREE),
        false,
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_missing_prompt_field() {
    let (allowed, msg) = validate_agent_prompt(None, Path::new(WORKTREE), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_empty_prompt() {
    let (allowed, msg) = validate_agent_prompt(Some(""), Path::new(WORKTREE), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_in_worktree_path() {
    // Relative `./src/lib.rs` joins onto worktree and normalizes
    // inside — exercises both the relative-candidate `Path::join`
    // branch and the CurDir arm of `normalize_path_lexical`.
    let prompt = "Read ./src/lib.rs for context.";
    let (allowed, msg) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true);
    assert!(allowed, "expected allow; got msg={}", msg);
}

#[test]
fn validate_agent_prompt_blocks_absolute_path_outside_worktree() {
    let prompt = "Read /etc/hosts and report the contents.";
    let (allowed, _) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true);
    assert!(!allowed);
}

#[test]
fn validate_agent_prompt_blocks_dotvenv_path_outside_worktree() {
    // .venv/ paths sit outside the worktree because the worktree root
    // is not a parent of .venv. When the resolved (worktree + .venv/...)
    // path normalizes to inside the worktree it's allowed; this test
    // pins the bare ".venv" candidate which contains traversal-free
    // segments and resolves to an out-of-worktree absolute reference.
    let prompt = "Inspect /home/alice/.venv/lib/foo.py";
    let (allowed, _) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true);
    assert!(!allowed);
}

#[test]
fn validate_agent_prompt_message_names_offending_path_and_worktree() {
    let (_, msg) = validate_agent_prompt(Some("Read /etc/hosts."), Path::new(WORKTREE), true);
    assert!(
        msg.contains("/etc/hosts"),
        "message must name path: {}",
        msg
    );
    assert!(
        msg.contains(WORKTREE),
        "message must name worktree: {}",
        msg
    );
}

#[test]
fn validate_agent_prompt_byte_capped_at_prompt_length_limit() {
    // Construct a prompt larger than AGENT_PROMPT_BYTE_CAP. Pad with
    // an ASCII run to within 2 bytes of the cap, then insert a
    // 4-byte UTF-8 codepoint straddling the cap boundary so the
    // char-boundary back-walk loop is exercised. Followed by a
    // /etc/hosts past the cap. The cap-sliced prefix should produce
    // no candidates → allow.
    let pad_len = AGENT_PROMPT_BYTE_CAP - 2;
    let prompt = format!("{}{}{}", "a".repeat(pad_len), "🦀", " /etc/hosts");
    let (allowed, _) = validate_agent_prompt(Some(&prompt), Path::new(WORKTREE), true);
    assert!(allowed, "post-cap content must not reach the scanner");
}

#[test]
fn validate_agent_prompt_blocks_traversal_path() {
    // Regex matches `/etc/../passwd`; validator rejects it for
    // containing `/../` — exercises the malformed-candidate branch
    // in validate_agent_prompt.
    let prompt = "Read /etc/../passwd and report.";
    let (allowed, msg) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true);
    assert!(!allowed);
    assert!(
        msg.contains("malformed"),
        "message must name malformed token: {}",
        msg
    );
}

#[test]
fn validate_agent_prompt_blocks_absolute_with_trailing_parentdir() {
    // `/tmp/foo/..` passes the validator (no leading `..`, no
    // interior `/../`) and resolves outside the worktree after
    // normalize_path_lexical pops `foo` — exercises the ParentDir
    // arm of normalize_path_lexical and the outside-worktree
    // rejection in validate_agent_prompt.
    let prompt = "Inspect /tmp/foo/.. and report.";
    let (allowed, _) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true);
    assert!(!allowed);
}
