//! PreToolUse hook validator for Bash and Agent tool calls.
//!
//! For Bash calls, checks the command against blocked patterns (compound
//! commands, redirection, deny list, whitelist).
//!
//! For Agent calls, blocks `general-purpose` sub-agents during active
//! FLOW phases. Custom plugin agents (`flow:*`) and specialized types
//! (`Explore`, `Plan`) are allowed through.
//!
//! Exit 0 — allow (command passes through to normal permission system)
//! Exit 2 — block (error message on stderr is fed back to the sub-agent)

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use super::transcript_walker::{
    any_skill_in_set_since_user, last_user_message_invokes_skill, most_recent_skill_since_user,
    normalize_gate_input,
};
use super::{
    build_permission_regexes, detect_branch_from_path, find_settings_and_root_from, is_flow_active,
    read_hook_input, resolve_main_root,
};
use crate::flow_paths::FlowPaths;
use crate::git::{current_branch_in, default_branch_in};
use crate::session_metrics::home_dir_or_empty;

/// Validate a Bash command string.
///
/// Returns `(allowed, message)`. Message is empty if allowed.
///
/// Layers 1-8 (compound commands, redirection, exec prefix,
/// destructive find, blanket restore, git diff with file args, deny
/// list, structural escape-hatch) are always enforced.
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
                     Use separate Bash calls for each command. \
                     See .claude/rules/no-escape-hatches.md.",
                    op
                ),
            );
        }
        Err(ScanError::Unclosed) => {
            return (
                false,
                "BLOCKED: Command has an unclosed single or double quote. \
                 Close the quote before running the command. \
                 See .claude/rules/no-escape-hatches.md."
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
             Write tool to create files. \
             See .claude/rules/no-escape-hatches.md."
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
             the behavior is identical. \
             See .claude/rules/no-escape-hatches.md."
                .to_string(),
        );
    }

    // Layer 4: Block destructive `find` flag forms structurally.
    // `find` with -exec, -execdir, -ok, -okdir, or -delete runs
    // arbitrary commands or recursively unlinks files. UNIVERSAL_ALLOW
    // permits `Bash(find *)` for read-only invocations (the safe
    // default with no destructive flag); this layer rejects the
    // destructive shapes regardless of `settings.json` content or
    // `flow_active` state, so the protection holds during the
    // pre-prime upgrade window AND outside FLOW phases. Tokenization
    // via `split_whitespace` catches path-omitted forms like
    // `find -exec rm /etc/passwd \;` and `find -delete` (find
    // defaults the path to `.` when absent) that a regex pattern
    // with a required path slot would silently pass.
    //
    // The check matches the literal command name `find` plus any
    // absolute-path variant ending with `/find`. Bash-quoted
    // (`'find'`) or escape-prefixed (`\find`) shapes are not caught
    // here — the same gap exists for every settings-driven layer in
    // this hook because they also tokenize on the literal command
    // string.
    const FIND_DESTRUCTIVE_FLAGS: &[&str] = &["-exec", "-execdir", "-ok", "-okdir", "-delete"];
    let mut find_tokens = stripped.split_whitespace();
    let first_token = find_tokens.next();
    let is_find_command =
        first_token == Some("find") || first_token.is_some_and(|t| t.ends_with("/find"));
    if is_find_command {
        for token in find_tokens {
            if FIND_DESTRUCTIVE_FLAGS.contains(&token) {
                return (
                    false,
                    format!(
                        "BLOCKED: 'find' with destructive flag '{}' is forbidden. \
                         `-exec`, `-execdir`, `-ok`, `-okdir`, and `-delete` \
                         run arbitrary commands or unlink files. Use Glob to \
                         discover paths and Read to inspect them. \
                         See .claude/rules/no-escape-hatches.md.",
                        token
                    ),
                );
            }
        }
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
                         This operation is explicitly forbidden. \
                         See .claude/rules/no-escape-hatches.md.",
                        command
                    ),
                );
            }
        }
    }

    // Layer 8: Structural escape-hatch program/flag block. Catches
    // indirect forms (absolute paths, env-var prefixes, flags-before-
    // trigger) that route around Layer 7's glob deny patterns. Fires
    // regardless of `settings` or `flow_active` so pre-prime sessions
    // and outside-FLOW invocations inherit the protection. See
    // `.claude/rules/no-escape-hatches.md` for the canonical
    // program/flag table this layer enforces structurally.
    if let Some(msg) = check_escape_hatch_structural(stripped) {
        return (false, msg);
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

/// Recognize file-descriptor redirect bytes in shapes like `2>&1`,
/// `>&2`, and `2>&-`. Returns true when:
///
/// - `bytes[idx] == b'&'` AND the immediately preceding byte is `>`
///   AND the immediately following byte is an ASCII digit or `-`
///   (the `&` participates in `>&<digit>` or `>&-` as a redirect
///   target marker, not as bash backgrounding and not as the
///   left half of `>& outfile` file-redirect syntax), OR
/// - `bytes[idx] == b'>'` AND the immediately following byte is `&`
///   AND the byte after `&` is an ASCII digit or `-` (the `>` opens
///   an FD-redirect of the form `>&<digit>...`, not a `>& outfile`
///   file-redirect that bash interprets as redirecting both stdout
///   and stderr to a file named `outfile`).
///
/// The digit-or-`-` constraint after `&` is load-bearing: bash's
/// `>& word` shape redirects both stdout and stderr to a file named
/// `word`. Without the constraint, the helper would carve out
/// `cmd >& outfile` as if it were FD-redirect, defeating Layer 2's
/// file-redirect block. The constraint narrows the carve-out to the
/// only grammatically valid FD targets.
///
/// Both predicates (compound-op and redirect) consult this helper
/// to skip FD-redirect bytes so common shapes like `cargo test 2>&1`
/// pass through. Bare `&` not preceded by `>` (e.g. `cmd & wait`,
/// `&1 cmd`) returns false here and is caught by the bare-`&` arm
/// of `compound_op_predicate`. Plain `>` not followed by `&` (e.g.
/// `cmd > /tmp/out`, `cmd >> file`) returns false here and is
/// caught by `redirect_predicate`. `>&` followed by anything other
/// than a digit or `-` (e.g. `>& outfile`, `>&letter`) also returns
/// false so Layer 2 still blocks file-redirect shapes.
fn is_fd_redirect_at(bytes: &[u8], idx: usize) -> bool {
    let cur = bytes.get(idx).copied();
    let prev = idx.checked_sub(1).and_then(|i| bytes.get(i).copied());
    let next = bytes.get(idx + 1).copied();
    let after_amp = bytes.get(idx + 2).copied();
    let next_is_fd_target = matches!(next, Some(b'0'..=b'9') | Some(b'-'));
    let after_amp_is_fd_target = matches!(after_amp, Some(b'0'..=b'9') | Some(b'-'));
    (cur == Some(b'&') && prev == Some(b'>') && next_is_fd_target)
        || (cur == Some(b'>') && next == Some(b'&') && after_amp_is_fd_target)
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
        // The bare-`&` arm matches the shell backgrounding operator —
        // bash spawns the command as a detached process, defeating
        // the CI gate and race-free state mutations that `bin/flow`
        // subcommands require. The `is_fd_redirect_at` check skips
        // `&` bytes that participate in FD-redirect shapes like
        // `2>&1`, `>&2`, and `2>&-`, where `&` is a redirect target
        // marker rather than backgrounding.
        b'&' if is_fd_redirect_at(bytes, i) => None,
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
/// `git log --format=>%s`) and is NOT part of an FD-redirect shape
/// like `2>&1` or `>&2` (consulted via `is_fd_redirect_at`). The `-`
/// carve-out the original byte scanner allowed is gone — an
/// adversarial case like `echo foo-->/tmp/out` exploited it to slip
/// an unquoted redirect past Layer 2.
fn redirect_predicate(bytes: &[u8], i: usize) -> Option<&'static str> {
    if bytes[i] != b'>' {
        return None;
    }
    if i > 0 && bytes[i - 1] == b'=' {
        return None;
    }
    if is_fd_redirect_at(bytes, i) {
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

/// Strip leading `KEY=VAL ` env-var prefix segments from `s` and
/// return the remainder. Zero or more segments are stripped — each
/// segment is an ASCII identifier (letter or `_` followed by
/// letters/digits/`_`), an `=`, a non-whitespace value, and a
/// trailing whitespace separator. The final token after env vars
/// is NOT stripped: an `s` of `"FOO=bar"` alone returns `"FOO=bar"`
/// because there is no whitespace boundary that proves a following
/// command exists. Used by Layer 8 to see past `FOO=bar bash -c`
/// to the effective program.
fn strip_env_prefix(s: &str) -> &str {
    let mut current = s.trim_start();
    loop {
        let bytes = current.as_bytes();
        if bytes.is_empty() {
            return current;
        }
        let first = bytes[0];
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return current;
        }
        let mut i = 0;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return current;
        }
        let mut j = i + 1;
        while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() {
            return current;
        }
        current = current[j..].trim_start();
    }
}

/// Return the basename of a first-token path. When `token` contains
/// no `/`, returns `token` unchanged. Otherwise returns the substring
/// after the final `/`. Used by Layer 8 to match `/usr/bin/bash`
/// against the escape-hatch program set by its basename `bash`.
fn first_token_basename(token: &str) -> &str {
    match token.rfind('/') {
        Some(idx) => &token[idx + 1..],
        None => token,
    }
}

/// Layer 8's structural escape-hatch check. Strips env-var prefix,
/// tokenizes on whitespace, basenames the first token, and matches
/// against the canonical escape-hatch program set from
/// `.claude/rules/no-escape-hatches.md`. Trigger-flag awareness keeps
/// legitimate sibling invocations (`bash -n` syntax check, `tmux ls`,
/// `rtk discover`) from being blocked while the eval shapes
/// (`bash -c`, `tmux send-keys`, `rtk proxy`) are rejected. Returns
/// `Some(message)` when the layer fires; the message names the
/// program, the escape-hatch class, the sanctioned alternative, and
/// cites `.claude/rules/no-escape-hatches.md` for the citation
/// contract test.
fn check_escape_hatch_structural(stripped: &str) -> Option<String> {
    let unwrapped = strip_env_and_wrappers(stripped);
    let mut tokens = unwrapped.split_whitespace();
    let first = tokens.next()?;
    let basename = first_token_basename(first);
    let rest: Vec<&str> = tokens.collect();

    match basename {
        "bash" | "sh" | "zsh" => {
            if has_flag_char(&rest, 'c') {
                Some(format!(
                    "BLOCKED: '{} -c' is a shell-eval escape hatch. \
                     Use separate Bash tool calls per command. \
                     See .claude/rules/no-escape-hatches.md.",
                    basename
                ))
            } else {
                None
            }
        }
        "eval" => Some(
            "BLOCKED: 'eval' is a shell-eval escape hatch. \
             Use separate Bash tool calls per command. \
             See .claude/rules/no-escape-hatches.md."
                .to_string(),
        ),
        "xargs" => Some(
            "BLOCKED: 'xargs' is a command-wrapper escape hatch. \
             Issue separate Bash calls per argument. \
             See .claude/rules/no-escape-hatches.md."
                .to_string(),
        ),
        "perl" => {
            if has_flag_char(&rest, 'e') || has_flag_char(&rest, 'E') {
                Some(
                    "BLOCKED: 'perl -e'/'perl -E' is an interpreter-eval escape hatch. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "python" | "python3" => {
            if has_flag_char(&rest, 'c') {
                Some(format!(
                    "BLOCKED: '{} -c' is an interpreter-eval escape hatch. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md.",
                    basename
                ))
            } else {
                None
            }
        }
        "ruby" => {
            if has_flag_char(&rest, 'e') {
                Some(
                    "BLOCKED: 'ruby -e' is an interpreter-eval escape hatch. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "node" => {
            if has_flag_char(&rest, 'e') || has_flag_char(&rest, 'p') {
                Some(
                    "BLOCKED: 'node -e'/'node -p' is an interpreter-eval escape hatch. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "osascript" => {
            if has_flag_char(&rest, 'e') {
                Some(
                    "BLOCKED: 'osascript -e' is an interpreter-eval escape hatch. \
                     AppleScript can shell out via `do shell script`. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "tclsh" => {
            if has_flag_char(&rest, 'c') {
                Some(
                    "BLOCKED: 'tclsh -c' is an interpreter-eval escape hatch. \
                     Tcl can shell out via `exec`. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "lua" => {
            if has_flag_char(&rest, 'e') {
                Some(
                    "BLOCKED: 'lua -e' is an interpreter-eval escape hatch. \
                     Lua can shell out via `os.execute`. \
                     Use the Read tool to view files and the Write tool to create files. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "nc" => Some(
            "BLOCKED: 'nc' is a network-bridge escape hatch. \
             Use the dedicated network tool surface. \
             See .claude/rules/no-escape-hatches.md."
                .to_string(),
        ),
        "ssh" => Some(
            "BLOCKED: 'ssh' is a network-bridge escape hatch. \
             Use the approved ssh wrapper script when remote access is required. \
             See .claude/rules/no-escape-hatches.md."
                .to_string(),
        ),
        "tmux" => {
            if rest.contains(&"send-keys") {
                Some(
                    "BLOCKED: 'tmux send-keys' is an inter-process escape hatch. \
                     Use direct Bash invocations, not multiplexer key injection. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "screen" => {
            if rest.contains(&"-X") {
                Some(
                    "BLOCKED: 'screen -X' is an inter-process escape hatch. \
                     Use direct Bash invocations, not multiplexer key injection. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "rtk" => {
            if rest.first() == Some(&"proxy") {
                Some(
                    "BLOCKED: 'rtk proxy' is a command-wrapper escape hatch. \
                     Use the underlying command directly through the sanctioned allow list. \
                     See .claude/rules/no-escape-hatches.md."
                        .to_string(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Alternate `strip_wrapper_launchers` and `strip_env_prefix` until
/// the input stabilizes. Handles both orderings of env-var prefix
/// and wrapper launcher: `env FOO=bar bash -c '...'` (wrapper-then-
/// env-args) AND `FOO=bar env bash -c '...'` (env-prefix-then-
/// wrapper). A single pass cannot cover both because each pass only
/// strips one layer at a time. The loop terminates when neither
/// stripper makes progress — bounded above by the number of tokens
/// in the input, so worst-case O(N²) in token count for a clearly
/// linear input.
fn strip_env_and_wrappers(s: &str) -> &str {
    let mut current = s;
    loop {
        let after = strip_env_prefix(strip_wrapper_launchers(current));
        if after.len() == current.len() {
            return current;
        }
        current = after;
    }
}

/// Strip leading wrapper-launcher tokens (`env`, `time`, `nice`,
/// `nohup`, `taskset`, `ionice`) so a wrapper-launched escape hatch
/// like `env bash -c 'cmd'` or `time bash -c 'cmd'` exposes its
/// effective program to the basename check. Each iteration consumes
/// the wrapper token; `strip_env_prefix` running afterward consumes
/// any KEY=VAL arguments env may carry (`env KEY=VAL bash -c`).
/// `env -u VAR bash -c` is a documented v1 boundary — the helper
/// stops at the first wrapper-flag token (`-u`) rather than
/// consuming flag args, so the program-set check sees `-u` as the
/// first token and returns None. The structural-layer rule's table
/// names this gap explicitly so a future tightening is a deliberate
/// design choice rather than discovery during adversarial review.
fn strip_wrapper_launchers(s: &str) -> &str {
    const WRAPPERS: &[&str] = &["env", "time", "nice", "nohup", "taskset", "ionice"];
    let mut current = s.trim_start();
    loop {
        let Some(first) = current.split_whitespace().next() else {
            return current;
        };
        let basename = first_token_basename(first);
        if !WRAPPERS.contains(&basename) {
            return current;
        }
        // Find the first whitespace boundary past the wrapper token.
        // Iterate chars rather than bytes so multi-byte UTF-8 paths
        // in absolute-path wrappers (`/opt/utf-8-path/env`) advance
        // correctly. `current` is the worktree-derived stripped
        // command, but any path can pass through.
        let mut idx = 0;
        for (i, c) in current.char_indices() {
            if c.is_whitespace() {
                idx = i;
                break;
            }
        }
        if idx == 0 {
            // Wrapper is the only token — nothing escapes through.
            return "";
        }
        current = current[idx..].trim_start();
    }
}

/// Return true iff any token in `rest` starts with `-` (but not
/// `--`) and contains the given short-flag character. Catches
/// combined-flag shapes like `bash -lc`, `bash -ic`, `bash -xc`,
/// `node -ep`, etc., which a literal `rest.contains(&"-c")` check
/// would miss (the token is `-lc`, not `-c`).
///
/// Long flags (`--login`, `--noprofile`) are excluded because
/// short-flag-character semantics do not apply.
fn has_flag_char(rest: &[&str], flag: char) -> bool {
    rest.iter().any(|t| {
        if !t.starts_with('-') || t.starts_with("--") {
            return false;
        }
        t.chars().skip(1).any(|c| c == flag)
    })
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

/// Extract the explicit `<branch>` positional argument from a
/// `bin/flow finalize-commit <msg-file> <branch>` invocation.
/// Returns the dequoted branch token when the shape matches and
/// both positional arguments are present, `None` otherwise.
///
/// Mirrors the `bin/flow` arm of `is_commit_invocation_inner`:
/// dequotes the first token, accepts the bare `bin/flow` form and
/// the `*/bin/flow` suffix form via `is_bin_flow_token`, and walks
/// past any future global flags between launcher and subcommand
/// before locating `finalize-commit`. After the subcommand token,
/// the next token is the message file; the token after that is the
/// branch.
///
/// Production consumer: Layer 10's `check_commit_during_flow` uses
/// the returned branch arg as the routing key for the destination-
/// aware integration-branch and active-flow checks — independent of
/// the hook's process cwd. Mirrors `finalize_commit::run_impl`'s
/// own branch-derived routing through `FlowPaths::worktree()`.
///
/// The returned branch is validated via `FlowPaths::is_valid_branch`
/// per `.claude/rules/branch-path-safety.md`. An invalid branch
/// (empty, `.`, `..`, slash-containing, or NUL-bearing) returns
/// `None` so the caller (`check_commit_during_flow`) falls through
/// to the cwd path rather than constructing
/// `<main_root>/.worktrees/<invalid>` with a path-traversal payload.
/// The cwd path's `is_commit_invocation` gate still recognizes the
/// invocation as a commit, so `git commit`-shaped active-flow
/// checks against the caller's cwd still fire when appropriate.
fn extract_finalize_commit_branch_arg(stripped: &str) -> Option<&str> {
    let mut tokens = stripped.split_whitespace();
    // Empty or whitespace-only commands fall through to the
    // `!is_bin_flow_token(first)` rejection below — no separate
    // early return is needed.
    let first_raw = tokens.next().unwrap_or("");
    let first = dequote_token(first_raw);
    if !is_bin_flow_token(first) {
        return None;
    }
    // Walk past any pre-subcommand flags so `bin/flow --verbose
    // finalize-commit <msg> <branch>` extracts the branch
    // unchanged.
    let mut found_subcommand = false;
    for t in tokens.by_ref() {
        if t == "finalize-commit" {
            found_subcommand = true;
            break;
        }
    }
    if !found_subcommand {
        return None;
    }
    // Skip the message-file token; require the branch token after.
    // Both `tokens.next()?` short-circuit when the user invoked
    // `bin/flow finalize-commit` with no positional args or only
    // the message file — without the second positional, this
    // helper has no branch to bind to and the caller falls back
    // to the cwd-based commit check.
    tokens.next()?;
    let branch_raw = tokens.next()?;
    let branch = dequote_token(branch_raw);
    // Validate before returning so downstream path construction
    // (`main_root.join(".worktrees").join(branch)`) cannot receive
    // a `..`, `.`, or `/`-containing payload. An invalid branch
    // produces `None` and the caller falls through to the cwd
    // path, which is the conservative behavior for arbitrary
    // user input.
    if !crate::flow_paths::FlowPaths::is_valid_branch(branch) {
        return None;
    }
    Some(branch)
}

/// Decide whether Layer 10's integration-branch arm fires for a
/// `bin/flow finalize-commit <msg> <branch>` destination-path
/// invocation. Delegates the destination decision to
/// `crate::flow_paths::finalize_commit_destination` — the same
/// helper `src/finalize_commit.rs::run_impl` uses to pick its
/// commit cwd — so the hook's block decision and the binary's
/// commit destination cannot drift: block exactly when the helper
/// routes the commit to the project root.
///
/// `default_branch_in(main_root)` is consulted first as a guard
/// and as the message's branch name. When it errs (no `origin`
/// remote, symbolic-ref unset, non-git directory) the integration
/// branch is undetectable, so this arm has no basis to fire and
/// returns `None` — preserving the fresh-clone behavior. The
/// active-flow check at the same callsite is independent (it walks
/// the worktree path) and still catches in-flow commits via the
/// state file. See `.claude/rules/concurrency-model.md` "Known
/// Limitations".
///
/// Per `.claude/rules/security-gates.md` "Gate-Action Atomicity
/// for Validated Paths", the block message names the canonical
/// integration branch from `default_branch_in`, not the raw
/// `branch_arg`. The helper's own normalize-both-sides comparison
/// (`.claude/rules/security-gates.md` "Normalize Before
/// Comparing") makes a case-variant `MAIN` arg resolve to the
/// project root and therefore block, with the message naming
/// canonical `main`.
///
/// `main_root` is the resolved project root (parent of the FLOW
/// state directory). It is both the `finalize_commit_destination`
/// root argument and the prefix the caller uses to construct
/// `<main_root>/.worktrees/<branch>/` for the active-flow arm.
fn match_finalize_commit_destination(branch_arg: &str, main_root: &Path) -> Option<String> {
    let integration = default_branch_in(main_root).ok()?;
    if crate::flow_paths::finalize_commit_destination(main_root, branch_arg) == main_root {
        Some(commit_block_message(&integration))
    } else {
        None
    }
}

/// Recognize a direct commit invocation that Layer 10 must block
/// when the effective cwd is on the integration branch. v1 matches:
/// `git ... commit` (skipping `-c k=v` and `-C path` between `git`
/// and the subcommand), `bin/flow ... finalize-commit` (matched by
/// `bin/flow` exact or `*/bin/flow` suffix), and `'git' commit` /
/// `"git" commit` (with the first token dequoted). `bash -c
/// '<inner>'` and `sh -c '<inner>'` wrappers do NOT need to be
/// unwrapped here because Layer 8 in `validate` blocks every
/// shell-eval shape (`bash -c`, `sh -c`, `zsh -c`, `eval`) before
/// Layer 10 runs — the wrapper itself is a structural escape hatch
/// per `.claude/rules/no-escape-hatches.md`.
fn is_commit_invocation(stripped: &str) -> bool {
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
/// guidance directing the engineer at `/flow:flow-commit`. The
/// guidance names the supported on-trunk path because Layer 10's
/// trunk carve-out (`flow_commit_trunk_carveout_applies`) accepts a
/// user-typed `/flow:flow-commit` invocation on the trunk branch
/// itself — a maintainer with a legitimate trunk commit need
/// (bootstrap repair, follow-up after a hot patch) is told how to
/// proceed rather than redirected to a feature worktree they have
/// no reason to open.
fn commit_block_message(branch: &str) -> String {
    format!(
        "BLOCKED: direct commits on the integration branch '{}' are not allowed. \
         Invoke /flow:flow-commit on the trunk branch instead. \
         This block is mechanical (Layer 10). See .claude/rules/no-escape-hatches.md.",
        branch
    )
}

/// Compose the Layer 10 block message naming the active flow's branch.
/// Returned when a commit invocation lands in a feature-branch worktree
/// that has an active FLOW state file. The message must contain
/// `BLOCKED`, the literal phrase "active flow", and the
/// `/flow:flow-commit` redirect so contract tests can assert the
/// distinct fire context.
fn commit_block_message_active_flow(branch: &str) -> String {
    format!(
        "BLOCKED: direct commits during an active flow on '{}' are not allowed. \
         Run /flow:flow-commit instead so CI and the skill's diff review run through \
         the gate. This block is mechanical (Layer 10). \
         See .claude/rules/no-escape-hatches.md.",
        branch
    )
}

/// Run Layer 10's commit-during-flow check against the appropriate
/// branch source. Returns `Some(message)` when the check fires
/// (the command is a commit invocation AND the routing key it binds
/// to either resolves to the integration branch OR has an active
/// FLOW state file); the caller eprintlns the message and exits 2.
/// Returns `None` when Layer 10 does not block.
///
/// Dispatch shape:
///
/// 1. **Finalize-commit destination path** — when the command is
///    `bin/flow finalize-commit <msg> <branch>` AND the branch
///    positional argument parses, the branch arg is the routing key.
///    The integration-branch check compares the branch arg against
///    `default_branch_in(<main_root>)` via
///    `match_finalize_commit_destination`. The active-flow check
///    runs at `<main_root>/.worktrees/<branch_arg>/` so an active
///    flow on that worktree fires the gate regardless of the
///    caller's process cwd. This mirrors
///    `finalize_commit::run_impl`'s own branch-derived routing
///    through `FlowPaths::worktree()` so the hook and the binary
///    agree on the destination.
///
/// 2. **Cwd path (fallback)** — for `git ... commit` invocations,
///    for `bin/flow finalize-commit` invocations whose branch arg
///    cannot be extracted (missing tokens, malformed shape), or
///    when `main_root` cannot be resolved from the cwd, the
///    historical cwd-based predicate runs. The hook's process cwd
///    and any `-C <path>` target are each checked against
///    `match_branch_at` and `check_active_flow_at`.
///
/// Per-candidate predicate ordering: integration-branch fires before
/// active-flow so the existing "integration branch" message wins on
/// the rare case where both apply (the integration branch itself
/// has an active flow).
///
/// Bootstrap carve-out: applied to the integration-branch arm in
/// BOTH dispatch shapes (destination path and cwd path), but NOT
/// to the `-C` target's `match_branch_at` callsite. The transcript
/// walker is session-scoped (not per-repo), so a bootstrap chain
/// accrued in session activity for repo A could otherwise authorize
/// a commit redirected via `-C <repo-B>` to repo B's integration
/// branch. The legitimate bootstrap windows (flow-start Step 2,
/// flow-prime Step 6, flow-release) always run with cwd ON the
/// integration branch — none uses `-C` to shift git's effective
/// cwd. See `.claude/rules/concurrency-model.md` "Bootstrap-skill
/// carve-out" for the cwd-only design.
///
/// Trunk carve-out: applied ONLY to the destination-path
/// integration-branch arm. Recognizes a user-typed
/// `/flow:flow-commit` slash command on the trunk branch itself so a
/// maintainer can commit to the trunk via `/flow:flow-commit`
/// without first opening a feature worktree. Wired alongside the
/// bootstrap carve-out's destination-path check (the suppressor is
/// `bootstrap || trunk` rather than `bootstrap` alone); the cwd-path
/// arm is intentionally not extended because a raw `git commit` (or
/// `git -C <trunk> commit`) on the integration branch carries no
/// slash-command marker for the gate to anchor on. See
/// `.claude/rules/no-escape-hatches.md` Layer C "Trunk carve-out".
fn check_commit_during_flow(
    command: &str,
    cwd: &Path,
    transcript_path: Option<&Path>,
    home: &Path,
) -> Option<String> {
    // Destination path: when the command names an explicit branch
    // via `bin/flow finalize-commit <msg> <branch>`, route on the
    // branch arg rather than the caller's cwd. Extract gates this
    // path on its own — it returns None for non-bin/flow shapes,
    // for bin/flow shapes that lack the `finalize-commit` token,
    // and for finalize-commit shapes whose positional arguments
    // are incomplete. When extract returns None, the dispatch
    // falls through to the cwd path (gated on is_commit_invocation)
    // which still covers `git commit`, `git -C <path> commit`, and
    // any malformed finalize-commit invocation.
    if let Some(branch_arg) = extract_finalize_commit_branch_arg(command) {
        let (_, project_root) = find_settings_and_root_from(cwd);
        if let Some(root) = project_root {
            let main_root = resolve_main_root(&root);
            if let Some(msg) = match_finalize_commit_destination(branch_arg, &main_root) {
                if !bootstrap_carveout_applies(command, transcript_path, home)
                    && !flow_commit_trunk_carveout_applies(transcript_path, home, cwd, &main_root)
                {
                    return Some(msg);
                }
            }
            let worktree_path = main_root.join(".worktrees").join(branch_arg);
            if let Some(msg) = check_active_flow_at(command, &worktree_path, transcript_path, home)
            {
                return Some(msg);
            }
            return None;
        }
    }

    // Cwd path: covers `git ... commit`, `git -C <path> commit`,
    // and any finalize-commit invocation whose branch arg cannot
    // be extracted. The is_commit_invocation gate filters out
    // non-commit commands so an `ls -la` on the integration branch
    // doesn't accidentally trigger the integration-branch block.
    if !is_commit_invocation(command) {
        return None;
    }
    if let Some(msg) = match_branch_at(cwd) {
        if !bootstrap_carveout_applies(command, transcript_path, home) {
            return Some(msg);
        }
    }
    if let Some(msg) = check_active_flow_at(command, cwd, transcript_path, home) {
        return Some(msg);
    }
    if let Some(p) = extract_dash_c_path(command) {
        let target = Path::new(p);
        if let Some(msg) = match_branch_at(target) {
            return Some(msg);
        }
        if let Some(msg) = check_active_flow_at(command, target, transcript_path, home) {
            return Some(msg);
        }
    }
    None
}

/// The closed set of sanctioned bootstrap-parent Skill names. Each
/// names a commit window where cwd is the integration branch by
/// design:
///
/// - `flow:flow-start` Step 2 invokes `/flow:flow-commit` to land a
///   `ci-fixer` dependency-repair commit before the user's feature
///   work begins.
/// - `flow:flow-prime` Step 6 invokes `/flow:flow-commit` to land
///   permission and stub-script setup that must reach `origin/<base>`
///   before any flow starts.
/// - `flow-release` publishes a version-bump commit on the
///   integration trunk; there is no feature branch where a release
///   tag could live, and the skill calls `bin/flow finalize-commit`
///   directly rather than delegating to `/flow:flow-commit`. It is
///   both the initiating skill AND its own most-recent-skill walker
///   match — the per-skill trust contract is described on
///   `transcript_shows_commit_window_skill`.
///
/// Namespacing asymmetry: the first two entries carry the `flow:`
/// prefix because `skills/flow-start/SKILL.md` and
/// `skills/flow-prime/SKILL.md` are plugin-marketplace skills —
/// Claude Code emits the namespaced name when the user types
/// `/flow:flow-start` or `/flow:flow-prime`. `flow-release` is a
/// project-local maintainer skill at `.claude/skills/flow-release/`
/// (not under `skills/`), so Claude Code emits the bare name when
/// the user types `/flow-release`. The constant reflects the
/// literal `input.skill` values the transcript walker observes.
///
/// Without a carve-out, Layer 10's integration-branch context blocks
/// every such commit and all three skills are unusable.
///
/// Extending this set is a Plan-phase decision: each new entry must
/// document the integration-branch commit window it sanctions and
/// the reason the bootstrap path cannot work on a feature branch.
/// See `.claude/rules/concurrency-model.md` "Editing Source on the
/// Base Branch".
const BOOTSTRAP_SKILLS: &[&str] = &["flow:flow-start", "flow:flow-prime", "flow-release"];

/// Three AND-combined conditions on the bootstrap-skill carve-out
/// for Layer 10's integration-branch context. The carve-out fires
/// (suppresses the integration-branch block) iff:
///
/// 1. `is_finalize_commit_invocation(command)` — the command shape
///    is `bin/flow ... finalize-commit`. Raw `git commit` is never
///    carved out; `git -C ... commit` matches `is_commit_invocation`
///    but not this finalize-commit-only predicate.
/// 2. The transcript shows a sanctioned commit-window skill —
///    EITHER `transcript_shows_commit_window_skill(path, home)` is
///    true (the most recent assistant Skill since the most recent
///    user turn names `flow:flow-commit`, the delegated commit path
///    used by `flow:flow-start` and `flow:flow-prime`), OR
///    `last_user_message_invokes_skill(path, "flow-release", home)`
///    is true (the most recent user-role turn typed `/flow-release`
///    as a slash command). `flow-release` is a user-only skill —
///    Claude Code records it only as a user-typed turn, never as an
///    assistant Skill tool_use — and it calls `bin/flow
///    finalize-commit` directly without delegating to
///    `/flow:flow-commit`, so the user-turn arm is its production
///    recognition path. This arm is scoped HERE rather than inside
///    the shared `transcript_shows_commit_window_skill` predicate,
///    because that predicate is also consumed by the active-flow
///    carve-out (`check_active_flow_at`) — recognizing `/flow-release`
///    there would widen the active-flow gate, which the
///    integration-trunk-only `flow-release` skill must never touch.
/// 3. `any_skill_in_set_since_user(path, home, BOOTSTRAP_SKILLS)`
///    returns true — a sanctioned bootstrap parent
///    (`flow:flow-start`, `flow:flow-prime`, or `flow-release`) is
///    recognized since the most recent real user turn, either as an
///    assistant Skill tool_use or as the user-typed slash-command
///    boundary turn itself (`flow:flow-prime` and `flow-release` are
///    user-only skills Claude Code records only as user-role turns).
///    The active-flow carve-out's `_continue_pending=commit` state
///    marker is unavailable on the integration branch, so this
///    second walker substitutes for the marker — the choreography is
///    verified entirely from the transcript.
///
/// Trust contract substitution: where the active-flow carve-out
/// uses (shape + marker + walker), the bootstrap carve-out uses
/// (shape + walker + walker). Both walker conditions are
/// load-bearing because the integration-branch context lacks the
/// belt-and-suspenders state-file marker.
///
/// `transcript_path` is unwrapped once at function entry — a
/// missing path fails the carve-out before either walker runs, so
/// both walkers see a known-Some `&Path`.
fn bootstrap_carveout_applies(command: &str, transcript_path: Option<&Path>, home: &Path) -> bool {
    if !is_finalize_commit_invocation(command) {
        return false;
    }
    let Some(path) = transcript_path else {
        return false;
    };
    // Condition 2's user-turn arm (`last_user_message_invokes_skill`
    // for `/flow-release`) is scoped HERE, not inside the shared
    // `transcript_shows_commit_window_skill` predicate.
    // `transcript_shows_commit_window_skill` is also consumed by the
    // active-flow carve-out (`check_active_flow_at`); recognizing the
    // `/flow-release` user turn inside it would widen the active-flow
    // gate, which the integration-trunk-only `flow-release` skill
    // must never touch.
    let commit_window = transcript_shows_commit_window_skill(Some(path), home)
        || last_user_message_invokes_skill(path, "flow-release", home);
    commit_window && any_skill_in_set_since_user(path, home, BOOTSTRAP_SKILLS)
}

/// Two AND-combined conditions on the trunk carve-out for Layer
/// 10's destination-path integration-branch arm. The carve-out
/// fires (suppresses the integration-branch block) iff BOTH hold:
///
/// 1. The caller's cwd is NOT inside an active-flow worktree.
///    Resolved via `detect_branch_from_path(cwd)` +
///    `is_flow_active(branch, main_root)`. The user typing
///    `/flow:flow-commit` from inside a feature-branch worktree
///    intended a commit on THAT worktree's branch — not a commit
///    on the integration trunk. Without this check, a model on a
///    feature-branch worktree could fire `bin/flow finalize-commit
///    msg.txt <trunk>` and the user-typed slash command would
///    spuriously authorize a trunk commit. The active-flow arm
///    has its own carve-out (`check_active_flow_at`) for the
///    legitimate feature-branch commit path keyed on
///    `_continue_pending=commit` + assistant-Skill `flow:flow-commit`;
///    the trunk carve-out's cwd-not-active-flow check is the
///    structural bound that prevents the user's feature-branch
///    intent from authorizing a trunk commit.
/// 2. `last_user_message_invokes_skill(path, "flow:flow-commit",
///    home)` — the most recent real user turn in the persisted
///    transcript STARTS with the namespaced
///    `<command-name>/flow:flow-commit</command-name>` slash-command
///    emission (or the two-line `<command-message>` shape Claude
///    Code 2.1.140+ emits). The user-typed slash command is the
///    unforgeable trust anchor: only a user-typed turn can satisfy
///    `last_user_message_invokes_skill`, so the model cannot
///    synthesize the marker and route around `/flow:flow-commit`'s
///    surrounding diff and message review.
///
/// `transcript_path` is unwrapped once at function entry — a missing
/// path fails the carve-out before the walker runs, so the walker
/// sees a known-Some `&Path`. Cleared per
/// `.claude/rules/external-input-validation.md` "Hook callsite
/// discipline" via the `Option`-returning shape rather than a
/// panicking `.expect`.
///
/// Caller precondition: the destination-path arm in
/// `check_commit_during_flow` invokes this helper only after
/// `extract_finalize_commit_branch_arg` has already filtered the
/// command shape to `bin/flow ... finalize-commit <msg> <branch>`,
/// so an additional `is_finalize_commit_invocation` check inside
/// this body would be unreachable defensive code. A future
/// maintainer who wires this carve-out into a sibling arm that
/// does NOT pre-filter the command shape must add the
/// `is_finalize_commit_invocation(command)` check at the new
/// callsite — adding it inside this function would require
/// extending the signature to accept the command string and
/// updating every existing callsite.
///
/// Scoping: this carve-out is wired only into the destination-path
/// integration-branch arm of `check_commit_during_flow`. The
/// active-flow arm has its own independent carve-out
/// (`check_active_flow_at`) keyed on the `_continue_pending=commit`
/// marker + an assistant-Skill flow-commit; adding the user-typed
/// recognition there would weaken the marker requirement for
/// feature-branch flows. The cwd-path arm is unchanged so a raw
/// `git commit` (or `git -C <trunk> commit`) on the integration
/// branch remains blocked: the user-typed slash command is the
/// signal that the maintainer reached for `/flow:flow-commit`
/// deliberately, and `git commit` does not carry that signal.
///
/// Threat-model bound: the carve-out's window stays open from the
/// user-typed `/flow:flow-commit` turn until the next real user
/// turn — the same documented bound the bootstrap carve-out carries
/// per `.claude/rules/concurrency-model.md` "threat-model bound".
/// CI still runs unconditionally inside `finalize_commit::run_impl`
/// regardless of the carve-out. CI verifies code quality, not
/// commit discipline — the reviewable choreography that Layer 10
/// protects is supplied by the `/flow:flow-commit` skill itself
/// (diff review + commit-message review + user approval), which
/// the user's slash-command invocation triggers. The carve-out
/// preserves the gate's protective intent because the same
/// choreography that protects every feature-branch commit also
/// protects the trunk commit when fired from a non-active-flow
/// cwd.
fn flow_commit_trunk_carveout_applies(
    transcript_path: Option<&Path>,
    home: &Path,
    cwd: &Path,
    main_root: &Path,
) -> bool {
    let Some(path) = transcript_path else {
        return false;
    };
    // Refuse to fire when cwd is inside an active-flow worktree —
    // the user's `/flow:flow-commit` intent bound to THAT worktree's
    // branch, not to the integration trunk. Without this check, a
    // feature-branch worktree's user-typed slash command could
    // authorize an arbitrary `bin/flow finalize-commit msg.txt
    // <trunk>` invocation, bypassing Layer 10 by transcript-
    // anchoring alone.
    if let Some(branch) = detect_branch_from_path(cwd) {
        if is_flow_active(&branch, main_root) {
            return false;
        }
    }
    last_user_message_invokes_skill(path, "flow:flow-commit", home)
}

/// Resolve the current branch and integration branch from the given
/// path; return the block message when they match (commit on
/// integration), otherwise None. Factored out so the cwd check and
/// the `-C path` check share one block-decision shape.
///
/// Scope: serves the cwd path of `check_commit_during_flow` —
/// `git ... commit`, `git -C <path> commit`, and any
/// `bin/flow finalize-commit` invocation whose branch argument
/// cannot be extracted. The `bin/flow finalize-commit <msg>
/// <branch>` shape with a valid branch argument routes through
/// `match_finalize_commit_destination` instead, where the routing
/// key is the explicit branch arg rather than `current_branch_in`.
fn match_branch_at(path: &Path) -> Option<String> {
    let current = current_branch_in(path)?;
    let integration = match default_branch_in(path) {
        Ok(b) => b,
        // When git cannot resolve `origin/HEAD`, the integration
        // branch is undetectable — Layer 9 has no basis to fire on
        // a feature-branch commit. Return None so unprimed/fresh-
        // clone repos do not block all commits. See
        // `match_finalize_commit_destination` for the parallel
        // rationale and `.claude/rules/concurrency-model.md`.
        Err(_) => return None,
    };
    if current == integration {
        Some(commit_block_message(&current))
    } else {
        None
    }
}

/// Resolve the branch and FLOW project root from the given path; if a
/// flow is active, return the active-flow block message UNLESS the
/// skill-commit carve-out applies. Returns None when no flow is
/// active or when the carve-out fires.
///
/// Reuses the canonical helpers `detect_branch_from_path`,
/// `find_settings_and_root_from`, `resolve_main_root`, and
/// `is_flow_active` so the active-flow definition stays consistent
/// across hooks (`validate-ask-user`, `validate-claude-paths`,
/// `stop_continue`, etc.) — no parallel discovery logic is introduced.
///
/// ## Skill-commit carve-out
///
/// The legitimate skill-driven commit path is `/flow:flow-commit` →
/// `bin/flow finalize-commit`. The flow-code, flow-review, and
/// flow-learn skills set `_continue_pending=commit` on the state file
/// via `bin/flow set-timestamp` immediately before invoking
/// /flow:flow-commit. `phase_enter()` clears the field on phase
/// advance, so the marker is `"commit"` only during the skill-driven
/// commit window.
///
/// The carve-out fires (returns None instead of the block message)
/// iff BOTH conditions hold:
///
/// 1. The command shape is `bin/flow ... finalize-commit` (NOT
///    `git commit`). Raw `git commit` is never legitimate during a
///    flow even when the marker is set.
/// 2. The state file's `_continue_pending` is the string `"commit"`.
///    The state-file read is fail-closed: any read or parse error
///    leaves the gate intact.
///
/// The integration-branch check (`match_branch_at`) runs ahead of
/// this function in `check_commit_during_flow` and is NOT carved out
/// — commits on the integration branch are blocked regardless of
/// the marker.
///
/// Trust contract: the carve-out trusts the surrounding skill
/// choreography (diff review, commit message review, user approval)
/// to remain in place. The hook gate preserves the CI invariant —
/// `finalize-commit` runs `ci::run_impl()` before `git commit` on
/// every invocation regardless of how the carve-out is reached. A
/// stronger one-shot-token design is on the table if the marker-only
/// gate proves insufficient in practice.
fn check_active_flow_at(
    command: &str,
    path: &Path,
    transcript_path: Option<&Path>,
    home: &Path,
) -> Option<String> {
    let branch = detect_branch_from_path(path)?;
    let (_, project_root) = find_settings_and_root_from(path);
    let root = project_root?;
    let main_root = resolve_main_root(&root);
    if !is_flow_active(&branch, &main_root) {
        return None;
    }
    if is_finalize_commit_invocation(command)
        && state_continue_pending_is_commit(&branch, &main_root)
        && transcript_shows_commit_window_skill(transcript_path, home)
    {
        return None;
    }
    Some(commit_block_message_active_flow(&branch))
}

/// Walker check for one of the AND-combined conditions on Layer 10's
/// two carve-outs (active-flow and bootstrap-skill). Returns true iff
/// the most recent assistant Skill tool_use call since the most
/// recent user turn in the persisted transcript at `transcript_path`
/// names one of the two sanctioned commit-window skills,
/// `flow:flow-commit` or `flow-release`:
///
/// - `flow:flow-commit` — the delegated commit path used by every
///   phase skill and by `flow:flow-start` / `flow:flow-prime` during
///   bootstrap. The trust is the standard `/flow:flow-commit`
///   choreography: diff review, commit-message review, user approval.
/// - `flow-release` — recognized here only in the non-production
///   shape where it appears as an assistant Skill tool_use. In
///   production `flow-release` is a user-only skill recorded as a
///   user-typed turn, never an assistant Skill; the bootstrap
///   carve-out (`bootstrap_carveout_applies`) recognizes that
///   user-typed `/flow-release` turn directly via
///   `last_user_message_invokes_skill`. The `flow-release` arm in
///   this `matches!` covers only the assistant-Skill shape.
///
/// Returns false when `transcript_path` is None, when the walker
/// cannot read the file, or when the most recent Skill call is
/// neither of the two sanctioned skills.
///
/// Shared-predicate scoping: this predicate is consumed by BOTH
/// `bootstrap_carveout_applies` (integration-branch context) and
/// `check_active_flow_at` (active-flow feature-branch context). The
/// user-typed `/flow-release` recognition is deliberately NOT placed
/// here — it lives in `bootstrap_carveout_applies` alone. Adding a
/// `/flow-release` user-turn arm to this shared predicate would widen
/// the active-flow carve-out, which the integration-trunk-only
/// `flow-release` skill must never touch.
///
/// The walker is the load-bearing predicate that proves the
/// surrounding skill choreography actually ran. For the active-flow
/// carve-out, the `_continue_pending=commit` marker on its own is
/// belt-and-suspenders for a fresh-session resume window; the walker
/// closes the bypass-shortcut surface where a model could write the
/// marker directly and invoke `bin/flow finalize-commit` without
/// going through `/flow:flow-commit`. For the bootstrap carve-out,
/// there is no analogous marker — both `bootstrap_carveout_applies`
/// walker conditions are load-bearing. See
/// `.claude/rules/no-escape-hatches.md` Layer C for the design.
fn transcript_shows_commit_window_skill(transcript_path: Option<&Path>, home: &Path) -> bool {
    let Some(path) = transcript_path else {
        return false;
    };
    let Some(skill) = most_recent_skill_since_user(path, home) else {
        return false;
    };
    // Normalize before comparing per `.claude/rules/security-gates.md`
    // "Normalize Before Comparing". The sibling
    // `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` walker normalizes
    // its candidate strings; this predicate must apply the same
    // discipline so the two AND-combined conditions in
    // `bootstrap_carveout_applies` cannot drift on case- or
    // whitespace-variant transcript emissions.
    let norm = normalize_gate_input(&skill);
    matches!(norm.as_str(), "flow:flow-commit" | "flow-release")
}

/// Recognize a `bin/flow ... finalize-commit` invocation specifically.
/// Mirrors the `bin/flow` arm of `is_commit_invocation_inner`: handles
/// the bare `bin/flow` token and the `*/bin/flow` suffix form via
/// `is_bin_flow_token`, dequotes the first token, and matches
/// `finalize-commit` as any subsequent token (so future global flags
/// between launcher and subcommand cannot defeat the matcher).
///
/// `bash -c '<inner>'` and `sh -c '<inner>'` wrappers do NOT need to
/// be unwrapped here because Layer 8 in `validate` blocks every
/// shell-eval shape before Layer 10 runs — the wrapper itself is a
/// structural escape hatch per `.claude/rules/no-escape-hatches.md`.
///
/// Returns false for `git commit` in any form. The skill carve-out
/// is finalize-commit-only — raw `git commit` is never legitimate
/// during a flow even when the state marker is set.
fn is_finalize_commit_invocation(stripped: &str) -> bool {
    is_finalize_commit_inner(stripped)
}

fn is_finalize_commit_inner(stripped: &str) -> bool {
    let mut tokens = stripped.split_whitespace();
    let first_raw = tokens.next().unwrap_or("");
    let first = dequote_token(first_raw);
    if !is_bin_flow_token(first) {
        return false;
    }
    tokens.any(|t| t == "finalize-commit")
}

/// Read `<main_root>/.flow-states/<branch>/state.json` and return
/// true iff `_continue_pending` is the string `"commit"`. Fail-closed:
/// returns false on any read or parse error (file unreadable, JSON
/// parse failure, key absent, wrong type). The fail-closed default
/// preserves Layer 10's block when the marker cannot be definitively
/// confirmed.
///
/// `FlowPaths::try_new` is called with `.expect()` because every
/// caller (`check_active_flow_at`) gates on `is_flow_active(&branch,
/// &main_root)` returning true. `is_flow_active` itself calls
/// `FlowPaths::try_new(root, branch)` and returns false on `None`,
/// so the same call here with the same arguments is guaranteed to
/// succeed. See `.claude/rules/testability-means-simplicity.md`
/// "When the test resists the real production path" — `.expect()`
/// on the unreachable arm does not create a coverage branch.
fn state_continue_pending_is_commit(branch: &str, main_root: &Path) -> bool {
    let paths = FlowPaths::try_new(main_root, branch)
        .expect("is_flow_active gate guarantees FlowPaths-valid branch");
    let Ok(content) = std::fs::read_to_string(paths.state_file()) else {
        return false;
    };
    let Ok(state) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    state.get("_continue_pending").and_then(|v| v.as_str()) == Some("commit")
}

/// Recognize a `bin/flow ... ci` invocation. Tokenizes on whitespace,
/// dequotes the first token, requires it to be `bin/flow` or any
/// absolute path ending in `/bin/flow`, skips leading global flags
/// (`--log-level info`, `--branch=foo`), then requires the first
/// non-flag token to equal `ci`.
///
/// The subcommand-position discipline prevents false positives where
/// a sibling `bin/flow` subcommand happens to take `ci` as a flag
/// value or positional argument: `bin/flow phase-enter --phase ci`,
/// `bin/flow log feat "Phase 2 ci notes"`,
/// `bin/flow set-timestamp --field ci`, and
/// `bin/flow phase-transition --action ci` all return false.
///
/// Returns false for non-`bin/flow` first tokens (`bin/test`,
/// `git ci`, `npm ci`) and for `bin/flow` invocations whose
/// subcommand is not `ci` (`bin/flow status`, `bin/flow finalize-commit`).
///
/// `bin/flow --log-level info ci` returns true: `--log-level` is a
/// global flag, `info` is its value (skipped), `ci` is the
/// subcommand. `bin/flow --log-level=info ci` likewise returns true:
/// `--log-level=info` is a self-contained flag arg, `ci` is the next
/// non-flag token.
fn is_flow_ci_invocation(stripped: &str) -> bool {
    let mut tokens = stripped.split_whitespace();
    let first_raw = tokens.next().unwrap_or("");
    let first = dequote_token(first_raw);
    if !is_bin_flow_token(first) {
        return false;
    }
    let mut consume_next_as_flag_value = false;
    for token in tokens {
        if consume_next_as_flag_value {
            consume_next_as_flag_value = false;
            continue;
        }
        if let Some(stripped_flag) = token.strip_prefix("--") {
            // `--flag=value` is self-contained; advance to next token.
            if stripped_flag.contains('=') {
                continue;
            }
            // `--flag` with separate value: consume next token too.
            consume_next_as_flag_value = true;
            continue;
        }
        // First non-flag token is the subcommand.
        return token == "ci";
    }
    false
}

/// Recognize the `--clean` flag in a tokenized command.
///
/// Inputs are normalized per `.claude/rules/security-gates.md`
/// "Normalize Before Comparing": case-folded to ASCII lowercase
/// before comparison. `--CLEAN`, `--Clean`, and `--clean` all match.
/// The `--clean=value` form (where value is any text) also matches:
/// users reach for the documented phantom-misses recovery path via
/// either bare or assigned-value flag shape. `--no-clean`,
/// `--cleanup`, and `--clean-files` do NOT match.
fn has_clean_flag(stripped: &str) -> bool {
    stripped.split_whitespace().any(|t| {
        let lower = t.to_ascii_lowercase();
        lower == "--clean" || lower.starts_with("--clean=")
    })
}

/// Read `<main_root>/.flow-states/<branch>/state.json` and return
/// true iff `current_phase == "flow-code"` AND
/// `phases.flow-code.status == "in_progress"`.
///
/// Fail-closed-as-NO-BLOCK. This helper INVERTS the fail-closed
/// posture of `state_continue_pending_is_commit` and Layer 10. Layer
/// 11 is a friction-prevention gate; the conservative response to a
/// state-file read error is "let `bin/flow ci` through" rather than
/// block on uncertainty. A mis-block in a wrongly-detected non-Code
/// phase would surprise the user; a missed block in a corrupted-state
/// edge case loses nothing because the user is already in an error-
/// recovery posture.
///
/// `FlowPaths::try_new` is called with `.expect()` because every
/// caller (`check_ci_during_code_phase`) gates on `is_flow_active`
/// returning true. `is_flow_active` itself calls `FlowPaths::try_new`
/// with the same arguments and returns false on `None`, so reaching
/// this helper guarantees the constructor succeeds.
///
/// String comparisons are normalized per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing":
/// `current_phase` and `phases.flow-code.status` are stripped of
/// NULs, trimmed, and ASCII-lowercased before comparison so a
/// hand-edited `" Flow-Code "` or `"In_Progress"` still trips the
/// gate.
///
/// The read is bounded at `STATE_FILE_BYTE_CAP` (8 MB) per
/// `.claude/rules/external-input-path-construction.md` "Enforce a
/// documented size cap on every external read". A corrupted or
/// maliciously-large state file cannot OOM-kill the hook.
fn state_is_in_code_phase(branch: &str, main_root: &Path) -> bool {
    let paths = FlowPaths::try_new(main_root, branch)
        .expect("is_flow_active gate guarantees FlowPaths-valid branch");
    let Ok(content) = read_state_file_capped(&paths.state_file()) else {
        return false;
    };
    let Ok(state) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    let current = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    if current != "flow-code" {
        return false;
    }
    let status = state
        .get("phases")
        .and_then(|p| p.get("flow-code"))
        .and_then(|p| p.get("status"))
        .and_then(|v| v.as_str())
        .map(normalize_gate_input)
        .unwrap_or_default();
    status == "in_progress"
}

/// Byte cap for `<main_root>/.flow-states/<branch>/state.json` reads.
/// 8 MB comfortably covers every state file the FLOW writer produces
/// — typical state files are 5-30 KB, the largest observed in
/// practice carry compaction summaries up to ~500 KB. The cap exists
/// to bound a corrupted or maliciously-large state file from
/// OOM-killing a hook script.
const STATE_FILE_BYTE_CAP: u64 = 8 * 1024 * 1024;

/// Read a state file with a documented size cap. Returns the file
/// contents truncated to `STATE_FILE_BYTE_CAP` bytes. The truncation
/// is silent — `serde_json::from_str` downstream will reject any
/// truncated-mid-token result on its own, falling through to the
/// caller's fail-closed branch.
fn read_state_file_capped(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = String::new();
    file.take(STATE_FILE_BYTE_CAP).read_to_string(&mut buf)?;
    Ok(buf)
}

/// Layer 11 block message. Redirects `bin/flow ci` to the per-file
/// gate during Code phase. The per-file gate enforces identical
/// 100/100/100 thresholds at seconds-scale; full CI at 3–4 minutes
/// is wasteful for single-file iteration. The pre-commit gate inside
/// `finalize-commit` still runs full CI on every commit, so
/// cross-file regressions are caught at the commit boundary.
///
/// `bin/flow ci --clean` is preserved as the documented phantom-
/// misses recovery path per `.claude/rules/per-file-coverage-iteration.md`.
const LAYER_11_BLOCK_MSG: &str = "BLOCKED: `bin/flow ci` is disabled during Code phase. \
    Run the per-file gate defined in the project's CLAUDE.md (the \"Default iteration \
    loop\") — for projects using cargo-llvm-cov nextest the form is \
    `bin/test tests/<name>.rs`; consult CLAUDE.md for the project-specific command. \
    The pre-commit gate runs full CI automatically inside `/flow:flow-commit`. To \
    clean stale coverage artifacts, `bin/flow ci --clean` is still allowed. See \
    .claude/rules/per-file-coverage-iteration.md \"Enforcement\".";

/// Layer 11: redirect `bin/flow ci` to the per-file gate during Code
/// phase. Fires when ALL of the following hold:
///
/// 1. `is_flow_ci_invocation(command)` — the command shape is
///    `bin/flow ... ci` (with any global flags before `ci`).
/// 2. `!has_clean_flag(command)` — the `--clean` carve-out is NOT
///    present. `bin/flow ci --clean` is the documented phantom-misses
///    fix and stays available throughout Code phase.
/// 3. An active flow exists at `<main_root>/.flow-states/<branch>/
///    state.json` resolved from `cwd`.
/// 4. `state_is_in_code_phase(branch, main_root)` — `current_phase`
///    is `flow-code` AND `phases.flow-code.status` is `in_progress`.
///
/// `finalize_commit::run_impl` calls `ci::run_impl()` as a Rust
/// function from inside the same process — it never reaches this
/// Bash hook, so the commit-time CI gate is structurally unaffected
/// by Layer 11. The redirect only catches Bash invocations the model
/// types directly during the Code phase.
///
/// Fail-closed-as-no-block. Every read or parse error in
/// `state_is_in_code_phase` returns false → no block. This inverts
/// Layer 10's fail-closed-by-blocking posture: a friction-prevention
/// gate's conservative response is "let the command through" because
/// mis-blocking a legitimate `bin/flow ci` in a wrongly-detected
/// non-Code phase would surprise the user.
fn check_ci_during_code_phase(command: &str, cwd: &Path) -> Option<String> {
    if !is_flow_ci_invocation(command) {
        return None;
    }
    if has_clean_flag(command) {
        return None;
    }
    let branch = detect_branch_from_path(cwd)?;
    let (_, project_root) = find_settings_and_root_from(cwd);
    let root = project_root?;
    let main_root = resolve_main_root(&root);
    if !is_flow_active(&branch, &main_root) {
        return None;
    }
    if !state_is_in_code_phase(&branch, &main_root) {
        return None;
    }
    Some(LAYER_11_BLOCK_MSG.to_string())
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

/// Read the state file at `state_path` and return `true` when
/// `_halt_pending` is truthy per
/// `.claude/rules/rust-patterns.md` "Hook Input Boolean Field
/// Tolerance". Reads are bounded at `STATE_FILE_BYTE_CAP` per
/// `.claude/rules/external-input-path-construction.md` so a
/// corrupted or hostile state file cannot OOM the hook. Every
/// error class (missing file, oversized read, non-JSON content,
/// missing field) returns `false`. Fail-open is the correct
/// posture: the halt gate's purpose is to refuse model-initiated
/// flow-advancing work during the halt window; a missing or
/// corrupt state file means no flow is halted.
fn is_halt_set(state_path: &std::path::Path) -> bool {
    use std::fs::File;
    use std::io::{BufReader, Read};
    let f = match File::open(state_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = String::new();
    let _ = BufReader::new(f.take(crate::hooks::stop_continue::STATE_FILE_BYTE_CAP))
        .read_to_string(&mut buf);
    serde_json::from_str::<Value>(&buf)
        .ok()
        .map(|v| crate::hooks::transcript_walker::is_truthy(v.get("_halt_pending")))
        .unwrap_or(false)
}

/// Recognize a Bash command that advances the autonomous flow — the
/// shape the halt gate must block. The closed set covers every
/// `bin/flow` subcommand that mutates state in a way that would
/// progress the flow past the user's halt directive: code-task
/// counter increment, phase entry / completion / transition, the
/// commit finalize, and the per-session utility marker that gates
/// multi-step utility skills. Other `bin/flow` subcommands (logging,
/// status, capture, plan-from-issue, etc.) are read-only or
/// non-advancing and must pass through the halt gate.
///
/// Tokenizes on whitespace per
/// `.claude/rules/rust-patterns.md` "Stateful Predicate-Based
/// Scanners" defence-in-depth — the matcher cannot be defeated by a
/// path prefix, env-var prefix, or wrapper launcher because the
/// upstream structural escape-hatch layer in `validate()` already
/// rejects those shapes.
fn is_flow_advancing_bash_command(cmd: &str) -> bool {
    let mut tokens = cmd.split_whitespace();
    // `run()` exits early when `command.is_empty()` (Agent-tool
    // dispatch path), so the halt-gate caller never invokes this
    // helper with an empty command — the first token is guaranteed
    // per `.claude/rules/testability-means-simplicity.md` "When
    // the test resists the real production path".
    let program = tokens
        .next()
        .expect("run() exits for command.is_empty() before halt gate");
    if !(program == "bin/flow" || program.ends_with("/bin/flow")) {
        return false;
    }
    // Layer 9's whitelist rejects bare `bin/flow` (no subcommand)
    // during active flows because every allow-list pattern requires
    // an argument (`Bash(*bin/flow *)`), and the halt gate only
    // runs when a flow is active. The second token is therefore
    // guaranteed in production.
    let subcommand = tokens
        .next()
        .expect("Layer 9 whitelist rejects bare bin/flow with no subcommand before halt gate");
    match subcommand {
        "phase-enter"
        | "phase-finalize"
        | "phase-transition"
        | "finalize-commit"
        | "set-utility-in-progress" => true,
        "set-timestamp" => {
            // Only block `--set code_task=*` updates; other fields
            // (like `code_task_name`) are non-advancing and must
            // pass even during halt. Clap accepts the flag in two
            // forms — space-separated (`--set code_task=4`,
            // producing tokens `["--set", "code_task=4"]`) and
            // equals-fused (`--set=code_task=4`, producing a
            // single token `--set=code_task=4`). The matcher
            // recognizes both: a token starting with `code_task=`
            // OR a token starting with `--set=code_task=`. Without
            // the equals form, a model invoking the fused syntax
            // during a halt would bypass the gate.
            tokens.any(|t| t.starts_with("code_task=") || t.starts_with("--set=code_task="))
        }
        _ => false,
    }
}

/// Decision core for the validate-pretool hook. Returns
/// `(exit_code, Option<stderr_message>)` so `run()` can translate the
/// decision into `eprintln!` + `process::exit` side effects.
///
/// `cwd` is the resolved working directory the hook reasons about —
/// `run()` supplies it. Every cwd-derived consumer (settings/project
/// root discovery, branch detection, `main_root`, the agent-prompt
/// `worktree_root`, and the Layer 10/11 + halt gates) reads the single
/// `cwd` value threaded here so they cannot diverge. A `None` cwd
/// (env::current_dir() failed upstream) makes each consumer fall
/// through to its no-flow default. Integration tests drive every
/// branch through the hook subprocess with fixture stdin payloads.
fn run_impl_main(hook_input: Option<Value>, cwd: Option<String>) -> (i32, Option<String>) {
    let hook_input = match hook_input {
        Some(input) => input,
        None => return (0, None),
    };

    // Resolve cwd-derived inputs ONCE from the single threaded `cwd`
    // value and reuse for settings discovery and branch detection. A
    // `None` cwd (env::current_dir() failed upstream, e.g. the
    // stale-cwd adversarial path) makes both settings and branch fall
    // through to None. Per `.claude/rules/testability-means-simplicity.md`
    // the prior `find_settings_and_root`/`detect_branch_from_cwd`
    // generic seams have been removed because their per-monomorphization
    // Err arms were unreachable through any production callsite — the
    // stale-cwd subprocess test covers the failure path here instead.
    let cwd: Option<PathBuf> = cwd.map(PathBuf::from);
    let (settings, project_root) = cwd
        .as_deref()
        .map(find_settings_and_root_from)
        .unwrap_or((None, None));
    // Derive branch and main_root independently of settings.json
    // presence per Review finding #9: a missing settings.json
    // (interrupted prime, .gitignore'd in CI) must not silently
    // disable the halt gate. settings.json is consulted only for
    // Layer 9 whitelist enforcement.
    let branch = cwd.as_deref().and_then(detect_branch_from_path);
    let main_root = match project_root.as_ref() {
        Some(r) => Some(resolve_main_root(r)),
        None => cwd.as_deref().map(resolve_main_root),
    };
    let flow_active = match (&branch, &main_root) {
        (Some(b), Some(r)) => is_flow_active(b, r),
        _ => false,
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let transcript_path: Option<PathBuf> = hook_input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let home = home_dir_or_empty();

    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Pre-validation: CI is always a gate; other commands only blocked in FLOW phases
    if let Some(bg) = tool_input.get("run_in_background") {
        if is_bg_truthy(bg) {
            if let Some(msg) = should_block_background(command, flow_active) {
                return (2, Some(msg));
            }
        }
    }
    if command.is_empty() {
        // No command means this is an Agent tool call, not Bash.
        // Layer ordering: the cheap `subagent_type` check runs first
        // (rejects `general-purpose` calls before any prompt
        // tokenization), then the expensive prompt-body scan runs on
        // the Agent's prompt field per issue #1704. The prompt scan
        // is scoped to active flows and bounded at
        // AGENT_PROMPT_BYTE_CAP.
        let subagent_type = tool_input.get("subagent_type").and_then(|v| v.as_str());
        let (allowed, message) = validate_agent(subagent_type, flow_active);
        if !allowed {
            return (2, Some(message));
        }
        let prompt_field = tool_input.get("prompt").and_then(|v| v.as_str());
        let worktree_root = cwd.as_deref().and_then(|c| {
            crate::flow_paths::compute_worktree_root(&c.to_string_lossy()).map(PathBuf::from)
        });
        if let Some(root) = worktree_root {
            let (prompt_allowed, prompt_message) =
                crate::hooks::agent_prompt_scan::validate_agent_prompt(
                    prompt_field,
                    &root,
                    flow_active,
                );
            if !prompt_allowed {
                return (2, Some(prompt_message));
            }
        }
        return (0, None);
    }

    let (allowed, message) = validate(command, settings.as_ref(), flow_active);
    if !allowed {
        return (2, Some(message));
    }

    // Layer 10: block direct commit invocations when the hook's
    // effective cwd resolves either to the integration branch named
    // by `default_branch_in` OR to a feature branch with an active
    // FLOW state file at `.flow-states/<branch>/state.json`. Layered
    // after validate() returns Ok rather than as another layer inside
    // validate() because validate() does not receive cwd — adding it
    // would expand the function's signature across every existing
    // caller. Commands blocked by Layers 1-9 never reach this point;
    // Layer 10 fires only when the command passes all preceding
    // structural gates AND is a commit invocation routed through one
    // of the two trigger contexts.
    if let Some(cwd_path) = cwd.as_deref() {
        if let Some(msg) =
            check_commit_during_flow(command, cwd_path, transcript_path.as_deref(), &home)
        {
            return (2, Some(msg));
        }
    }

    // Layer 11: redirect `bin/flow ci` to the per-file gate during
    // Code phase. Layered outside `validate()` for the same reason
    // as Layer 10 — the gate needs `cwd` to resolve the branch and
    // state file. The single carve-out is `bin/flow ci --clean` (the
    // documented phantom-misses recovery path). `finalize-commit`
    // calls `ci::run_impl()` as a Rust function and never reaches
    // this Bash hook, so the commit-time CI gate is structurally
    // unaffected. See `.claude/rules/per-file-coverage-iteration.md`.
    if let Some(cwd_path) = cwd.as_deref() {
        if let Some(msg) = check_ci_during_code_phase(command, cwd_path) {
            return (2, Some(msg));
        }
    }

    // Halt gate: block flow-advancing Bash commands when the
    // active flow's state file has `_halt_pending=true`. The gate
    // closes the surface where a model would otherwise advance the
    // counter, transition phases, or commit while the user has
    // paused the autonomous flow. `/flow:flow-continue` clears the
    // halt by calling `bin/flow clear-halt`, which is itself self-
    // gated (Layer 1 of `validate-skill` plus the transcript-walker
    // check inside `clear-halt::run_impl`) — so this gate does NOT
    // need an explicit pass-through for `clear-halt`: the command
    // is not in `is_flow_advancing_bash_command`'s allowlist and
    // falls through.
    if let (Some(b), Some(r)) = (&branch, &main_root) {
        if is_flow_advancing_bash_command(command) {
            // `FlowPaths::try_new` returns None on slash- or
            // NUL-containing branches per
            // `.claude/rules/branch-path-safety.md`. An invalid
            // branch cannot have an active flow at any
            // `.flow-states/<branch>/` path so the halt gate
            // correctly falls through (`unwrap_or(false)`).
            let halt = crate::flow_paths::FlowPaths::try_new(r, b)
                .map(|paths| is_halt_set(&paths.state_file()))
                .unwrap_or(false);
            if halt {
                return (
                    2,
                    Some(
                        "BLOCKED: this flow is halted. The autonomous flow paused after a user \
                         message and stays paused until the user explicitly resumes or aborts. \
                         The model cannot advance the flow (counter, phase, commit, marker) \
                         while halted. Two exits are available — only the user can take them: \
                         type `/flow:flow-continue` to resume, or `/flow:flow-abort` to close \
                         the flow. See .claude/rules/autonomous-phase-discipline.md."
                            .to_string(),
                    ),
                );
            }
        }
    }

    (0, None)
}

/// Run the validate-pretool hook (entry point from CLI). Thin wrapper
/// around `run_impl_main` that resolves the cwd and translates the
/// decision into stderr + exit-code side effects.
///
/// The cwd is resolved from the hook payload's `cwd` field via
/// `resolve_hook_cwd`, with `std::env::current_dir()` as fallback. The
/// payload `cwd` is the session/sub-agent worktree; the hook
/// subprocess's own `env::current_dir()` can resolve to the main repo
/// root, where the five cwd consumers (branch detection, `main_root`,
/// `flow_active`, the agent-prompt `worktree_root`, and the
/// Layer 10/11 and halt gates) would all see the wrong directory —
/// most visibly the agent-prompt scan would resolve no worktree and
/// skip, letting an out-of-worktree path reach a sub-agent. The
/// fallback's `.ok()` None arm (cwd inode unlinked) is covered by the
/// stale-cwd subprocess test.
pub fn run() {
    let hook_input = read_hook_input();
    let cwd = hook_input.as_ref().and_then(crate::hooks::resolve_hook_cwd);
    let (code, message) = run_impl_main(hook_input, cwd);
    if let Some(msg) = message {
        eprintln!("{}", msg);
    }
    std::process::exit(code);
}
