//! `bin/flow plugin-bin-flow` — resolve and print the absolute plugin
//! `bin/flow` path for substitution into FLOW sub-agent commands.
//!
//! The plugin-root prefix on `bin/flow` is correct inside a parent
//! SKILL bash fence (Claude Code expands the plugin-root env var there)
//! but wrong inside a sub-agent prompt: the child runs the command with
//! no expansion and the literal unexpanded token, and the resolved
//! absolute path — being outside the worktree — would otherwise trip
//! the parent-side `agent_prompt_scan` gate. The parent calls this
//! subcommand, captures the absolute path, and substitutes it into the
//! adversarial / ci-fixer agent prompt, so the sub-agent runs a plain
//! absolute `…/bin/flow …`.
//!
//! The resolved path is `<CLAUDE_PLUGIN_ROOT>/bin/flow`, reading the
//! same `CLAUDE_PLUGIN_ROOT` env value the `agent_prompt_scan`
//! plugin-root carve-out reads — one source for the path the gate then
//! allows. On an unset, empty, or non-absolute `CLAUDE_PLUGIN_ROOT` the
//! subcommand returns a non-zero structured error (never a path, never
//! a panic) so every consumer halts and surfaces the error rather than
//! embedding a non-path string into an agent prompt.
//!
//! Tests live at `tests/plugin_bin_flow.rs` and drive the binary
//! through `CARGO_BIN_EXE_flow-rs`.

use std::path::Path;

/// Resolve the absolute plugin `bin/flow` path from a
/// `CLAUDE_PLUGIN_ROOT` value. Returns `Ok((path, 0))` — the path is
/// `<root>/bin/flow` — when the value is present, non-empty, and
/// absolute. Returns `Err((msg, 1))` when it is unset/empty (the
/// `None`/empty arm) or non-absolute.
///
/// Applies the same NUL-strip + surrounding-whitespace-trim hygiene the
/// `agent_prompt_scan` plugin-root carve-out applies, so the path this
/// resolver emits and the prefix that carve-out admits derive
/// identically from the same `CLAUDE_PLUGIN_ROOT` value — the
/// one-source contract holds even for a hygiene-affected env value (a
/// trailing-newline or NUL-padded root would otherwise produce a path
/// the trimmed/NUL-stripped carve-out could never admit).
///
/// Pure over its input so every branch is reachable without mutating
/// process env; `run_impl_main` reads the env and delegates here.
pub fn run_impl(claude_plugin_root: Option<&str>) -> Result<(String, i32), (String, i32)> {
    let cleaned = claude_plugin_root.unwrap_or("").replace('\0', "");
    let root = cleaned.trim();
    if root.is_empty() {
        return Err((
            "CLAUDE_PLUGIN_ROOT is unset or empty; cannot resolve the plugin bin/flow path"
                .to_string(),
            1,
        ));
    }
    if !Path::new(root).is_absolute() {
        return Err((
            format!(
                "CLAUDE_PLUGIN_ROOT `{}` is not an absolute path; cannot resolve the plugin bin/flow path",
                root
            ),
            1,
        ));
    }
    let path = Path::new(root).join("bin").join("flow");
    Ok((path.to_string_lossy().into_owned(), 0))
}

/// Main-arm dispatcher reading `CLAUDE_PLUGIN_ROOT` from the process
/// env. `dispatch::dispatch_text` appends the trailing newline on the
/// `Ok` path; the `Err` message lands on stderr with exit 1.
pub fn run_impl_main() -> Result<(String, i32), (String, i32)> {
    run_impl(std::env::var("CLAUDE_PLUGIN_ROOT").ok().as_deref())
}
