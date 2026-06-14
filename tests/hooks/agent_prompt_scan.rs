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

// Existing call sites pass `None` for plugin_root: these cases exercise
// behavior unrelated to the plugin-root carve-out, so no carve-out is in
// effect. The carve-out's own cases below pass a `Some(...)` value.
const NO_PLUGIN_ROOT: Option<&str> = None;

#[test]
fn validate_agent_prompt_silent_outside_active_flow() {
    let (allowed, msg) = validate_agent_prompt(
        Some("Read /etc/hosts for inspection."),
        Path::new(WORKTREE),
        false,
        NO_PLUGIN_ROOT,
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_missing_prompt_field() {
    let (allowed, msg) = validate_agent_prompt(None, Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_empty_prompt() {
    let (allowed, msg) = validate_agent_prompt(Some(""), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn validate_agent_prompt_allows_in_worktree_path() {
    // Relative `./src/lib.rs` joins onto worktree and normalizes
    // inside — exercises both the relative-candidate `Path::join`
    // branch and the CurDir arm of `normalize_path_lexical`.
    let prompt = "Read ./src/lib.rs for context.";
    let (allowed, msg) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(allowed, "expected allow; got msg={}", msg);
}

#[test]
fn validate_agent_prompt_blocks_absolute_path_outside_worktree() {
    let prompt = "Read /etc/hosts and report the contents.";
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
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
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(!allowed);
}

#[test]
fn validate_agent_prompt_message_names_offending_path_and_worktree() {
    let (_, msg) = validate_agent_prompt(
        Some("Read /etc/hosts."),
        Path::new(WORKTREE),
        true,
        NO_PLUGIN_ROOT,
    );
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
    let (allowed, _) =
        validate_agent_prompt(Some(&prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(allowed, "post-cap content must not reach the scanner");
}

#[test]
fn validate_agent_prompt_blocks_traversal_path() {
    // Regex matches `/etc/../passwd`; validator rejects it for
    // containing `/../` — exercises the malformed-candidate branch
    // in validate_agent_prompt.
    let prompt = "Read /etc/../passwd and report.";
    let (allowed, msg) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
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
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(!allowed);
}

// --- plugin-root carve-out ---

const PLUGIN_ROOT: &str = "/Users/alice/.claude/plugins/flow";

#[test]
fn agent_prompt_allows_path_under_plugin_root() {
    // The parent substitutes the resolved absolute plugin `bin/flow`
    // path into the adversarial / ci-fixer agent prompt. That path is
    // under the plugin root, outside the worktree — the carve-out must
    // allow it so engaging the worktree gate does not hard-block the
    // launch.
    let prompt = "Re-run /Users/alice/.claude/plugins/flow/bin/flow ci to verify.";
    let (allowed, msg) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, Some(PLUGIN_ROOT));
    assert!(allowed, "plugin-root path must be allowed; msg={msg}");
}

#[test]
fn agent_prompt_blocks_out_of_worktree_path_when_plugin_root_set() {
    // The carve-out is scoped to the plugin-root subtree only: a path
    // outside both the worktree AND the plugin root is still blocked
    // even when plugin_root is a valid absolute path.
    let prompt = "Read /etc/hosts and report.";
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, Some(PLUGIN_ROOT));
    assert!(
        !allowed,
        "non-plugin-root out-of-worktree path must block even with plugin_root set"
    );
}

#[test]
fn agent_prompt_plugin_root_carveout_fails_closed_when_unset() {
    // Fail closed: plugin_root None means no carve-out — a path that
    // WOULD be under the plugin root is blocked because the gate cannot
    // confirm the plugin root.
    let prompt = "Run /Users/alice/.claude/plugins/flow/bin/flow ci now.";
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(!allowed, "unset plugin_root must not admit the carve-out");
}

#[test]
fn agent_prompt_plugin_root_carveout_fails_closed_when_empty() {
    // Fail closed: an empty plugin_root (after trim) is not a usable
    // root — block.
    let prompt = "Run /Users/alice/.claude/plugins/flow/bin/flow ci now.";
    let (allowed, _) = validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, Some("   "));
    assert!(!allowed, "empty plugin_root must not admit the carve-out");
}

#[test]
fn agent_prompt_plugin_root_carveout_fails_closed_when_not_absolute() {
    // Fail closed: a non-absolute plugin_root is rejected before the
    // prefix comparison.
    let prompt = "Run /Users/alice/.claude/plugins/flow/bin/flow ci now.";
    let (allowed, _) = validate_agent_prompt(
        Some(prompt),
        Path::new(WORKTREE),
        true,
        Some("relative/plugins/flow"),
    );
    assert!(
        !allowed,
        "non-absolute plugin_root must not admit the carve-out"
    );
}

#[test]
fn agent_prompt_plugin_root_carveout_trims_and_strips_nul() {
    // The env value is hygiene-normalized (NUL strip + surrounding
    // whitespace trim) before the absolute/prefix checks — but the
    // comparison itself is case-preserving (paths are case-sensitive).
    let prompt = "Re-run /Users/alice/.claude/plugins/flow/bin/flow ci to verify.";
    let padded = "  \0/Users/alice/.claude/plugins/flow\0  ";
    let (allowed, msg) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, Some(padded));
    assert!(
        allowed,
        "trimmed/NUL-stripped plugin_root must admit the carve-out; msg={msg}"
    );
}

// --- .flow-states/ carve-out (Task 10 Branch Enumeration Table) ---

#[test]
fn agent_prompt_allows_flow_states_diff_path() {
    // Branch A: the reviewer launch passes the substantive-diff path
    // (under this flow's own <project_root>/.flow-states/<branch>/,
    // outside the worktree) in the agent prompt. The carve-out derives
    // project_root (/Users/alice) and branch (feat) from the
    // worktree_root and allows the .flow-states/feat/ candidate so
    // engaging the worktree gate does not hard-block Review sub-agent
    // launches.
    let prompt = "Read the substantive diff at \
                  /Users/alice/.flow-states/feat/full-diff.diff and review it.";
    let (allowed, msg) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(allowed, "flow-states diff path must be allowed; msg={msg}");
}

#[test]
fn agent_prompt_blocks_arbitrary_out_of_worktree_path() {
    // Branch B: a candidate that is outside the worktree AND outside
    // this flow's <project_root>/.flow-states/<branch>/ subtree is
    // still blocked. The carve-out is scoped to the branch subtree
    // only — it does not widen to the whole project root.
    let prompt = "Read /Users/alice/src/other.rs and report.";
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(!allowed, "non-.flow-states out-of-worktree path must block");
}

#[test]
fn agent_prompt_flow_states_derive_handles_no_worktrees_segment() {
    // Branch C: when worktree_root lacks the /.worktrees/ anchor,
    // compute_worktree_paths returns None, so project_root cannot be
    // derived and the carve-out does not apply — the out-of-worktree
    // candidate (even a .flow-states/-shaped one) is blocked rather
    // than crashing.
    let worktree_root = Path::new("/Users/alice/plainroot");
    let prompt = "Read /Users/alice/elsewhere/.flow-states/x and report.";
    let (allowed, _) = validate_agent_prompt(Some(prompt), worktree_root, true, NO_PLUGIN_ROOT);
    assert!(
        !allowed,
        "no-/.worktrees/ worktree_root yields no project_root → block"
    );
}

#[test]
fn agent_prompt_flow_states_derive_uses_rightmost_worktrees() {
    // Branch D: a worktree_root whose project_root itself contains a
    // `/.worktrees/` directory must resolve project_root at the
    // RIGHTMOST anchor (via compute_worktree_paths' rfind). The
    // carve-out's branch subtree is therefore
    // /home/dev/.worktrees/outer/.flow-states/feat, not
    // /home/dev/.flow-states/feat. A diff path under the rightmost
    // project_root's .flow-states/<branch>/ is allowed; a leftmost
    // derivation would have blocked it.
    let worktree_root = Path::new("/home/dev/.worktrees/outer/.worktrees/feat");
    let prompt = "Review /home/dev/.worktrees/outer/.flow-states/feat/full-diff.diff now.";
    let (allowed, msg) = validate_agent_prompt(Some(prompt), worktree_root, true, NO_PLUGIN_ROOT);
    assert!(
        allowed,
        "rightmost project_root .flow-states/<branch>/ must be allowed; msg={msg}"
    );
}

#[test]
fn agent_prompt_blocks_other_branch_flow_states() {
    // Branch-scope regression guard: a `.flow-states/` candidate under
    // a DIFFERENT branch than the current worktree (worktree_root =
    // /Users/alice/.worktrees/feat → branch `feat`) must be blocked.
    // `/Users/alice/.flow-states/other-branch/state.json` is under the
    // shared `.flow-states/` root but NOT under `.flow-states/feat/`,
    // so the branch-scoped carve-out does not admit it — preventing a
    // sub-agent prompt from being pointed at another concurrent flow's
    // per-branch state. A whole-`.flow-states/`-root carve-out would
    // have allowed it.
    let prompt = "Read /Users/alice/.flow-states/other-branch/state.json and report.";
    let (allowed, _) =
        validate_agent_prompt(Some(prompt), Path::new(WORKTREE), true, NO_PLUGIN_ROOT);
    assert!(
        !allowed,
        "a .flow-states/ path under a different branch must be blocked"
    );
}
