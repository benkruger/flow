//! Shared classifier for "protected" repo paths that mid-flow subprocess
//! gates must guard against.
//!
//! `is_protected_path` matches the same set as the
//! `validate-claude-paths` PreToolUse hook: `CLAUDE.md` (any directory
//! level) and any path passing through a `.claude/rules/` or
//! `.claude/skills/` directory component. `.claude/settings.json` and
//! `.claude/settings.local.json` are intentionally NOT protected — those
//! are user-owned config that prime mutates via `merge_settings`.
//!
//! Two consumers share the classifier:
//!
//! 1. `src/hooks/validate_claude_paths.rs` — blocks Edit/Write tool calls
//!    on protected paths during an active FLOW phase, redirecting the
//!    model to `bin/flow write-rule`.
//! 2. `src/write_rule.rs::run_impl_main` — blocks the same paths at the
//!    subprocess layer when `--path` resolves to the main repo while a
//!    flow is active, so a model that calls write-rule directly cannot
//!    bypass the worktree-only invariant.
//!
//! Drift contract: when the hook adds a new protected basename, this
//! helper must follow in the same commit so the subprocess gate stays
//! aligned. Tests live at tests/protected_paths.rs per
//! .claude/rules/test-placement.md — no inline #[cfg(test)] in this file.
//!
//! Matching is ASCII-case-insensitive for `.claude`, `rules`, `skills`,
//! and `CLAUDE.md` so a caller on a case-insensitive filesystem
//! (macOS APFS/HFS+ by default) cannot bypass the gate by writing to
//! `.CLAUDE/rules/foo.md` or `Claude.md` — which resolve to the same
//! inode as the protected names.

use std::path::Path;

/// Return true when `path` targets a protected `.claude/` location.
///
/// Protected: `.claude/rules/` (any depth), `.claude/skills/` (any
/// depth), and any file whose basename normalizes to `CLAUDE.md`.
/// Not protected: `.claude/settings.json`, `.claude/settings.local.json`,
/// or any path outside those families.
///
/// Empty paths return `false` so callers passing an empty `--path` or
/// missing `file_path` field are not falsely classified as protected.
pub fn is_protected_path(path: &Path) -> bool {
    let components: Vec<&str> = path
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();

    if components.is_empty() {
        return false;
    }

    // Check for .claude/rules/ or .claude/skills/ at any depth.
    for (i, comp) in components.iter().enumerate() {
        if comp.eq_ignore_ascii_case(".claude") && i + 1 < components.len() {
            let next = components[i + 1];
            if next.eq_ignore_ascii_case("rules") || next.eq_ignore_ascii_case("skills") {
                return true;
            }
        }
    }

    // Check for CLAUDE.md at any level. The empty-components early-return
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
