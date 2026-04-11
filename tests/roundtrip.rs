use flow_rs::state::{FlowState, Phase, PhaseStatus, SkillConfig};

const STATE_JSON: &str = r#"{
  "schema_version": 1,
  "branch": "app-payment-webhooks",
  "repo": "org/repo",
  "pr_number": 42,
  "pr_url": "https://github.com/org/repo/pull/42",
  "started_at": "2026-02-20T10:00:00-08:00",
  "current_phase": "flow-code",
  "files": {
    "plan": ".flow-states/app-payment-webhooks-plan.md",
    "dag": ".flow-states/app-payment-webhooks-dag.md",
    "log": ".flow-states/app-payment-webhooks.log",
    "state": ".flow-states/app-payment-webhooks.json"
  },
  "session_tty": "/dev/ttys001",
  "session_id": "abc-123",
  "transcript_path": null,
  "notes": [
    {
      "phase": "flow-code",
      "phase_name": "Code",
      "timestamp": "2026-02-20T14:23:00-08:00",
      "type": "correction",
      "note": "Test correction"
    }
  ],
  "prompt": "fix #83 payment webhooks",
  "phases": {
    "flow-start": {
      "name": "Start",
      "status": "complete",
      "started_at": "2026-02-20T10:00:00-08:00",
      "completed_at": "2026-02-20T10:05:00-08:00",
      "session_started_at": null,
      "cumulative_seconds": 300,
      "visit_count": 1
    },
    "flow-plan": {
      "name": "Plan",
      "status": "complete",
      "started_at": "2026-02-20T10:05:00-08:00",
      "completed_at": "2026-02-20T10:30:00-08:00",
      "session_started_at": null,
      "cumulative_seconds": 1500,
      "visit_count": 1
    },
    "flow-code": {
      "name": "Code",
      "status": "in_progress",
      "started_at": "2026-02-20T10:30:00-08:00",
      "completed_at": null,
      "session_started_at": "2026-02-20T10:30:00-08:00",
      "cumulative_seconds": 0,
      "visit_count": 1
    },
    "flow-code-review": {
      "name": "Code Review",
      "status": "pending",
      "started_at": null,
      "completed_at": null,
      "session_started_at": null,
      "cumulative_seconds": 0,
      "visit_count": 0
    },
    "flow-learn": {
      "name": "Learn",
      "status": "pending",
      "started_at": null,
      "completed_at": null,
      "session_started_at": null,
      "cumulative_seconds": 0,
      "visit_count": 0
    },
    "flow-complete": {
      "name": "Complete",
      "status": "pending",
      "started_at": null,
      "completed_at": null,
      "session_started_at": null,
      "cumulative_seconds": 0,
      "visit_count": 0
    }
  },
  "phase_transitions": [
    {"from": "flow-start", "to": "flow-plan", "timestamp": "2026-02-20T10:05:00-08:00"},
    {"from": "flow-plan", "to": "flow-code", "timestamp": "2026-02-20T10:30:00-08:00"}
  ],
  "skills": {
    "flow-start": {"continue": "manual"},
    "flow-plan": {"continue": "auto", "dag": "auto"},
    "flow-code": {"commit": "manual", "continue": "manual"},
    "flow-code-review": {"commit": "auto", "continue": "auto"},
    "flow-learn": {"commit": "auto", "continue": "auto"},
    "flow-abort": "auto",
    "flow-complete": "auto"
  },
  "issues_filed": [],
  "plan_file": null,
  "dag_file": ".flow-states/app-payment-webhooks-dag.md",
  "code_task": 2,
  "code_tasks_total": 5,
  "code_task_name": "Implement webhooks",
  "_auto_continue": "/flow:flow-code"
}"#;

#[test]
fn deserialize_real_state_file() {
    let state: FlowState = serde_json::from_str(STATE_JSON).unwrap();

    assert_eq!(state.schema_version, 1);
    assert_eq!(state.branch, "app-payment-webhooks");
    assert_eq!(state.repo, Some("org/repo".into()));
    assert_eq!(state.pr_number, Some(42));
    assert_eq!(state.current_phase, "flow-code");
    assert_eq!(state.prompt, Some("fix #83 payment webhooks".into()));

    // Phase map
    let start = state.phases.get(&Phase::FlowStart).unwrap();
    assert_eq!(start.status, PhaseStatus::Complete);
    assert_eq!(start.cumulative_seconds, 300);
    assert_eq!(start.visit_count, 1);

    let code = state.phases.get(&Phase::FlowCode).unwrap();
    assert_eq!(code.status, PhaseStatus::InProgress);

    let review = state.phases.get(&Phase::FlowCodeReview).unwrap();
    assert_eq!(review.status, PhaseStatus::Pending);

    // Notes
    assert_eq!(state.notes.len(), 1);
    assert_eq!(state.notes[0].note_type, "correction");

    // Phase transitions
    assert_eq!(state.phase_transitions.len(), 2);
    assert_eq!(state.phase_transitions[0].to, "flow-plan");

    // Skills — mixed types
    let skills = state.skills.unwrap();
    assert!(matches!(skills.get("flow-abort").unwrap(), SkillConfig::Simple(s) if s == "auto"));
    assert!(matches!(
        skills.get("flow-code").unwrap(),
        SkillConfig::Detailed(_)
    ));

    // Files
    assert_eq!(
        state.files.plan,
        Some(".flow-states/app-payment-webhooks-plan.md".into())
    );

    // Legacy fields
    assert!(state.plan_file.is_none());
    assert_eq!(
        state.dag_file,
        Some(".flow-states/app-payment-webhooks-dag.md".into())
    );

    // Transient fields
    assert_eq!(state.auto_continue, Some("/flow:flow-code".into()));
    assert_eq!(state.code_task, Some(2));
    assert_eq!(state.code_tasks_total, Some(5));
}

#[test]
fn roundtrip_serialize_deserialize() {
    let state1: FlowState = serde_json::from_str(STATE_JSON).unwrap();
    let json = serde_json::to_string(&state1).unwrap();
    let state2: FlowState = serde_json::from_str(&json).unwrap();
    assert_eq!(state1, state2);
}

#[test]
fn minimal_state_deserializes() {
    // A minimal state file with only required fields and no optional ones
    let json = r#"{
      "schema_version": 1,
      "branch": "test",
      "started_at": "2026-01-01T00:00:00Z",
      "current_phase": "flow-start",
      "files": {
        "plan": null,
        "dag": null,
        "log": ".flow-states/test.log",
        "state": ".flow-states/test.json"
      },
      "phases": {
        "flow-start": {
          "name": "Start",
          "status": "in_progress",
          "started_at": "2026-01-01T00:00:00Z",
          "completed_at": null,
          "session_started_at": "2026-01-01T00:00:00Z",
          "cumulative_seconds": 0,
          "visit_count": 1
        }
      },
      "phase_transitions": []
    }"#;
    let state: FlowState = serde_json::from_str(json).unwrap();
    assert_eq!(state.branch, "test");
    assert!(state.skills.is_none());
    assert!(state.notes.is_empty());
    assert!(state.pr_number.is_none());
}
