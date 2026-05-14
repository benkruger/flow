//! `bin/flow resolve-skill-mode` — the single tested source of truth
//! for resolving the autonomy mode of the two terminal skills
//! `flow-complete` and `flow-abort`.
//!
//! Both terminal skills' `## Mode Resolution` sections call this
//! subcommand as the single place that reads `skills.<name>` from the
//! state file. Given `--skill {flow-complete|flow-abort}` and an
//! optional `--branch` override, it tolerates every config shape a
//! real `.flow.json`-seeded state file can carry (bare string,
//! object with a `continue` axis, missing/null/wrong-type entry),
//! normalizes the resolved value, clamps it to the `{auto, manual}`
//! set, and returns a deterministic
//! `{"status":"ok","mode":"manual"|"auto"}`.
//!
//! The fallback for both skills is `manual` — the conservative
//! direction matching prime's Recommended preset intent and the
//! per-phase defaults already encoded in
//! `crate::phase_enter::resolve_mode`.
//!
//! Read-only: no `cwd_scope::enforce` call. Per
//! `.claude/rules/external-input-validation.md` and
//! `.claude/rules/branch-path-safety.md`, the `--branch` override is
//! untrusted shell input and routes through `FlowPaths::try_new` so a
//! slash-containing, empty, or traversal branch surfaces as a
//! structured error rather than a panic. Per
//! `.claude/rules/security-gates.md`, both `--skill` and the resolved
//! `skills.<name>` value are normalized (NUL-stripped, trimmed,
//! ASCII-lowercased via `normalize_gate_input`) and checked against a
//! positive allowlist — `--skill` against [`ALLOWED_SKILLS`], the
//! resolved mode against `MODE_VALUES`.
//!
//! `run_impl` returns `Value` unconditionally — every failure mode is
//! a structured `{"status":"error",...}` payload or a fallback, so
//! there is no infrastructure-failure `Err` path and the paired
//! `run_impl_main` wraps as `(value, 0)` per the "Exit code
//! convention for business errors" in `.claude/rules/rust-patterns.md`.
//!
//! Tests live at `tests/resolve_skill_mode.rs`.

use std::fs;
use std::path::Path;

use clap::Parser;
use serde_json::{json, Value};

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;

/// CLI args for `bin/flow resolve-skill-mode`.
#[derive(Parser, Debug)]
#[command(
    name = "resolve-skill-mode",
    about = "Resolve the configured autonomy mode of a terminal skill"
)]
pub struct Args {
    /// Skill whose mode to resolve — `flow-complete` or `flow-abort`.
    #[arg(long)]
    pub skill: String,

    /// Override branch for state file lookup.
    #[arg(long)]
    pub branch: Option<String>,
}

/// The terminal skills `resolve-skill-mode` answers for. A positive
/// allowlist — anything else is rejected with a structured error so a
/// future skill name added to the domain cannot silently pass the
/// gate.
pub const ALLOWED_SKILLS: &[&str] = &["flow-complete", "flow-abort"];

/// Conservative fallback mode used whenever the config is missing,
/// empty, the wrong type, or otherwise unparseable. `manual` is the
/// safe direction: it asks the user before the destructive /
/// environment-mutating action the terminal skills perform.
pub const FALLBACK_MODE: &str = "manual";

/// Normalize a gate input before an allowlist comparison: strip NUL
/// bytes, trim surrounding whitespace, lowercase with ASCII
/// semantics. Per `.claude/rules/security-gates.md` "Normalize Before
/// Comparing". Shared by both gates in this module: `--skill` against
/// [`ALLOWED_SKILLS`], and the resolved `skills.<name>` value against
/// `MODE_VALUES`. The allowlist entries are already lowercase and
/// trimmed, so normalization runs on the caller side only.
pub fn normalize_gate_input(s: &str) -> String {
    s.replace('\0', "").trim().to_ascii_lowercase()
}

/// Valid resolved modes. [`resolve`] normalizes the `skills.<skill>`
/// config value and clamps anything outside this set to
/// [`FALLBACK_MODE`], so callers can rely on the result being exactly
/// `"auto"` or `"manual"`.
const MODE_VALUES: &[&str] = &["auto", "manual"];

