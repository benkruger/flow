//! Library-level tests for `flow_rs::ci`. Migrated from inline
//! `#[cfg(test)]` per `.claude/rules/test-placement.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use flow_rs::ci::{
    any_tool_is_stub, bin_tool_sequence, delete_profraws_recursive, eprint_summary, format_elapsed,
    program_stdout, run_clean, run_impl, run_once, run_with_retry, sentinel_path, tree_snapshot,
    write_or_remove_sentinel, Args, CiTool, STUB_MARKER,
};

fn init_git_repo(dir: &Path, initial_branch: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {:?} failed", args);
    };
    run(&["init", "--initial-branch", initial_branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
}

#[test]
fn format_elapsed_under_one_second_uses_ms() {
    assert_eq!(format_elapsed(0), "0ms");
    assert_eq!(format_elapsed(999), "999ms");
}

#[test]
fn format_elapsed_under_one_minute_uses_fractional_seconds() {
    assert_eq!(format_elapsed(1_000), "1.0s");
    assert_eq!(format_elapsed(38_600), "38.6s");
    assert_eq!(format_elapsed(59_999), "60.0s");
}

#[test]
fn format_elapsed_one_minute_and_above_uses_minutes_seconds() {
    assert_eq!(format_elapsed(60_000), "1m0s");
    assert_eq!(format_elapsed(125_000), "2m5s");
    assert_eq!(format_elapsed(3_605_000), "60m5s");
}

#[test]
fn eprint_summary_empty_phases_is_noop() {
    eprint_summary(&[], 0);
}

#[test]
fn tree_snapshot_empty_repo_returns_64_char_hex() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let hash = tree_snapshot(dir.path(), None);
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(hash.chars().all(|c| !c.is_ascii_uppercase()));
}

#[test]
fn tree_snapshot_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let a = tree_snapshot(dir.path(), None);
    let b = tree_snapshot(dir.path(), None);
    assert_eq!(a, b);
}

#[test]
fn tree_snapshot_differs_on_tracked_edit() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    fs::write(dir.path().join("app.py"), "version = 1\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add app"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let baseline = tree_snapshot(dir.path(), None);
    fs::write(dir.path().join("app.py"), "version = 2\n").unwrap();
    let after = tree_snapshot(dir.path(), None);
    assert_ne!(baseline, after);
}

#[test]
fn tree_snapshot_differs_on_untracked_add() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let baseline = tree_snapshot(dir.path(), None);
    fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
    let after = tree_snapshot(dir.path(), None);
    assert_ne!(baseline, after);
}

#[test]
fn tree_snapshot_untracked_content_edit_changes_hash() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    fs::write(dir.path().join("notes.txt"), "draft 1\n").unwrap();
    let first = tree_snapshot(dir.path(), None);
    fs::write(dir.path().join("notes.txt"), "draft 2\n").unwrap();
    let second = tree_snapshot(dir.path(), None);
    assert_ne!(first, second);
}

#[test]
fn tree_snapshot_untracked_rename_changes_hash() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    fs::write(dir.path().join("old.txt"), "content\n").unwrap();
    let first = tree_snapshot(dir.path(), None);
    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
    let second = tree_snapshot(dir.path(), None);
    assert_ne!(first, second);
}

#[test]
fn write_or_remove_sentinel_removes_on_any_stub() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinel");
    fs::write(&path, "old").unwrap();
    write_or_remove_sentinel(&path, "new", true);
    assert!(!path.exists());
}

#[test]
fn write_or_remove_sentinel_writes_on_not_stub() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("subdir").join("sentinel");
    write_or_remove_sentinel(&path, "snapshot", false);
    assert_eq!(fs::read_to_string(&path).unwrap(), "snapshot");
}

#[test]
fn write_or_remove_sentinel_handles_parentless_path() {
    let empty = Path::new("");
    write_or_remove_sentinel(empty, "snap", false);
}

#[test]
fn program_stdout_missing_binary_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        program_stdout(dir.path(), "/no/such/program-deadbeef", &[]),
        ""
    );
}

#[test]
fn tree_snapshot_simulate_branch_changes_hash() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let plain = tree_snapshot(dir.path(), None);
    let simulated = tree_snapshot(dir.path(), Some("other-branch"));
    assert_ne!(plain, simulated);
}

#[test]
fn tree_snapshot_simulate_branch_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let a = tree_snapshot(dir.path(), Some("feature-x"));
    let b = tree_snapshot(dir.path(), Some("feature-x"));
    assert_eq!(a, b);
}

