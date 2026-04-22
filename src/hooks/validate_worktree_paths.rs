//! PreToolUse hook that validates file tool calls in FLOW worktrees.
//!
//! Two enforcement layers:
//! 1. **Worktree path redirection** — blocks file tool calls that target the
//!    main repo when the working directory is inside a FLOW worktree, directing
//!    the caller to use the worktree copy instead.
//! 2. **Shared config protection** — blocks Edit/Write calls on shared
//!    configuration files (`.gitignore`, `Cargo.toml`, `.github/`, etc.) when
//!    the CWD is inside a `.worktrees/` directory (the flow-active proxy).
//!    Only Edit and Write tool names trigger the block — Read/Glob/Grep are
//!    allowed so codebase exploration is not impacted. The block message
//!    directs the caller to confirm with the user before proceeding.
//!
//! Fires on Edit, Write, Read, Glob, and Grep tool calls.
//!
//! Exit 0 — allow (path is fine or not in a worktree)
//! Exit 2 — block (path targets main repo, or shared config Edit/Write)

use std::path::Path;

use serde_json::Value;

use super::read_hook_input;
use crate::flow_paths::FlowStatesDir;

const WORKTREE_MARKER: &str = ".worktrees/";

/// Filenames that are shared configuration affecting all engineers.
///
/// Matches the canonical list from `.claude/rules/permissions.md`
/// "Shared Config Files" section. `.claude/settings.json` is excluded
/// because `validate-claude-paths` already covers it.
const SHARED_CONFIG_FILENAMES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    "Makefile",
    "Rakefile",
    "justfile",
    "package.json",
    "requirements.txt",
    "go.mod",
    "Cargo.toml",
];

/// Check if a file path targets a shared configuration file.
///
/// Returns `true` when the filename matches one of the nine canonical
/// shared-config filenames, or when the path passes through a `.github/`
/// directory (workflows, issue templates, CODEOWNERS).
pub fn is_shared_config(file_path: &str) -> bool {
    let path = Path::new(file_path);
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Check filename against the exact-match list. Empty file_path
    // yields an empty components vec → `.last()` is None → inner
    // block is skipped → fall through to the .github loop and
    // `return false` below, matching the intent of the prior
    // early-return without a separate uncovered guard.
    if let Some(filename) = components.last() {
        if SHARED_CONFIG_FILENAMES.contains(filename) {
            return true;
        }
    }

    // Check for .github/ directory with descendants
    for (i, comp) in components.iter().enumerate() {
        if *comp == ".github" && i + 1 < components.len() {
            return true;
        }
    }

    false
}

/// Check if an Edit/Write on a shared config file should be blocked.
///
/// Returns `(allowed, message)`. Only blocks when all of:
/// - `tool_name` is "Edit" or "Write" (reads are fine)
/// - CWD is inside a `.worktrees/` directory
/// - `file_path` is inside the worktree (not targeting main repo or external paths)
/// - the file matches the shared-config list
pub fn validate_shared_config(file_path: &str, cwd: &str, tool_name: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    if tool_name != "Edit" && tool_name != "Write" {
        return (true, String::new());
    }

    // The hook fires on all Edit/Write calls, but shared-config blocking
    // only applies during active flows. The `.worktrees/` marker in CWD is
    // the flow-active proxy — outside a worktree, the gate is a no-op so
    // pre-flow and post-flow edits are not blocked.
    if !cwd.contains(WORKTREE_MARKER) {
        return (true, String::new());
    }

    // Only block paths inside the worktree cwd
    let cwd_prefix = format!("{}/", cwd);
    if !file_path.starts_with(&cwd_prefix) && file_path != cwd {
        return (true, String::new());
    }

    if !is_shared_config(file_path) {
        return (true, String::new());
    }

    // For .github/ directory matches, surface `.github/` as the protected
    // boundary rather than the leaf filename (e.g. "ci.yml" is not inherently
    // shared config — the `.github/` directory is).
    let display_name = if file_path.contains("/.github/") {
        ".github/".to_string()
    } else {
        Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
            .to_string()
    };

    (
        false,
        format!(
            "BLOCKED: {} is a shared configuration file that affects every engineer \
             in the repository. Modifying it during a FLOW phase requires explicit \
             user permission. Use AskUserQuestion to confirm with the user before \
             proceeding. See .claude/rules/permissions.md \"Shared Config Files\" section.",
            display_name
        ),
    )
}

/// Extract the file path from tool input.
///
/// Edit/Write/Read use `file_path`. Glob/Grep use `path`.
pub fn get_file_path(tool_input: &Value) -> String {
    if let Some(fp) = tool_input.get("file_path").and_then(|v| v.as_str()) {
        return fp.to_string();
    }
    if let Some(p) = tool_input.get("path").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    String::new()
}

/// Validate that `file_path` targets the worktree, not the main repo.
///
/// Returns `(allowed, message)`.
pub fn validate(file_path: &str, cwd: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    let marker_pos = match cwd.find(WORKTREE_MARKER) {
        Some(pos) => pos,
        None => return (true, String::new()), // not in a worktree
    };

    let project_root = cwd[..marker_pos].trim_end_matches('/');

    // Paths outside the project are always fine (~/.claude, /tmp, etc.)
    let prefix = format!("{}/", project_root);
    if !file_path.starts_with(&prefix) {
        return (true, String::new());
    }

    // Paths inside the worktree are fine
    let cwd_prefix = format!("{}/", cwd);
    if file_path.starts_with(&cwd_prefix) || file_path == cwd {
        return (true, String::new());
    }

    // .flow-states/ is the shared state directory at the main repo — always fine
    let flow_states_dir = FlowStatesDir::new(Path::new(project_root));
    let flow_states_prefix = format!("{}/", flow_states_dir.path().to_string_lossy());
    if file_path.starts_with(&flow_states_prefix) {
        return (true, String::new());
    }

    // Block: path targets main repo from inside a worktree
    let relative = &file_path[project_root.len() + 1..];
    let corrected = format!("{}/{}", cwd, relative);

    (
        false,
        format!(
            "BLOCKED: You are in worktree {}. Use {} instead of {}",
            cwd, corrected, file_path
        ),
    )
}

/// Decision core for the validate-worktree-paths hook. Returns
/// `(exit_code, Option<stderr_message>)` so `run()` can translate to
/// `process::exit` + `eprintln!` side effects. Integration tests
/// drive every branch through the hook subprocess with fixture
/// stdin payloads.
fn run_impl_main(hook_input: Option<Value>, cwd: Option<String>) -> (i32, Option<String>) {
    let hook_input = match hook_input {
        Some(v) => v,
        None => return (0, None),
    };

    let tool_input = hook_input
        .get("tool_input")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let file_path = get_file_path(&tool_input);
    if file_path.is_empty() {
        return (0, None);
    }

    let cwd = match cwd {
        Some(p) => p,
        None => return (0, None),
    };

    let (allowed, message) = validate(&file_path, &cwd);
    if !allowed {
        return (2, Some(message));
    }

    let tool_name = hook_input
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let (sc_allowed, sc_message) = validate_shared_config(&file_path, &cwd, tool_name);
    if !sc_allowed {
        return (2, Some(sc_message));
    }

    (0, None)
}

/// Run the validate-worktree-paths hook (entry point from CLI). Thin
/// wrapper around `run_impl_main` that translates decisions into
/// stderr + exit code side effects.
pub fn run() {
    let hook_input = read_hook_input();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let (code, message) = run_impl_main(hook_input, cwd);
    if let Some(msg) = message {
        eprintln!("{}", msg);
    }
    std::process::exit(code);
}
