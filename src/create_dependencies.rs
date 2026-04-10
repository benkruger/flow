//! Copy framework dependency template.
//!
//! Copies frameworks/<name>/dependencies to the target project's
//! bin/dependencies. Skips if bin/dependencies already exists (user
//! may have customized it). Makes the file executable.
//!
//! Usage:
//!   bin/flow create-dependencies <project_root> --framework <name>
//!
//! Output (JSON to stdout):
//!   {"status": "ok", "path": "bin/dependencies"}
//!   {"status": "skipped", "message": "bin/dependencies already exists"}
//!   {"status": "error", "message": "..."}

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use clap::Parser;
use serde_json::json;

use crate::utils::frameworks_dir;

#[derive(Parser, Debug)]
#[command(
    name = "create-dependencies",
    about = "Copy framework dependency template"
)]
pub struct Args {
    /// Project root directory
    pub project_root: String,

    /// Framework name
    #[arg(long)]
    pub framework: String,
}

/// Copy framework dependency template to bin/dependencies.
///
/// If `fw_dir` is provided, use it as the frameworks directory.
/// Otherwise, auto-detect from plugin root.
pub fn create(project_root: &str, framework: &str, fw_dir: Option<&Path>) -> serde_json::Value {
    let fw = match fw_dir {
        Some(d) => d.to_path_buf(),
        None => match frameworks_dir() {
            Some(d) => d,
            None => {
                return json!({
                    "status": "error",
                    "message": "Cannot find frameworks directory"
                });
            }
        },
    };

    let template_path = fw.join(framework).join("dependencies");
    if !template_path.exists() {
        return json!({
            "status": "error",
            "message": format!("Framework not found: {}", framework)
        });
    }

    let project = Path::new(project_root);
    let bin_dir = project.join("bin");
    let dependencies = bin_dir.join("dependencies");

    if dependencies.exists() {
        return json!({
            "status": "skipped",
            "message": "bin/dependencies already exists"
        });
    }

    if let Err(e) = fs::create_dir_all(&bin_dir) {
        return json!({
            "status": "error",
            "message": format!("Cannot create bin directory: {}", e)
        });
    }

    let content = match fs::read_to_string(&template_path) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "status": "error",
                "message": format!("Cannot read template: {}", e)
            });
        }
    };

    if let Err(e) = fs::write(&dependencies, &content) {
        return json!({
            "status": "error",
            "message": format!("Cannot write bin/dependencies: {}", e)
        });
    }

    // Set executable: 0o755
    if let Err(e) = fs::set_permissions(&dependencies, fs::Permissions::from_mode(0o755)) {
        return json!({
            "status": "error",
            "message": format!("Cannot set permissions: {}", e)
        });
    }

    json!({
        "status": "ok",
        "path": "bin/dependencies"
    })
}

pub fn run(args: Args) {
    let result = create(&args.project_root, &args.framework, None);

    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("error");
    println!("{}", result);

    if status == "error" {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn frameworks_test_dir() -> std::path::PathBuf {
        // Use the real frameworks directory in this repo
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        std::path::PathBuf::from(manifest).join("frameworks")
    }

    #[test]
    fn args_parse_all_required() {
        let args = Args::try_parse_from([
            "create-dependencies",
            "/tmp/project",
            "--framework",
            "rails",
        ]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.project_root, "/tmp/project");
        assert_eq!(args.framework, "rails");
    }

    #[test]
    fn args_missing_project_root_fails() {
        let args = Args::try_parse_from(["create-dependencies", "--framework", "rails"]);
        assert!(args.is_err());
    }

    #[test]
    fn args_missing_framework_fails() {
        let args = Args::try_parse_from(["create-dependencies", "/tmp/project"]);
        assert!(args.is_err());
    }

    #[test]
    fn creates_bin_dependencies_from_template() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        let result = create(dir.path().to_str().unwrap(), "rails", Some(&fw));
        assert_eq!(result["status"], "ok");
        let deps = dir.path().join("bin").join("dependencies");
        assert!(deps.exists());
        let content = fs::read_to_string(&deps).unwrap();
        assert!(content.contains("bundle update"));
    }

    #[test]
    fn created_file_is_executable() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        create(dir.path().to_str().unwrap(), "rails", Some(&fw));
        let deps = dir.path().join("bin").join("dependencies");
        let perms = fs::metadata(&deps).unwrap().permissions();
        assert!(perms.mode() & 0o100 != 0);
    }

    #[test]
    fn skips_if_bin_dependencies_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(
            bin_dir.join("dependencies"),
            "#!/usr/bin/env bash\n# custom\n",
        )
        .unwrap();
        let fw = frameworks_test_dir();
        let result = create(dir.path().to_str().unwrap(), "rails", Some(&fw));
        assert_eq!(result["status"], "skipped");
        // Original content preserved
        let content = fs::read_to_string(bin_dir.join("dependencies")).unwrap();
        assert!(content.contains("# custom"));
    }

    #[test]
    fn creates_bin_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        create(dir.path().to_str().unwrap(), "python", Some(&fw));
        assert!(dir.path().join("bin").join("dependencies").exists());
    }

    #[test]
    fn error_when_invalid_framework() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        let result = create(dir.path().to_str().unwrap(), "nonexistent", Some(&fw));
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn python_template_content() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        create(dir.path().to_str().unwrap(), "python", Some(&fw));
        let content = fs::read_to_string(dir.path().join("bin").join("dependencies")).unwrap();
        assert!(content.contains(".venv/bin/pip"));
    }

    #[test]
    fn ios_template_content() {
        let dir = tempfile::tempdir().unwrap();
        let fw = frameworks_test_dir();
        create(dir.path().to_str().unwrap(), "ios", Some(&fw));
        let content = fs::read_to_string(dir.path().join("bin").join("dependencies")).unwrap();
        assert!(content.contains("Package.swift"));
    }
}
