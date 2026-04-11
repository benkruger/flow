use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use crate::phase_config::{self, PhaseConfig, PHASE_ORDER};
use crate::utils::{derive_feature, elapsed_since, format_time};

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use indexmap::IndexMap;
    use serde_json::json;

    const VERSION: &str = "0.8.2";

    fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
        let mut phases = serde_json::Map::new();
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
            phases.insert(
                p.to_string(),
                json!({
                    "name": match p {
                        "flow-start" => "Start",
                        "flow-plan" => "Plan",
                        "flow-code" => "Code",
                        "flow-code-review" => "Code Review",
                        "flow-learn" => "Learn",
                        "flow-complete" => "Complete",
                        _ => p,
                    },
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
}
