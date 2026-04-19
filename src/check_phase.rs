//! Phase-entry gate logic and `bin/flow check-phase` CLI driver.
//!
//! `check_phase()` is the pure predicate: given a state JSON value
//! and a target phase, return `(allowed, message)`. `run_impl_main()`
//! is the thin CLI driver that loads the state file from disk and
//! routes the predicate's output through the plain-text contract that
//! `bin/flow check-phase` consumers (the `validate-claude-paths` hook
//! and other phase-entry gates) parse from stdout. The driver is the
//! `main.rs` `Commands::CheckPhase` arm's only behaviour — main.rs
//! delegates here and prints the returned string via
//! `dispatch::dispatch_text`.

use std::path::Path;

use indexmap::IndexMap;
use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::output::json_error_string;
use crate::phase_config::{self, load_phase_config, PhaseConfig, PHASE_ORDER};

/// Check if entry into `phase` is allowed given the state JSON.
///
/// Returns `Ok((allowed, message))` where message may be empty if allowed
/// with no note. Returns `Err` if the phase name is invalid.
pub fn check_phase(
    state: &Value,
    phase: &str,
    phase_config: Option<&PhaseConfig>,
) -> Result<(bool, String), String> {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names: IndexMap<String, String> = phase_config::phase_names();
    let default_numbers: IndexMap<String, usize> = phase_config::phase_numbers();
    let default_commands: IndexMap<String, String> = phase_config::commands();

    #[allow(clippy::type_complexity)]
    let (order, names, numbers, commands): (
        &Vec<String>,
        &IndexMap<String, String>,
        &IndexMap<String, usize>,
        &IndexMap<String, String>,
    ) = match phase_config {
        Some(cfg) => (&cfg.order, &cfg.names, &cfg.numbers, &cfg.commands),
        None => (
            &default_order,
            &default_names,
            &default_numbers,
            &default_commands,
        ),
    };

    let phase_idx = match order.iter().position(|p| p == phase) {
        Some(idx) => idx,
        None => {
            return Err(format!(
                "Invalid phase: {}. Must be one of: {}",
                phase,
                order.join(", ")
            ));
        }
    };

    // First phase has no prerequisites
    if phase_idx == 0 {
        return Ok((true, String::new()));
    }

    let prev = &order[phase_idx - 1];
    let phases = state.get("phases").and_then(|v| v.as_object());

    let prev_data = phases.and_then(|p| p.get(prev.as_str()));
    let prev_status = prev_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    let prev_name = match names.get(prev.as_str()) {
        Some(n) => n.clone(),
        None => prev.clone(),
    };
    let prev_num = numbers.get(prev.as_str()).copied().unwrap_or(0);
    let prev_cmd = match commands.get(prev.as_str()) {
        Some(c) => c.clone(),
        None => format!("/flow:{}", prev),
    };

    let phase_name = match names.get(phase) {
        Some(n) => n.clone(),
        None => phase.to_string(),
    };
    let phase_num = numbers.get(phase).copied().unwrap_or(0);

    if prev_status != "complete" {
        let msg = format!(
            "BLOCKED: Phase {}: {} must be complete before entering Phase {}: {}.\n\
             Phase {} current status: {}\n\
             Complete it first with: {}",
            prev_num, prev_name, phase_num, phase_name, prev_num, prev_status, prev_cmd
        );
        return Ok((false, msg));
    }

    // Allowed — check if revisiting
    let this_data = phases.and_then(|p| p.get(phase));
    let this_status = this_data
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    if this_status == "complete" {
        let visits = match this_data.and_then(|d| d.get("visit_count")) {
            Some(v) => v.as_i64().unwrap_or(0),
            None => 0,
        };
        let msg = format!(
            "NOTE: Phase {}: {} was previously completed ({} visit(s)). Re-entering.",
            phase_num, phase_name, visits
        );
        return Ok((true, msg));
    }

    Ok((true, String::new()))
}

