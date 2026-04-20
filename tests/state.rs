//! Integration tests for `src/state.rs` — serde round-trips and key
//! semantics of the `FlowState` data types. Every behavior test for
//! the module lives here per `.claude/rules/test-placement.md`.

use indexmap::IndexMap;

use flow_rs::state::{Phase, PhaseStatus, SkillConfig};

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
