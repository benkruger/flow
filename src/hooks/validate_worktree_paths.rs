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
use crate::flow_paths::{compute_worktree_paths, FlowStatesDir};

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
/// Returns `(allowed, message)`. The edit would block when all of:
/// - `tool_name` is "Edit" or "Write" (reads are fine and never
///   consult or consume a marker)
/// - CWD is inside a `.worktrees/` directory
/// - `file_path` is inside the worktree (not targeting main repo or external paths)
/// - the file matches the shared-config list
///
/// The "proceed" half: immediately before the block return, the gate
/// consults+consumes a single-use approval marker via
/// `shared_config_approval::check_and_consume_approval` keyed on
/// `(project_root, branch, file_path)` — `project_root`/branch derived
/// from `cwd` via `compute_worktree_paths` (branch = the worktree
/// directory name). A valid unconsumed marker allows the edit exactly
/// once (the marker is deleted). Any absence, corruption, IO error,
/// per-file mismatch, or unresolvable worktree keeps blocking
/// (fail-closed — a corrupt marker can never become an escape hatch,
/// the deliberate asymmetry vs. Layer 11). The block message names
/// the `bin/flow approve-shared-config` recovery path and the exact
/// `approve shared-config: <path>` phrase the user must type so the
/// transcript self-gate (`user_approved_shared_config_edit`) can
/// authorize the subcommand.
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

    // Proceed half: a valid unconsumed single-use approval marker
    // for this exact file allows the edit once. project_root/branch
    // come from the same `compute_worktree_paths` the worktree gate
    // uses (branch = the worktree directory name, structurally
    // `/`-free). Any unresolvable worktree, invalid branch, or
    // marker IO/parse failure falls through to the block
    // (fail-closed) because `check_and_consume_approval` returns
    // false on every error class.
    if let Some((project_root, worktree_root)) = compute_worktree_paths(cwd) {
        // `compute_worktree_paths` only returns `Some` for a path of
        // the form `<root>/.worktrees/<branch>` with a non-empty,
        // trailing-slash-stripped branch segment, so `file_name` is
        // structurally `Some`. The `.expect` documents that
        // invariant; it is unreachable, not a panic vector.
        let branch = Path::new(worktree_root)
            .file_name()
            .and_then(|n| n.to_str())
            .expect("compute_worktree_paths yields a path ending in the branch segment");
        if crate::shared_config_approval::check_and_consume_approval(
            Path::new(project_root),
            branch,
            file_path,
        ) {
            return (true, String::new());
        }
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
             user permission. To authorize this single edit, the USER must reply \
             with the exact line `approve shared-config: {}`. Do NOT run \
             `bin/flow approve-shared-config` until the user has sent that exact \
             reply — wait for it. Once the user has replied, run \
             `bin/flow approve-shared-config --path {}` and retry the edit. A \
             `not_user_approved` result means the user has not yet replied: keep \
             waiting, do not retry the edit or re-run the subcommand in a loop. \
             The grant is single-use and scoped to this file. See \
             .claude/rules/permissions.md \"Shared Config Files\" section.",
            display_name, file_path, file_path
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

/// Detect a `.flow-states/` write that targets a worktree-internal copy
/// instead of the canonical main-repo location.
///
/// `.flow-states/` is the shared state directory and lives ONLY at
/// `<project_root>/.flow-states/`. A tool call that writes to
/// `<project_root>/.worktrees/<branch>/.flow-states/...` (worktree root)
/// or `<project_root>/.worktrees/<branch>/<service>/.flow-states/...`
/// (mono-repo service subdir) would create a misplaced copy invisible
/// to the readers (cleanup, discovery scanners, hooks) that scan only
/// the canonical location.
///
/// Input normalization runs before matching so the canonical-only
/// invariant holds across filesystem variants and bypass shapes:
///
/// - **Doubled slashes** (`<root>//.worktrees/...`) are collapsed to
///   single slashes so the worktrees-prefix probe matches the
///   intended segment rather than falling through to the generic
///   main-repo block (which would name a recursive worktree path
///   in its redirect message).
/// - **Case variants** (`.Flow-States/`, `.FLOW-STATES/`) are matched
///   case-insensitively. macOS APFS is case-insensitive by default, so
///   any case variant resolves to the same inode as `.flow-states/`;
///   without case-insensitive matching, a model writing
///   `.Flow-States/foo` would silently land in the canonical inode
///   without ever invoking the helper.
///
/// The returned canonical path uses `project_root` verbatim and
/// joins a sanitized suffix that drops `..`, `.`, and empty segments.
/// Sanitization keeps the redirect message safe to follow — naming a
/// `..`-containing path as the canonical destination would mislead
/// the caller toward path-traversal usage even though the gate
/// itself blocked the original write.
///
/// Returns `Some(canonical_path)` when `file_path` (after
/// normalization) resolves to a `.flow-states/` segment underneath
/// `<project_root>/.worktrees/<branch>/`. Returns `None` for paths
/// outside `<project_root>/.worktrees/`, paths inside the worktree
/// without a `.flow-states/` segment, and substring matches like
/// `foo-flow-states-bar` (the match requires the literal
/// `/.flow-states/` segment with both slashes).
///
/// Pure string operations — no `Path` construction or filesystem
/// reads. The `file_path` input is `tool_input.file_path` from Claude
/// Code (untrusted model output), so the helper avoids any code path
/// that could surface a path-traversal or filesystem-read sink.
pub fn detect_misplaced_flow_states(file_path: &str, project_root: &str) -> Option<String> {
    let normalized = collapse_double_slashes(file_path);
    let normalized_lower = normalized.to_ascii_lowercase();
    let worktrees_prefix = format!("{}/.worktrees/", project_root.to_ascii_lowercase());
    if !normalized_lower.starts_with(&worktrees_prefix) {
        return None;
    }
    let after_worktrees = &normalized_lower[worktrees_prefix.len()..];
    let branch_end = after_worktrees.find('/')?;
    let after_branch = &after_worktrees[branch_end..];
    let flow_states_idx = after_branch.find("/.flow-states/")?;
    let suffix_start =
        worktrees_prefix.len() + branch_end + flow_states_idx + "/.flow-states/".len();
    let suffix = sanitize_canonical_suffix(&normalized[suffix_start..]);
    Some(format!("{}/.flow-states/{}", project_root, suffix))
}

/// Collapse runs of `/` to a single `/` so doubled-slash bypass shapes
/// (`<root>//.worktrees/...`) match the same segment as the canonical
/// shape. Pure string operation, ASCII-only, no allocation when the
/// input has no doubled slashes (returns a clone via the iterator).
fn collapse_double_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

/// Drop `..`, `.`, and empty segments from the suffix before
/// interpolating it into the canonical redirect message. The gate
/// has already blocked the misplaced write; sanitization here keeps
/// the "use this instead" message safe to follow rather than
/// suggesting a path-traversal-containing destination.
fn sanitize_canonical_suffix(suffix: &str) -> String {
    suffix
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".." && *s != ".")
        .collect::<Vec<&str>>()
        .join("/")
}