/// Driver for the `bin/flow check-phase` subcommand.
///
/// Returns `(output, exit_code)`. The output is plain text for the
/// BLOCKED/NOTE/allowed paths and a JSON error object for
/// branch-resolution, file-read, and parse errors — mirroring the
/// mixed output contract of the pre-extraction inline dispatch. The
/// first-phase short-circuit returns `("", 0)`.
///
/// Tests supply `root` as a fixture TempDir containing
/// `.flow-states/<branch>.json`; `branch_override` is required so the
/// helper does not shell out to `git rev-parse` against the host
/// worktree.
pub fn run_impl_main(phase: &str, branch_override: Option<&str>, root: &Path) -> (String, i32) {
    run_impl_main_with_resolver(phase, branch_override, root, &resolve_branch)
}

/// Seam-injected variant of [`run_impl_main`] that accepts a custom
/// branch resolver closure. Production passes `resolve_branch`; tests
/// pass a closure that returns `None` to exercise the
/// "could not determine current git branch" arm.
pub fn run_impl_main_with_resolver(
    phase: &str,
    branch_override: Option<&str>,
    root: &Path,
    resolver: &dyn Fn(Option<&str>, &Path) -> Option<String>,
) -> (String, i32) {
    // First phase has no prerequisites — short-circuit before touching
    // the filesystem or resolving a branch.
    if phase == PHASE_ORDER[0] {
        return (String::new(), 0);
    }

    let branch = match resolver(branch_override, root) {
        Some(b) => b,
        None => {
            return (
                "BLOCKED: Could not determine current git branch.".to_string(),
                1,
            );
        }
    };

    // `resolve_branch` may return a raw git ref (slash-containing,
    // empty) when no state file matches. `FlowPaths::new` panics on
    // those; use `try_new` per `.claude/rules/external-input-validation.md`
    // and treat invalid branches as "no active flow" just like the
    // missing-state-file case below.
    let paths = match FlowPaths::try_new(root, &branch) {
        Some(p) => p,
        None => {
            return (
                format!(
                    "BLOCKED: No FLOW feature in progress on branch \"{}\".\nRun /flow:flow-start to begin a new feature.",
                    branch
                ),
                1,
            );
        }
    };
    let state_file = paths.state_file();
    if !state_file.exists() {
        return (
            format!(
                "BLOCKED: No FLOW feature in progress on branch \"{}\".\nRun /flow:flow-start to begin a new feature.",
                branch
            ),
            1,
        );
    }

    let content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(e) => {
            return (format!("BLOCKED: Could not read state file: {}", e), 1);
        }
    };

    let state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (format!("BLOCKED: Could not read state file: {}", e), 1);
        }
    };

    let frozen_path = paths.frozen_phases();
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    match check_phase(&state, phase, frozen_config.as_ref()) {
        Ok((allowed, output)) => (output, if allowed { 0 } else { 1 }),
        Err(msg) => (json_error_string(&msg, &[]), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase_config::{self, PHASE_ORDER};
    use indexmap::IndexMap;
    use serde_json::json;

    /// Build a minimal state Value matching conftest.py make_state().
    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let phase_names = phase_config::phase_names();
        let mut phases = serde_json::Map::new();
        for &p in PHASE_ORDER {
            let status = phase_statuses
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| *v)
                .unwrap_or("pending");
            let visit_count = if status == "complete" || status == "in_progress" {
                1
            } else {
                0
            };
            phases.insert(
                p.to_string(),
                json!({
                    "name": phase_names.get(p).unwrap_or(&String::new()),
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": if status == "in_progress" { json!("2026-01-01T00:00:00Z") } else { json!(null) },
                    "cumulative_seconds": 0,
                    "visit_count": visit_count,
                }),
            );
        }
        json!({
            "branch": "test-feature",
            "current_phase": current_phase,
            "phases": phases,
        })
    }

    // --- Phase status checks ---

    #[test]
    fn previous_phase_pending_blocks() {
        let state = make_state("flow-plan", &[("flow-start", "pending")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
        assert!(output.contains("pending"));
    }

    #[test]
    fn previous_phase_in_progress_blocks() {
        let state = make_state("flow-plan", &[("flow-start", "in_progress")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
        assert!(output.contains("in_progress"));
    }

    #[test]
    fn previous_phase_complete_allows() {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        let (allowed, _output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
    }

    #[test]
    fn sequential_chain_phase_4_with_1_to_3_complete() {
        let state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
            ],
        );
        let (allowed, _) = check_phase(&state, "flow-code-review", None).unwrap();
        assert!(allowed);
    }

    // --- Re-entry ---

    #[test]
    fn re_entering_completed_phase_shows_note() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        state["phases"]["flow-plan"]["visit_count"] = json!(2);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
        assert!(output.contains("previously completed"));
        assert!(output.contains("2 visit(s)"));
    }

    /// Exercises line 114 (`None => 0`) — re-entering a completed phase
    /// whose state lacks the `visit_count` field, e.g., a state file
    /// from before the visit-counter feature shipped.
    #[test]
    fn re_entering_completed_phase_without_visit_count_reports_zero() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        // Strip the visit_count field so the production match falls
        // through to the None arm.
        state["phases"]["flow-plan"]
            .as_object_mut()
            .unwrap()
            .remove("visit_count");
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
        assert!(output.contains("previously completed"));
        assert!(output.contains("0 visit(s)"));
    }

    #[test]
    fn first_visit_no_previously_completed_message() {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(allowed);
        assert!(!output.contains("previously completed"));
    }

    #[test]
    fn phase_5_requires_phase_4_complete() {
        let state = make_state(
            "flow-learn",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-learn", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("Phase 4"));
    }

    #[test]
    fn phase_6_requires_phase_5_complete() {
        let state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-complete", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("Phase 5"));
    }

    #[test]
    fn missing_phases_key_blocks() {
        let state = json!({"branch": "test", "current_phase": "flow-plan"});
        let (allowed, output) = check_phase(&state, "flow-plan", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
    }

    #[test]
    fn blocked_message_includes_correct_command() {
        let state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "pending"),
            ],
        );
        let (allowed, output) = check_phase(&state, "flow-code-review", None).unwrap();
        assert!(!allowed);
        assert!(output.contains("/flow:flow-code"));
    }

    #[test]
    fn invalid_phase_name_errors() {
        let state = make_state("flow-start", &[("flow-start", "complete")]);
        let result = check_phase(&state, "nonexistent", None);
        assert!(result.is_err());
    }

    // --- Frozen config ---

    #[test]
    fn check_phase_uses_frozen_config() {
        let config = PhaseConfig {
            order: vec![
                "flow-start".into(),
                "flow-plan".into(),
                "flow-code-review".into(),
            ],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Start".into());
                m.insert("flow-plan".into(), "Plan".into());
                m.insert("flow-code-review".into(), "Review".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-plan".into(), 2);
                m.insert("flow-code-review".into(), 3);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:a".into());
                m.insert("flow-plan".into(), "/t:b".into());
                m.insert("flow-code-review".into(), "/t:c".into());
                m
            },
        };
        let state = make_state(
            "flow-code-review",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        let (allowed, _) = check_phase(&state, "flow-code-review", Some(&config)).unwrap();
        assert!(allowed);
    }

    #[test]
    fn check_phase_frozen_config_uses_correct_predecessor() {
        // Custom order: start, code, plan — so plan's predecessor is code
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-code".into(), "flow-plan".into()],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Start".into());
                m.insert("flow-code".into(), "Code".into());
                m.insert("flow-plan".into(), "Plan".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-code".into(), 2);
                m.insert("flow-plan".into(), 3);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:a".into());
                m.insert("flow-code".into(), "/t:b".into());
                m.insert("flow-plan".into(), "/t:c".into());
                m
            },
        };
        // In default order, plan's predecessor is start (complete) → allowed
        // In custom order, plan's predecessor is code (pending) → blocked
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-code", "pending")],
        );
        let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
        assert!(!allowed);
        assert!(output.contains("BLOCKED"));
    }

    // --- First-phase fast path ---

    #[test]
    fn first_phase_has_no_prerequisites() {
        // Targeting flow-start (phase_idx == 0) returns Ok((true, "")) via
        // the early-return at `if phase_idx == 0`, short-circuiting the
        // rest of the function.
        let state = make_state("flow-start", &[]);
        let (allowed, output) = check_phase(&state, "flow-start", None).unwrap();
        assert!(allowed);
        assert!(output.is_empty());
    }

    // --- Frozen-config lookup misses ---

    #[test]
    fn missing_prev_name_falls_back_to_key() {
        // `names` is missing an entry for "flow-start" → the
        // `.unwrap_or_else(|| prev.clone())` closure runs.
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-plan".into()],
            names: IndexMap::new(),
            numbers: IndexMap::new(),
            commands: IndexMap::new(),
        };
        let state = make_state("flow-plan", &[("flow-start", "pending")]);
        let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
        assert!(!allowed);
        // With no names registered, the error message falls back to the
        // raw phase keys and command defaults like `/flow:flow-start`.
        assert!(output.contains("flow-start"));
        assert!(output.contains("/flow:flow-start"));
    }

    // --- run_impl_main (main.rs CheckPhase arm driver) ---

    fn write_state(root: &std::path::Path, branch: &str, state: Value) {
        let dir = root.join(".flow-states");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.json", branch));
        std::fs::write(&path, state.to_string()).unwrap();
    }

    #[test]
    fn run_impl_main_first_phase_returns_empty_and_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let (out, code) = run_impl_main(PHASE_ORDER[0], Some("any"), dir.path());
        assert_eq!(code, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn run_impl_main_no_state_file_returns_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
        assert_eq!(code, 1);
        assert!(out.contains("BLOCKED"));
        assert!(out.contains("No FLOW feature in progress"));
        assert!(out.contains("test"));
    }

    /// Exercises line 212 — `load_phase_config(&frozen_path).ok()` runs
    /// when a frozen-phases file is present alongside the state file.
    /// Without this test the load-frozen branch in `run_impl_main` is
    /// never exercised — sibling tests skip the frozen file.
    #[test]
    fn run_impl_main_loads_frozen_phase_config_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let states = root.join(".flow-states");
        std::fs::create_dir_all(&states).unwrap();
        let branch = "test-frozen-load";
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        std::fs::write(
            states.join(format!("{}.json", branch)),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();

        // Plant a frozen phases file the FlowPaths::frozen_phases() call
        // will discover. The schema mirrors the production loader.
        let frozen = json!({
            "order": ["flow-start", "flow-plan", "flow-code", "flow-code-review", "flow-learn", "flow-complete"],
            "phases": {
                "flow-start": {"name": "Start", "command": "/flow:flow-start"},
                "flow-plan": {"name": "Plan", "command": "/flow:flow-plan"},
                "flow-code": {"name": "Code", "command": "/flow:flow-code"},
                "flow-code-review": {"name": "Code Review", "command": "/flow:flow-code-review"},
                "flow-learn": {"name": "Learn", "command": "/flow:flow-learn"},
                "flow-complete": {"name": "Complete", "command": "/flow:flow-complete"},
            }
        });
        std::fs::write(
            states.join(format!("{}-phases.json", branch)),
            serde_json::to_string(&frozen).unwrap(),
        )
        .unwrap();

        let (output, code) = run_impl_main("flow-plan", Some(branch), &root);
        assert_eq!(code, 0);
        // Re-entering "in_progress" phase is allowed: empty output.
        assert!(output.is_empty(), "unexpected message: {}", output);
    }

    /// Exercises lines 148-151 — the resolver returning `None` reports
    /// "Could not determine current git branch." Production never hits
    /// this directly because `resolve_branch` falls back to the raw git
    /// branch name, so the seam-injection variant is required to drive
    /// the arm.
    #[test]
    fn run_impl_main_with_resolver_none_returns_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = |_: Option<&str>, _: &Path| -> Option<String> { None };
        let (output, code) = run_impl_main_with_resolver("flow-plan", None, dir.path(), &resolver);
        assert_eq!(code, 1);
        assert!(
            output.contains("BLOCKED: Could not determine current git branch"),
            "got: {}",
            output
        );
    }

    /// Exercises lines 185-186 — the `Err` arm of `read_to_string` when
    /// the state-file path resolves to a directory instead of a file.
    /// `Path::exists()` returns true for directories too, so the
    /// existence check passes and the read attempt produces an Err.
    #[test]
    fn run_impl_main_state_file_is_directory_returns_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Create the state file path as a directory, not a file.
        let states = root.join(".flow-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::create_dir(states.join("test-feature.json")).unwrap();

        let (output, code) = run_impl_main("flow-plan", Some("test-feature"), &root);
        assert_eq!(code, 1);
        assert!(
            output.contains("BLOCKED: Could not read state file"),
            "expected read-error message, got: {}",
            output
        );
    }

    #[test]
    fn run_impl_main_unparseable_state_file_returns_blocked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".flow-states")).unwrap();
        std::fs::write(
            dir.path().join(".flow-states").join("test.json"),
            "not-valid-json",
        )
        .unwrap();
        let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
        assert_eq!(code, 1);
        assert!(out.contains("BLOCKED"));
        assert!(out.contains("Could not read state file"));
    }

    #[test]
    fn run_impl_main_allowed_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        write_state(dir.path(), "test", state);
        let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
        assert_eq!(code, 0);
        // Allowed + first visit → empty message.
        assert!(out.is_empty());
    }

    #[test]
    fn run_impl_main_blocked_returns_one_with_blocked_message() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("flow-plan", &[("flow-start", "pending")]);
        write_state(dir.path(), "test", state);
        let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
        assert_eq!(code, 1);
        assert!(out.contains("BLOCKED"));
    }

    #[test]
    fn run_impl_main_reentry_returns_note_and_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        state["phases"]["flow-plan"]["visit_count"] = json!(2);
        write_state(dir.path(), "test", state);
        let (out, code) = run_impl_main("flow-plan", Some("test"), dir.path());
        assert_eq!(code, 0);
        assert!(out.contains("previously completed"));
    }

    #[test]
    fn run_impl_main_slash_branch_returns_blocked_no_panic() {
        // `--branch feature/foo` (standard git ref shape) must not
        // panic — must route through `FlowPaths::try_new` to the
        // "no active flow on this branch" branch. Guards against the
        // Issue #1054 recurrence the adversarial agent caught.
        let dir = tempfile::tempdir().unwrap();
        let (out, code) = run_impl_main("flow-plan", Some("feature/foo"), dir.path());
        assert_eq!(code, 1);
        assert!(out.contains("BLOCKED"), "output: {}", out);
        assert!(out.contains("feature/foo"), "output: {}", out);
    }

    #[test]
    fn run_impl_main_empty_branch_returns_blocked_no_panic() {
        // Empty `--branch ""` must not panic — must route through
        // `FlowPaths::try_new` to the blocked message.
        let dir = tempfile::tempdir().unwrap();
        let (out, code) = run_impl_main("flow-plan", Some(""), dir.path());
        assert_eq!(code, 1);
        assert!(out.contains("BLOCKED"), "output: {}", out);
    }

    #[test]
    fn run_impl_main_invalid_phase_returns_json_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("flow-start", &[("flow-start", "complete")]);
        write_state(dir.path(), "test", state);
        let (out, code) = run_impl_main("nonexistent", Some("test"), dir.path());
        assert_eq!(code, 1);
        let parsed: Value = serde_json::from_str(&out).expect("invalid-phase path emits JSON");
        assert_eq!(parsed["status"], "error");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("Invalid phase"));
    }

    #[test]
    fn missing_phase_name_falls_back_to_key() {
        // `names` is missing the TARGET phase ("flow-plan") → the
        // `.unwrap_or_else(|| phase.to_string())` closure runs.
        let mut names = IndexMap::new();
        names.insert("flow-start".into(), "Start".into());
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-plan".into()],
            names,
            numbers: IndexMap::new(),
            commands: IndexMap::new(),
        };
        let state = make_state("flow-plan", &[("flow-start", "pending")]);
        let (allowed, output) = check_phase(&state, "flow-plan", Some(&config)).unwrap();
        assert!(!allowed);
        // Target phase name falls back to its key.
        assert!(output.contains("flow-plan"));
    }
}