#[test]
fn tree_snapshot_different_simulate_values_differ() {
    let dir = tempfile::tempdir().unwrap();
    init_git_repo(dir.path(), "main");
    let a = tree_snapshot(dir.path(), Some("branch-a"));
    let b = tree_snapshot(dir.path(), Some("branch-b"));
    assert_ne!(a, b);
}

#[test]
fn tree_snapshot_non_git_dir_returns_stable_hash() {
    let dir = tempfile::tempdir().unwrap();
    let a = tree_snapshot(dir.path(), None);
    let b = tree_snapshot(dir.path(), None);
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
}

fn write_script(path: &Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct CiFixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    branch: String,
}

fn make_ci_fixture() -> CiFixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    init_git_repo(&path, "main");

    let exclude_file = path.join(".git").join("info").join("exclude");
    fs::create_dir_all(exclude_file.parent().unwrap()).unwrap();
    fs::write(&exclude_file, ".flow-states/\n").unwrap();

    CiFixture {
        _dir: dir,
        path,
        branch: "main".to_string(),
    }
}

fn single_tool(script_path: &Path) -> Vec<CiTool> {
    vec![CiTool {
        name: "test".to_string(),
        program: script_path.to_string_lossy().to_string(),
        args: vec![],
    }]
}

fn fixture_sentinel(f: &CiFixture) -> PathBuf {
    sentinel_path(&f.path, &f.branch)
}

#[test]
fn bin_tool_sequence_empty_when_no_scripts() {
    let f = make_ci_fixture();
    let tools = bin_tool_sequence(&f.path);
    assert!(tools.is_empty());
}

#[test]
fn bin_tool_sequence_picks_up_present_scripts() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    write_script(
        &f.path.join("bin").join("test"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let tools = bin_tool_sequence(&f.path);
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "format");
    assert_eq!(tools[1].name, "test");
}

#[test]
fn bin_tool_sequence_preserves_order() {
    let f = make_ci_fixture();
    for name in ["test", "build", "lint", "format"] {
        write_script(
            &f.path.join("bin").join(name),
            "#!/usr/bin/env bash\nexit 0\n",
        );
    }
    let tools = bin_tool_sequence(&f.path);
    assert_eq!(tools.len(), 4);
    assert_eq!(tools[0].name, "format");
    assert_eq!(tools[1].name, "lint");
    assert_eq!(tools[2].name, "build");
    assert_eq!(tools[3].name, "test");
}

#[test]
fn bin_tool_sequence_skips_directories() {
    let f = make_ci_fixture();
    fs::create_dir_all(f.path.join("bin").join("format")).unwrap();
    write_script(
        &f.path.join("bin").join("test"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let tools = bin_tool_sequence(&f.path);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test");
}

#[test]
fn run_once_runs_tools_and_creates_sentinel() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["skipped"], false);
    assert!(fixture_sentinel(&f).exists());
}

#[test]
fn run_once_propagates_rebuild_and_simulate_branch_env() {
    let f = make_ci_fixture();
    let marker = f.path.join("env-marker");
    let script = f.path.join("env-probe.sh");
    write_script(
        &script,
        &format!(
            "#!/usr/bin/env bash\nprintf 'rebuild=%s sim=%s\\n' \"${{FLOW_CI_REBUILD:-}}\" \"${{FLOW_SIMULATE_BRANCH:-}}\" > {}\nexit 0\n",
            marker.display()
        ),
    );
    let tools = single_tool(&script);

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        true,
        Some("simulated-feature"),
        true,
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");

    let env_dump = std::fs::read_to_string(&marker).unwrap();
    assert!(env_dump.contains("rebuild=1"));
    assert!(env_dump.contains("sim=simulated-feature"));
}

#[test]
fn run_once_skips_when_sentinel_and_clean() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let (first, _) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(first["skipped"], false);

    let (second, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(second["skipped"], true);
}

#[test]
fn run_once_sentinel_different_content_falls_through() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "stale-content-that-wont-match").unwrap();

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_once_sentinel_unreadable_falls_through() {
    use std::os::unix::fs::PermissionsExt;
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "anything").unwrap();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o000)).unwrap();

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code, 0);
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_once_failure_removes_sentinel() {
    let f = make_ci_fixture();
    let pass = f.path.join("pass.sh");
    write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&pass);

    let _ = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert!(fixture_sentinel(&f).exists());

    let fail = f.path.join("fail.sh");
    write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
    let fail_tools = single_tool(&fail);

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &fail_tools,
        Some(&f.branch),
        true,
        None,
        false,
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_once_force_bypasses_sentinel() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let (first, _) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(first["skipped"], false);

    let (second, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), true, None, false);
    assert_eq!(code, 0);
    assert_eq!(second["skipped"], false);
}

