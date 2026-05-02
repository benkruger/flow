//! Integration tests for `format_status` — panel rendering and the
//! `run_impl_main` state-discovery dispatch.

use chrono::{FixedOffset, TimeZone};
use flow_rs::format_status::{
    format_all_complete, format_multi_panel, format_panel, run_impl_main,
};
use flow_rs::phase_config::PhaseConfig;
use indexmap::IndexMap;
use serde_json::{json, Value};

mod common;

const VERSION: &str = "0.8.2";

fn make_state(current_phase: &str, phase_statuses: &[(&str, &str)]) -> Value {
    let mut phases = serde_json::Map::new();
    let phase_names = flow_rs::phase_config::phase_names();
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
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(branch_dir.join("state.json"), state.to_string()).unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".flow-states")).unwrap();
    let result = run_impl_main(Some("nonexistent-branch"), &root);
    assert_eq!(result, Ok((String::new(), 1)));
}

#[test]
fn format_status_run_impl_main_unknown_branch_falls_back_to_other_state_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let sibling_dir = root.join(".flow-states").join("sibling-feature");
    std::fs::create_dir_all(&sibling_dir).unwrap();
    let mut sibling = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    sibling["branch"] = json!("sibling-feature");
    std::fs::write(
        sibling_dir.join("state.json"),
        serde_json::to_string(&sibling).unwrap(),
    )
    .unwrap();

    let (text, code) = run_impl_main(Some("requested-but-absent"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("sibling-feature"),
        "expected sibling branch name in fallback panel, got: {}",
        text
    );
}

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

