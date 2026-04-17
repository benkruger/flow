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
    mutate_state_with_lock(state_path, transform_fn, |f| f.lock_exclusive())
}

/// Test seam for `mutate_state` — accepts an injectable lock closure so
/// unit tests can simulate `lock_exclusive()` failures without triggering
/// real OS-level lock contention. The public wrapper above supplies
/// `fs2::FileExt::lock_exclusive`; the inline `Lock` error test in
/// `#[cfg(test)] mod tests` passes a closure that returns `Err` so the
/// `MutateError::Lock` arm is exercised. No production caller uses this
/// function directly.
fn mutate_state_with_lock<F, L>(
    state_path: &Path,
    transform_fn: F,
    lock_fn: L,
) -> Result<Value, MutateError>
where
    F: FnOnce(&mut Value),
    L: FnOnce(&std::fs::File) -> std::io::Result<()>,
{
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_path)
        .map_err(|e| MutateError::Io(format!("{}: {}", state_path.display(), e)))?;

    lock_fn(&file).map_err(|e| {
        MutateError::Lock(format!("Failed to lock {}: {}", state_path.display(), e))
    })?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| MutateError::Io(format!("Failed to read {}: {}", state_path.display(), e)))?;

    let mut state: Value = serde_json::from_str(&content).map_err(|e| {
        MutateError::Json(format!("Invalid JSON in {}: {}", state_path.display(), e))
    })?;

    transform_fn(&mut state);

    let output = serde_json::to_string_pretty(&state)
        .map_err(|e| MutateError::Json(format!("Failed to serialize: {}", e)))?;

    write_and_truncate(&mut file, output.as_bytes())?;

    // Lock released on drop
    Ok(state)
}

/// Seek to start, write data, and truncate to the written length.
///
/// Encapsulates the three I/O operations that follow JSON serialization
/// in `mutate_state`. Extracted so tests can exercise the error arms
/// (write failure on a read-only fd, truncate failure) without needing
/// a full mutate_state round-trip.
fn write_and_truncate(file: &mut std::fs::File, data: &[u8]) -> Result<(), MutateError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|e| MutateError::Io(format!("Failed to seek: {}", e)))?;

    file.write_all(data)
        .map_err(|e| MutateError::Io(format!("Failed to write: {}", e)))?;

    file.set_len(data.len() as u64)
        .map_err(|e| MutateError::Io(format!("Failed to truncate: {}", e)))?;

    Ok(())
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
    fn mutate_state_array_root_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let content = "[1, 2, 3]";
        fs::write(&path, content).unwrap();
        // Array root is valid JSON but mutate_state should still parse it.
        // The transform may not do anything useful, but it should not panic.
        let result = mutate_state(&path, |_state| {});
        assert!(result.is_ok());
        // File content is rewritten (pretty-printed array)
        let after = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&after).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn mutate_state_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "").unwrap();
        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
        // File must be unchanged (still empty)
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, "");
    }

    #[test]
    fn mutate_state_non_json_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let content = "hello world";
        fs::write(&path, content).unwrap();
        let result = mutate_state(&path, |_| {});
        assert!(result.is_err());
        // File must be unchanged
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, content);
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
    fn mutate_state_preserves_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Keys in non-alphabetical order
        fs::write(&path, r#"{"zebra": 1, "apple": 2, "mango": 3}"#).unwrap();

        mutate_state(&path, |state| {
            state["mango"] = json!(99);
        })
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // Key order must match original: zebra, apple, mango
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
        mutate_state(&path, |state| {
            captured = state["value"].as_i64().unwrap();
            state["value"] = json!(captured + 1);
        })
        .unwrap();

        assert_eq!(captured, 42);
    }

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
        // Ensures MutateError implements std::error::Error trait.
        let err: Box<dyn std::error::Error> = Box::new(MutateError::Io("test".to_string()));
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn mutate_state_error_wraps_missing_file_as_io() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let err = mutate_state(&path, |_| {}).unwrap_err();
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
        let err = mutate_state(&path, |_| {}).unwrap_err();
        assert!(
            matches!(err, MutateError::Json(_)),
            "Expected Json variant, got: {:?}",
            err
        );
    }

    /// Covers the `MutateError::Lock` arm (lines 26-27 of the
    /// pre-refactor `mutate_state` body, now lines 35-37 of
    /// `mutate_state_with_lock`). The lock failure is simulated by
    /// passing a closure that returns `Err(io::Error)` in place of
    /// `fs2::FileExt::lock_exclusive`. No OS-level lock manipulation
    /// is needed — the seam lets the test inject the failure
    /// deterministically.
    #[test]
    fn mutate_state_with_lock_error_wraps_as_lock_variant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "{}").unwrap();
        let err = mutate_state_with_lock(
            &path,
            |_| {},
            |_| Err(std::io::Error::other("simulated lock failure")),
        )
        .unwrap_err();
        assert!(
            matches!(err, MutateError::Lock(ref m) if m.contains("simulated lock failure")),
            "Expected Lock variant with simulated message, got: {:?}",
            err
        );
    }

    // --- write_and_truncate ---

    #[test]
    fn write_and_truncate_success() {
        // Use longer initial content so the replacement is shorter and
        // set_len actually truncates (exercises the truncation path).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.json");
        fs::write(&path, "much longer initial content that will be truncated").unwrap();
        let initial_len = fs::metadata(&path).unwrap().len();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let data = b"short";
        write_and_truncate(&mut file, data).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, "short");
        let final_len = fs::metadata(&path).unwrap().len();
        assert!(
            final_len < initial_len,
            "file should be truncated: initial={}, final={}",
            initial_len,
            final_len
        );
    }

    #[test]
    fn write_and_truncate_readonly_fd_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readonly.json");
        fs::write(&path, "content").unwrap();
        // Open read-only — write_all will fail with a permission error.
        let mut file = OpenOptions::new().read(true).open(&path).unwrap();
        let err = write_and_truncate(&mut file, b"new data").unwrap_err();
        let msg = match &err {
            MutateError::Io(m) => m.clone(),
            _ => String::new(),
        };
        assert!(
            matches!(err, MutateError::Io(_)),
            "Expected Io variant, got: {:?}",
            err
        );
        assert!(
            msg.contains("Failed to write") || msg.contains("Failed to seek"),
            "expected write or seek failure, got: {}",
            msg
        );
    }
}