#[test]
fn run_once_stops_on_first_tool_failure() {
    let f = make_ci_fixture();
    let fail = f.path.join("fail.sh");
    write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
    let pass = f.path.join("pass.sh");
    write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");

    let marker = f.path.join("second-ran");
    let mark_script = f.path.join("mark.sh");
    write_script(
        &mark_script,
        &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker.display()),
    );

    let tools = vec![
        CiTool {
            name: "format".to_string(),
            program: fail.to_string_lossy().to_string(),
            args: vec![],
        },
        CiTool {
            name: "test".to_string(),
            program: mark_script.to_string_lossy().to_string(),
            args: vec![],
        },
    ];

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("format"));
    assert!(!marker.exists());
}

#[test]
fn run_once_empty_tools_errors() {
    let f = make_ci_fixture();
    let (out, code) = run_once(&f.path, &f.path, &[], Some(&f.branch), false, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("No ./bin/{format,lint,build,test} scripts"));
}

#[test]
fn run_with_retry_empty_tools_errors() {
    let f = make_ci_fixture();
    let (out, code) = run_with_retry(&f.path, &f.path, &[], Some(&f.branch), 3, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out.get("skipped").is_none());
    assert!(out.get("attempts").is_none());
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_once_stub_script_suppresses_sentinel() {
    let f = make_ci_fixture();
    let script = f.path.join("stub.sh");
    write_script(
        &script,
        "#!/usr/bin/env bash\n# FLOW-STUB-UNCONFIGURED (remove this line)\necho 'stub' >&2\nexit 0\n",
    );
    let tools = single_tool(&script);

    let (out, code) = run_once(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        false,
        None,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["stubs_detected"], true);
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_with_retry_stub_script_suppresses_sentinel() {
    let f = make_ci_fixture();
    let script = f.path.join("stub.sh");
    write_script(
        &script,
        "#!/usr/bin/env bash\n# FLOW-STUB-UNCONFIGURED (remove this line)\necho 'stub' >&2\nexit 0\n",
    );
    let tools = single_tool(&script);

    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["stubs_detected"], true);
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_once_detached_head_no_sentinel() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let flow_states = f.path.join(".flow-states");
    fs::create_dir_all(&flow_states).unwrap();
    fs::write(flow_states.join("unrelated-marker.txt"), "x").unwrap();

    let (out, code) = run_once(&f.path, &f.path, &tools, None, false, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["skipped"], false);
    let entries: Vec<_> = fs::read_dir(&flow_states)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with("-ci-passed"))
        .collect();
    assert!(entries.is_empty());
}

#[test]
fn retry_pass_first_attempt() {
    let f = make_ci_fixture();
    let script = f.path.join("pass.sh");
    write_script(&script, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&script);

    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["attempts"], 1);
    assert!(out.get("flaky").is_none());
    assert!(fixture_sentinel(&f).exists());
}

#[test]
fn retry_flaky() {
    let f = make_ci_fixture();
    let script = f.path.join("flaky.sh");
    write_script(
        &script,
        &format!(
            r#"#!/usr/bin/env bash
COUNTER_FILE="{}/counter"
if [ -f "$COUNTER_FILE" ]; then
  COUNT=$(($(cat "$COUNTER_FILE") + 1))
else
  COUNT=1
fi
echo "$COUNT" > "$COUNTER_FILE"
if [ "$COUNT" -lt 2 ]; then
  echo "FAIL: flaky" >&2
  exit 1
fi
exit 0
"#,
            f.path.display()
        ),
    );
    let tools = single_tool(&script);

    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["attempts"], 2);
    assert_eq!(out["flaky"], true);
    let first_fail = out["first_failure_output"].as_str().unwrap();
    assert!(first_fail.contains("FAIL"));
}

#[test]
fn retry_consistent_failure() {
    let f = make_ci_fixture();
    let script = f.path.join("fail.sh");
    write_script(
        &script,
        "#!/usr/bin/env bash\necho 'CI FAILED' >&2\nexit 1\n",
    );
    let tools = single_tool(&script);

    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert_eq!(out["attempts"], 3);
    assert_eq!(out["consistent"], true);
    assert!(out["output"].as_str().unwrap().contains("CI FAILED"));
}

fn default_args() -> Args {
    Args {
        force: false,
        retry: 0,
        branch: None,
        simulate_branch: None,
        format: false,
        lint: false,
        build: false,
        test: false,
        audit: false,
        clean: false,
        trailing: Vec::new(),
    }
}

#[test]
fn cli_recursion_guard() {
    let f = make_ci_fixture();
    let args = Args {
        branch: Some(f.branch.clone()),
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, true);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["skipped"], true);
    assert_eq!(out["reason"], "recursion guard");
}

