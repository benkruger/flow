//! Integration tests for `bin/flow auto-close-parent` and its library surface.
//!
//! Drives the blocked-by cascade through the `GhApiRunner` seam so no
//! test spawns a real `gh` subprocess. The cascade walks the GitHub
//! REST issue-dependencies endpoints (`dependencies/blocking` and
//! `dependencies/blocked_by`) and closes every issue whose remaining
//! blockers are all closed.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{create_gh_stub, create_git_repo_with_remote, parse_output};
use flow_rs::auto_close_parent::{
    cascade_close_unblocked, check_milestone_closed, fetch_milestone_number,
    parse_milestone_number, run_api, run_impl_main, run_with_current_dir_from, safe_default_ok,
    should_close_milestone, Args, GhApiRunner, MAX_CASCADE_DEPTH,
};

fn run_cmd(repo: &Path, args: &[&str], stub_dir: &Path) -> Output {
    let path_env = format!(
        "{}:{}",
        stub_dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("auto-close-parent")
        .args(args)
        .current_dir(repo)
        .env("PATH", &path_env)
        .env("CLAUDE_PLUGIN_ROOT", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

/// Extract the issue number from a `repos/{repo}/issues/N/dependencies/...` URL.
///
/// Used by the chain-shaped runner that responds to any issue's blocking
/// and blocked_by endpoints with a numerically-adjacent neighbor.
fn parse_issue_number_from_url(joined: &str) -> Option<i64> {
    let idx = joined.find("/issues/")?;
    let rest = &joined[idx + "/issues/".len()..];
    let end = rest.find('/')?;
    rest[..end].parse().ok()
}

// --- cascade_close_unblocked branch coverage (A–H) ---

/// Branch A: GET issues/X/dependencies/blocking returns an empty list.
/// Closes nothing; no recursion.
#[test]
fn cascade_no_blocking_issues_closes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("/dependencies/blocking") {
            Ok("[]".to_string())
        } else {
            Err(format!("unexpected call: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Branch B: GET issues/X/dependencies/blocking returns Err. Cascade
/// returns empty Vec without panic.
#[test]
fn cascade_blocking_fetch_failure_is_best_effort() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Branch C: 5 blocks 6, 6 blocks 5 (cycle). Cascade from 5 closes 6
/// exactly once, then the visited set short-circuits the re-visit.
#[test]
fn cascade_cycle_terminates_via_visited_set() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocking") {
            Ok(r#"[{"number":5}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/5/dependencies/blocked_by") {
            Ok(r#"[{"number":6,"state":"closed"}]"#.to_string())
        } else if joined.contains("issue close 6") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed, vec![6]);
}

/// Branch D: A linear chain longer than the defensive depth bound
/// halts at MAX_CASCADE_DEPTH. closed.len() equals the bound.
#[test]
fn cascade_depth_bound_halts_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    // Chain shape: N blocks N+1 indefinitely; each N's only blocker is
    // N-1 (closed). The runner extrapolates from URL.
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("/dependencies/blocking") {
            let n = parse_issue_number_from_url(&joined).unwrap_or(0);
            Ok(format!(r#"[{{"number":{}}}]"#, n + 1))
        } else if joined.contains("/dependencies/blocked_by") {
            let n = parse_issue_number_from_url(&joined).unwrap_or(0);
            Ok(format!(r#"[{{"number":{},"state":"closed"}}]"#, n - 1))
        } else if joined.contains("issue close") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed.len(), MAX_CASCADE_DEPTH);
}

/// Branch E: 6's blocker set still has at least one open blocker.
/// Cascade does not close 6.
#[test]
fn cascade_open_blocker_leaves_issue_open() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"},{"number":7,"state":"open"}]"#.to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Branch F: Every blocker of 6 is closed; cascade closes 6, then
/// recurses on 6 with an empty blocking list.
#[test]
fn cascade_all_blockers_closed_closes_and_recurses() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocking") {
            Ok("[]".to_string())
        } else if joined.contains("issue close 6") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed, vec![6]);
}

/// Branch G: GET issues/Y/dependencies/blocked_by fails for candidate
/// Y. Cascade skips Y and continues with the next candidate.
#[test]
fn cascade_blocked_by_fetch_failure_skips_issue() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6},{"number":7}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Err("rate limited".to_string())
        } else if joined.contains("issues/7/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/7/dependencies/blocking") {
            Ok("[]".to_string())
        } else if joined.contains("issue close 7") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed, vec![7]);
}

