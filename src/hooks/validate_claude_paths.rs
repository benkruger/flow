//! PreToolUse hook that blocks Edit/Write/Read/Glob/Grep on:
//!
//! 1. `.claude/rules/`, `.claude/skills/`, and `CLAUDE.md` — only Edit
//!    and Write, only during active FLOW phases. Redirects to
//!    `bin/flow write-rule`. Read/Glob/Grep of these paths is preserved
//!    so the model can read and search rule and skill files during a
//!    flow.
//! 2. `~/.claude/projects/` (the Claude Code persisted transcript
//!    root) — Edit, Write, Read, Glob, AND Grep, in ALL contexts (not
//!    just active flows). The matcher walks the path components and
//!    fires whenever any segment matches `.claude` followed by
//!    `projects` (case-insensitive). The auto-memory subdirectory
//!    (`~/.claude/projects/<project-id>/memory/...`) is CARVED OUT so
//!    Read of the user's MEMORY.md continues to work — the
//!    transcript-tampering threat model targets the per-session JSONL
//!    files at the second-level project-id directory, not the memory
//!    subtree which the UNIVERSAL_ALLOW `Read(~/.claude/projects/*/memory/*)`
//!    entry preserves. Transcript tampering could subvert
//!    `validate-skill`'s user-only block by injecting a fake user
//!    `<command-name>` line; a model Read/Glob/Grep of the transcript
//!    root also sits outside the project root and would surface a
//!    permission prompt mid-flow. The internal walkers in
//!    `validate-skill` and `validate-ask-user` use `fs::read_to_string`
//!    from inside Rust subcommands rather than the Read tool, so
//!    blocking Read at the tool layer does not affect them.
//!
//!    The block message leads with a redirect to
//!    `bin/flow write-rule --path .claude/rules/<topic>.md` so a
//!    behavioral constraint the model wanted to persist as memory has
//!    a concrete path to land as a project rule instead. The message
//!    points at `.claude/rules/persistence-routing.md` as the routing
//!    decision tree.
//!
//! Fires on Edit, Write, Read, Glob, and Grep tool calls.
//!
//! Exit 0 — allow (path is not protected, or no FLOW phase active and
//!          path is not in the always-protected transcript root, or
//!          the tool is Read/Glob/Grep and the path is a non-transcript
//!          protected path, or the path is under the auto-memory
//!          subdirectory carve-out)
//! Exit 2 — block

use std::path::Path;

use super::{detect_branch_from_path, is_flow_active, read_hook_input, resolve_main_root};
use crate::flow_paths::FlowStatesDir;
use crate::protected_paths::is_protected_path;

/// Returns `true` when `file_path` passes through a `.claude/projects/`
/// directory at any depth, EXCEPT when the path enters the auto-memory
/// subdirectory (`.claude/projects/<project-id>/memory/...`). The Claude
/// Code harness persists session transcripts under
/// `<home>/.claude/projects/<project_id>/<session>.jsonl`; Edit, Write,
/// Read, Glob, and Grep on that family of paths is a tampering vector
/// for `validate-skill`'s user-only-skill block AND surfaces a
/// permission prompt because the path sits outside the project root.
/// Blocked across all contexts (not just active flows).
///
/// The auto-memory carve-out preserves Read access to the user's
/// MEMORY.md, which lives at
/// `~/.claude/projects/<project-id>/memory/MEMORY.md`. The
/// UNIVERSAL_ALLOW pattern `Read(~/.claude/projects/*/memory/*)`
/// documents this carve-out at the settings layer; the hook honors the
/// same boundary by checking the third path component for `memory`
/// (case-insensitive) and returning `false` so legitimate memory reads
/// pass through.
///
/// Matching is ASCII-case-insensitive for `.claude`, `projects`, and
/// `memory` so a caller on a case-insensitive filesystem (macOS
/// APFS/HFS+ by default) cannot bypass the gate by writing to
/// `.CLAUDE/Projects/...` — matches the same discipline used by
/// `is_protected_path`.
fn is_transcript_path(file_path: &str) -> bool {
    let path = Path::new(file_path);
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    for (i, comp) in components.iter().enumerate() {
        if comp.eq_ignore_ascii_case(".claude") && i + 1 < components.len() {
            let next = components[i + 1];
            if next.eq_ignore_ascii_case("projects") {
                // Carve out `.claude/projects/<project-id>/memory/...`:
                // the auto-memory subdirectory is preserved per the
                // UNIVERSAL_ALLOW Read(~/.claude/projects/*/memory/*)
                // entry. Memory files are not transcript content; the
                // tampering threat targets the per-session JSONL at
                // the second-level project-id directory.
                if i + 3 < components.len() {
                    let memory_candidate = components[i + 3];
                    if memory_candidate.eq_ignore_ascii_case("memory") {
                        return false;
                    }
                }
                return true;
            }
        }
    }
    false
}

