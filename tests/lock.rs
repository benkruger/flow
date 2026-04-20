//! Integration tests for `src/lock.rs` — exercises the `mutate_state`
//! public surface plus the `mutate_state_with_lock` seam for injecting
//! lock failures.

use std::fs;

use serde_json::{json, Value};

use flow_rs::lock::{mutate_state, mutate_state_with_lock, MutateError};

// --- mutate_state ---

#[test]
fn mutate_state_basic_transform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"count": 0}"#).unwrap();

    let result = mutate_state(&path, &mut |state| {
        state["count"] = json!(1);
    })
    .unwrap();

    assert_eq!(result["count"], 1);
    let content = fs::read_to_string(&path).unwrap();
    let on_disk: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(on_disk["count"], 1);
}

#[test]
fn mutate_state_adds_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"branch": "test"}"#).unwrap();

    let result = mutate_state(&path, &mut |state| {
        state["new_field"] = json!("added");
    })
    .unwrap();

    assert_eq!(result["branch"], "test");
    assert_eq!(result["new_field"], "added");
}

#[test]
fn mutate_state_valid_json_after_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"items": [1, 2, 3]}"#).unwrap();

    mutate_state(&path, &mut |state| {
        if let Some(arr) = state["items"].as_array_mut() {
            arr.push(json!(4));
        }
    })
    .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["items"].as_array().unwrap().len(), 4);
}

#[test]
fn mutate_state_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = mutate_state(&path, &mut |_| {});
    assert!(result.is_err());
}

#[test]
fn mutate_state_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{corrupt").unwrap();
    let result = mutate_state(&path, &mut |_| {});
    assert!(result.is_err());
}

#[test]
fn mutate_state_array_root_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let content = "[1, 2, 3]";
    fs::write(&path, content).unwrap();
    let result = mutate_state(&path, &mut |_state| {});
    assert!(result.is_ok());
    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn mutate_state_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "").unwrap();
    let result = mutate_state(&path, &mut |_| {});
    assert!(result.is_err());
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, "");
}

#[test]
fn mutate_state_non_json_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let content = "hello world";
    fs::write(&path, content).unwrap();
    let result = mutate_state(&path, &mut |_| {});
    assert!(result.is_err());
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, content);
}

/// A file whose bytes are not valid UTF-8 causes `read_to_string` to
/// return an `io::Error(InvalidData)`. `mutate_state` propagates it via
/// `?` through the `From<io::Error> for MutateError` impl, yielding
/// `MutateError::Io`.
#[test]
fn mutate_state_non_utf8_file_returns_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, [0x80u8]).unwrap();
    let err = mutate_state(&path, &mut |_| {}).unwrap_err();
    assert!(
        matches!(err, MutateError::Io(_)),
        "Expected Io variant, got: {:?}",
        err
    );
}

#[test]
fn mutate_state_truncates_when_shorter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(
        &path,
        r#"{"long_key": "this is a very long value that takes up space"}"#,
    )
    .unwrap();
    let initial_len = fs::metadata(&path).unwrap().len();

    mutate_state(&path, &mut |state| {
        state["long_key"] = json!("short");
    })
    .unwrap();

    let final_len = fs::metadata(&path).unwrap().len();
    assert!(final_len < initial_len);

    let content = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["long_key"], "short");
}

#[test]
fn mutate_state_preserves_key_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"zebra": 1, "apple": 2, "mango": 3}"#).unwrap();

    mutate_state(&path, &mut |state| {
        state["mango"] = json!(99);
    })
    .unwrap();

    let content = fs::read_to_string(&path).unwrap();
    let keys: Vec<&str> = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('"') {
                Some(trimmed.split('"').nth(1).unwrap())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(keys, vec!["zebra", "apple", "mango"]);
}

#[test]
fn mutate_state_transform_receives_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, r#"{"value": 42}"#).unwrap();

    let mut captured = 0i64;
    mutate_state(&path, &mut |state| {
        captured = state["value"].as_i64().unwrap();
        state["value"] = json!(captured + 1);
    })
    .unwrap();

    assert_eq!(captured, 42);
}

// --- MutateError Display / std::error::Error / From impls ---

#[test]
fn mutate_error_display_formats_io() {
    let err = MutateError::Io("disk full".to_string());
    assert_eq!(err.to_string(), "I/O error: disk full");
}

#[test]
fn mutate_error_display_formats_lock() {
    let err = MutateError::Lock("already locked".to_string());
    assert_eq!(err.to_string(), "Lock error: already locked");
}

#[test]
fn mutate_error_display_formats_json() {
    let err = MutateError::Json("parse failure".to_string());
    assert_eq!(err.to_string(), "JSON error: parse failure");
}

#[test]
fn mutate_error_implements_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(MutateError::Io("test".to_string()));
    assert!(err.to_string().contains("test"));
}

#[test]
fn mutate_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let mutate_err: MutateError = io_err.into();
    assert!(matches!(mutate_err, MutateError::Io(ref m) if m.contains("missing")));
}

#[test]
fn mutate_error_from_serde_json_error() {
    let json_err = serde_json::from_str::<Value>("{bad").unwrap_err();
    let mutate_err: MutateError = json_err.into();
    assert!(matches!(mutate_err, MutateError::Json(_)));
}

#[test]
fn mutate_error_lock_constructor() {
    let io_err = std::io::Error::other("lock failed");
    let mutate_err = MutateError::lock(io_err);
    assert!(matches!(mutate_err, MutateError::Lock(ref m) if m.contains("lock failed")));
}

#[test]
fn mutate_error_debug_format() {
    // Exercises the Debug derive on all variants.
    assert!(format!("{:?}", MutateError::Io("x".into())).contains("Io"));
    assert!(format!("{:?}", MutateError::Lock("x".into())).contains("Lock"));
    assert!(format!("{:?}", MutateError::Json("x".into())).contains("Json"));
}

#[test]
fn mutate_state_error_wraps_missing_file_as_io() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let err = mutate_state(&path, &mut |_| {}).unwrap_err();
    assert!(
        matches!(err, MutateError::Io(_)),
        "Expected Io variant, got: {:?}",
        err
    );
}

#[test]
fn mutate_state_error_wraps_invalid_json_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{invalid").unwrap();
    let err = mutate_state(&path, &mut |_| {}).unwrap_err();
    assert!(
        matches!(err, MutateError::Json(_)),
        "Expected Json variant, got: {:?}",
        err
    );
}

// --- mutate_state_with_lock ---

/// Covers the `MutateError::Lock` arm by injecting a closure that
/// returns `Err(io::Error)` instead of `fs2::FileExt::lock_exclusive`.
#[test]
fn mutate_state_with_lock_error_wraps_as_lock_variant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, "{}").unwrap();
    let err = mutate_state_with_lock(&path, &mut |_| {}, &mut |_| {
        Err(std::io::Error::other("simulated lock failure"))
    })
    .unwrap_err();
    assert!(
        matches!(err, MutateError::Lock(ref m) if m.contains("simulated lock failure")),
        "Expected Lock variant with simulated message, got: {:?}",
        err
    );
}
