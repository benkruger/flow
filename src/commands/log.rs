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
/// Acquires an exclusive file lock before writing.
pub fn append_log(root: &Path, branch: &str, message: &str) -> Result<(), std::io::Error> {
    let paths = FlowPaths::new(root, branch);
    fs::create_dir_all(paths.flow_states_dir())?;
    let log_path = paths.log_file();
    let timestamp = utils::now();

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    file.lock_exclusive()?;
    let mut writer = std::io::BufWriter::new(&file);
    writeln!(writer, "{} {}", timestamp, message)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_to_existing_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join(".flow-states");
        fs::create_dir(&log_dir).unwrap();
        let log_file = log_dir.join("my-feature.log");
        fs::write(&log_file, "existing line\n").unwrap();

        append_log(dir.path(), "my-feature", "[Phase 1] Step 5 — test (exit 0)").unwrap();

        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.starts_with("existing line\n"));
        assert!(content.contains("[Phase 1] Step 5 — test (exit 0)"));
        // Should have exactly 2 lines
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn creates_new_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join(".flow-states");
        fs::create_dir(&log_dir).unwrap();

        append_log(dir.path(), "feat-branch", "[Phase 1] test message").unwrap();

        let log_file = log_dir.join("feat-branch.log");
        assert!(log_file.exists());
        let content = fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("[Phase 1] test message"));
    }

    #[test]
    fn creates_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();

        append_log(dir.path(), "branch", "message").unwrap();

        assert!(dir.path().join(".flow-states").is_dir());
        assert!(dir.path().join(".flow-states").join("branch.log").exists());
    }

    #[test]
    fn multiple_appends() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join(".flow-states");
        fs::create_dir(&log_dir).unwrap();

        append_log(dir.path(), "branch", "first").unwrap();
        append_log(dir.path(), "branch", "second").unwrap();

        let content = fs::read_to_string(log_dir.join("branch.log")).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("first"));
        assert!(lines[1].ends_with("second"));
    }

    #[test]
    fn run_impl_main_success_returns_empty_stderr_zero_code() {
        let dir = tempfile::tempdir().unwrap();
        let (msg, code) = run_impl_main(dir.path(), "branch", "message");
        assert_eq!(msg, "");
        assert_eq!(code, 0);
    }

    #[test]
    fn run_impl_main_failure_returns_stderr_one_code() {
        // Place a regular file at .flow-states/ so create_dir_all fails.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".flow-states"), "I am a file, not a dir").unwrap();
        let (msg, code) = run_impl_main(dir.path(), "branch", "message");
        assert_eq!(code, 1);
        assert!(msg.starts_with("flow log:"), "got: {}", msg);
    }

    #[test]
    fn timestamp_is_included() {
        let dir = tempfile::tempdir().unwrap();

        append_log(dir.path(), "branch", "test").unwrap();

        let content = fs::read_to_string(dir.path().join(".flow-states/branch.log")).unwrap();
        let line = content.trim();
        // Should have format: "YYYY-MM-DDTHH:MM:SS-HH:MM test"
        assert!(line.contains('T'), "Timestamp should contain 'T': {}", line);
        assert!(line.ends_with("test"));
    }
}
