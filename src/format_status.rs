use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::phase_config::{self, find_state_files, load_phase_config, PhaseConfig, PHASE_ORDER};
use crate::utils::{derive_feature, detect_dev_mode, elapsed_since, format_time, read_version};

/// Column width for phase name alignment.
const NAME_WIDTH: usize = 12;

/// Build the status panel string from state dict and version.
pub fn format_panel(
    state: &Value,
    version: &str,
    now: Option<DateTime<FixedOffset>>,
    dev_mode: bool,
    phase_config: Option<&PhaseConfig>,
) -> String {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names = phase_config::phase_names();
    let default_numbers = phase_config::phase_numbers();
    let default_commands = phase_config::commands();

    let order = phase_config.map(|c| &c.order).unwrap_or(&default_order);
    let names = phase_config.map(|c| &c.names).unwrap_or(&default_names);
    let numbers = phase_config.map(|c| &c.numbers).unwrap_or(&default_numbers);
    let commands = phase_config
        .map(|c| &c.commands)
        .unwrap_or(&default_commands);

    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => return String::new(),
    };

    // Check if all phases are complete
    let all_complete = order.iter().all(|key| {
        phases
            .get(key.as_str())
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            == Some("complete")
    });

    if all_complete {
        return format_all_complete(state, version, dev_mode, phase_config);
    }

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(format!("  FLOW v{} — Current Status{}", version, dev_label));
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(String::new());

    let branch = state.get("branch").and_then(|b| b.as_str()).unwrap_or("");
    lines.push(format!("  Feature : {}", derive_feature(branch)));
    lines.push(format!("  Branch  : {}", branch));
    // Subdirectory scope (only shown when non-empty). When the user
    // started the flow inside a mono-repo subdir, relative_cwd records
    // the path so the agent (and the panel reader) can see which
    // subdirectory the flow operates in.
    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !relative_cwd.is_empty() {
        lines.push(format!("  Subdir  : {}", relative_cwd));
    }
    lines.push(format!(
        "  PR      : {}",
        state
            .get("pr_url")
            .and_then(|u| u.as_str())
            .unwrap_or("N/A")
    ));

    // Elapsed time
    let started_at = state.get("started_at").and_then(|s| s.as_str());
    let elapsed = elapsed_since(started_at, now);
    lines.push(format!("  Elapsed : {}", format_time(elapsed)));

    // Notes count (omit if zero)
    let notes = state
        .get("notes")
        .and_then(|n| n.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if notes > 0 {
        lines.push(format!("  Notes   : {}", notes));
    }

    lines.push(String::new());
    lines.push("  Phases".to_string());
    lines.push("  ------".to_string());

    let mut current_phase_data: Option<&Value> = None;

    for key in order {
        let phase = phases.get(key.as_str());
        let status = phase
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let name = names
            .get(key.as_str())
            .map(|s| s.as_str())
            .unwrap_or(key.as_str());
        let num = numbers.get(key.as_str()).copied().unwrap_or(0);

        if status == "complete" {
            let seconds = phase
                .and_then(|p| p.get("cumulative_seconds"))
                .and_then(|s| s.as_i64())
                .unwrap_or(0);
            let time_str = format_time(seconds);
            let padded_name = format!("{:<width$}", name, width = NAME_WIDTH);
            lines.push(format!(
                "  [x] Phase {}:  {} ({})",
                num, padded_name, time_str
            ));
        } else if status == "in_progress" {
            let padded_name = format!("{:<width$}", name, width = NAME_WIDTH);
            lines.push(format!(
                "  [>] Phase {}:  {} <-- YOU ARE HERE",
                num, padded_name
            ));
            current_phase_data = phase;
        } else {
            lines.push(format!("  [ ] Phase {}:  {}", num, name));
        }
    }

    lines.push(String::new());

    if let Some(cpd) = current_phase_data {
        let mut seconds = cpd
            .get("cumulative_seconds")
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        let session_started = cpd.get("session_started_at").and_then(|s| s.as_str());
        if let Some(ss) = session_started {
            if !ss.is_empty() {
                seconds += elapsed_since(Some(ss), now);
            }
        }
        let visits = cpd.get("visit_count").and_then(|v| v.as_i64()).unwrap_or(0);
        lines.push(format!(
            "  Time in current phase : {}",
            format_time(seconds)
        ));
        lines.push(format!("  Times visited         : {}", visits));
        lines.push(String::new());
    }

    // Continue (in_progress) vs Next (pending)
    let current = state
        .get("current_phase")
        .and_then(|c| c.as_str())
        .unwrap_or("flow-start");
    let current_status = phases
        .get(current)
        .and_then(|p| p.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");
    let default_cmd = format!("/flow:{}", current);
    if current_status == "in_progress" {
        let cmd = commands
            .get(current)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  Continue: {}", cmd));
    } else {
        let cmd = commands
            .get(current)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  Next: {}", cmd));
    }
    lines.push(String::new());
    lines.push("────────────────────────────────────────────".to_string());

    lines.join("\n")
}

/// Build the enriched all-complete panel.
pub fn format_all_complete(
    state: &Value,
    version: &str,
    dev_mode: bool,
    phase_config: Option<&PhaseConfig>,
) -> String {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names = phase_config::phase_names();
    let default_numbers = phase_config::phase_numbers();

    let order = phase_config.map(|c| &c.order).unwrap_or(&default_order);
    let names = phase_config.map(|c| &c.names).unwrap_or(&default_names);
    let numbers = phase_config.map(|c| &c.numbers).unwrap_or(&default_numbers);

    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => return String::new(),
    };

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    lines.push(format!(
        "  FLOW v{} — All Phases Complete!{}",
        version, dev_label
    ));
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    lines.push(String::new());

    let branch = state.get("branch").and_then(|b| b.as_str()).unwrap_or("");
    lines.push(format!("  Feature : {}", derive_feature(branch)));
    // Subdirectory scope (only shown when non-empty). Mirrors the
    // in-progress panel in format_panel: when a flow was started
    // inside a mono-repo subdirectory, the user needs to see which
    // one even after the flow is complete.
    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !relative_cwd.is_empty() {
        lines.push(format!("  Subdir  : {}", relative_cwd));
    }
    lines.push(format!(
        "  PR      : {}",
        state
            .get("pr_url")
            .and_then(|u| u.as_str())
            .unwrap_or("N/A")
    ));

    // Total elapsed from phase timings
    let total: i64 = order
        .iter()
        .map(|key| {
            phases
                .get(key.as_str())
                .and_then(|p| p.get("cumulative_seconds"))
                .and_then(|s| s.as_i64())
                .unwrap_or(0)
        })
        .sum();
    lines.push(format!("  Elapsed : {}", format_time(total)));

    lines.push(String::new());
    lines.push("  Phases".to_string());
    lines.push("  ------".to_string());

    for key in order {
        let phase = phases.get(key.as_str());
        let padded_name = format!(
            "{:<width$}",
            names
                .get(key.as_str())
                .map(|s| s.as_str())
                .unwrap_or(key.as_str()),
            width = NAME_WIDTH
        );
        let seconds = phase
            .and_then(|p| p.get("cumulative_seconds"))
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        let time_str = format_time(seconds);
        let num = numbers.get(key.as_str()).copied().unwrap_or(0);
        lines.push(format!(
            "  [x] Phase {}:  {} ({})",
            num, padded_name, time_str
        ));
    }

    lines.push(String::new());
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

    lines.join("\n")
}

