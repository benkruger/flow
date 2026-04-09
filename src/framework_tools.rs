//! Framework-aware tool command mapping.
//!
//! Maps `(framework, tool_type)` pairs to concrete shell commands.
//! Each subcommand module (`build`, `test_runner`, `lint`, `format_check`)
//! calls [`tool_command`] to get the command for the detected framework,
//! then spawns it with inherited stdio.
//!
//! No-op tools (e.g. `build` for Python) return `None`, signaling the
//! caller to skip with a `{"status": "skipped"}` JSON response.

use std::path::Path;

/// The four tool types that `bin/flow` can dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    Build,
    Test,
    Lint,
    Format,
}

/// A resolved tool command: the program to run and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Return the tool command for a `(framework, tool_type)` pair.
///
/// Returns `Ok(Some(cmd))` when the framework has a tool for this type,
/// `Ok(None)` when the operation is a no-op for this framework, or
/// `Err(msg)` when the framework is unknown.
pub fn tool_command(framework: &str, tool_type: ToolType) -> Result<Option<ToolCommand>, String> {
    match framework {
        "rust" => Ok(rust_tool(tool_type)),
        "python" => Ok(python_tool(tool_type)),
        "rails" => Ok(rails_tool(tool_type)),
        "go" => Ok(go_tool(tool_type)),
        "ios" => Ok(ios_tool(tool_type)),
        _ => Err(format!("Unknown framework: {}", framework)),
    }
}

fn rust_tool(tool_type: ToolType) -> Option<ToolCommand> {
    match tool_type {
        ToolType::Build => Some(ToolCommand {
            program: "cargo".into(),
            args: vec!["build".into(), "--quiet".into()],
        }),
        ToolType::Test => Some(ToolCommand {
            program: "cargo".into(),
            args: vec![
                "nextest".into(),
                "run".into(),
                "--status-level".into(),
                "none".into(),
                "--final-status-level".into(),
                "fail".into(),
            ],
        }),
        ToolType::Lint => Some(ToolCommand {
            program: "cargo".into(),
            args: vec![
                "clippy".into(),
                "--all-targets".into(),
                "--quiet".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ],
        }),
        ToolType::Format => Some(ToolCommand {
            program: "cargo".into(),
            args: vec!["fmt".into(), "--check".into()],
        }),
    }
}

fn python_tool(tool_type: ToolType) -> Option<ToolCommand> {
    match tool_type {
        ToolType::Build => None,
        ToolType::Test => Some(ToolCommand {
            program: "python3".into(),
            args: vec!["-m".into(), "pytest".into(), "tests/".into(), "-v".into()],
        }),
        ToolType::Lint => Some(ToolCommand {
            program: ".venv/bin/ruff".into(),
            args: vec!["check".into()],
        }),
        ToolType::Format => Some(ToolCommand {
            program: ".venv/bin/ruff".into(),
            args: vec!["format".into(), "--check".into()],
        }),
    }
}

fn rails_tool(tool_type: ToolType) -> Option<ToolCommand> {
    match tool_type {
        ToolType::Build => None,
        ToolType::Test => Some(ToolCommand {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "bundle exec ruby -Ilib -Itest test/*_test.rb".into(),
            ],
        }),
        ToolType::Lint => Some(ToolCommand {
            program: "rubocop".into(),
            args: vec!["-A".into()],
        }),
        ToolType::Format => None,
    }
}

fn go_tool(tool_type: ToolType) -> Option<ToolCommand> {
    match tool_type {
        ToolType::Build => Some(ToolCommand {
            program: "go".into(),
            args: vec!["build".into(), "./...".into()],
        }),
        ToolType::Test => Some(ToolCommand {
            program: "go".into(),
            args: vec!["test".into(), "./...".into(), "-v".into()],
        }),
        ToolType::Lint => Some(ToolCommand {
            program: "go".into(),
            args: vec!["vet".into(), "./...".into()],
        }),
        ToolType::Format => Some(ToolCommand {
            program: "go".into(),
            args: vec!["fmt".into(), "./...".into()],
        }),
    }
}

