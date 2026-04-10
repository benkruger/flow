//! PreToolUse hook validator for Bash and Agent tool calls.
//!
//! For Bash calls, checks the command against blocked patterns (compound
//! commands, redirection, file-read commands, deny list, whitelist).
//!
//! For Agent calls, blocks `general-purpose` sub-agents during active
//! FLOW phases. Custom plugin agents (`flow:*`) and specialized types
//! (`Explore`, `Plan`) are allowed through.
//!
//! Exit 0 — allow (command passes through to normal permission system)
//! Exit 2 — block (error message on stderr is fed back to the sub-agent)

use regex::Regex;
use serde_json::Value;

use super::{
    build_permission_regexes, detect_branch_from_cwd, find_settings_and_root, is_flow_active,
    read_hook_input, resolve_main_root, FILE_READ_COMMANDS,
};

/// Validate a Bash command string.
///
/// Returns `(allowed, message)`. Message is empty if allowed.
///
/// Layers 1-8 (compound commands, redirection, exec prefix, blanket
/// restore, git diff with file args, deny list, file-read commands)
/// are always enforced.
///
/// Layer 9 (whitelist enforcement) is only enforced when both settings
/// are provided AND `flow_active` is true.
pub fn validate(command: &str, settings: Option<&Value>, flow_active: bool) -> (bool, String) {
    // Layer 1: Block compound commands (&&, ;, |)
    if command.contains("&&") || command.contains('|') || has_unescaped_semicolon(command) {
        return (
            false,
            "BLOCKED: Compound commands (&&, ;, |) are not allowed. \
             Use separate Bash calls for each command."
                .to_string(),
        );
    }

    // Layer 2: Block shell redirection (>, >>, 2>, etc.)
    if has_redirect(command) {
        return (
            false,
            "BLOCKED: Shell redirection (>, >>) is not allowed. \
             Use the Read tool to view file contents and the \
             Write tool to create files."
                .to_string(),
        );
    }

    // Layer 3: Block exec prefix — triggers Claude Code's built-in
    // "evaluates arguments as shell code" safety heuristic, causing
    // permission prompts that break autonomous flows. Plain command
    // invocation is functionally identical.
    let stripped = command.trim();
    if stripped.starts_with("exec ") {
        return (
            false,
            "BLOCKED: 'exec' prefix triggers a permission prompt. \
             Remove 'exec' and run the command directly — \
             the behavior is identical."
                .to_string(),
        );
    }

    // Layer 5: Block blanket restore (git restore . wipes all changes)
    if stripped == "git restore ." {
        return (
            false,
            "BLOCKED: 'git restore .' discards ALL changes without review. \
             Use 'git restore <file>' for each file individually. \
             Before restoring, run 'git diff' to capture what will be lost."
                .to_string(),
        );
    }

    // Layer 6: Block git diff with file-path arguments
    if stripped.starts_with("git diff") {
        // Check for " -- " followed by a non-space character
        let re = Regex::new(r" -- \S").unwrap();
        if re.is_match(stripped) {
            return (
                false,
                "BLOCKED: 'git diff' with file path arguments is not allowed. \
                 Use the Read tool to view file contents and the Grep tool \
                 to search for patterns."
                    .to_string(),
            );
        }
    }

    // Layer 7: Deny-list check — deny always wins over allow
    if let Some(settings) = settings {
        let deny_regexes = build_permission_regexes(settings, "deny");
        for regex in &deny_regexes {
            if regex.is_match(stripped) {
                return (
                    false,
                    format!(
                        "BLOCKED: Command matches deny list: '{}'. \
                         This operation is explicitly forbidden.",
                        command
                    ),
                );
            }
        }
    }

    // Layer 8: Block file-read commands
    let first_word = stripped.split_whitespace().next().unwrap_or("");
    if FILE_READ_COMMANDS.contains(&first_word) {
        return (
            false,
            format!(
                "BLOCKED: '{}' is not allowed. \
                 Use the dedicated tool instead \
                 (Read for cat/head/tail, Grep for grep/rg, \
                 Glob for find/ls).",
                first_word
            ),
        );
    }

    // Layer 9: Whitelist check — only during an active flow
    if let Some(settings) = settings {
        if flow_active {
            let allow_regexes = build_permission_regexes(settings, "allow");
            if !allow_regexes.is_empty() && !allow_regexes.iter().any(|r| r.is_match(command)) {
                return (
                    false,
                    format!(
                        "BLOCKED: Command not in allow list: '{}'. \
                         Check .claude/settings.json allow patterns.",
                        command
                    ),
                );
            }
        }
    }

    (true, String::new())
}