/// Build a summary panel listing multiple active features.
pub fn format_multi_panel(
    results: &[(std::path::PathBuf, Value, String)],
    version: &str,
    dev_mode: bool,
) -> String {
    let names = phase_config::phase_names();
    let numbers = phase_config::phase_numbers();
    let cmds = phase_config::commands();

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(format!(
        "  FLOW v{} — Multiple Features Active{}",
        version, dev_label
    ));
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(String::new());

    for (i, (_path, state, matched_branch)) in results.iter().enumerate() {
        let phase_key = state
            .get("current_phase")
            .and_then(|c| c.as_str())
            .unwrap_or("flow-start");
        let phase_name = names
            .get(phase_key)
            .map(|s| s.as_str())
            .unwrap_or(phase_key);
        let phase_num: String = numbers
            .get(phase_key)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let phase_status = state
            .get("phases")
            .and_then(|p| p.get(phase_key))
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let default_cmd = format!("/flow:{}", phase_key);
        let cmd = cmds
            .get(phase_key)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  {}. {}", i + 1, derive_feature(matched_branch)));
        lines.push(format!("     Branch : {}", matched_branch));
        lines.push(format!(
            "     Phase  : {} — {} ({})",
            phase_num, phase_name, phase_status
        ));
        lines.push(format!("     Next   : {}", cmd));
        lines.push(String::new());
    }

    lines.push("────────────────────────────────────────────".to_string());
    lines.join("\n")
}

