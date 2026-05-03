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

use std::path::Path;

use regex::Regex;
use serde_json::Value;

use super::{
    build_permission_regexes, detect_branch_from_path, find_settings_and_root_from, is_flow_active,
    read_hook_input, resolve_main_root, FILE_READ_COMMANDS,
};
use crate::git::{current_branch_in, default_branch_in};

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
    // Layer 1: Block compound commands and command substitution at the
    // command-structure level. Operator characters inside single quotes,
    // double quotes, or backslash escapes are treated as literal data
    // because bash itself does not interpret them as operators there.
    // An unclosed quote at end-of-input is pessimistically blocked — it
    // is malformed input and could otherwise hide a structural operator
    // from the scanner.
    match scan_unquoted(command, compound_op_predicate) {
        Ok(Some(op)) => {
            return (
                false,
                format!(
                    "BLOCKED: Compound commands ({}) are not allowed outside quoted arguments. \
                     Use separate Bash calls for each command.",
                    op
                ),
            );
        }
        Err(ScanError::Unclosed) => {
            return (
                false,
                "BLOCKED: Command has an unclosed single or double quote. \
                 Close the quote before running the command."
                    .to_string(),
            );
        }
        Ok(None) => {}
    }

    // Layer 2: Block shell redirection (>, >>, 2>, etc.) in unquoted
    // positions. Layer 1 already rejected unclosed-quote inputs, so any
    // command that reaches here is guaranteed quote-balanced and a
    // successful scan is sufficient.
    if let Ok(Some(_)) = scan_unquoted(command, redirect_predicate) {
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

/// Error returned by `scan_unquoted` when the command ends inside a
/// single- or double-quoted region. The caller must treat this as a
/// pessimistic block — an unclosed quote is malformed input that could
/// be used to hide a structural operator from the scanner.
enum ScanError {
    Unclosed,
}

/// Walk `command` as bytes with bash quote-state tracking and invoke
/// `predicate(bytes, i)` ONLY at byte positions where the scanner is in
/// Normal state (outside all quotes and not mid-escape). Returns the
/// first predicate hit, `Ok(None)` on clean scan, or
/// `Err(ScanError::Unclosed)` when the scan ends inside a quote.
///
/// A single shared scanner backs both Layer 1 (compound operators) and
/// Layer 2 (shell redirection) so quote semantics stay in lockstep —
/// fixing a scanning bug in one place fixes it in both.
fn scan_unquoted<F>(command: &str, predicate: F) -> Result<Option<&'static str>, ScanError>
where
    F: Fn(&[u8], usize) -> Option<&'static str>,
{
    #[derive(PartialEq)]
    enum State {
        Normal,
        Single,
        Double,
    }

    let bytes = command.as_bytes();
    let mut state = State::Normal;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match state {
            State::Normal => match b {
                b'\'' => state = State::Single,
                b'"' => state = State::Double,
                b'\\' => {
                    // Skip the following byte regardless of what it is.
                    // If the backslash is the final byte, the escape is
                    // a no-op and the loop exits cleanly.
                    i += 1;
                }
                _ => {
                    if let Some(op) = predicate(bytes, i) {
                        return Ok(Some(op));
                    }
                }
            },
            State::Single => {
                // Single quotes are fully literal — no escapes, no
                // substitution. Only the closing `'` ends the region.
                if b == b'\'' {
                    state = State::Normal;
                }
            }
            State::Double => match b {
                b'\\' => {
                    // Inside double quotes, backslash escapes the next
                    // byte (typically `"`, `\`, `$`, `` ` ``).
                    i += 1;
                }
                b'"' => state = State::Normal,
                // Bash expands `$(...)` and backtick substitution INSIDE
                // double quotes — single quotes are the only context
                // that fully suppresses expansion. These are always
                // blocked in any non-single-quoted position regardless
                // of which predicate is running.
                b'$' if bytes.get(i + 1) == Some(&b'(') => {
                    return Ok(Some("$("));
                }
                b'`' => {
                    return Ok(Some("`"));
                }
                _ => {}
            },
        }
        i += 1;
    }

    if state != State::Normal {
        return Err(ScanError::Unclosed);
    }
    Ok(None)
}