/// iOS build and test require project-specific args (scheme, destination)
/// that vary per project. Return None until project-level configuration
/// is implemented.
fn ios_tool(tool_type: ToolType) -> Option<ToolCommand> {
    match tool_type {
        ToolType::Build => None,
        ToolType::Test => None,
        ToolType::Lint => None,
        ToolType::Format => None,
    }
}

/// Detect the framework for the current project.
///
/// Tries the state file first (fast path — works inside FLOW phases),
/// then falls back to `detect_framework` (works outside phases).
///
/// Returns `Err` if no framework can be determined.
pub fn detect_framework_for_project(
    cwd: &Path,
    root: &Path,
    branch: Option<&str>,
) -> Result<String, String> {
    // Fast path: read from state file
    if let Some(branch) = branch {
        let state_path = root.join(".flow-states").join(format!("{}.json", branch));
        if state_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&state_path) {
                if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(fw) = state
                        .get("framework")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        return Ok(fw.to_string());
                    }
                }
            }
        }
    }

    // Fallback: detect from project files
    let fw_dir = crate::utils::frameworks_dir()
        .ok_or_else(|| "Plugin root not found — cannot detect framework".to_string())?;
    let detected = crate::detect_framework::detect(cwd, &fw_dir);
    if detected.is_empty() {
        return Err("No framework detected. Ensure the project has framework marker files (Cargo.toml, requirements.txt, Gemfile, go.mod, *.xcodeproj).".to_string());
    }
    detected[0]
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Framework detected but has no name".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tool_command() mapping tests ---

    #[test]
    fn rust_build_returns_cargo_build() {
        let cmd = tool_command("rust", ToolType::Build).unwrap().unwrap();
        assert_eq!(cmd.program, "cargo");
        assert_eq!(cmd.args[0], "build");
        assert!(cmd.args.contains(&"--quiet".to_string()));
    }

    #[test]
    fn rust_test_returns_cargo_nextest() {
        let cmd = tool_command("rust", ToolType::Test).unwrap().unwrap();
        assert_eq!(cmd.program, "cargo");
        assert_eq!(cmd.args[0], "nextest");
        assert_eq!(cmd.args[1], "run");
    }

    #[test]
    fn rust_lint_returns_clippy_all_targets() {
        let cmd = tool_command("rust", ToolType::Lint).unwrap().unwrap();
        assert_eq!(cmd.program, "cargo");
        assert_eq!(cmd.args[0], "clippy");
        assert!(cmd.args.contains(&"--all-targets".to_string()));
        assert!(cmd.args.contains(&"-D".to_string()));
        assert!(cmd.args.contains(&"warnings".to_string()));
    }

    #[test]
    fn rust_format_returns_cargo_fmt_check() {
        let cmd = tool_command("rust", ToolType::Format).unwrap().unwrap();
        assert_eq!(cmd.program, "cargo");
        assert_eq!(cmd.args[0], "fmt");
        assert!(cmd.args.contains(&"--check".to_string()));
    }

    #[test]
    fn python_build_is_noop() {
        assert!(tool_command("python", ToolType::Build).unwrap().is_none());
    }

    #[test]
    fn python_test_returns_pytest() {
        let cmd = tool_command("python", ToolType::Test).unwrap().unwrap();
        assert_eq!(cmd.program, "python3");
        assert!(cmd.args.contains(&"pytest".to_string()));
    }

    #[test]
    fn python_lint_returns_ruff_check() {
        let cmd = tool_command("python", ToolType::Lint).unwrap().unwrap();
        assert_eq!(cmd.program, ".venv/bin/ruff");
        assert_eq!(cmd.args[0], "check");
    }

    #[test]
    fn python_format_returns_ruff_format_check() {
        let cmd = tool_command("python", ToolType::Format).unwrap().unwrap();
        assert_eq!(cmd.program, ".venv/bin/ruff");
        assert!(cmd.args.contains(&"format".to_string()));
        assert!(cmd.args.contains(&"--check".to_string()));
    }

    #[test]
    fn rails_build_is_noop() {
        assert!(tool_command("rails", ToolType::Build).unwrap().is_none());
    }

    #[test]
    fn rails_test_returns_shell_with_bundle() {
        let cmd = tool_command("rails", ToolType::Test).unwrap().unwrap();
        assert_eq!(cmd.program, "sh");
        assert_eq!(cmd.args[0], "-c");
        assert!(cmd.args[1].contains("bundle exec ruby"));
        assert!(cmd.args[1].contains("test/*_test.rb"));
    }

    #[test]
    fn rails_lint_returns_rubocop() {
        let cmd = tool_command("rails", ToolType::Lint).unwrap().unwrap();
        assert_eq!(cmd.program, "rubocop");
        assert_eq!(cmd.args[0], "-A");
    }

    #[test]
    fn rails_format_is_noop() {
        assert!(tool_command("rails", ToolType::Format).unwrap().is_none());
    }

    #[test]
    fn go_build_returns_go_build() {
        let cmd = tool_command("go", ToolType::Build).unwrap().unwrap();
        assert_eq!(cmd.program, "go");
        assert_eq!(cmd.args[0], "build");
        assert!(cmd.args.contains(&"./...".to_string()));
    }

    #[test]
    fn go_test_returns_go_test() {
        let cmd = tool_command("go", ToolType::Test).unwrap().unwrap();
        assert_eq!(cmd.program, "go");
        assert_eq!(cmd.args[0], "test");
    }

    #[test]
    fn go_lint_returns_go_vet() {
        let cmd = tool_command("go", ToolType::Lint).unwrap().unwrap();
        assert_eq!(cmd.program, "go");
        assert_eq!(cmd.args[0], "vet");
    }

    #[test]
    fn go_format_returns_go_fmt() {
        let cmd = tool_command("go", ToolType::Format).unwrap().unwrap();
        assert_eq!(cmd.program, "go");
        assert_eq!(cmd.args[0], "fmt");
    }

    #[test]
    fn ios_build_is_noop() {
        assert!(tool_command("ios", ToolType::Build).unwrap().is_none());
    }

    #[test]
    fn ios_test_is_noop() {
        assert!(tool_command("ios", ToolType::Test).unwrap().is_none());
    }

    #[test]
    fn ios_lint_is_noop() {
        assert!(tool_command("ios", ToolType::Lint).unwrap().is_none());
    }

    #[test]
    fn ios_format_is_noop() {
        assert!(tool_command("ios", ToolType::Format).unwrap().is_none());
    }

    #[test]
    fn unknown_framework_returns_error() {
        let result = tool_command("cobol", ToolType::Build);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown framework"));
    }

    // --- no-op summary tests ---

    #[test]
    fn all_noops_are_correct() {
        // Exhaustive: verify the exact set of no-ops matches the plan
        let noops = [
            ("python", ToolType::Build),
            ("rails", ToolType::Build),
            ("rails", ToolType::Format),
            ("ios", ToolType::Build),
            ("ios", ToolType::Test),
            ("ios", ToolType::Lint),
            ("ios", ToolType::Format),
        ];
        for (fw, tt) in &noops {
            assert!(
                tool_command(fw, *tt).unwrap().is_none(),
                "{} {:?} should be a no-op",
                fw,
                tt
            );
        }
    }

    #[test]
    fn all_non_noops_return_some() {
        let frameworks = ["rust", "python", "rails", "go", "ios"];
        let tool_types = [
            ToolType::Build,
            ToolType::Test,
            ToolType::Lint,
            ToolType::Format,
        ];
        let noops = [
            ("python", ToolType::Build),
            ("rails", ToolType::Build),
            ("rails", ToolType::Format),
            ("ios", ToolType::Build),
            ("ios", ToolType::Test),
            ("ios", ToolType::Lint),
            ("ios", ToolType::Format),
        ];
        for fw in &frameworks {
            for tt in &tool_types {
                let is_noop = noops.iter().any(|(f, t)| f == fw && t == tt);
                let result = tool_command(fw, *tt).unwrap();
                if is_noop {
                    assert!(result.is_none(), "{} {:?} should be None", fw, tt);
                } else {
                    assert!(result.is_some(), "{} {:?} should be Some", fw, tt);
                }
            }
        }
    }
}