/// Extract the target file path from `tool_input`. Edit, Write, and
/// Read use `file_path`; Glob uses `pattern` (with optional `path`);
/// Grep uses `path`. The helper checks all three keys in priority
/// order so the hook sees the same target the model addressed,
/// regardless of which tool emitted the call.
///
/// Mirrors `validate_worktree_paths::get_file_path` with `pattern`
/// added for Glob — `.claude/rules/security-gates.md` "Enumerate
/// Bypass Variants Before Coding" requires the gate to cover every
/// path field the matcher family carries.
fn get_file_path(tool_input: &serde_json::Value) -> String {
    if let Some(fp) = tool_input.get("file_path").and_then(|v| v.as_str()) {
        return fp.to_string();
    }
    if let Some(p) = tool_input.get("path").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    if let Some(pat) = tool_input.get("pattern").and_then(|v| v.as_str()) {
        return pat.to_string();
    }
    String::new()
}

/// Normalize `tool_name` for gate comparison: strip NULs, trim
/// whitespace, lowercase with ASCII semantics. Mirrors
/// `.claude/rules/security-gates.md` "Normalize Before Comparing" —
/// gate inputs must be robust to case variants and whitespace padding
/// that the harness or future Claude Code releases could emit.
fn normalize_tool_name(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Validate that an Edit/Write/Read/Glob/Grep on this path is allowed.
///
/// `tool_name` is the literal name from the hook input (`"Edit"`,
/// `"Write"`, `"Read"`, `"Glob"`, `"Grep"`, …). Comparison is
/// normalized (NUL-stripped, whitespace-trimmed, ASCII-lowercased) per
/// `.claude/rules/security-gates.md` "Normalize Before Comparing" so
/// case variants and trailing whitespace cannot bypass or falsely
/// trip the gate.
///
/// Transcript-root paths (Layer 2 above) block for any tool, including
/// Read/Glob/Grep — tampering, content enumeration, and permission-
/// prompt surface are all prevented. Protected paths (Layer 1:
/// `.claude/rules/`, `.claude/skills/`, `CLAUDE.md`) block only for
/// mutating tools (Edit, Write); Read/Glob/Grep of those paths is
/// preserved so the model can read and search rule and skill files
/// during a flow. An unrecognized tool_name (empty, future tool name)
/// falls into the Edit/Write block class — fail-closed so a new
/// mutating tool surface added by Claude Code doesn't silently bypass
/// the protected-path gate.
///
/// Returns `(allowed, message)`.
pub fn validate(file_path: &str, flow_active: bool, tool_name: &str) -> (bool, String) {
    if file_path.is_empty() {
        return (true, String::new());
    }

    // Transcript paths blocked regardless of flow_active and tool_name.
    // Tampering with the persisted transcript can subvert
    // validate-skill's user-only block by injecting a fake user
    // `<command-name>` line; a model Read/Glob/Grep of the transcript
    // root also sits outside the project root and would surface a
    // permission prompt mid-flow. The block must fire even pre-flow
    // / post-flow and for any tool that can address the path.
    if is_transcript_path(file_path) {
        return (
            false,
            "BLOCKED: `~/.claude/projects/` is the Claude Code persisted \
             transcript root. Edit, Write, Read, Glob, and Grep are all \
             forbidden here. The auto-memory subdirectory \
             (`~/.claude/projects/<project-id>/memory/...`) IS allowed \
             — only the transcript JSONL files at the project-id level \
             are blocked.\n\n\
             To capture a behavioral constraint that every engineer \
             should follow, write a project rule: \
             `${CLAUDE_PLUGIN_ROOT}/bin/flow write-rule \
             --path .claude/rules/<topic>.md --content-file <temp>`.\n\n\
             To capture a user-specific preference, ask the user to add \
             it to `~/.claude/CLAUDE.md` manually — there is no in-FLOW \
             path for memory writes by design.\n\n\
             For post-compaction context recovery, read \
             `compact_summary` from \
             `.flow-states/<branch>/state.json` instead of the raw \
             transcript JSONL — see \
             `.claude/rules/post-compaction-recovery.md`.\n\n\
             Routing question? See \
             `.claude/rules/persistence-routing.md` (Rules are the \
             default; Memory is the exception).\n\n\
             The internal transcript walkers in validate-skill and \
             validate-ask-user use fs::read_to_string from Rust \
             subprocesses, not the Read tool, so blocking the Read tool \
             at this layer does not affect them. Edit, Write, Read, \
             Glob, and Grep are blocked across all contexts (not just \
             active flows) because tampering with — or model-tool \
             reading of — the transcript can subvert validate-skill's \
             user-only skill block or surface a permission prompt \
             mid-flow."
                .to_string(),
        );
    }

    if !flow_active {
        return (true, String::new());
    }

    // Protected-path block (Layer 1) applies only to mutating tools.
    // Read/Glob/Grep on `.claude/rules/<x>.md`,
    // `.claude/skills/<x>/SKILL.md`, and `CLAUDE.md` is preserved so the
    // model can read and search rule and skill files during an active
    // flow — `system-reminder` injections cover the auto-load case, but
    // explicit Read/Glob/Grep calls remain valid. An unrecognized
    // tool_name falls into the mutating branch (fail-closed) so a new
    // mutating tool surface added by Claude Code doesn't silently
    // bypass the gate.
    let normalized = normalize_tool_name(tool_name);
    if normalized == "read" || normalized == "glob" || normalized == "grep" {
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

    let file_path = get_file_path(&tool_input);
    if file_path.is_empty() {
        return (0, None);
    }

    let tool_name = hook_input
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

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

    let (allowed, message) = validate(&file_path, flow_active, tool_name);
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