/// Compound-operator predicate for `scan_unquoted`. Returns the matched
/// operator when the byte at `i` begins a structural shell operator:
/// compound commands (`&&`, `||`, `|`, `;`), backgrounding (bare `&`),
/// input redirection (`<`, `<<`, `<<<`, `<(`), or command substitution
/// (`$(`, backtick). The scanner only calls this in Normal state, so
/// operator characters inside single-quoted arguments are inert by
/// construction. `$(` and backticks are also caught inside double
/// quotes by `scan_unquoted` itself, because bash expands both there.
fn compound_op_predicate(bytes: &[u8], i: usize) -> Option<&'static str> {
    match bytes[i] {
        b'&' if bytes.get(i + 1) == Some(&b'&') => Some("&&"),
        // Bare `&` is the shell backgrounding operator. It is always
        // structural in Normal state — bash spawns the command as a
        // detached process, defeating the CI gate and race-free state
        // mutations that `bin/flow` subcommands require.
        b'&' => Some("&"),
        b'|' if bytes.get(i + 1) == Some(&b'|') => Some("||"),
        b'|' => Some("|"),
        b';' => Some(";"),
        // Any unquoted `<` is the start of an input redirection
        // (`< file`, `<< HEREDOC`, `<<< here-string`, `<(...)` process
        // substitution). None of these are supported by FLOW's
        // dedicated-tool discipline, and `<(...)` in particular
        // launches a subprocess whose output becomes a named pipe —
        // the same risk class as `$(...)`. Blocking the single byte
        // catches every variant.
        b'<' => Some("<"),
        b'$' if bytes.get(i + 1) == Some(&b'(') => Some("$("),
        b'`' => Some("`"),
        _ => None,
    }
}

/// Redirect predicate for `scan_unquoted`. Returns `Some(">")` when the
/// byte at `i` is an unquoted `>` that is NOT immediately preceded by
/// `=` (the carve-out for flag-value patterns like
/// `git log --format=>%s`). The `-` carve-out the original byte scanner
/// allowed is gone — an adversarial case like `echo foo-->/tmp/out`
/// exploited it to slip an unquoted redirect past Layer 2.
fn redirect_predicate(bytes: &[u8], i: usize) -> Option<&'static str> {
    if bytes[i] != b'>' {
        return None;
    }
    if i > 0 && bytes[i - 1] == b'=' {
        return None;
    }
    Some(">")
}

/// Whether the first token is a `bin/flow` launcher invocation —
/// either bare `bin/flow` or any absolute path ending in `/bin/flow`.
/// Mirrors the suffix-match used by `is_flow_command` further below
/// so the two matchers stay in lockstep on the same family of paths.
fn is_bin_flow_token(token: &str) -> bool {
    token == "bin/flow" || token.ends_with("/bin/flow")
}

/// Strip leading and trailing single quotes, then leading and
/// trailing double quotes, from a shell token. Bash dequotes command
/// names before exec, so `'git' commit` runs the same as `git
/// commit` — Layer 10 must too. The `trim_matches` chain strips ALL
/// leading and trailing quote characters of each kind, not a
/// matched pair, which is a permissive v1 heuristic: the worst case
/// is over-stripping a malformed token (e.g. `'git` becomes `git`
/// even though the trailing quote is missing), which can only widen
/// the matcher's recognition surface for adversarial inputs and
/// cannot under-block a legitimate `git commit`.
fn dequote_token(s: &str) -> &str {
    s.trim_matches('\'').trim_matches('"')
}