/// Resolve the continue-mode for `skill` from a parsed state file
/// value.
///
/// Extracts a raw value from every `skills.<skill>` shape a real
/// `.flow.json`-seeded state file can carry:
///
/// - bare string (`"auto"`) → that string
/// - object (`{"continue": "auto"}` or
///   `{"commit": .., "continue": ..}`) → the `continue` axis value
/// - missing `skills` key, non-object root, missing entry,
///   `null`/number/array/bool entry, object with no `continue` (or a
///   non-string `continue`) → the empty string
///
/// The raw value is then normalized via [`normalize_gate_input`]
/// (NUL-strip, trim, ASCII-lowercase) and checked against the
/// positive `MODE_VALUES` allowlist. Anything outside the set — the
/// empty string, a typo like `"xyzzy"`, or a value that does not
/// normalize to a member — resolves to [`FALLBACK_MODE`]. The
/// returned value is therefore always exactly `"auto"` or
/// `"manual"`, matching the documented `mode` contract.
pub fn resolve(state: &Value, skill: &str) -> String {
    let raw = match state.get("skills").and_then(|s| s.get(skill)) {
        Some(entry) => {
            if let Some(s) = entry.as_str() {
                s
            } else if let Some(obj) = entry.as_object() {
                obj.get("continue").and_then(|c| c.as_str()).unwrap_or("")
            } else {
                ""
            }
        }
        None => "",
    };
    let normalized = normalize_gate_input(raw);
    if MODE_VALUES.contains(&normalized.as_str()) {
        normalized
    } else {
        FALLBACK_MODE.to_string()
    }
}

/// Resolve the autonomy mode for `args.skill` and return a structured
/// JSON payload.
///
/// Outcomes:
/// - `--skill` outside [`ALLOWED_SKILLS`] →
///   `{"status":"error","reason":"invalid_skill",...}`
/// - `--branch` (or the resolved current branch) fails
///   `FlowPaths::try_new` →
///   `{"status":"error","reason":"invalid_branch",...}`
/// - no current branch and no override (detached HEAD / non-git cwd)
///   → `{"status":"ok","mode":"manual"}` — no active flow, safe
///   default
/// - state file missing / empty / non-JSON / non-object root →
///   `{"status":"ok","mode":"manual"}`
/// - state file parses → `{"status":"ok","mode":<resolved>}` via
///   [`resolve`]
pub fn run_impl(args: &Args, root: &Path) -> Value {
    let skill = normalize_gate_input(&args.skill);
    if !ALLOWED_SKILLS.contains(&skill.as_str()) {
        return json!({
            "status": "error",
            "reason": "invalid_skill",
            "message": format!(
                "--skill must be one of {:?}, got {:?}",
                ALLOWED_SKILLS, args.skill
            ),
        });
    }
    let branch = match resolve_branch(args.branch.as_deref(), root) {
        Some(b) => b,
        None => return json!({"status": "ok", "mode": FALLBACK_MODE}),
    };
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => {
            return json!({
                "status": "error",
                "reason": "invalid_branch",
                "message": format!(
                    "invalid branch {:?}: must be non-empty and contain no '/' or NUL",
                    branch
                ),
            });
        }
    };
    let mode = match fs::read_to_string(paths.state_file()) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(state) => resolve(&state, &skill),
            Err(_) => FALLBACK_MODE.to_string(),
        },
        Err(_) => FALLBACK_MODE.to_string(),
    };
    json!({"status": "ok", "mode": mode})
}

/// Main-arm dispatcher. `resolve-skill-mode` has no
/// infrastructure-failure path — every outcome is a structured JSON
/// payload — so the exit code is always `0` per the "Exit code
/// convention for business errors" in `.claude/rules/rust-patterns.md`.
/// Callers parse the `status` field to distinguish success from error.
pub fn run_impl_main(args: &Args, root: &Path) -> (Value, i32) {
    (run_impl(args, root), 0)
}