/// Check for unescaped semicolons in a command string.
fn has_unescaped_semicolon(command: &str) -> bool {
    let bytes = command.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b';' && (i == 0 || bytes[i - 1] != b'\\') {
            return true;
        }
    }
    false
}

/// Check for shell redirection operators (>, >>, 2>, etc.).
/// Excludes `=` and `-` immediately before `>` (e.g. `--format=>%s`).
fn has_redirect(command: &str) -> bool {
    let bytes = command.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'>' {
            if i > 0 && (bytes[i - 1] == b'=' || bytes[i - 1] == b'-') {
                continue;
            }
            return true;
        }
    }
    false
}

/// Determine whether a command should be blocked from run_in_background.
///
/// `bin/flow` (any subcommand) and `bin/ci` are always blocked — every
/// `bin/flow` subcommand is either a CI gate or a state mutation, and
/// `bin/ci` is a CI gate by convention. Other commands are only
/// blocked from background execution during an active FLOW phase.
///
/// Returns `Some(error_message)` if the command should be blocked,
/// `None` if the command is allowed to run in the background.
pub fn should_block_background(command: &str, flow_active: bool) -> Option<String> {
    if is_flow_command(command) {
        return Some(
            "BLOCKED: bin/flow and bin/ci must never run in the background. \
             Every bin/flow subcommand is a gate or state mutation — it must \
             complete before any downstream action proceeds. \
             Run it in the foreground."
                .to_string(),
        );
    }
    if flow_active {
        return Some(
            "BLOCKED: run_in_background is not allowed during a FLOW phase. \
             Use parallel foreground calls instead."
                .to_string(),
        );
    }
    None
}

/// Validate an Agent tool call by subagent type.
///
/// During an active FLOW phase, blocks `general-purpose` sub-agents
/// (explicit or default when `subagent_type` is absent). All other
/// types — custom plugin agents (`flow:*`), specialized built-in
/// types (`Explore`, `Plan`), etc. — are allowed through.
///
/// Outside a FLOW phase, all agent types are allowed.
///
/// Returns `(allowed, message)`. Message is empty if allowed.
pub fn validate_agent(subagent_type: Option<&str>, flow_active: bool) -> (bool, String) {
    if !flow_active {
        return (true, String::new());
    }
    let normalized = subagent_type.map(|s| s.trim().to_ascii_lowercase());
    let is_general_purpose = match normalized.as_deref() {
        None | Some("") | Some("general-purpose") => true,
        Some(_) => false,
    };
    if is_general_purpose {
        return (
            false,
            "BLOCKED: general-purpose sub-agents are not allowed during FLOW phases. \
             Use a custom plugin sub-agent (flow:ci-fixer, flow:reviewer, etc.) or \
             a specialized agent type (Explore, Plan) instead."
                .to_string(),
        );
    }
    (true, String::new())
}

/// Check whether a command invokes bin/flow (any subcommand) or bin/ci.
///
/// Matches by tokenizing on whitespace, so path prefixes and trailing
/// arguments are handled. The suffix match on `/bin/ci` and `/bin/flow`
/// is intentional: it covers both FLOW's own binary and target projects'
/// `bin/ci` scripts, which are CI gates by convention. Rejects
/// substring-containing commands like `npm run ci` (first token is `npm`)
/// and `git commit`.
fn is_flow_command(command: &str) -> bool {
    let first = match command.split_whitespace().next() {
        Some(t) => t,
        None => return false,
    };
    if first == "bin/ci" || first.ends_with("/bin/ci") {
        return true;
    }
    first == "bin/flow" || first.ends_with("/bin/flow")
}