/// Validate that `file_path` targets the worktree, not the main repo.
///
/// Returns `(allowed, message)`.
///
/// The project_root and worktree_root come from `compute_worktree_paths`
/// — a single shared computation so this hook and `ci::run_impl` agree
/// on the worktree boundary. The helper handles the rightmost-occurrence
/// invariant (a project path containing `.worktrees/` does not produce a
/// false match) and the empty-branch edge case (cwd ending exactly in
/// `.worktrees/` returns `None` — treat as "not in a worktree" and
/// allow). See `compute_worktree_paths` doc for the full branch table.
pub fn validate(file_path: &str, cwd: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    let (project_root, worktree_root) = match compute_worktree_paths(cwd) {
        Some(pair) => pair,
        None => return (true, String::new()), // not in a worktree
    };

    // Paths outside the project are always fine (~/.claude, /tmp, etc.)
    let prefix = format!("{}/", project_root);
    if !file_path.starts_with(&prefix) {
        return (true, String::new());
    }

    // Reject worktree-internal `.flow-states/` writes BEFORE the
    // cwd-inside check below — otherwise a tool call to
    // `<root>/.worktrees/<branch>/<service>/.flow-states/...`
    // would be silently accepted whenever the cwd is the same
    // service subdirectory. The state directory lives ONLY at
    // `<project_root>/.flow-states/`, never inside a worktree.
    if let Some(canonical) = detect_misplaced_flow_states(file_path, project_root) {
        return (
            false,
            format!(
                "BLOCKED: .flow-states/ lives at the main repo, \
                 not the worktree. Use {} instead of {}",
                canonical, file_path
            ),
        );
    }

    // Paths inside the worktree are fine
    let worktree_prefix = format!("{}/", worktree_root);
    if file_path.starts_with(&worktree_prefix) || file_path == worktree_root {
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
    let corrected = format!("{}/{}", worktree_root, relative);

    // Issue #1704 branch C: autonomous-flow-strict response shape.
    // When the active flow is configured for autonomous execution,
    // a human-readable BLOCKED message would surface to the model
    // as a Claude Code permission prompt mid-flow (defeating the
    // autonomous-mode contract). Emit a structured JSON envelope
    // instead so the model can parse the rejection and react.
    // Default (non-autonomous-flow) behavior unchanged.
    //
    // Residual gap: this branch fires ONLY when the path is inside
    // `project_root` but outside the worktree (the existing block
    // path). Paths outside `project_root` (~/.config, /tmp, etc.)
    // are allowed by validate() earlier and stay outside this
    // hook's jurisdiction — see the `paths outside the project are
    // always fine` branch above and the residual-gap negative test.
    //
    // `validate_claude_paths.rs` is deliberately NOT extended here
    // per the plan's Mirror-Pattern Audit — its fail-closed
    // posture and protected-path scope differ from this hook.
    // `worktree_root` is the canonical `<main_root>/.worktrees/<branch>/`
    // path resolved upstream by `compute_worktree_root`. The basename
    // (`file_name`) of that path is the branch — derived structurally
    // from the worktree layout, not from a state-derived string. A
    // detached HEAD or invalid branch makes `worktree_root` empty,
    // in which case `is_autonomous_flow_active` receives `None` and
    // returns false (fail-open).
    let branch = Path::new(worktree_root)
        .file_name()
        .and_then(|n| n.to_str());
    if crate::flow_paths::is_autonomous_flow_active(Path::new(project_root), branch) {
        let envelope = serde_json::json!({
            "status": "error",
            "reason": "out_of_worktree_in_autonomous",
            "blocked_path": file_path,
            "worktree": worktree_root,
            "autonomous": true,
        });
        return (false, envelope.to_string());
    }

    (
        false,
        format!(
            "BLOCKED: You are in worktree {}. Use {} instead of {}",
            worktree_root, corrected, file_path
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
