//! Append timestamped log lines to `.flow-states/<branch>.log`.
//!
//! Tests live at tests/logging.rs per .claude/rules/test-placement.md —
//! no inline #[cfg(test)] in this file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process;

use fs2::FileExt;

use crate::flow_paths::FlowPaths;
use crate::git;
use crate::utils;

/// Append a timestamped message to `.flow-states/<branch>.log`.
///
/// Creates the `.flow-states/` directory if it does not exist.
/// Acquires an exclusive advisory lock before writing (best-effort —
/// on rare lock-acquisition failures, the O_APPEND open still
/// guarantees torn-write-free small appends on POSIX filesystems).
/// Write errors from `writeln!` are ignored for the same reason;
/// callers treat log failures as non-fatal.
pub fn append_log(root: &Path, branch: &str, message: &str) -> Result<(), std::io::Error> {
    let paths = FlowPaths::new(root, branch);
    fs::create_dir_all(paths.flow_states_dir())?;
    let log_path = paths.log_file();
    let timestamp = utils::now();

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let _ = file.lock_exclusive();
    let mut writer = std::io::BufWriter::new(&file);
    let _ = writeln!(writer, "{} {}", timestamp, message);

    // Lock released on drop
    Ok(())
}

/// Testable wrapper that returns an exit code instead of calling
/// `process::exit`. Returns `(stderr_message, exit_code)` — empty
/// stderr on success.
pub fn run_impl_main(root: &Path, branch: &str, message: &str) -> (String, i32) {
    match append_log(root, branch, message) {
        Ok(()) => (String::new(), 0),
        Err(e) => (format!("flow log: {}", e), 1),
    }
}

/// CLI entry point — exit 1 on error, no output on success.
pub fn run(branch: &str, message: &str) {
    let root = git::project_root();
    let (stderr_msg, code) = run_impl_main(&root, branch, message);
    if !stderr_msg.is_empty() {
        eprintln!("{}", stderr_msg);
    }
    if code != 0 {
        process::exit(code);
    }
}