#[test]
fn run_impl_no_bin_scripts_returns_error() {
    let f = make_ci_fixture();
    let args = Args {
        branch: Some(f.branch.clone()),
        force: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("No ./bin/{format,lint,build,test} scripts"));
}

#[test]
fn run_impl_runs_present_bin_scripts() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        force: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_impl_retry_dispatches_to_retry_path() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        force: false,
        retry: 2,
        simulate_branch: None,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert!(out.get("attempts").is_some());
    assert_eq!(out["attempts"], 1);
}

#[test]
fn run_impl_retry_with_sentinel_skips_before_dispatch() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let args_first = Args {
        branch: Some(f.branch.clone()),
        force: false,
        retry: 0,
        simulate_branch: None,
        ..default_args()
    };
    let (first_out, _) = run_impl(&args_first, &f.path, &f.path, false);
    assert_eq!(first_out["skipped"], false);
    assert!(fixture_sentinel(&f).exists());

    let args_retry = Args {
        branch: Some(f.branch.clone()),
        force: false,
        retry: 2,
        simulate_branch: None,
        ..default_args()
    };
    let (out, code) = run_impl(&args_retry, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["skipped"], true);
    assert_eq!(out["reason"], "no changes since last CI pass");
    assert!(out.get("attempts").is_none());
}

#[test]
fn retry_tool_failure_mid_sequence() {
    let f = make_ci_fixture();
    let pass = f.path.join("pass.sh");
    write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
    let fail = f.path.join("fail.sh");
    write_script(
        &fail,
        "#!/usr/bin/env bash\necho 'TOOL2 FAILED' >&2\nexit 1\n",
    );
    let tools = vec![
        CiTool {
            name: "format".to_string(),
            program: pass.to_string_lossy().to_string(),
            args: vec![],
        },
        CiTool {
            name: "test".to_string(),
            program: fail.to_string_lossy().to_string(),
            args: vec![],
        },
    ];
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["consistent"], true);
    assert!(out["output"].as_str().unwrap().contains("TOOL2 FAILED"));
}

#[test]
fn run_with_retry_propagates_rebuild_and_simulate_branch_env() {
    let f = make_ci_fixture();
    let marker = f.path.join("retry-env-marker");
    let script = f.path.join("retry-env-probe.sh");
    write_script(
        &script,
        &format!(
            "#!/usr/bin/env bash\nprintf 'rebuild=%s sim=%s\\n' \"${{FLOW_CI_REBUILD:-}}\" \"${{FLOW_SIMULATE_BRANCH:-}}\" > {}\nexit 0\n",
            marker.display()
        ),
    );
    let tools = single_tool(&script);

    let (out, code) = run_with_retry(
        &f.path,
        &f.path,
        &tools,
        Some(&f.branch),
        1,
        Some("retry-feature"),
        true,
    );
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");

    let env_dump = std::fs::read_to_string(&marker).unwrap();
    assert!(env_dump.contains("rebuild=1"));
    assert!(env_dump.contains("sim=retry-feature"));
}

#[test]
fn retry_flaky_via_marker_file() {
    let f = make_ci_fixture();
    let marker = f.path.join("flaky-marker");
    let script = f.path.join("flaky-marker.sh");
    write_script(
        &script,
        &format!(
            r#"#!/usr/bin/env bash
MARKER="{}"
if [ -f "$MARKER" ]; then
  exit 0
else
  : > "$MARKER"
  echo "FIRST FAIL" >&2
  exit 1
fi
"#,
            marker.display()
        ),
    );
    let tools = single_tool(&script);
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 3, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["flaky"], true);
    assert_eq!(out["attempts"], 2);
    let first_fail = out["first_failure_output"].as_str().unwrap();
    assert!(first_fail.contains("FIRST FAIL"));
}

#[test]
fn retry_all_attempts_fail_removes_sentinel() {
    let f = make_ci_fixture();
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "stale-content").unwrap();
    assert!(sentinel.exists());

    let script = f.path.join("always-fail.sh");
    write_script(
        &script,
        "#!/usr/bin/env bash\necho 'ALWAYS FAIL' >&2\nexit 1\n",
    );
    let tools = single_tool(&script);
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["consistent"], true);
    assert!(!sentinel.exists());
}

#[test]
fn any_tool_is_stub_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("tool.sh");
    write_script(
        &script,
        &format!("#!/usr/bin/env bash\n# {}\nexit 0\n", STUB_MARKER),
    );
    fs::set_permissions(&script, fs::Permissions::from_mode(0o000)).unwrap();

    let tools = vec![CiTool {
        name: "test".to_string(),
        program: script.to_string_lossy().to_string(),
        args: vec![],
    }];
    let result = any_tool_is_stub(&tools);
    assert!(!result);

    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn run_once_spawn_failure() {
    let f = make_ci_fixture();
    let tools = vec![CiTool {
        name: "format".to_string(),
        program: "/nonexistent/path/to/tool".to_string(),
        args: vec![],
    }];
    let (out, code) = run_once(&f.path, &f.path, &tools, Some(&f.branch), true, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("failed to run"));
}