/// Branch H: 5 (leaf, just closed) blocks 10 (parent); 10 blocks 20
/// (source). Each level's only blocker is the level above it. Cascade
/// closes both 10 and 20 in order.
#[test]
fn cascade_multi_level_closes_to_source() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":10}]"#.to_string())
        } else if joined.contains("issues/10/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/10/dependencies/blocking") {
            Ok(r#"[{"number":20}]"#.to_string())
        } else if joined.contains("issues/20/dependencies/blocked_by") {
            Ok(r#"[{"number":10,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/20/dependencies/blocking") {
            Ok("[]".to_string())
        } else if joined.contains("issue close 10") || joined.contains("issue close 20") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed, vec![10, 20]);
}

/// Additional branch — close subprocess fails for Y. Cascade does NOT
/// push Y to closed and does NOT recurse into Y's blocking list.
#[test]
fn cascade_close_command_failure_does_not_record_or_recurse() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issue close 6") {
            Err("permission denied".to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Edge: an empty blocker list is treated as "not closeable" — GitHub
/// only returns an empty array when there are no recorded blockers,
/// which means Y isn't part of the dependency graph the cascade walks.
#[test]
fn cascade_empty_blockers_list_does_not_close() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok("[]".to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Edge: malformed blocking JSON is treated as a fetch failure.
#[test]
fn cascade_malformed_blocking_json_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok("not json".to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Edge: malformed blocked_by JSON skips the candidate.
#[test]
fn cascade_malformed_blocked_by_json_skips_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok("not json".to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(closed.is_empty());
}

/// Edge: a candidate JSON entry missing the `number` field is skipped.
#[test]
fn cascade_candidate_without_number_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"title":"no number"},{"number":7}]"#.to_string())
        } else if joined.contains("issues/7/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/7/dependencies/blocking") {
            Ok("[]".to_string())
        } else if joined.contains("issue close 7") {
            Ok(String::new())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert_eq!(closed, vec![7]);
}