#[test]
fn format_panel_no_phases_key_returns_empty_string() {
    let state = json!({
        "branch": "test-feature",
        "current_phase": "flow-plan",
    });
    assert_eq!(format_panel(&state, "9.9.9", None, false, None), "");
}

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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let branch = "test-frozen";
    let branch_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&branch_dir).unwrap();
    let state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    std::fs::write(
        branch_dir.join("state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

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
        branch_dir.join("phases.json"),
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
    assert!(panel.contains("All Phases Complete"), "Panel:\n{}", panel);
    assert!(
        panel.contains("Feature : Test Feature"),
        "Panel:\n{}",
        panel
    );
    assert!(panel.contains("[x] Phase 1:"), "Panel:\n{}", panel);
    assert!(panel.contains("[x] Phase 6:"), "Panel:\n{}", panel);
}

#[test]
fn format_status_all_complete_with_relative_cwd_renders_subdir_line() {
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
    assert!(panel.contains("All Phases Complete"), "Panel:\n{}", panel);
    assert!(panel.contains("Subdir  : api"), "Panel:\n{}", panel);
}

#[test]
fn panel_session_started_at_empty_string_not_added_to_elapsed() {
    // Covers the `if !ss.is_empty()` false branch — session_started_at
    // is present as an empty string, so the live-elapsed addition is
    // skipped.
    let mut state = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    state["phases"]["flow-plan"]["cumulative_seconds"] = json!(60);
    state["phases"]["flow-plan"]["session_started_at"] = json!("");
    let panel = format_panel(&state, VERSION, None, false, None);
    assert!(
        panel.contains("Time in current phase : 1m"),
        "Panel:\n{}",
        panel
    );
}

#[test]
fn format_all_complete_dev_mode_shows_label() {
    let all_phases = [
        "flow-start",
        "flow-plan",
        "flow-code",
        "flow-code-review",
        "flow-learn",
        "flow-complete",
    ];
    let statuses: Vec<(&str, &str)> = all_phases.iter().map(|&p| (p, "complete")).collect();
    let state = make_state("flow-complete", &statuses);
    let panel = format_panel(&state, VERSION, None, true, None);
    assert!(panel.contains("[DEV MODE]"), "Panel:\n{}", panel);
    assert!(panel.contains("All Phases Complete"), "Panel:\n{}", panel);
}

#[test]
fn format_multi_panel_dev_mode_shows_label() {
    let state_a = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    let state_b = make_state(
        "flow-code",
        &[
            ("flow-start", "complete"),
            ("flow-plan", "complete"),
            ("flow-code", "in_progress"),
        ],
    );
    let results = vec![
        (
            std::path::PathBuf::from("/tmp/a.json"),
            state_a,
            "alpha".to_string(),
        ),
        (
            std::path::PathBuf::from("/tmp/b.json"),
            state_b,
            "beta".to_string(),
        ),
    ];
    let panel = format_multi_panel(&results, VERSION, true);
    assert!(panel.contains("[DEV MODE]"), "Panel:\n{}", panel);
}

#[test]
fn format_multi_panel_unknown_phase_uses_question_mark_number() {
    // Covers `numbers.get(phase_key)` returning None → fallback to
    // `"?"`. The phase key `"flow-unknown"` is not in the default
    // phase_numbers map.
    let state = json!({
        "branch": "x",
        "current_phase": "flow-unknown",
        "pr_url": "https://example.com/1",
        "started_at": "2026-01-01T00:00:00-08:00",
        "notes": [],
        "phases": {
            "flow-unknown": {
                "status": "in_progress",
                "cumulative_seconds": 0,
                "visit_count": 0,
                "started_at": null,
                "completed_at": null,
                "session_started_at": null,
                "name": "Unknown"
            }
        }
    });
    let results = vec![(
        std::path::PathBuf::from("/tmp/x.json"),
        state,
        "x-branch".to_string(),
    )];
    let panel = format_multi_panel(&results, VERSION, false);
    assert!(
        panel.contains("? — flow-unknown"),
        "Expected '? — flow-unknown' in panel:\n{}",
        panel
    );
}

#[test]
fn run_impl_main_no_branch_in_non_git_dir_returns_err_exit_2() {
    // Subprocess test: spawn `bin/flow format-status` with cwd set to
    // a non-git tempdir and no --branch flag. `current_branch()` spawns
    // `git branch --show-current` in the subprocess cwd; git fails in a
    // non-git directory, so `resolve_branch` returns None and
    // run_impl_main emits the branch-resolution error at exit code 2.
    // Covers the `None => return Err(...)` branch in run_impl_main.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["format-status"])
        .current_dir(&root)
        .env_remove("FLOW_SIMULATE_BRANCH")
        .env_remove("FLOW_CI_RUNNING")
        .env("GIT_CEILING_DIRECTORIES", &root)
        .output()
        .expect("spawn flow-rs format-status");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Could not determine current branch"),
        "expected branch-resolve error in stderr, got: {}",
        stderr
    );
}

#[test]
fn run_impl_main_invalid_branch_json_falls_back_to_directory_scan() {
    // Covers the `all` tail expression at line 377 — the exact
    // branch has a state file BUT its JSON is invalid, so
    // find_state_files(root, branch) returns empty. The fallback
    // scan across the directory then picks up other valid state
    // files.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let state_dir = root.join(".flow-states");
    let target_dir = state_dir.join("target-branch");
    let sibling_dir = state_dir.join("sibling-branch");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&sibling_dir).unwrap();
    // Target branch has malformed JSON.
    std::fs::write(target_dir.join("state.json"), "this is not valid json").unwrap();
    // Sibling branch has valid JSON. Use a distinctive branch
    // field so the rendered panel has a greppable signal the
    // fallback fired.
    let mut sibling = make_state(
        "flow-plan",
        &[("flow-start", "complete"), ("flow-plan", "in_progress")],
    );
    sibling["branch"] = json!("fallback-sentinel");
    std::fs::write(
        sibling_dir.join("state.json"),
        serde_json::to_string(&sibling).unwrap(),
    )
    .unwrap();

    let (text, code) = run_impl_main(Some("target-branch"), &root).expect("ok path");
    assert_eq!(code, 0);
    assert!(
        text.contains("fallback-sentinel"),
        "expected sibling branch sentinel in fallback panel, got: {}",
        text
    );
}

// Use `common` to prevent warnings about unused mod.
#[allow(dead_code)]
fn _use_common() {
    let _ = common::repo_root;
}