#[test]
fn retry_spawn_failure() {
    let f = make_ci_fixture();
    let tools = vec![CiTool {
        name: "format".to_string(),
        program: "/nonexistent/path/to/tool".to_string(),
        args: vec![],
    }];
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, Some(&f.branch), 2, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("failed to run"));
}

#[test]
fn run_once_spawn_failure_no_branch_skips_sentinel_removal() {
    let f = make_ci_fixture();
    let tools = vec![CiTool {
        name: "format".to_string(),
        program: "/nonexistent/path/to/tool".to_string(),
        args: vec![],
    }];
    let (out, code) = run_once(&f.path, &f.path, &tools, None, true, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("failed to run"));
}

#[test]
fn run_once_tool_failure_no_branch_skips_sentinel_removal() {
    let f = make_ci_fixture();
    let fail = f.path.join("fail.sh");
    write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
    let tools = single_tool(&fail);
    let (out, code) = run_once(&f.path, &f.path, &tools, None, true, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("failed"));
}

#[test]
fn retry_fail_no_branch_skips_sentinel_removal() {
    // Retry failure path with branch=None exercises the None arm of
    // `if let Some(ref path) = sentinel` inside the failure else-branch.
    let f = make_ci_fixture();
    let fail = f.path.join("fail.sh");
    write_script(&fail, "#!/usr/bin/env bash\nexit 1\n");
    let tools = single_tool(&fail);
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, None, 2, None, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn retry_pass_no_branch_skips_sentinel_write() {
    let f = make_ci_fixture();
    let pass = f.path.join("pass.sh");
    write_script(&pass, "#!/usr/bin/env bash\nexit 0\n");
    let tools = single_tool(&pass);
    let (out, code) = run_with_retry(&f.path, &f.path, &tools, None, 2, None, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_impl_audit_test_prepends_audit_flag() {
    let f = make_ci_fixture();
    let marker = f.path.join("args-dump");
    let script = f.path.join("bin").join("test");
    write_script(
        &script,
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            marker.display()
        ),
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        test: true,
        audit: true,
        trailing: vec!["filter1".to_string()],
        ..default_args()
    };
    let (_out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    let dump = fs::read_to_string(&marker).unwrap();
    let lines: Vec<&str> = dump.lines().collect();
    assert_eq!(lines[0], "--audit");
    assert!(lines.contains(&"filter1"));
}

#[test]
fn run_impl_cwd_scope_rejects_wrong_dir() {
    let f = make_ci_fixture();
    // Write a state file with relative_cwd = "sub" so enforce expects
    // cwd to be <root>/sub or deeper. Running from the repo root will
    // fail the drift check.
    let branch_dir = f.path.join(".flow-states").join(&f.branch);
    fs::create_dir_all(&branch_dir).unwrap();
    fs::write(
        branch_dir.join("state.json"),
        r#"{"relative_cwd": "sub"}"#,
    )
    .unwrap();
    fs::create_dir_all(f.path.join("sub")).unwrap();

    let args = Args {
        branch: Some(f.branch.clone()),
        force: true,
        ..default_args()
    };
    // cwd = repo root, but expected is <root>/sub → drift.
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("cwd drift"));
}

#[test]
fn args_selected_phase_none_when_no_flag_set() {
    let args = default_args();
    assert_eq!(args.selected_phase(), None);
}

#[test]
fn args_selected_phase_format() {
    let args = Args {
        format: true,
        ..default_args()
    };
    assert_eq!(args.selected_phase(), Some("format"));
}

#[test]
fn args_selected_phase_lint() {
    let args = Args {
        lint: true,
        ..default_args()
    };
    assert_eq!(args.selected_phase(), Some("lint"));
}

#[test]
fn args_selected_phase_build() {
    let args = Args {
        build: true,
        ..default_args()
    };
    assert_eq!(args.selected_phase(), Some("build"));
}

#[test]
fn args_selected_phase_test() {
    let args = Args {
        test: true,
        ..default_args()
    };
    assert_eq!(args.selected_phase(), Some("test"));
}

#[test]
fn run_impl_format_flag_runs_only_format() {
    let f = make_ci_fixture();
    for name in ["format", "lint", "build", "test"] {
        let marker = f.path.join(format!("{}-ran", name));
        let marker_str = marker.to_string_lossy().to_string();
        write_script(
            &f.path.join("bin").join(name),
            &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker_str),
        );
    }
    let args = Args {
        branch: Some(f.branch.clone()),
        format: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert!(f.path.join("format-ran").exists());
    assert!(!f.path.join("lint-ran").exists());
    assert!(!f.path.join("build-ran").exists());
    assert!(!f.path.join("test-ran").exists());
}

#[test]
fn run_impl_test_flag_runs_only_test() {
    let f = make_ci_fixture();
    for name in ["format", "lint", "build", "test"] {
        let marker = f.path.join(format!("{}-ran", name));
        let marker_str = marker.to_string_lossy().to_string();
        write_script(
            &f.path.join("bin").join(name),
            &format!("#!/usr/bin/env bash\ntouch {}\nexit 0\n", marker_str),
        );
    }
    let args = Args {
        branch: Some(f.branch.clone()),
        test: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert!(f.path.join("test-ran").exists());
    assert!(!f.path.join("format-ran").exists());
}

#[test]
fn run_impl_format_flag_missing_script_returns_specific_error() {
    let f = make_ci_fixture();
    let args = Args {
        branch: Some(f.branch.clone()),
        format: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    let msg = out["message"].as_str().unwrap();
    assert!(msg.contains("./bin/format script"));
}

#[test]
fn run_impl_single_phase_does_not_write_sentinel() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        format: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert!(!fixture_sentinel(&f).exists());
}

#[test]
fn run_impl_single_phase_ignores_existing_sentinel() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let full = Args {
        branch: Some(f.branch.clone()),
        ..default_args()
    };
    let _ = run_impl(&full, &f.path, &f.path, false);
    assert!(fixture_sentinel(&f).exists());

    let args = Args {
        branch: Some(f.branch.clone()),
        format: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_impl_build_flag_sets_rebuild_env() {
    let f = make_ci_fixture();
    let marker = f.path.join("build-rebuild-marker");
    let build_script = f.path.join("bin").join("build");
    write_script(
        &build_script,
        &format!(
            "#!/usr/bin/env bash\nif [ -n \"${{FLOW_CI_REBUILD:-}}\" ]; then echo rebuilt > {}; fi\nexit 0\n",
            marker.display()
        ),
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        build: true,
        ..default_args()
    };
    let (_out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert!(marker.exists());
}

#[test]
fn run_impl_sentinel_unreadable_falls_through_and_runs() {
    use std::os::unix::fs::PermissionsExt;
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "unreadable").unwrap();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o000)).unwrap();

    let args = Args {
        branch: Some(f.branch.clone()),
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code, 0);
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_impl_trailing_args_forwarded_to_single_phase_tool() {
    let f = make_ci_fixture();
    let marker = f.path.join("trailing-marker");
    write_script(
        &f.path.join("bin").join("test"),
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            marker.display()
        ),
    );
    let args = Args {
        branch: Some(f.branch.clone()),
        test: true,
        trailing: vec!["--".to_string(), "arg1".to_string()],
        ..default_args()
    };
    let (_out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    let dump = fs::read_to_string(&marker).unwrap();
    assert!(dump.contains("arg1"));
}

#[test]
fn run_impl_no_branch_skips_sentinel_and_runs_tools() {
    // Non-git cwd: resolve_branch_in returns None, so the sentinel
    // skip block's `if let Some(ref branch)` takes the None arm and
    // execution falls through to bin_tool_sequence.
    let tmp = tempfile::tempdir().unwrap();
    let args = Args {
        branch: None,
        ..default_args()
    };
    let (out, code) = run_impl(&args, tmp.path(), tmp.path(), false);
    // No bin/* scripts → structured error (runs past the sentinel block).
    assert_eq!(code, 1);
    assert_eq!(out["status"], "error");
    assert!(out["message"].as_str().unwrap().contains("No ./bin/"));
}

#[test]
fn run_impl_sentinel_content_differs_runs_tools() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    // Seed a sentinel with unrelated content so the content==snapshot
    // check fails and we fall through to running tools.
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "stale-snapshot-bytes").unwrap();

    let args = Args {
        branch: Some(f.branch.clone()),
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    // Not skipped — tools ran and refreshed the sentinel.
    assert_eq!(out["skipped"], false);
}

#[test]
fn run_impl_force_bypasses_sentinel_skip() {
    let f = make_ci_fixture();
    write_script(
        &f.path.join("bin").join("format"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    let full = Args {
        branch: Some(f.branch.clone()),
        ..default_args()
    };
    let (first, _) = run_impl(&full, &f.path, &f.path, false);
    assert_eq!(first["skipped"], false);

    let (skipped_out, _) = run_impl(&full, &f.path, &f.path, false);
    assert_eq!(skipped_out["skipped"], true);

    let forced = Args {
        branch: Some(f.branch.clone()),
        force: true,
        ..default_args()
    };
    let (forced_out, code) = run_impl(&forced, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(forced_out["skipped"], false);
}

// --- delete_profraws_recursive ---

#[test]
fn delete_profraws_recursive_missing_dir_returns_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist");
    let (count, bytes) = delete_profraws_recursive(&missing);
    assert_eq!(count, 0);
    assert_eq!(bytes, 0);
}

#[test]
fn delete_profraws_recursive_removes_top_and_nested() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Top-level profraw.
    fs::write(root.join("a.profraw"), b"x".repeat(100)).unwrap();
    // Nested profraw.
    let nested = root.join("debug").join("deps");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("b.profraw"), b"y".repeat(50)).unwrap();
    // Non-profraw — must survive.
    fs::write(root.join("keepme.txt"), b"keep").unwrap();

    let (count, bytes) = delete_profraws_recursive(root);
    assert_eq!(count, 2);
    assert_eq!(bytes, 150);
    assert!(!root.join("a.profraw").exists());
    assert!(!nested.join("b.profraw").exists());
    assert!(root.join("keepme.txt").exists());
}

/// Covers the `let Ok(meta) = fs::metadata(&path) else { continue }`
/// Err arm. A dangling symlink is listed by `read_dir` but
/// `fs::metadata` follows the symlink and returns `ENOENT` — the
/// iteration falls through without touching `bytes` or `count`.
#[cfg(unix)]
#[test]
fn delete_profraws_recursive_skips_dangling_symlinks() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // A real profraw so the function has something to process.
    fs::write(root.join("real.profraw"), b"x".repeat(10)).unwrap();
    // A dangling symlink with a .profraw extension — metadata() on
    // it fails with ENOENT.
    symlink(
        root.join("nonexistent-target"),
        root.join("dangling.profraw"),
    )
    .unwrap();

    let (count, bytes) = delete_profraws_recursive(root);

    // Only the real profraw was removed and its bytes counted.
    assert_eq!(count, 1);
    assert_eq!(bytes, 10);
    assert!(!root.join("real.profraw").exists());
    // The dangling symlink survives — the loop skipped it.
    assert!(root.join("dangling.profraw").symlink_metadata().is_ok());
}

/// Covers the `fs::remove_file(...).is_ok() == false` arm. A
/// read-only parent directory lets `fs::metadata` succeed (`x` bit
/// allows stat) but blocks `remove_file` with `EACCES`. `bytes` still
/// accumulates; `count` does not.
#[cfg(unix)]
#[test]
fn delete_profraws_recursive_handles_remove_failure() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let readonly = tmp.path().join("readonly");
    fs::create_dir(&readonly).unwrap();
    fs::write(readonly.join("locked.profraw"), b"z".repeat(25)).unwrap();

    // r-x on the parent dir: stat succeeds (needs x), remove fails
    // (needs w).
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).unwrap();

    let (count, bytes) = delete_profraws_recursive(tmp.path());

    // Restore permissions so the TempDir drop can clean up.
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o755)).unwrap();

    // metadata() succeeded → bytes counted. remove_file() failed →
    // count stayed zero.
    assert_eq!(count, 0);
    assert_eq!(bytes, 25);
    // File survives the failed removal.
    assert!(readonly.join("locked.profraw").exists());
}