/// Check whether a JSON value represents a truthy `run_in_background` flag.
///
/// Claude Code's Bash tool schema defines `run_in_background` as a bool,
/// but we defensively accept truthy non-bool forms (string `"true"`,
/// non-zero integer) so a schema-confused caller cannot bypass the CI
/// gate by passing the wrong JSON type. Null, bool false, empty string,
/// zero, and non-truthy strings all return false.
fn is_bg_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::String(s) => s.eq_ignore_ascii_case("true") || s == "1",
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Run the validate-pretool hook (entry point from CLI).
pub fn run() {
    let hook_input = match read_hook_input() {
        Some(input) => input,
        None => std::process::exit(0),
    };

    let (settings, project_root) = find_settings_and_root();
    let branch = if settings.is_some() {
        detect_branch_from_cwd()
    } else {
        None
    };
    let main_root = project_root.as_ref().map(|r| resolve_main_root(r));
    let flow_active = match (&branch, &main_root) {
        (Some(b), Some(r)) => is_flow_active(b, r),
        _ => false,
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Pre-validation: CI is always a gate; other commands only blocked in FLOW phases
    if let Some(bg) = tool_input.get("run_in_background") {
        if is_bg_truthy(bg) {
            if let Some(msg) = should_block_background(command, flow_active) {
                eprintln!("{}", msg);
                std::process::exit(2);
            }
        }
    }
    if command.is_empty() {
        // No command means this is an Agent tool call, not Bash.
        let subagent_type = tool_input.get("subagent_type").and_then(|v| v.as_str());
        let (allowed, message) = validate_agent(subagent_type, flow_active);
        if !allowed {
            eprintln!("{}", message);
            std::process::exit(2);
        }
        std::process::exit(0);
    }

    let (allowed, message) = validate(command, settings.as_ref(), flow_active);
    if !allowed {
        eprintln!("{}", message);
        std::process::exit(2);
    }

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_settings() -> Value {
        json!({
            "permissions": {
                "allow": [
                    "Bash(git status)",
                    "Bash(git diff *)",
                    "Bash(*bin/*)",
                ],
                "deny": []
            }
        })
    }

    fn deny_settings() -> Value {
        json!({
            "permissions": {
                "allow": ["Bash(git *)"],
                "deny": [
                    "Bash(git rebase *)",
                    "Bash(git push --force *)",
                    "Bash(git push -f *)",
                    "Bash(git reset --hard *)",
                    "Bash(git stash *)",
                    "Bash(git checkout *)",
                    "Bash(git clean *)",
                ]
            }
        })
    }

    // --- Basic allow tests ---

    #[test]
    fn test_allows_bin_flow_ci() {
        let (allowed, msg) = validate("bin/flow ci", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_bin_ci() {
        let (allowed, msg) = validate("bin/ci", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_git_add() {
        let (allowed, msg) = validate("git add -A", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_git_diff() {
        let (allowed, msg) = validate("git diff HEAD", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_empty_command() {
        let (allowed, msg) = validate("", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    // --- Compound command blocking ---

    #[test]
    fn test_blocks_compound_and() {
        let (allowed, msg) = validate("cd .worktrees/test && git status", None, true);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
        assert!(msg.contains("separate Bash calls"));
    }

    #[test]
    fn test_blocks_compound_semicolon() {
        let (allowed, msg) = validate("bin/ci; echo done", None, true);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
    }

    #[test]
    fn test_blocks_pipe() {
        let (allowed, msg) = validate("git show HEAD:file.py | sed 's/foo/bar/'", None, true);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
        assert!(msg.contains("separate Bash calls"));
    }

    #[test]
    fn test_blocks_or_operator() {
        let (allowed, msg) = validate("bin/ci || echo failed", None, true);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
    }

    // --- File-read command blocking ---

    #[test]
    fn test_blocks_cat() {
        let (allowed, msg) = validate("cat lib/foo.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("Read"));
    }

    #[test]
    fn test_blocks_grep() {
        let (allowed, msg) = validate("grep -r 'pattern' lib/", None, true);
        assert!(!allowed);
        assert!(msg.contains("Grep"));
    }

    #[test]
    fn test_blocks_rg() {
        let (allowed, msg) = validate("rg 'pattern' lib/", None, true);
        assert!(!allowed);
        assert!(msg.contains("Grep"));
    }

    #[test]
    fn test_blocks_find() {
        let (allowed, msg) = validate("find . -name '*.py'", None, true);
        assert!(!allowed);
        assert!(msg.contains("Glob"));
    }

    #[test]
    fn test_blocks_ls() {
        let (allowed, msg) = validate("ls -la lib/", None, true);
        assert!(!allowed);
        assert!(msg.contains("Glob"));
    }

    #[test]
    fn test_blocks_head() {
        let (allowed, msg) = validate("head -20 lib/foo.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("Read"));
    }

    #[test]
    fn test_blocks_tail() {
        let (allowed, msg) = validate("tail -f log.txt", None, true);
        assert!(!allowed);
        assert!(msg.contains("Read"));
    }

    // --- Exec prefix ---

    #[test]
    fn test_blocks_exec_prefix() {
        let (allowed, msg) = validate("exec /Users/ben/code/flow/bin/flow ci", None, true);
        assert!(!allowed);
        assert!(msg.contains("exec"));
        assert!(msg.contains("permission prompt"));
    }

    #[test]
    fn test_blocks_exec_bare_command() {
        let (allowed, msg) = validate("exec bin/flow ci", None, true);
        assert!(!allowed);
        assert!(msg.contains("exec"));
    }

    #[test]
    fn test_allows_command_without_exec() {
        let (allowed, msg) = validate("/Users/ben/code/flow/bin/flow ci", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    // --- Blanket restore ---

    #[test]
    fn test_blocks_git_restore_dot() {
        let (allowed, msg) = validate("git restore .", None, true);
        assert!(!allowed);
        assert!(msg.contains("git restore ."));
        assert!(msg.contains("individually"));
    }

    #[test]
    fn test_allows_git_restore_specific_file() {
        let (allowed, msg) = validate("git restore lib/foo.py", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    // --- Git diff with file args ---

    #[test]
    fn test_blocks_git_diff_with_file_args() {
        let (allowed, msg) = validate("git diff origin/main..HEAD -- file.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
        assert!(msg.contains("Read"));
    }

    #[test]
    fn test_blocks_git_diff_head_with_file_args() {
        let (allowed, msg) = validate("git diff HEAD -- src/lib/foo.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_blocks_git_diff_cached_with_file_args() {
        let (allowed, msg) = validate("git diff --cached -- file.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_allows_git_diff_without_file_args() {
        let (allowed, msg) = validate("git diff origin/main..HEAD", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_git_diff_stat() {
        let (allowed, msg) = validate("git diff --stat", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    // --- Whitelist ---

    #[test]
    fn test_whitelist_allows_matching_command() {
        let s = sample_settings();
        let (allowed, msg) = validate("git status", Some(&s), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_whitelist_allows_glob_match() {
        let s = sample_settings();
        let (allowed, msg) = validate("git diff HEAD", Some(&s), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_whitelist_allows_bin_glob() {
        let s = sample_settings();
        let (allowed, _) = validate("bin/ci", Some(&s), true);
        assert!(allowed);
    }

    #[test]
    fn test_whitelist_allows_leading_glob() {
        let s = sample_settings();
        let (allowed, _) = validate("/usr/local/bin/flow ci", Some(&s), true);
        assert!(allowed);
    }

    #[test]
    fn test_whitelist_allows_chmod_absolute_path() {
        let s = json!({"permissions": {"allow": ["Bash(chmod +x *)"], "deny": []}});
        let (allowed, msg) = validate(
            "chmod +x /Users/ben/code/hh/.worktrees/feature/bin/qa",
            Some(&s),
            true,
        );
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_whitelist_blocks_unmatched_command() {
        let s = sample_settings();
        let (allowed, msg) = validate("curl http://example.com", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("not in allow list"));
        assert!(msg.contains("curl http://example.com"));
    }

    #[test]
    fn test_whitelist_blocks_rm_rf() {
        let s = sample_settings();
        let (allowed, msg) = validate("rm -rf /", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("not in allow list"));
    }

    #[test]
    fn test_whitelist_skipped_when_no_settings() {
        let (allowed, msg) = validate("curl http://example.com", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_whitelist_skipped_when_empty_allow() {
        let s = json!({"permissions": {"allow": []}});
        let (allowed, _) = validate("curl http://example.com", Some(&s), true);
        assert!(allowed);
    }

    // --- flow_active parameter ---

    #[test]
    fn test_flow_active_false_allows_unlisted_command() {
        let s = sample_settings();
        let (allowed, msg) = validate("npm test", Some(&s), false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_flow_active_true_blocks_unlisted_command() {
        let s = sample_settings();
        let (allowed, msg) = validate("npm test", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("not in allow list"));
    }

    #[test]
    fn test_flow_active_false_still_blocks_compound() {
        let s = sample_settings();
        let (allowed, msg) = validate("git status && git diff", Some(&s), false);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
    }

    #[test]
    fn test_flow_active_false_still_blocks_file_read() {
        let s = sample_settings();
        let (allowed, msg) = validate("cat README.md", Some(&s), false);
        assert!(!allowed);
        assert!(msg.contains("Read"));
    }

    #[test]
    fn test_flow_active_false_still_blocks_deny() {
        let s = deny_settings();
        let (allowed, msg) = validate("git rebase main", Some(&s), false);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    #[test]
    fn test_flow_active_false_still_blocks_redirect() {
        let s = sample_settings();
        let (allowed, msg) = validate("git log > /tmp/out.txt", Some(&s), false);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("redirection"));
    }

    #[test]
    fn test_flow_active_default_blocks_unlisted() {
        let s = sample_settings();
        let (allowed, msg) = validate("npm test", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("not in allow list"));
    }

    #[test]
    fn test_compound_blocked_before_whitelist() {
        let s = sample_settings();
        let (allowed, msg) = validate("git status && git diff", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("Compound commands"));
    }

    #[test]
    fn test_file_read_blocked_before_whitelist() {
        let s = sample_settings();
        let (allowed, msg) = validate("cat README.md", Some(&s), true);
        assert!(!allowed);
        assert!(msg.contains("Read"));
    }

    // --- Deny list ---

    #[test]
    fn test_deny_blocks_matching_command() {
        let s = deny_settings();
        let (allowed, msg) = validate("git rebase main", Some(&s), true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let s = deny_settings();
        let (allowed, msg) = validate("git checkout feature-branch", Some(&s), true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    #[test]
    fn test_deny_blocks_force_push() {
        let s = deny_settings();
        let (allowed, msg) = validate("git push --force origin main", Some(&s), true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    #[test]
    fn test_deny_blocks_hard_reset() {
        let s = deny_settings();
        let (allowed, msg) = validate("git reset --hard HEAD~1", Some(&s), true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    #[test]
    fn test_deny_allows_non_matching_command() {
        let s = deny_settings();
        let (allowed, msg) = validate("git status", Some(&s), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_deny_skipped_when_no_settings() {
        let (allowed, msg) = validate("git rebase main", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_deny_skipped_when_empty_deny() {
        let s = json!({"permissions": {"allow": ["Bash(git status)"], "deny": []}});
        let (allowed, msg) = validate("git status", Some(&s), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_deny_skipped_when_no_deny_key() {
        let s = json!({"permissions": {"allow": ["Bash(git status)"]}});
        let (allowed, msg) = validate("git status", Some(&s), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_deny_runs_before_allow() {
        let s = json!({
            "permissions": {
                "allow": ["Bash(git stash *)"],
                "deny": ["Bash(git stash *)"]
            }
        });
        let (allowed, msg) = validate("git stash save", Some(&s), true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("deny"));
    }

    // --- Redirect blocking ---

    #[test]
    fn test_blocks_redirect_output() {
        let (allowed, msg) = validate("git show HEAD:file.py > /tmp/out.py", None, true);
        assert!(!allowed);
        assert!(msg.contains("Read tool"));
        assert!(msg.contains("Write tool"));
    }

    #[test]
    fn test_blocks_redirect_append() {
        let (allowed, msg) = validate("git log >> /tmp/out.txt", None, true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("redirection"));
    }

    #[test]
    fn test_blocks_redirect_stderr() {
        let (allowed, msg) = validate("git status 2> /tmp/err.txt", None, true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("redirection"));
    }

    #[test]
    fn test_blocks_redirect_no_space() {
        let (allowed, msg) = validate("git show HEAD:file.py>/tmp/out.py", None, true);
        assert!(!allowed);
        assert!(msg.to_lowercase().contains("redirection"));
    }

    #[test]
    fn test_allows_no_redirect() {
        let (allowed, msg) = validate("git diff --diff-filter=M", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_allows_arrow_in_flag() {
        let (allowed, msg) = validate("git log --format=>%s", None, true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    // --- run_in_background blocking ---

    #[test]
    fn test_blocks_background_bin_flow_ci_outside_flow() {
        let msg = should_block_background("bin/flow ci", false);
        assert!(msg.is_some());
        let text = msg.unwrap();
        assert!(text.contains("bin/flow"));
        assert!(text.contains("bin/ci"));
    }

    #[test]
    fn test_blocks_background_bin_flow_ci_with_args_outside_flow() {
        let msg = should_block_background("bin/flow ci --retry 3", false);
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_bin_ci_outside_flow() {
        let msg = should_block_background("bin/ci", false);
        assert!(msg.is_some());
        // Error message must name both forms so callers that ran `bin/ci`
        // don't get misled by a message that only names `bin/flow`.
        assert!(msg.unwrap().contains("bin/ci"));
    }

    #[test]
    fn test_blocks_background_absolute_bin_flow_ci_outside_flow() {
        let msg = should_block_background("/Users/ben/code/flow/bin/flow ci", false);
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_absolute_bin_ci_outside_flow() {
        let msg = should_block_background("/Users/ben/code/flow/bin/ci", false);
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_bin_flow_finalize_commit() {
        let msg = should_block_background("bin/flow finalize-commit .flow-commit-msg main", false);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("bin/flow"));
    }

    #[test]
    fn test_blocks_background_bin_flow_phase_transition() {
        let msg = should_block_background("bin/flow phase-transition --action complete", false);
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_absolute_bin_flow_finalize_commit() {
        let msg = should_block_background(
            "/Users/ben/code/flow/bin/flow finalize-commit .flow-commit-msg main",
            false,
        );
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_bare_bin_flow() {
        let msg = should_block_background("bin/flow", false);
        assert!(msg.is_some());
    }

    #[test]
    fn test_blocks_background_any_command_inside_flow() {
        let msg = should_block_background("echo hi", true);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("FLOW phase"));
    }

    #[test]
    fn test_allows_background_non_flow_outside_flow() {
        let msg = should_block_background("echo hi", false);
        assert!(msg.is_none());
    }

    #[test]
    fn test_does_not_false_positive_on_commands_containing_flow() {
        // "npm run ci" first token is "npm" — not a FLOW command
        assert!(should_block_background("npm run ci", false).is_none());
        // "git commit" has no relation to flow
        assert!(should_block_background("git commit", false).is_none());
        // "npm run flow" first token is "npm"
        assert!(should_block_background("npm run flow", false).is_none());
    }

    // --- is_bg_truthy: defensive JSON type handling ---

    #[test]
    fn test_is_bg_truthy_bool_true() {
        assert!(is_bg_truthy(&json!(true)));
    }

    #[test]
    fn test_is_bg_truthy_bool_false() {
        assert!(!is_bg_truthy(&json!(false)));
    }

    #[test]
    fn test_is_bg_truthy_string_true() {
        // A schema-confused caller passing "true" as a string must not bypass
        // the CI gate. Case-insensitive.
        assert!(is_bg_truthy(&json!("true")));
        assert!(is_bg_truthy(&json!("True")));
        assert!(is_bg_truthy(&json!("TRUE")));
    }

    #[test]
    fn test_is_bg_truthy_string_one() {
        assert!(is_bg_truthy(&json!("1")));
    }

    #[test]
    fn test_is_bg_truthy_string_other() {
        assert!(!is_bg_truthy(&json!("false")));
        assert!(!is_bg_truthy(&json!("yes")));
        assert!(!is_bg_truthy(&json!("")));
        assert!(!is_bg_truthy(&json!("foreground")));
    }

    #[test]
    fn test_is_bg_truthy_integer_nonzero() {
        assert!(is_bg_truthy(&json!(1)));
        assert!(is_bg_truthy(&json!(42)));
        assert!(is_bg_truthy(&json!(-1)));
    }

    #[test]
    fn test_is_bg_truthy_integer_zero() {
        assert!(!is_bg_truthy(&json!(0)));
    }

    #[test]
    fn test_is_bg_truthy_null() {
        assert!(!is_bg_truthy(&Value::Null));
    }

    #[test]
    fn test_is_bg_truthy_object_and_array() {
        assert!(!is_bg_truthy(&json!({})));
        assert!(!is_bg_truthy(&json!([])));
    }

    // --- Agent validation ---

    #[test]
    fn test_validate_agent_blocks_general_purpose_when_flow_active() {
        let (allowed, msg) = validate_agent(Some("general-purpose"), true);
        assert!(!allowed);
        assert!(msg.contains("general-purpose"));
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_validate_agent_blocks_absent_type_when_flow_active() {
        let (allowed, msg) = validate_agent(None, true);
        assert!(!allowed);
        assert!(msg.contains("general-purpose"));
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_validate_agent_allows_flow_namespace_when_flow_active() {
        let (allowed, msg) = validate_agent(Some("flow:ci-fixer"), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_validate_agent_allows_explore_when_flow_active() {
        let (allowed, msg) = validate_agent(Some("Explore"), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_validate_agent_allows_plan_when_flow_active() {
        let (allowed, msg) = validate_agent(Some("Plan"), true);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_validate_agent_allows_general_purpose_when_no_flow() {
        let (allowed, msg) = validate_agent(Some("general-purpose"), false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_validate_agent_allows_absent_type_when_no_flow() {
        let (allowed, msg) = validate_agent(None, false);
        assert!(allowed);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_validate_agent_blocks_case_variants_when_flow_active() {
        let (allowed, _) = validate_agent(Some("General-Purpose"), true);
        assert!(!allowed);
        let (allowed, _) = validate_agent(Some("GENERAL-PURPOSE"), true);
        assert!(!allowed);
    }

    #[test]
    fn test_validate_agent_blocks_empty_string_when_flow_active() {
        let (allowed, msg) = validate_agent(Some(""), true);
        assert!(!allowed);
        assert!(msg.contains("BLOCKED"));
    }

    #[test]
    fn test_validate_agent_blocks_whitespace_padded_when_flow_active() {
        let (allowed, _) = validate_agent(Some(" general-purpose "), true);
        assert!(!allowed);
    }
}
