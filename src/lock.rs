use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use fs2::FileExt;
use serde_json::Value;

/// Atomic read-lock-transform-write for state files.
///
/// Opens the file, acquires an exclusive advisory lock, reads and parses
/// JSON, calls transform_fn to mutate the value, then writes back and
/// releases the lock (on drop).
///
/// Returns the final (mutated) state Value.
pub fn mutate_state<F>(state_path: &Path, transform_fn: F) -> Result<Value, MutateError>
where
    F: FnOnce(&mut Value),
{
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_path)
        .map_err(|e| MutateError::Io(format!("{}: {}", state_path.display(), e)))?;

    file.lock_exclusive()
        .map_err(|e| MutateError::Lock(format!("Failed to lock {}: {}", state_path.display(), e)))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| MutateError::Io(format!("Failed to read {}: {}", state_path.display(), e)))?;

    let mut state: Value = serde_json::from_str(&content)
        .map_err(|e| MutateError::Json(format!("Invalid JSON in {}: {}", state_path.display(), e)))?;

    transform_fn(&mut state);

    let output = serde_json::to_string_pretty(&state)
        .map_err(|e| MutateError::Json(format!("Failed to serialize: {}", e)))?;

    file.seek(SeekFrom::Start(0))
        .map_err(|e| MutateError::Io(format!("Failed to seek: {}", e)))?;

    file.write_all(output.as_bytes())
        .map_err(|e| MutateError::Io(format!("Failed to write: {}", e)))?;

    file.set_len(output.len() as u64)
        .map_err(|e| MutateError::Io(format!("Failed to truncate: {}", e)))?;

    // Lock released on drop
    Ok(state)
}

/// Errors from mutate_state.
#[derive(Debug)]
pub enum MutateError {
    Io(String),
    Lock(String),
    Json(String),
}

impl std::fmt::Display for MutateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MutateError::Io(s) => write!(f, "I/O error: {}", s),
            MutateError::Lock(s) => write!(f, "Lock error: {}", s),
            MutateError::Json(s) => write!(f, "JSON error: {}", s),
        }
    }
}

impl std::error::Error for MutateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn mutate_state_basic_transform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"count": 0}"#).unwrap();

        let result = mutate_state(&path, |state| {
            state["count"] = json!(1);
        })
        .unwrap();

        assert_eq!(result["count"], 1);

        // Verify file was updated on disk
        let content = fs::read_to_string(&path).unwrap();
        let on_disk: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(on_disk["count"], 1);
    }

    #[test]
    fn mutate_state_adds_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"branch": "test"}"#).unwrap();

        let result = mutate_state(&path, |state| {
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

        mutate_state(&path, |state| {
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
        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn mutate_state_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "{corrupt").unwrap();
        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn mutate_state_truncates_when_shorter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Write a long initial value
        fs::write(
            &path,
            r#"{"long_key": "this is a very long value that takes up space"}"#,
        )
        .unwrap();
        let initial_len = fs::metadata(&path).unwrap().len();

        mutate_state(&path, |state| {
            state["long_key"] = json!("short");
        })
        .unwrap();

        let final_len = fs::metadata(&path).unwrap().len();
        assert!(final_len < initial_len);

        // Must still be valid JSON
        let content = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["long_key"], "short");
    }

    #[test]
    fn mutate_state_transform_receives_current_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"value": 42}"#).unwrap();

        let mut captured = 0i64;
        mutate_state(&path, |state| {
            captured = state["value"].as_i64().unwrap();
            state["value"] = json!(captured + 1);
        })
        .unwrap();

        assert_eq!(captured, 42);
    }
}
