//! PreToolUse hook that blocks Edit/Write on .claude/rules/, .claude/skills/,
//! and CLAUDE.md during active FLOW phases, redirecting to bin/flow write-rule.
//!
//! Fires on Edit and Write tool calls.
//!
//! Exit 0 — allow (path is not protected, or no FLOW phase active)
//! Exit 2 — block (path is protected and FLOW phase is active)

use std::path::Path;

use super::{detect_branch_from_path, is_flow_active, read_hook_input, resolve_main_root};
use crate::flow_paths::FlowStatesDir;
use crate::protected_paths::is_protected_path;

/// Validate that an Edit/Write on this path is allowed.
///
/// Returns `(allowed, message)`.
pub fn validate(file_path: &str, flow_active: bool) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    if !flow_active {
        return (true, String::new());
    }

    if !is_protected_path(Path::new(file_path)) {
        return (true, String::new());
    }

    (
        false,
        "BLOCKED: .claude/ paths are protected during FLOW phases. \
         Use `${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule --path <target> --content-file <temp>` instead. \
         Write the full file content to a temp file in .flow-states/, \
         then run the write-rule command."
            .to_string(),
    )
}

/// Find the project root by walking up from `cwd` for a `.flow-states/`
/// directory. Pure helper — accepts `cwd` as a parameter so unit tests
/// can drive every branch with a `TempDir` fixture. Mirrors the sibling
/// cwd-injection pattern in `src/hooks/mod.rs`
/// (`find_settings_and_root_from`, `detect_branch_from_path`).
fn find_project_root_in(cwd: &Path) -> Option<std::path::PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if FlowStatesDir::new(&current).path().is_dir() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Pure core of the validate-claude-paths hook.
///
/// Accepts the parsed stdin payload and the resolved cwd as injected
/// dependencies so every branch is reachable from unit tests with a
/// `TempDir` fixture. `cwd` is optional so the wrapper can pass
/// `std::env::current_dir().ok()` without an untestable fallback
/// closure — an unresolvable cwd means no project_root can be
/// detected, so the hook silently allows the action. Follows the
/// `run_impl_main` pattern in `.claude/rules/rust-patterns.md` —
/// `process::exit` and stderr I/O live in the thin `run()` wrapper
/// below.
///
/// Return contract:
/// - `(0, None)` → allow silently (wrapper exits 0, no stderr)
/// - `(2, Some(message))` → block (wrapper prints message to stderr, exits 2)
pub fn run_impl_main(
    hook_input: Option<serde_json::Value>,
    cwd: Option<&Path>,
) -> (i32, Option<String>) {
    let hook_input = match hook_input {
        Some(v) => v,
        None => return (0, None),
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let file_path = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if file_path.is_empty() {
        return (0, None);
    }

    // Unresolvable cwd (None) flows through the same branch as
    // "no .flow-states/ ancestor" — project_root ends up None and
    // flow_active stays false, so the hook silently allows the action.
    let project_root = cwd.and_then(find_project_root_in);
    let branch = match (project_root.as_ref(), cwd) {
        (Some(_), Some(c)) => detect_branch_from_path(c),
        _ => None,
    };
    let flow_active = match (&branch, &project_root) {
        (Some(b), Some(r)) => is_flow_active(b, &resolve_main_root(r)),
        _ => false,
    };

    let (allowed, message) = validate(file_path, flow_active);
    if !allowed {
        return (2, Some(message));
    }

    (0, None)
}

/// Run the validate-claude-paths hook (entry point from CLI).
///
/// Thin wrapper: reads stdin, resolves `std::env::current_dir()`,
/// calls `run_impl_main`, writes any block message to stderr, and
/// exits with the returned code.
pub fn run() {
    let input = read_hook_input();
    let cwd = std::env::current_dir().ok();
    let (code, message) = run_impl_main(input, cwd.as_deref());
    if let Some(m) = message {
        eprintln!("{}", m);
    }
    std::process::exit(code);
}