// --- run_clean ---

#[test]
fn run_clean_removes_sentinel_profraws_and_cache_dirs() {
    let f = make_ci_fixture();

    // Seed: sentinel for this branch
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "doesntmatter").unwrap();

    // Seed: profraws in target/llvm-cov-target (top and nested)
    let llvm = f.path.join("target").join("llvm-cov-target");
    fs::create_dir_all(&llvm).unwrap();
    fs::write(llvm.join("top.profraw"), b"aaaa").unwrap();
    let nested = llvm.join("debug").join("deps");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("nested.profraw"), b"bbb").unwrap();

    // Seed: incremental dir with a marker file
    let inc = llvm.join("debug").join("incremental");
    fs::create_dir_all(&inc).unwrap();
    fs::write(inc.join("marker"), b"i").unwrap();

    // Seed: target/debug/flow-rs — must NOT be removed.
    let flow_rs = f.path.join("target").join("debug").join("flow-rs");
    fs::create_dir_all(flow_rs.parent().unwrap()).unwrap();
    fs::write(&flow_rs, b"binary").unwrap();

    let (out, code) = run_clean(&f.path, &f.path, Some(&f.branch));
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["cleaned"]["sentinel_removed"], true);
    assert_eq!(out["cleaned"]["profraw_count"], 2);
    assert_eq!(out["cleaned"]["profraw_bytes"], 7);
    assert_eq!(out["cleaned"]["deps_removed"], true);
    assert_eq!(out["cleaned"]["incremental_removed"], true);

    // Disk state
    assert!(!sentinel.exists(), "sentinel should be gone");
    assert!(!nested.exists(), "deps dir should be gone");
    assert!(!inc.exists(), "incremental dir should be gone");
    assert!(!llvm.join("top.profraw").exists());
    // flow-rs binary preserved
    assert!(flow_rs.exists(), "target/debug/flow-rs must survive clean");
}

