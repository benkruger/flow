//! Integration tests for `src/state.rs` — serde round-trips and key
//! semantics of the `FlowState` data types. Every behavior test for
//! the module lives here per `.claude/rules/test-placement.md`.

use indexmap::IndexMap;

use flow_rs::state::{ModelTokens, Phase, PhaseStatus, SkillConfig, StepSnapshot, WindowSnapshot};

#[test]
fn phase_serialize_all_variants() {
    let cases = [
        (Phase::FlowStart, "\"flow-start\""),
        (Phase::FlowPlan, "\"flow-plan\""),
        (Phase::FlowCode, "\"flow-code\""),
        (Phase::FlowCodeReview, "\"flow-code-review\""),
        (Phase::FlowLearn, "\"flow-learn\""),
        (Phase::FlowComplete, "\"flow-complete\""),
    ];
    for (variant, expected) in cases {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected, "serialize {:?}", variant);
        let back: Phase = serde_json::from_str(&json).unwrap();
        assert_eq!(back, variant, "roundtrip {:?}", variant);
    }
}

#[test]
fn phase_status_serialize_all_variants() {
    let cases = [
        (PhaseStatus::Pending, "\"pending\""),
        (PhaseStatus::InProgress, "\"in_progress\""),
        (PhaseStatus::Complete, "\"complete\""),
    ];
    for (variant, expected) in cases {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected, "serialize {:?}", variant);
        let back: PhaseStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, variant, "roundtrip {:?}", variant);
    }
}

#[test]
fn skill_config_simple() {
    let json = "\"auto\"";
    let config: SkillConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config, SkillConfig::Simple("auto".into()));
    assert_eq!(serde_json::to_string(&config).unwrap(), json);
}

#[test]
fn skill_config_detailed() {
    let json = r#"{"commit":"auto","continue":"manual"}"#;
    let config: SkillConfig = serde_json::from_str(json).unwrap();
    let mut expected = IndexMap::new();
    expected.insert("commit".to_string(), "auto".to_string());
    expected.insert("continue".to_string(), "manual".to_string());
    assert_eq!(config, SkillConfig::Detailed(expected));
}

#[test]
fn phase_as_indexmap_key() {
    let mut map = IndexMap::new();
    map.insert(Phase::FlowStart, "start");
    map.insert(Phase::FlowCode, "code");
    assert_eq!(map.get(&Phase::FlowStart), Some(&"start"));
    assert_eq!(map.get(&Phase::FlowCode), Some(&"code"));
    assert_eq!(map.get(&Phase::FlowPlan), None);
}

#[test]
fn phase_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h1 = DefaultHasher::new();
    let mut h2 = DefaultHasher::new();
    Phase::FlowCode.hash(&mut h1);
    Phase::FlowCode.hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
}

#[test]
fn phase_debug_format() {
    assert_eq!(format!("{:?}", Phase::FlowStart), "FlowStart");
    assert_eq!(format!("{:?}", Phase::FlowComplete), "FlowComplete");
}

#[test]
fn phase_copy_semantics() {
    let p = Phase::FlowLearn;
    let q = p;
    assert_eq!(p, q);
}

#[test]
fn phase_status_debug_copy() {
    assert_eq!(format!("{:?}", PhaseStatus::Pending), "Pending");
    let s = PhaseStatus::Complete;
    let t = s;
    assert_eq!(s, t);
}

// --- ModelTokens ---

/// Confirm `ModelTokens::default()` produces all-zero counters. The
/// Default derive is the helper Task 3's capture loop relies on to
/// initialize per-model accumulators before the first transcript
/// line increments them.
#[test]
fn model_tokens_default_is_all_zero() {
    let m = ModelTokens::default();
    assert_eq!(m.input, 0);
    assert_eq!(m.output, 0);
    assert_eq!(m.cache_create, 0);
    assert_eq!(m.cache_read, 0);
}

// --- WindowSnapshot ---

/// Round-trip a fully populated `WindowSnapshot` and assert every
/// field survives unchanged. Guards against accidental field
/// removal, rename, or `skip_serializing_if` regressions that would
/// silently drop data on the wire.
#[test]
fn window_snapshot_roundtrip_full() {
    let mut by_model = IndexMap::new();
    by_model.insert(
        "claude-opus-4-7".to_string(),
        ModelTokens {
            input: 1234,
            output: 5678,
            cache_create: 90,
            cache_read: 4321,
        },
    );
    let snap = WindowSnapshot {
        captured_at: "2026-05-04T10:00:00-07:00".to_string(),
        session_id: Some("abc-123".to_string()),
        model: Some("claude-opus-4-7".to_string()),
        five_hour_pct: Some(42),
        seven_day_pct: Some(7),
        session_input_tokens: Some(1234),
        session_output_tokens: Some(5678),
        session_cache_creation_tokens: Some(90),
        session_cache_read_tokens: Some(4321),
        session_cost_usd: Some(0.987654),
        by_model,
        turn_count: Some(15),
        tool_call_count: Some(73),
        context_at_last_turn_tokens: Some(123_456),
        context_window_pct: Some(61.5),
    };
    let json = serde_json::to_string(&snap).expect("serialize");
    let back: WindowSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, snap);
}