/// When `stripped` is a `bash -c <arg>` or `sh -c <arg>` invocation,
/// return the inner script string with one layer of surrounding
/// quotes removed. Otherwise return None. Used to re-evaluate the
/// inner command through the same matcher one level deeper. v1 does
/// not recurse a second time (`bash -c 'bash -c "..."'` falls
/// through to allow), does not handle env-var-indirected launchers
/// (`SHELL=bash $SHELL -c '...'`), and does not handle bash flags
/// before `-c` (`bash --norc -c '...'`) — these shapes pass through
/// to the standard first-token check, which sees `bash` as the
/// first token and returns false from `is_commit_invocation_inner`.
fn unwrap_bash_c(stripped: &str) -> Option<String> {
    let after = stripped
        .strip_prefix("bash -c ")
        .or_else(|| stripped.strip_prefix("sh -c "))?;
    Some(dequote_token(after.trim_start()).to_string())
}

/// Walk `tokens` skipping git-level flags that take an argument
/// (`-c k=v`, `-C path`) until the first non-flag token. Returns
/// that token as the effective git subcommand, or None if the
/// iterator exhausts. v1 only handles the two flag forms named in
/// the plan's Task 8 — adversarial bypasses via `--git-dir`,
/// `--work-tree`, etc. are out of scope.
fn next_git_subcommand<'a, I>(tokens: &mut I) -> Option<&'a str>
where
    I: Iterator<Item = &'a str>,
{
    while let Some(t) = tokens.next() {
        if t == "-c" || t == "-C" {
            tokens.next();
            continue;
        }
        return Some(t);
    }
    None
}

/// Extract the value of a `-C <path>` argument from a `git ...`
/// command, if present. Returns the path as a borrowed slice of
/// `stripped` for the caller to convert to a `PathBuf`. Used by
/// Layer 10 to also resolve the branch from git's effective cwd
/// when `-C` shifts it away from the hook's process cwd.
fn extract_dash_c_path(stripped: &str) -> Option<&str> {
    let mut tokens = stripped.split_whitespace();
    while let Some(t) = tokens.next() {
        if t == "-C" {
            return tokens.next();
        }
    }
    None
}

/// Recognize a direct commit invocation that Layer 10 must block
/// when the effective cwd is on the integration branch. v1 matches:
/// `git ... commit` (skipping `-c k=v` and `-C path` between `git`
/// and the subcommand), `bin/flow ... finalize-commit` (matched by
/// `bin/flow` exact or `*/bin/flow` suffix), `'git' commit` /
/// `"git" commit` (with the first token dequoted), and `bash -c
/// '<inner>'` / `sh -c '<inner>'` (re-evaluating the inner script).
fn is_commit_invocation(stripped: &str) -> bool {
    if let Some(inner) = unwrap_bash_c(stripped) {
        return is_commit_invocation_inner(&inner);
    }
    is_commit_invocation_inner(stripped)
}

fn is_commit_invocation_inner(stripped: &str) -> bool {
    let mut tokens = stripped.split_whitespace();
    let first_raw = tokens.next().unwrap_or("");
    let first = dequote_token(first_raw);
    if first == "git" {
        return next_git_subcommand(&mut tokens) == Some("commit");
    }
    if is_bin_flow_token(first) {
        // bin/flow today exposes no global flags between launcher
        // and subcommand, but a future addition (`--verbose`,
        // `--log-level <value>`, etc.) must not bypass Layer 10.
        // Match `finalize-commit` as any subsequent token rather
        // than the immediate next token. False-positive risk is
        // negligible: split_whitespace tokenization preserves
        // surrounding quotes, so a literal `finalize-commit`
        // appearing inside a quoted argument string keeps its
        // quote characters and never compares equal.
        return tokens.any(|t| t == "finalize-commit");
    }
    false
}

/// Compose the Layer 10 block message naming the integration branch.
/// The message is a fixed-shape string the contract tests assert on
/// (must contain `BLOCKED` and the branch name) and the user-facing
/// guidance directing the engineer at `/flow:flow-commit`.
fn commit_block_message(branch: &str) -> String {
    format!(
        "BLOCKED: direct commits on the integration branch '{}' are not allowed. \
         Run /flow:flow-commit from a feature worktree instead. \
         This block is mechanical (Layer 10).",
        branch
    )
}