#[test]
fn run_clean_nothing_to_clean_is_noop() {
    let f = make_ci_fixture();
    // No sentinel, no target dir. Clean should still succeed with
    // everything false/zero.
    let (out, code) = run_clean(&f.path, &f.path, Some(&f.branch));
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["cleaned"]["sentinel_removed"], false);
    assert_eq!(out["cleaned"]["profraw_count"], 0);
    assert_eq!(out["cleaned"]["profraw_bytes"], 0);
    assert_eq!(out["cleaned"]["deps_removed"], false);
    assert_eq!(out["cleaned"]["incremental_removed"], false);
}

#[test]
fn run_clean_no_branch_skips_sentinel_only() {
    // Non-git cwd: resolve_branch_in returns None, sentinel step
    // is a no-op, but profraws/deps/incremental still get cleaned.
    let tmp = tempfile::tempdir().unwrap();
    let llvm = tmp.path().join("target").join("llvm-cov-target");
    fs::create_dir_all(&llvm).unwrap();
    fs::write(llvm.join("x.profraw"), b"z").unwrap();

    let (out, code) = run_clean(tmp.path(), tmp.path(), None);
    assert_eq!(code, 0);
    assert_eq!(out["cleaned"]["branch"], serde_json::Value::Null);
    assert_eq!(out["cleaned"]["sentinel_removed"], false);
    assert_eq!(out["cleaned"]["profraw_count"], 1);
}