/// Shared close ledger for `cascade_convergent_diamond_closes_all_three`.
/// `OnceLock<Mutex<...>>` (used pervasively in `tests/common/mod.rs`)
/// satisfies the `'static + Fn(...)` bound on `&GhApiRunner` without
/// pulling in `thread_local!` std internals that the per-file
/// coverage gate cannot cover. Only one test references this static.
fn diamond_closed() -> &'static std::sync::Mutex<std::collections::HashSet<i64>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i64>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Convergent (diamond) dependency graph: 5 → 6 → 8 and 5 → 7 → 8. The
/// cascade closes 6 first. When walking 6's downstream, 8's blocked_by
/// query returns [6 closed, 7 open] — Y is not yet closeable. The
/// cascade must NOT permanently mark 8 as visited at that moment, or
/// the next branch (5 → 7 → 8) would find 8 in the visited set and
/// silently skip it, leaving 8 open after both its blockers closed.
///
/// Asserts the fix in `cascade_recurse`: candidates only enter
/// `visited` after a successful `gh issue close`, so an unclose-able
/// Y remains eligible for re-evaluation when the convergent branch
/// closes Y's other blocker.
#[test]
fn cascade_convergent_diamond_closes_all_three() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();

    {
        let mut s = diamond_closed().lock().unwrap();
        s.clear();
        s.insert(5);
    }

    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if let Some(idx) = joined.find("issue close ") {
            let rest = &joined[idx + "issue close ".len()..];
            let end = rest.find(' ').unwrap_or(rest.len());
            if let Ok(n) = rest[..end].trim().parse::<i64>() {
                diamond_closed().lock().unwrap().insert(n);
            }
            return Ok(String::new());
        }
        let state_of = |n: i64| -> &'static str {
            if diamond_closed().lock().unwrap().contains(&n) {
                "closed"
            } else {
                "open"
            }
        };
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":6},{"number":7}]"#.to_string())
        } else if joined.contains("issues/6/dependencies/blocked_by") {
            Ok(format!(r#"[{{"number":5,"state":"{}"}}]"#, state_of(5)))
        } else if joined.contains("issues/6/dependencies/blocking") {
            Ok(r#"[{"number":8}]"#.to_string())
        } else if joined.contains("issues/7/dependencies/blocked_by") {
            Ok(format!(r#"[{{"number":5,"state":"{}"}}]"#, state_of(5)))
        } else if joined.contains("issues/7/dependencies/blocking") {
            Ok(r#"[{"number":8}]"#.to_string())
        } else if joined.contains("issues/8/dependencies/blocked_by") {
            Ok(format!(
                r#"[{{"number":6,"state":"{}"}},{{"number":7,"state":"{}"}}]"#,
                state_of(6),
                state_of(7),
            ))
        } else if joined.contains("issues/8/dependencies/blocking") {
            Ok("[]".to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };

    let closed = cascade_close_unblocked("owner/repo", 5, &cwd, runner);
    assert!(
        closed.contains(&6),
        "diamond closure: 6 must close (first branch). closed = {:?}",
        closed
    );
    assert!(
        closed.contains(&7),
        "diamond closure: 7 must close (second branch). closed = {:?}",
        closed
    );
    assert!(
        closed.contains(&8),
        "diamond closure: 8 must close after both blockers (6 and 7) close; got closed = {:?}",
        closed
    );
}

// --- run_impl_main output shape ---

/// The output payload is `{status, closed_issues: [i64],
/// milestone_closed: bool}`. The old `parent_closed: bool` field must
/// not appear.
#[test]
fn run_impl_main_output_shape_has_closed_issues_array() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 5,
    };
    let runner: &GhApiRunner = &|_, _| Err("simulated".to_string());
    let (value, code) = run_impl_main(args, &cwd, runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert!(value["closed_issues"].is_array());
    assert_eq!(value["closed_issues"].as_array().unwrap().len(), 0);
    assert!(value["milestone_closed"].is_boolean());
    assert_eq!(value["milestone_closed"], false);
    assert!(
        value.get("parent_closed").is_none(),
        "parent_closed field must not appear in the new shape"
    );
}

/// Happy path through `run_impl_main`: cascade closes one issue AND
/// milestone closes too. The output carries the new shape.
#[test]
fn run_impl_main_happy_path_closes_cascade_and_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 5,
    };
    let runner: &GhApiRunner = &|args, _| {
        let joined = args.join(" ");
        if joined.contains("issues/5/dependencies/blocking") {
            Ok(r#"[{"number":10}]"#.to_string())
        } else if joined.contains("issues/10/dependencies/blocked_by") {
            Ok(r#"[{"number":5,"state":"closed"}]"#.to_string())
        } else if joined.contains("issues/10/dependencies/blocking") {
            Ok("[]".to_string())
        } else if joined.contains("issue close 10") {
            Ok(String::new())
        } else if joined.contains("--jq .milestone.number") {
            Ok("3\n".to_string())
        } else if joined.contains("milestones/3") && joined.contains("PATCH") {
            Ok(String::new())
        } else if joined.contains("milestones/3") {
            Ok(r#"{"open_issues":0}"#.to_string())
        } else {
            Err(format!("unexpected: {}", joined))
        }
    };
    let (value, code) = run_impl_main(args, &cwd, runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["closed_issues"], serde_json::json!([10]));
    assert_eq!(value["milestone_closed"], true);
}

/// All runner failures still yield the safe-default success envelope.
#[test]
fn run_impl_main_all_runner_failures_returns_safe_default() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 999,
    };
    let runner: &GhApiRunner = &|_, _| Err("simulated".to_string());
    let (value, code) = run_impl_main(args, &cwd, runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["closed_issues"], serde_json::json!([]));
    assert_eq!(value["milestone_closed"], false);
}

// --- Milestone helpers ---

#[test]
fn should_close_milestone_zero_open_lib() {
    let json = r#"{"open_issues": 0, "closed_issues": 5}"#;
    assert!(should_close_milestone(json));
}

#[test]
fn should_close_milestone_has_open_lib() {
    let json = r#"{"open_issues": 2, "closed_issues": 3}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn should_close_milestone_missing_field_lib() {
    let json = r#"{"closed_issues": 5}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn should_close_milestone_invalid_json_lib() {
    assert!(!should_close_milestone("not json"));
}

#[test]
fn should_close_milestone_null_open_issues_lib() {
    let json = r#"{"open_issues": null}"#;
    assert!(!should_close_milestone(json));
}

#[test]
fn parse_milestone_number_present() {
    let json = r#"{"milestone": {"number": 3}}"#;
    assert_eq!(parse_milestone_number(json), Some(3));
}

#[test]
fn parse_milestone_number_absent() {
    assert_eq!(parse_milestone_number("{}"), None);
}

#[test]
fn parse_milestone_number_invalid_json() {
    assert_eq!(parse_milestone_number("not json"), None);
}

#[test]
fn parse_milestone_number_not_dict() {
    let json = r#"{"milestone": "not_a_dict"}"#;
    assert_eq!(parse_milestone_number(json), None);
}

#[test]
fn parse_milestone_number_not_int() {
    let json = r#"{"milestone": {"number": "not_int"}}"#;
    assert_eq!(parse_milestone_number(json), None);
}

#[test]
fn parse_milestone_number_null() {
    let json = r#"{"milestone": null}"#;
    assert_eq!(parse_milestone_number(json), None);
}

#[test]
fn check_milestone_closed_standalone_null_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("null\n".to_string());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_empty_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(String::new());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_unparseable_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("not_an_int".to_string());
    assert!(!check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_standalone_succeeds_then_closes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<String>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            "3\n".to_string(),
            r#"{"open_issues":0}"#.to_string(),
            String::new(),
        ]));
    let runner: &GhApiRunner = &move |_, _| Ok(queue.borrow_mut().pop_front().unwrap_or_default());
    assert!(check_milestone_closed("owner/repo", 5, None, &cwd, runner));
}

#[test]
fn check_milestone_closed_patch_command_fails_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let queue: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>> =
        std::cell::RefCell::new(std::collections::VecDeque::from(vec![
            Ok(r#"{"open_issues":0}"#.to_string()),
            Err("patch failed".to_string()),
        ]));
    let runner: &GhApiRunner = &move |_, _| {
        queue
            .borrow_mut()
            .pop_front()
            .expect("test runner queue exhausted")
    };
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn check_milestone_closed_milestone_fetch_error_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Err("network error".to_string());
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn check_milestone_closed_open_issues_nonzero_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(r#"{"open_issues":2}"#.to_string());
    assert!(!check_milestone_closed(
        "owner/repo",
        5,
        Some(3),
        &cwd,
        runner
    ));
}

#[test]
fn fetch_milestone_number_present() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok(r#"{"milestone":{"number":7}}"#.to_string());
    assert_eq!(
        fetch_milestone_number("owner/repo", 5, &cwd, runner),
        Some(7)
    );
}

#[test]
fn fetch_milestone_number_absent() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Ok("{}".to_string());
    assert_eq!(fetch_milestone_number("owner/repo", 5, &cwd, runner), None);
}

#[test]
fn fetch_milestone_number_runner_error() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let runner: &GhApiRunner = &|_, _| Err("network".to_string());
    assert_eq!(fetch_milestone_number("owner/repo", 5, &cwd, runner), None);
}