/// Run Layer 10's commit-on-integration-branch check against the
/// effective cwd. Returns `Some(message)` when the check fires (the
/// command is a commit invocation AND at least one candidate cwd
/// resolves to the integration branch); the caller eprintlns the
/// message and exits 2. Returns `None` when Layer 10 does not block
/// — either the command is not a commit invocation, no candidate
/// cwd is in a git repo, or every resolved branch differs from its
/// own integration branch.
///
/// Candidates are the hook's process cwd plus any `-C <path>`
/// argument extracted from the command — `git -C <other> commit`
/// shifts git's effective cwd onto `<other>`, so the branch must be
/// resolved from there too. Layer 10 blocks if EITHER candidate
/// matches the integration branch.
fn check_commit_on_integration(command: &str, cwd: &Path) -> Option<String> {
    if !is_commit_invocation(command) {
        return None;
    }
    if let Some(msg) = match_branch_at(cwd) {
        return Some(msg);
    }
    if let Some(p) = extract_dash_c_path(command) {
        if let Some(msg) = match_branch_at(Path::new(p)) {
            return Some(msg);
        }
    }
    None
}

/// Resolve the current branch and integration branch from the given
/// path; return the block message when they match (commit on
/// integration), otherwise None. Factored out so the cwd check and
/// the `-C path` check share one block-decision shape.
fn match_branch_at(path: &Path) -> Option<String> {
    let current = current_branch_in(path)?;
    let integration = default_branch_in(path);
    if current == integration {
        Some(commit_block_message(&current))
    } else {
        None
    }
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
        // When `as_i64()` returns `Some`, the Number was stored as an
        // integer variant — truthy iff the value is non-zero. When
        // `as_i64()` returns `None`, the Number was stored as a float;
        // `is_some_and(|f| f != 0.0)` classifies it truthy iff the
        // float is non-zero. serde_json guarantees every `Value::Number`
        // is representable as at least one of i64/u64/f64, so the `None`
        // arm always finds a finite f64.
        Value::Number(n) => match n.as_i64() {
            Some(i) => i != 0,
            None => n.as_f64().is_some_and(|f| f != 0.0),
        },
        _ => false,
    }
}

/// Run the validate-pretool hook (entry point from CLI).
pub fn run() {
    let hook_input = match read_hook_input() {
        Some(input) => input,
        None => std::process::exit(0),
    };

    // Resolve cwd ONCE and reuse for both settings discovery and
    // branch detection. env::current_dir() can fail when the cwd
    // inode has been unlinked (e.g. the stale-cwd adversarial path);
    // in that case both settings and branch fall through to None.
    // Per `.claude/rules/testability-means-simplicity.md` the prior
    // `find_settings_and_root`/`detect_branch_from_cwd` generic seams
    // have been removed because their per-monomorphization Err arms
    // were unreachable through any production callsite — the
    // stale-cwd subprocess test covers the failure path here instead.
    let cwd = std::env::current_dir().ok();
    let (settings, project_root) = cwd
        .as_deref()
        .map(find_settings_and_root_from)
        .unwrap_or((None, None));
    let branch = if settings.is_some() {
        cwd.as_deref().and_then(detect_branch_from_path)
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

    // Layer 10: block direct commit invocations when the hook's
    // effective cwd resolves to the integration branch. Layered after
    // validate() returns Ok rather than as another layer inside
    // validate() because validate() does not receive cwd — adding it
    // would expand the function's signature across every existing
    // caller. Commands blocked by Layers 1-9 never reach this point;
    // Layer 10 fires only when the command passes all preceding
    // structural gates AND is a commit invocation on the integration
    // branch.
    if let Some(cwd_path) = cwd.as_deref() {
        if let Some(msg) = check_commit_on_integration(command, cwd_path) {
            eprintln!("{}", msg);
            std::process::exit(2);
        }
    }

    std::process::exit(0);
}
