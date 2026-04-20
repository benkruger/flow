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
///
/// `transform_fn` is taken as `&mut dyn FnMut` so the function is
/// non-generic — all callers share a single monomorphization, keeping
/// per-file coverage measurable. Errors from the recoverable operations
/// (open, lock, read, JSON parse) become `MutateError`. The post-
/// transform write-back uses `.expect()` for seek/write/set_len because
/// those failures on an already-open, already-locked regular state file
/// under local storage are essentially unreachable (disk-full, EIO on
/// a <10KB file) — panicking with a clear message beats silently
/// returning `Ok` or adding an untestable Err branch.
pub fn mutate_state(
    state_path: &Path,
    transform_fn: &mut dyn FnMut(&mut Value),
) -> Result<Value, MutateError> {
    mutate_state_with_lock(state_path, transform_fn, &mut |f| f.lock_exclusive())
}

/// Test seam for `mutate_state` — accepts an injectable lock closure
/// so tests can simulate `lock_exclusive()` failures without triggering
/// real OS-level lock contention.
pub fn mutate_state_with_lock(
    state_path: &Path,
    transform_fn: &mut dyn FnMut(&mut Value),
    lock_fn: &mut dyn FnMut(&std::fs::File) -> std::io::Result<()>,
) -> Result<Value, MutateError> {
    let mut file = OpenOptions::new().read(true).write(true).open(state_path)?;

    lock_fn(&file).map_err(MutateError::lock)?;

    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let mut state: Value = serde_json::from_str(&content)?;

    transform_fn(&mut state);

    // Value -> JSON string is infallible for any well-formed Value.
    let output = serde_json::to_string_pretty(&state).expect("Value serializes");

    file.seek(SeekFrom::Start(0))
        .expect("seek to start on an open rw file");
    file.write_all(output.as_bytes())
        .expect("write to an already-open, already-locked rw state file");
    file.set_len(output.len() as u64)
        .expect("set_len on an already-open rw file");

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

impl MutateError {
    /// Construct the Lock variant from an `io::Error` returned by the
    /// injected lock closure. Exposed so `mutate_state_with_lock` can
    /// tag lock failures distinctly from other I/O failures without a
    /// per-callsite `map_err` closure.
    pub fn lock(e: std::io::Error) -> Self {
        MutateError::Lock(e.to_string())
    }
}

impl From<std::io::Error> for MutateError {
    fn from(e: std::io::Error) -> Self {
        MutateError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for MutateError {
    fn from(e: serde_json::Error) -> Self {
        MutateError::Json(e.to_string())
    }
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