// --- safe_default_ok ---

#[test]
fn safe_default_ok_returns_ok_with_empty_closed_and_false_milestone() {
    let (value, code) = safe_default_ok();
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["closed_issues"], serde_json::json!([]));
    assert_eq!(value["milestone_closed"], false);
    assert!(
        value.get("parent_closed").is_none(),
        "parent_closed field must not appear"
    );
}

// --- run_with_current_dir_from ---

#[test]
fn run_with_current_dir_from_cwd_err_returns_safe_default() {
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 1,
    };
    let runner: &GhApiRunner = &|_, _| Ok(String::new());
    let (value, code) = run_with_current_dir_from(
        args,
        || Err(std::io::Error::other("simulated current_dir failure")),
        runner,
    );
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["closed_issues"], serde_json::json!([]));
    assert_eq!(value["milestone_closed"], false);
}

#[test]
fn run_with_current_dir_from_cwd_ok_routes_to_run_impl_main() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().to_path_buf();
    let args = Args {
        repo: "owner/repo".to_string(),
        issue_number: 1,
    };
    let runner: &GhApiRunner = &|_, _| Ok(String::new());
    let (value, code) = run_with_current_dir_from(args, move || Ok(cwd.clone()), runner);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
}

// --- Subprocess integration tests ---

#[test]
fn subprocess_happy_path_closes_cascade_and_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *issues/5/dependencies/blocking* ]]; then\n\
           echo '[{\"number\":10}]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/10/dependencies/blocked_by* ]]; then\n\
           echo '[{\"number\":5,\"state\":\"closed\"}]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issues/10/dependencies/blocking* ]]; then\n\
           echo '[]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *issue*close*10* ]]; then\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *--jq*milestone* ]]; then\n\
           echo '3'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* && \"$*\" == *PATCH* ]]; then\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":0}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["closed_issues"], serde_json::json!([10]));
    assert_eq!(data["milestone_closed"], true);
}

