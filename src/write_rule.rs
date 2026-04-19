//! Write content to a target file path.
//!
//! Usage:
//!   bin/flow write-rule --path <target> --content-file <temp>
//!
//! Output (JSON to stdout):
//!   Success: {"status": "ok", "path": "<target_path>"}
//!   Error:   {"status": "error", "message": "..."}

use std::fs;
use std::path::Path;

use clap::Parser;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(name = "write-rule", about = "Write content to a target file")]
pub struct Args {
    /// Target file path
    #[arg(long)]
    pub path: String,
    /// Path to file containing content (file is deleted after reading)
    #[arg(long = "content-file")]
    pub content_file: String,
}

/// Read content from a file and delete it.
/// Returns Ok(content) or Err(message).
pub fn read_content_file(path: &str) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Could not read content file '{}': {}", path, e))?;

    // Delete the content file after reading, ignore errors
    let _ = fs::remove_file(path);

    Ok(content)
}

/// Write content to the target path, creating parent dirs as needed.
/// Returns Ok(()) or Err(message).
pub fn write_rule(target_path: &str, content: &str) -> Result<(), String> {
    let path = Path::new(target_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directories for '{}': {}", target_path, e))?;
    }

    fs::write(path, content).map_err(|e| format!("Could not write to '{}': {}", target_path, e))?;

    Ok(())
}

pub fn run_impl_main(args: &Args) -> (serde_json::Value, i32) {
    let content = match read_content_file(&args.content_file) {
        Ok(c) => c,
        Err(e) => return (json!({"status": "error", "message": e}), 1),
    };
    if let Err(e) = write_rule(&args.path, &content) {
        return (json!({"status": "error", "message": e}), 1);
    }
    (json!({"status": "ok", "path": &args.path}), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- read_content_file ---

    #[test]
    fn read_content_file_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let content_file = dir.path().join("content.md");
        fs::write(&content_file, "# My Rule\n\nDo the thing.\n").unwrap();

        let content = read_content_file(content_file.to_str().unwrap()).unwrap();
        assert_eq!(content, "# My Rule\n\nDo the thing.\n");
        assert!(!content_file.exists());
    }

    #[test]
    fn read_content_file_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nonexistent.md");

        let result = read_content_file(missing.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not read content file"));
    }

    // --- write_rule ---

    #[test]
    fn write_rule_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("rules").join("topic.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();

        let result = write_rule(target.to_str().unwrap(), "# Topic\n\nRule text.\n");
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "# Topic\n\nRule text.\n"
        );
    }

    #[test]
    fn write_rule_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir
            .path()
            .join("deep")
            .join("nested")
            .join("dir")
            .join("rule.md");

        let result = write_rule(target.to_str().unwrap(), "content");
        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(&target).unwrap(), "content");
    }

    #[test]
    fn write_rule_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("rule.md");
        fs::write(&target, "old content").unwrap();

        let result = write_rule(target.to_str().unwrap(), "new content");
        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
    }

    #[test]
    fn write_rule_write_error() {
        let dir = tempfile::tempdir().unwrap();
        let readonly = dir.path().join("readonly");
        fs::create_dir_all(&readonly).unwrap();

        // Make the directory read-only
        let mut perms = fs::metadata(&readonly).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&readonly, perms).unwrap();

        let target = readonly.join("rule.md");
        let result = write_rule(target.to_str().unwrap(), "content");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not write"));

        // Restore permissions for cleanup
        let mut perms = fs::metadata(&readonly).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&readonly, perms).unwrap();
    }

    #[test]
    fn write_rule_create_dir_error() {
        let dir = tempfile::tempdir().unwrap();
        // Place a regular file where the parent directory needs to be.
        // create_dir_all("blocker/rule.md"'s parent) fails because
        // "blocker" already exists as a file, not a directory.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "I am a file").unwrap();

        let target = blocker.join("nested").join("rule.md");
        let result = write_rule(target.to_str().unwrap(), "content");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not create directories"));
    }

    #[test]
    fn write_rule_empty_path_errors() {
        // Empty string path: parent() returns None so create_dir_all is
        // skipped, and fs::write on an empty path returns an OS error.
        let result = write_rule("", "content");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Could not write"));
    }

    // --- end-to-end ---

    #[test]
    fn end_to_end_write() {
        let dir = tempfile::tempdir().unwrap();
        let content_file = dir.path().join("content.md");
        fs::write(&content_file, "# Rule\n\nDo it.\n").unwrap();
        let target = dir.path().join(".claude").join("rules").join("topic.md");

        let content = read_content_file(content_file.to_str().unwrap()).unwrap();
        let result = write_rule(target.to_str().unwrap(), &content);

        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(&target).unwrap(), "# Rule\n\nDo it.\n");
        assert!(!content_file.exists());
    }
}