#[test]
fn run_clean_slash_branch_skips_sentinel_without_panic() {
    // External-input guard: a `--branch feature/foo` override must
    // not panic. FlowPaths::try_new returns None; sentinel step
    // becomes a no-op.
    let tmp = tempfile::tempdir().unwrap();
    let (out, code) = run_clean(tmp.path(), tmp.path(), Some("feature/foo"));
    assert_eq!(code, 0);
    assert_eq!(out["cleaned"]["sentinel_removed"], false);
}

// --- run_impl --clean dispatch ---

#[test]
fn run_impl_clean_short_circuits_before_tools() {
    // Even with no bin/* scripts (which normally fails), clean
    // returns ok because it short-circuits before bin_tool_sequence.
    let f = make_ci_fixture();
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "snapshot").unwrap();

    let args = Args {
        branch: Some(f.branch.clone()),
        clean: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, false);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_eq!(out["cleaned"]["sentinel_removed"], true);
    assert!(!sentinel.exists());
}

#[test]
fn run_impl_clean_dominates_recursion_guard() {
    // --clean must run even inside FLOW_CI_RUNNING=1 so a user
    // manually invoking it during CI gets the expected behavior
    // rather than a silent "recursion guard skip".
    let f = make_ci_fixture();
    let sentinel = fixture_sentinel(&f);
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, "snapshot").unwrap();

    let args = Args {
        branch: Some(f.branch.clone()),
        clean: true,
        ..default_args()
    };
    let (out, code) = run_impl(&args, &f.path, &f.path, true);
    assert_eq!(code, 0);
    assert_eq!(out["status"], "ok");
    assert_ne!(out["reason"], "recursion guard");
    assert!(!sentinel.exists());
}

// --- clap parsing for `bin/flow ci --test tests/<file>.rs` ---

#[test]
fn args_parse_test_with_test_file_routes_to_trailing() {
    // Regression guard for the per-file CI path: the user invocation
    // `bin/flow ci --test tests/foo.rs` must produce Args {test:true,
    // trailing:["tests/foo.rs"]} so run_impl forwards the filename to
    // bin/test as a per-file argument.
    let args = Args::parse_from(["ci", "--test", "tests/foo.rs"]);
    assert!(args.test);
    assert_eq!(args.trailing, vec!["tests/foo.rs".to_string()]);
}

#[test]
fn args_parse_clean_flag() {
    let args = Args::parse_from(["ci", "--clean"]);
    assert!(args.clean);
    assert!(!args.force);
    assert!(!args.test);
}