#[test]
fn subprocess_all_gh_failures_returns_safe_default() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(&repo, "#!/bin/bash\nexit 1\n");

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["closed_issues"], serde_json::json!([]));
    assert_eq!(data["milestone_closed"], false);
}

#[test]
fn subprocess_milestone_still_closes_when_open_zero_no_cascade() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *dependencies/blocking* ]]; then\n\
           echo '[]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *--jq*milestone* ]]; then\n\
           echo '3'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* && \"$*\" == *PATCH* ]]; then\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":0}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["closed_issues"], serde_json::json!([]));
    assert_eq!(data["milestone_closed"], true);
}

#[test]
fn subprocess_milestone_does_not_close_when_open_remain() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_git_repo_with_remote(dir.path());
    let stub_dir = create_gh_stub(
        &repo,
        "#!/bin/bash\n\
         if [[ \"$*\" == *dependencies/blocking* ]]; then\n\
           echo '[]'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *--jq*milestone* ]]; then\n\
           echo '3'\n\
           exit 0\n\
         fi\n\
         if [[ \"$*\" == *milestones/3* ]]; then\n\
           echo '{\"open_issues\":2,\"closed_issues\":3}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
    );

    let output = run_cmd(
        &repo,
        &["--repo", "owner/name", "--issue-number", "5"],
        &stub_dir,
    );

    assert_eq!(output.status.code(), Some(0));
    let data = parse_output(&output);
    assert_eq!(data["milestone_closed"], false);
}

// --- run_api ---

fn install_failing_gh_stub() -> tempfile::TempDir {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let stub_dir = tempfile::tempdir().unwrap();
    let stub = stub_dir.path().join("gh");
    let mut f = std::fs::File::create(&stub).unwrap();
    f.write_all(b"#!/bin/bash\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();
    stub_dir
}

fn with_stub_path<F: FnOnce()>(stub_dir: &Path, f: F) {
    use std::sync::Mutex;
    static PATH_LOCK: Mutex<()> = Mutex::new(());
    let _guard = PATH_LOCK.lock().unwrap();
    let original = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stub_dir.display(), original);
    unsafe {
        std::env::set_var("PATH", new_path);
    }
    f();
    unsafe {
        std::env::set_var("PATH", original);
    }
}

#[test]
fn run_api_with_failing_gh_returns_err_lib() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        let result = run_api(&["gh", "api", "repos/x/y/issues/1"], &cwd);
        assert!(result.is_err());
    });
}

#[test]
fn run_impl_main_production_with_failing_gh_returns_ok_safe_default() {
    let stub_dir = install_failing_gh_stub();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().canonicalize().unwrap();
    with_stub_path(stub_dir.path(), || {
        let args = Args {
            repo: "owner/repo".to_string(),
            issue_number: 5,
        };
        let (value, code) = run_impl_main(args, &cwd, &run_api);
        assert_eq!(code, 0);
        assert_eq!(value["status"], "ok");
        assert_eq!(value["closed_issues"], serde_json::json!([]));
        assert_eq!(value["milestone_closed"], false);
    });
}

// --- pre_exec subprocess test for stale cwd ---

#[cfg(unix)]
#[test]
fn auto_close_parent_subprocess_with_stale_cwd_uses_safe_default() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    fs::create_dir(&cwd).expect("mkdir doomed");

    let cwd_for_preexec = cwd.clone();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.arg("auto-close-parent");
    cmd.args(["--repo", "x/y", "--issue-number", "1"]);
    cmd.current_dir(&cwd);
    cmd.env_remove("FLOW_CI_RUNNING");
    cmd.env("GH_TOKEN", "invalid");
    cmd.env("HOME", &root);
    cmd.env("GIT_CEILING_DIRECTORIES", &root);

    // SAFETY: `pre_exec` requires the closure to be async-signal-safe.
    // `libc::rmdir` is listed as AS-safe by POSIX; we only call it and
    // return Ok — no memory allocation, no panic surfaces.
    let preexec_path = std::ffi::CString::new(cwd_for_preexec.to_str().expect("utf8").as_bytes())
        .expect("CString from cwd path");
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let output = cmd.output().expect("spawn flow-rs auto-close-parent");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 for safe-default path, got status {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"closed_issues\":[]"),
        "expected safe-default JSON, got stdout={}",
        stdout
    );
    assert!(
        stdout.contains("\"milestone_closed\":false"),
        "expected safe-default JSON, got stdout={}",
        stdout
    );
}