/// Round-trip a snapshot whose every optional numeric field is
/// `None`. Confirms the `skip_serializing_if` annotations omit
/// `None` values and deserialization repopulates them as `None` via
/// `serde(default)`.
#[test]
fn window_snapshot_roundtrip_all_none() {
    let snap = WindowSnapshot {
        captured_at: "2026-05-04T10:00:00-07:00".to_string(),
        session_id: None,
        model: None,
        five_hour_pct: None,
        seven_day_pct: None,
        session_input_tokens: None,
        session_output_tokens: None,
        session_cache_creation_tokens: None,
        session_cache_read_tokens: None,
        session_cost_usd: None,
        by_model: IndexMap::new(),
        turn_count: None,
        tool_call_count: None,
        context_at_last_turn_tokens: None,
        context_window_pct: None,
    };
    let json = serde_json::to_string(&snap).expect("serialize");
    // All-None except `captured_at` should serialize to a single-key
    // object; the skip_serializing_if guards must hold.
    assert_eq!(json, r#"{"captured_at":"2026-05-04T10:00:00-07:00"}"#);
    let back: WindowSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, snap);
}

/// Round-trip a snapshot carrying multiple `by_model` entries.
/// Confirms IndexMap insertion-order preservation and that each
/// `ModelTokens` entry round-trips with all four counters intact.
#[test]
fn window_snapshot_by_model_roundtrip() {
    let mut by_model = IndexMap::new();
    by_model.insert(
        "claude-opus-4-7".to_string(),
        ModelTokens {
            input: 100,
            output: 200,
            cache_create: 300,
            cache_read: 400,
        },
    );
    by_model.insert(
        "claude-sonnet-4-6".to_string(),
        ModelTokens {
            input: 10,
            output: 20,
            cache_create: 30,
            cache_read: 40,
        },
    );
    let snap = WindowSnapshot {
        captured_at: "2026-05-04T10:00:00-07:00".to_string(),
        session_id: None,
        model: None,
        five_hour_pct: None,
        seven_day_pct: None,
        session_input_tokens: None,
        session_output_tokens: None,
        session_cache_creation_tokens: None,
        session_cache_read_tokens: None,
        session_cost_usd: None,
        by_model,
        turn_count: None,
        tool_call_count: None,
        context_at_last_turn_tokens: None,
        context_window_pct: None,
    };
    let json = serde_json::to_string(&snap).expect("serialize");
    let back: WindowSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, snap);
    // Insertion order preserved through round-trip.
    let keys: Vec<&str> = back.by_model.keys().map(|s| s.as_str()).collect();
    assert_eq!(keys, vec!["claude-opus-4-7", "claude-sonnet-4-6"]);
}

// --- StepSnapshot ---

/// Round-trip a `StepSnapshot` and confirm the embedded
/// `WindowSnapshot` flattens into the outer JSON object so each
/// step record is a single flat document rather than a nested
/// `{"snapshot": {...}}` shape.
#[test]
fn step_snapshot_flattens_window_fields() {
    let snap = WindowSnapshot {
        captured_at: "2026-05-04T10:00:00-07:00".to_string(),
        session_id: Some("sid".to_string()),
        model: None,
        five_hour_pct: Some(33),
        seven_day_pct: None,
        session_input_tokens: Some(1),
        session_output_tokens: Some(2),
        session_cache_creation_tokens: None,
        session_cache_read_tokens: None,
        session_cost_usd: None,
        by_model: IndexMap::new(),
        turn_count: Some(4),
        tool_call_count: None,
        context_at_last_turn_tokens: None,
        context_window_pct: None,
    };
    let step = StepSnapshot {
        step: 3,
        field: "code_task".to_string(),
        snapshot: snap.clone(),
    };
    let json = serde_json::to_string(&step).expect("serialize");
    // Flatten places snapshot fields at the same level as step/field.
    assert!(json.contains(r#""step":3"#));
    assert!(json.contains(r#""field":"code_task""#));
    assert!(json.contains(r#""captured_at":"2026-05-04T10:00:00-07:00""#));
    assert!(json.contains(r#""five_hour_pct":33"#));
    // No nested snapshot key.
    assert!(!json.contains(r#""snapshot""#));
    let back: StepSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, step);
}