/// Driver for the `bin/flow format-status` subcommand.
///
/// Returns `Result<(stdout_text, code), (stderr_text, code)>`:
///
/// - `Ok((panel, 0))` — a single-flow panel or a multi-flow panel
///   was rendered and should be written to stdout with exit 0.
/// - `Ok(("", 1))` — no state files exist for any branch. The caller
///   exits 1 silently (historical contract: no stdout or stderr).
/// - `Err(("Could not determine current branch", 2))` — branch
///   resolution failed. The caller writes the message to stderr
///   and exits 2.
///
/// Tests supply `root` as a fixture TempDir and `branch_override`
/// explicitly so the helper does not shell out to `git rev-parse`
/// against the host worktree.
pub fn run_impl_main(
    branch_override: Option<&str>,
    root: &Path,
) -> Result<(String, i32), (String, i32)> {
    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => {
            return Err(("Could not determine current branch".to_string(), 2));
        }
    };

    let results = find_state_files(root, &branch);
    let results = if results.is_empty() {
        let all = find_state_files(root, "");
        if all.is_empty() {
            return Ok((String::new(), 1));
        }
        all
    } else {
        results
    };

    let version = read_version();
    let dev_mode = detect_dev_mode(root);

    if results.len() > 1 {
        return Ok((format_multi_panel(&results, &version, dev_mode), 0));
    }

    let (_state_path, state, matched_branch) = &results[0];
    let frozen_path = FlowPaths::new(root, matched_branch).frozen_phases();
    let phase_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    Ok((
        format_panel(state, &version, None, dev_mode, phase_config.as_ref()),
        0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use indexmap::IndexMap;
    use serde_json::json;

    const VERSION: &str = "0.8.2";

    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let mut phases = serde_json::Map::new();
        let phase_names = crate::phase_config::phase_names();
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        for &p in &all_phases {
            let status = phase_statuses
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| *v)
                .unwrap_or("pending");
            let name = phase_names.get(p).cloned().unwrap_or_default();
            phases.insert(
                p.to_string(),
                json!({
                    "name": name,
                    "status": status,
                    "started_at": null,
                    "completed_at": null,
                    "session_started_at": null,
                    "cumulative_seconds": 0,
                    "visit_count": 0,
                }),
            );
        }

        json!({
            "branch": "test-feature",
            "pr_url": "https://github.com/test/test/pull/1",
            "started_at": "2026-01-01T00:00:00-08:00",
            "current_phase": current_phase,
            "notes": [],
            "phases": phases,
        })
    }

    // --- Panel header ---

    #[test]
    fn panel_includes_header_with_version() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains(&format!("FLOW v{} — Current Status", VERSION)),
            "Panel:\n{}",
            panel
        );
    }

    #[test]
    fn panel_includes_feature_and_branch() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("Feature : Test Feature"),
            "Panel:\n{}",
            panel
        );
        assert!(
            panel.contains("Branch  : test-feature"),
            "Panel:\n{}",
            panel
        );
    }

    #[test]
    fn panel_includes_pr_url() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("PR      : https://github.com/test/test/pull/1"),
            "Panel:\n{}",
            panel
        );
    }

    // --- Phase display ---

    #[test]
    fn panel_shows_completed_phase_with_timing() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(300);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("[x] Phase 1:"), "Panel:\n{}", panel);
        assert!(panel.contains("(5m)"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_shows_in_progress_marker() {
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("[>] Phase 2:"), "Panel:\n{}", panel);
        assert!(panel.contains("<-- YOU ARE HERE"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_shows_pending_phases() {
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("[ ] Phase 3:"), "Panel:\n{}", panel);
    }

    // --- Timing ---

    #[test]
    fn panel_shows_current_phase_timing() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(120);
        state["phases"]["flow-plan"]["session_started_at"] = json!(null);
        state["phases"]["flow-plan"]["visit_count"] = json!(2);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("Time in current phase : 2m"),
            "Panel:\n{}",
            panel
        );
        assert!(
            panel.contains("Times visited         : 2"),
            "Panel:\n{}",
            panel
        );
    }

    #[test]
    fn in_progress_phase_shows_live_elapsed() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(0);
        state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-01T00:00:00Z");
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 0, 10, 0)
            .unwrap();
        let panel = format_panel(&state, VERSION, Some(now), false, None);
        assert!(
            panel.contains("Time in current phase : 10m"),
            "Panel:\n{}",
            panel
        );
    }

    #[test]
    fn in_progress_phase_adds_live_to_cumulative() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(600);
        state["phases"]["flow-plan"]["session_started_at"] = json!("2026-01-01T00:00:00Z");
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 0, 5, 0)
            .unwrap();
        let panel = format_panel(&state, VERSION, Some(now), false, None);
        assert!(
            panel.contains("Time in current phase : 15m"),
            "Panel:\n{}",
            panel
        );
    }

    #[test]
    fn panel_shows_elapsed_time() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["started_at"] = json!("2026-01-01T00:00:00Z");
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 1, 2, 0, 0)
            .unwrap();
        let panel = format_panel(&state, VERSION, Some(now), false, None);
        assert!(panel.contains("Elapsed : 2h 0m"), "Panel:\n{}", panel);
    }

    // --- Notes ---

    #[test]
    fn panel_shows_notes_count() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["notes"] = json!([{"text": "note 1"}, {"text": "note 2"}, {"text": "note 3"}]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("Notes   : 3"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_hides_notes_when_zero() {
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(!panel.contains("Notes"), "Panel:\n{}", panel);
    }

    // --- Continue vs Next ---

    #[test]
    fn panel_continue_label_when_in_progress() {
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("Continue: /flow:flow-plan"),
            "Panel:\n{}",
            panel
        );
        assert!(!panel.contains("Next:"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_next_label_when_phase_complete() {
        let state = make_state(
            "flow-code",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("Next: /flow:flow-code"), "Panel:\n{}", panel);
        assert!(!panel.contains("Continue:"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_next_label_when_phase_pending() {
        let state = make_state("flow-plan", &[("flow-start", "complete")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("Next: /flow:flow-plan"), "Panel:\n{}", panel);
        assert!(!panel.contains("Continue:"), "Panel:\n{}", panel);
    }

    // --- All complete ---

    #[test]
    fn panel_all_complete_shows_timing() {
        let all_phases = [
            "flow-start",
            "flow-plan",
            "flow-code",
            "flow-code-review",
            "flow-learn",
            "flow-complete",
        ];
        let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
        let mut state = make_state("flow-complete", &statuses);
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(900);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(3600);
        state["phases"]["flow-code-review"]["cumulative_seconds"] = json!(870);
        state["phases"]["flow-learn"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-complete"]["cumulative_seconds"] = json!(20);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains(&format!("FLOW v{} — All Phases Complete!", VERSION)),
            "Panel:\n{}",
            panel
        );
        assert!(
            panel.contains("Feature : Test Feature"),
            "Panel:\n{}",
            panel
        );
        assert!(
            panel.contains("PR      : https://github.com/test/test/pull/1"),
            "Panel:\n{}",
            panel
        );
        assert!(panel.contains("Elapsed : 1h 35m"), "Panel:\n{}", panel);
        for i in 1..=6 {
            assert!(
                panel.contains(&format!("[x] Phase {}:", i)),
                "Missing phase {} in panel:\n{}",
                i,
                panel
            );
        }
    }

    // --- Timing formats ---

    #[test]
    fn panel_timing_formats() {
        let mut state = make_state(
            "flow-code-review",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "in_progress"),
            ],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(30);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(3660);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(120);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(panel.contains("(<1m)"), "Panel:\n{}", panel);
        assert!(panel.contains("(1h 1m)"), "Panel:\n{}", panel);
        assert!(panel.contains("(2m)"), "Panel:\n{}", panel);
    }

    // --- All 6 phases ---

    #[test]
    fn panel_has_all_6_phases() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        for i in 1..=6 {
            assert!(
                panel.contains(&format!("Phase {}:", i)),
                "Missing phase {} in panel:\n{}",
                i,
                panel
            );
        }
    }

    // --- Dev mode ---

    #[test]
    fn panel_shows_dev_mode_label() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, true, None);
        assert!(panel.contains("[DEV MODE]"), "Panel:\n{}", panel);
    }

    #[test]
    fn panel_hides_dev_mode_when_false() {
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(!panel.contains("DEV MODE"), "Panel:\n{}", panel);
    }

    // --- Frozen phase config ---

    #[test]
    fn panel_uses_frozen_phase_config() {
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-plan".into()],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Begin".into());
                m.insert("flow-plan".into(), "Design".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-plan".into(), 2);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:begin".into());
                m.insert("flow-plan".into(), "/t:design".into());
                m
            },
        };

        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let panel = format_panel(&state, VERSION, None, false, Some(&config));
        assert!(panel.contains("Begin"), "Panel:\n{}", panel);
        assert!(panel.contains("Design"), "Panel:\n{}", panel);
        assert!(
            !panel.contains("Code"),
            "Panel should not contain default phase names:\n{}",
            panel
        );
    }

    #[test]
    fn all_complete_uses_frozen_phase_config() {
        let config = PhaseConfig {
            order: vec!["flow-start".into(), "flow-plan".into()],
            names: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "Begin".into());
                m.insert("flow-plan".into(), "Design".into());
                m
            },
            numbers: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), 1);
                m.insert("flow-plan".into(), 2);
                m
            },
            commands: {
                let mut m = IndexMap::new();
                m.insert("flow-start".into(), "/t:begin".into());
                m.insert("flow-plan".into(), "/t:design".into());
                m
            },
        };

        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "complete")],
        );
        let panel = format_panel(&state, VERSION, None, false, Some(&config));
        assert!(panel.contains("All Phases Complete"), "Panel:\n{}", panel);
        assert!(panel.contains("Begin"), "Panel:\n{}", panel);
        assert!(panel.contains("Design"), "Panel:\n{}", panel);
    }

    // --- Multi-panel ---

    #[test]
    fn multi_panel_lists_features() {
        let state_a = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let mut state_b = make_state(
            "flow-code",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "in_progress"),
            ],
        );
        state_b["branch"] = json!("other-feature");

        let results = vec![
            (
                std::path::PathBuf::from("/tmp/a.json"),
                state_a,
                "test-feature".to_string(),
            ),
            (
                std::path::PathBuf::from("/tmp/b.json"),
                state_b,
                "other-feature".to_string(),
            ),
        ];

        let panel = format_multi_panel(&results, VERSION, false);
        assert!(
            panel.contains("Multiple Features Active"),
            "Panel:\n{}",
            panel
        );
        assert!(panel.contains("1. Test Feature"), "Panel:\n{}", panel);
        assert!(panel.contains("2. Other Feature"), "Panel:\n{}", panel);
        assert!(panel.contains("Branch : test-feature"), "Panel:\n{}", panel);
        assert!(
            panel.contains("Branch : other-feature"),
            "Panel:\n{}",
            panel
        );
    }

    // --- run_impl_main (main.rs FormatStatus arm driver) ---

    fn write_state_file(root: &std::path::Path, branch: &str, state: &Value) {
        let dir = root.join(".flow-states");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.json", branch)), state.to_string()).unwrap();
    }

    #[test]
    fn run_impl_main_no_state_files_returns_empty_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_impl_main(Some("test"), dir.path());
        assert_eq!(result, Ok((String::new(), 1)));
    }

    #[test]
    fn run_impl_main_single_state_returns_panel_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let state = make_state("flow-start", &[("flow-start", "in_progress")]);
        write_state_file(dir.path(), "only-feature", &state);
        let (text, code) = run_impl_main(Some("only-feature"), dir.path()).expect("ok path");
        assert_eq!(code, 0);
        // Single-flow panel header.
        assert!(text.contains("FLOW"), "Panel:\n{}", text);
    }

    #[test]
    fn run_impl_main_multi_state_returns_multi_panel_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s1 = make_state("flow-start", &[("flow-start", "in_progress")]);
        let s2 = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        write_state_file(dir.path(), "first-feature", &s1);
        write_state_file(dir.path(), "second-feature", &s2);
        // Branch override that does not match — falls back to find_state_files(root, "") which returns both.
        let (text, code) = run_impl_main(Some("nonexistent"), dir.path()).expect("ok path");
        assert_eq!(code, 0);
        assert!(
            text.contains("Multiple Features Active"),
            "Multi panel header missing:\n{}",
            text
        );
    }

    #[test]
    fn run_impl_main_branch_match_returns_single_panel_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s1 = make_state("flow-start", &[("flow-start", "in_progress")]);
        let s2 = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        write_state_file(dir.path(), "first-feature", &s1);
        write_state_file(dir.path(), "second-feature", &s2);
        // Exact branch match → single panel even though multiple states exist.
        let (text, code) = run_impl_main(Some("second-feature"), dir.path()).expect("ok path");
        assert_eq!(code, 0);
        assert!(
            !text.contains("Multiple Features Active"),
            "Expected single panel, got multi:\n{}",
            text
        );
    }

    // --- format_multi_panel direct coverage ---

    #[test]
    fn format_status_multi_panel_renders_two_flows() {
        // Construct two (PathBuf, Value, String) tuples and call
        // format_multi_panel directly so the multi-panel rendering path
        // is exercised without routing through run_impl_main's state
        // discovery. Sibling test run_impl_main_multi_state_returns_multi_panel_exit_0
        // covers the same function via the production dispatch path;
        // this test pins format_multi_panel's rendering contract
        // independently of the state-discovery surface.
        let state_a = make_state(
            "flow-code",
            &[("flow-start", "complete"), ("flow-code", "in_progress")],
        );
        let state_b = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        let results = vec![
            (
                std::path::PathBuf::from("/tmp/state-a.json"),
                state_a,
                "feature-a".to_string(),
            ),
            (
                std::path::PathBuf::from("/tmp/state-b.json"),
                state_b,
                "feature-b".to_string(),
            ),
        ];
        let panel = format_multi_panel(&results, VERSION, false);
        assert!(
            panel.contains("Multiple Features Active"),
            "Panel:\n{}",
            panel
        );
        assert!(panel.contains("Feature A"), "Panel:\n{}", panel);
        assert!(panel.contains("Feature B"), "Panel:\n{}", panel);
        assert!(panel.contains("Branch : feature-a"), "Panel:\n{}", panel);
        assert!(panel.contains("Branch : feature-b"), "Panel:\n{}", panel);
    }

    #[test]
    fn format_status_run_impl_main_no_state_files_returns_ok_empty_1() {
        // Pin the silent-exit-1 contract documented at run_impl_main's
        // doc comment: no state files in .flow-states → Ok(("", 1)).
        // Sibling test run_impl_main_no_state_files_returns_empty_exit_1
        // covers the same branch from the same angle; this
        // plan-named test locks the contract under the specific
        // name flow-plan Task 3 enumerated.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".flow-states")).unwrap();
        let result = run_impl_main(Some("nonexistent-branch"), &root);
        assert_eq!(result, Ok((String::new(), 1)));
    }

    /// Exercises lines 376-377 — the fallback branch where the requested
    /// branch has no state file BUT another state file exists in the
    /// project. `run_impl_main` falls through to render whatever state
    /// files are present.
    #[test]
    fn format_status_run_impl_main_unknown_branch_falls_back_to_other_state_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let state_dir = root.join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        // Plant a sibling state file under a different branch name so
        // find_state_files(root, "") returns something while
        // find_state_files(root, requested_branch) returns empty.
        let mut sibling = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        sibling["branch"] = json!("sibling-feature");
        std::fs::write(
            state_dir.join("sibling-feature.json"),
            serde_json::to_string(&sibling).unwrap(),
        )
        .unwrap();

        let (text, code) = run_impl_main(Some("requested-but-absent"), &root).expect("ok path");
        // The fallback rendered the sibling flow's panel, so exit 0 with
        // non-empty text.
        assert_eq!(code, 0);
        assert!(
            text.contains("sibling-feature"),
            "expected sibling branch name in fallback panel, got: {}",
            text
        );
    }

    /// Exercises line 72 — the `Subdir` line is emitted when the state's
    /// `relative_cwd` is non-empty (mono-repo flow started in a subdir).
    #[test]
    fn format_panel_renders_subdir_line_when_relative_cwd_set() {
        let mut state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        state["relative_cwd"] = json!("api");
        let panel = format_panel(&state, "9.9.9", None, false, None);
        assert!(
            panel.contains("Subdir  : api"),
            "expected Subdir line, got: {}",
            panel
        );
    }

    /// Exercises line 37 — the early return when state has no `phases`
    /// key. `format_panel` returns an empty string rather than panic.
    #[test]
    fn format_panel_no_phases_key_returns_empty_string() {
        let state = json!({
            "branch": "test-feature",
            "current_phase": "flow-plan",
        });
        assert_eq!(format_panel(&state, "9.9.9", None, false, None), "");
    }

    /// Exercises line 208 — the early return when `format_all_complete`
    /// is invoked with a state that lacks a `phases` key. The function
    /// is `pub` so the direct call path is part of the API contract.
    #[test]
    fn format_all_complete_no_phases_key_returns_empty_string() {
        let state = json!({
            "branch": "test-feature",
            "pr_url": "https://example.com/pr/1",
        });
        assert_eq!(format_all_complete(&state, "9.9.9", false, None), "");
    }

    #[test]
    fn format_status_run_impl_main_loads_frozen_phase_config() {
        // The frozen_path.exists() branch in run_impl_main (~L391) loads
        // the frozen phase config that a flow captured at flow-start,
        // so a panel rendered mid-flow uses the ordering the flow was
        // started with even if main's phase config has since changed.
        // No other test reaches this branch.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let branch = "test-frozen";
        let state_dir = root.join(".flow-states");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state = make_state(
            "flow-plan",
            &[("flow-start", "complete"), ("flow-plan", "in_progress")],
        );
        std::fs::write(
            state_dir.join(format!("{}.json", branch)),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();

        // frozen phases JSON shape expected by load_phase_config:
        // top-level `order` array plus `phases` object whose entries
        // carry `name` and `command` strings. Numbers derive from the
        // order index, so we override only the display name for
        // flow-plan to give the panel a detectable signal.
        let frozen = json!({
            "order": [
                "flow-start",
                "flow-plan",
                "flow-code",
                "flow-code-review",
                "flow-learn",
                "flow-complete"
            ],
            "phases": {
                "flow-start": {"name": "Start", "command": "/flow:flow-start"},
                "flow-plan": {"name": "Custom Plan Name", "command": "/flow:flow-plan-custom"},
                "flow-code": {"name": "Code", "command": "/flow:flow-code"},
                "flow-code-review": {"name": "Code Review", "command": "/flow:flow-code-review"},
                "flow-learn": {"name": "Learn", "command": "/flow:flow-learn"},
                "flow-complete": {"name": "Complete", "command": "/flow:flow-complete"}
            }
        });
        std::fs::write(
            state_dir.join(format!("{}-phases.json", branch)),
            serde_json::to_string(&frozen).unwrap(),
        )
        .unwrap();

        let (text, code) = run_impl_main(Some(branch), &root).expect("ok path");
        assert_eq!(code, 0);
        assert!(
            text.contains("Custom Plan Name"),
            "Panel should reflect frozen phase name:\n{}",
            text
        );
    }

    #[test]
    fn format_status_all_complete_renders_all_phases_complete_panel() {
        // format_panel dispatches to format_all_complete when every
        // phase is "complete" (L49-51). Exercising that branch covers
        // the all-complete panel's border, feature line, PR line,
        // total-elapsed calculation, and per-phase [x] rows — none of
        // which any other test reaches.
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "complete"),
            ],
        );
        state["phases"]["flow-start"]["cumulative_seconds"] = json!(36);
        state["phases"]["flow-plan"]["cumulative_seconds"] = json!(300);
        state["phases"]["flow-code"]["cumulative_seconds"] = json!(600);
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("All Phases Complete"),
            "Expected all-complete panel header:\n{}",
            panel
        );
        assert!(
            panel.contains("Feature : Test Feature"),
            "Expected feature line:\n{}",
            panel
        );
        assert!(
            panel.contains("[x] Phase 1:"),
            "Expected Phase 1 completed row:\n{}",
            panel
        );
        assert!(
            panel.contains("[x] Phase 6:"),
            "Expected Phase 6 completed row:\n{}",
            panel
        );
    }

    #[test]
    fn format_status_all_complete_with_relative_cwd_renders_subdir_line() {
        // Covers the relative_cwd branch inside format_all_complete
        // (L231-233) — shown when the flow was started from a
        // mono-repo subdirectory.
        let mut state = make_state(
            "flow-complete",
            &[
                ("flow-start", "complete"),
                ("flow-plan", "complete"),
                ("flow-code", "complete"),
                ("flow-code-review", "complete"),
                ("flow-learn", "complete"),
                ("flow-complete", "complete"),
            ],
        );
        state["relative_cwd"] = json!("api");
        let panel = format_panel(&state, VERSION, None, false, None);
        assert!(
            panel.contains("All Phases Complete"),
            "Expected all-complete panel:\n{}",
            panel
        );
        assert!(
            panel.contains("Subdir  : api"),
            "Expected Subdir line when relative_cwd is set:\n{}",
            panel
        );
    }
}
