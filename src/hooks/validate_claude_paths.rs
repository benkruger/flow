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

/// Check if a file path targets a protected .claude/ location.
///
/// Protected: .claude/rules/ (any depth), .claude/skills/ (any depth),
/// CLAUDE.md (any level).
/// Not protected: .claude/settings.json, .claude/settings.local.json.
///
/// Matching is ASCII-case-insensitive for `.claude`, `rules`, `skills`,
/// and `CLAUDE.md` so a caller on a case-insensitive filesystem
/// (macOS APFS/HFS+ by default) cannot bypass the gate by writing to
/// `.CLAUDE/rules/foo.md` or `Claude.md` — which resolve to the same
/// inode as the protected names.
pub fn is_protected_path(file_path: &str) -> bool {
    if file_path.is_empty() {
        return false;
    }

    let path = Path::new(file_path);
    let components: Vec<&str> = path
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();

    // Check for .claude/rules/ or .claude/skills/ at any depth.
    for (i, comp) in components.iter().enumerate() {
        if comp.eq_ignore_ascii_case(".claude") && i + 1 < components.len() {
            let next = components[i + 1];
            if next.eq_ignore_ascii_case("rules") || next.eq_ignore_ascii_case("skills") {
                return true;
            }
        }
    }

    // Check for CLAUDE.md at any level. The empty-string early-return
    // above guarantees `components` is non-empty, so `.last()` is always
    // `Some`. Substituting `""` for the unreachable None case keeps the
    // comparison safe (`""` cannot match `"CLAUDE.md"` under any casing)
    // and avoids producing a None arm that no test can reach.
    let filename = components.last().copied().unwrap_or("");
    if filename.eq_ignore_ascii_case("CLAUDE.md") {
        return true;
    }

    false
}

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

    if !is_protected_path(file_path) {
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
