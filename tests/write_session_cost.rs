//! Integration tests for `src/write_session_cost.rs` — the
//! `write-session-cost` subcommand that writes the active session's
//! token-derived cost to the per-session cost file so month-to-date
//! spend reconciles with the token counts.
//!
//! `run_impl_main` is driven with fixture-controlled inputs (stdin
//! JSON, tempdir project_root + home, fixed clock). One subprocess
//! test spawns the compiled binary to exercise the real stdin →
//! file path. Per `.claude/rules/testing-gotchas.md` "macOS
//! Subprocess Path Canonicalization", every fixture root is
//! canonicalized so prefix comparisons hold.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

use flow_rs::session_cost::{cost_file_path, read_monthly_aggregate};
use flow_rs::write_session_cost::run_impl_main;

/// Build `<root>/.claude/projects/<project>/session.jsonl` containing
/// one priced-opus assistant turn, and return its path.
fn priced_transcript(root: &Path, project: &str, input: i64, output: i64) -> PathBuf {
    let dir = root.join(".claude").join("projects").join(project);
    fs::create_dir_all(&dir).expect("mkdir projects");
    let line = format!(
        r#"{{"type":"assistant","message":{{"model":"claude-opus-4-7","role":"assistant","content":[{{"type":"text","text":"hi"}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    );
    let path = dir.join("session.jsonl");
    fs::write(&path, format!("{}\n", line)).expect("write transcript");
    path
}

fn stdin_json(session_id: &str, transcript: Option<&Path>) -> String {
    match transcript {
        Some(p) => format!(
            r#"{{"session_id":"{}","transcript_path":"{}"}}"#,
            session_id,
            p.to_string_lossy()
        ),
        None => format!(r#"{{"session_id":"{}"}}"#, session_id),
    }
}

// --- run_impl_main ---

/// A priced-opus session writes its token-derived cost to the path
/// `cost_file_path` produces, as a single finite float on one line,
/// and `read_monthly_aggregate` sums it.
#[test]
fn run_impl_main_writes_token_derived_cost_to_cost_file() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = priced_transcript(&root, "proj", 100, 50);
    let stdin = stdin_json("sid-write", Some(&transcript));

    let (value, code) = run_impl_main(&stdin, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");

    // Opus: 100 input * $5/MTok + 50 output * $25/MTok = $0.00175.
    let path = cost_file_path(&root, "sid-write").expect("cost path");
    let content = fs::read_to_string(&path).expect("cost file written");
    assert!(!content.contains('\n') || content.trim_end().lines().count() == 1);
    let parsed: f64 = content.trim().parse().expect("single finite float");
    assert!(parsed.is_finite());
    assert!((parsed - 0.00175).abs() < 1e-9, "got {parsed}");

    let mtd = read_monthly_aggregate(&root);
    assert!(
        (mtd - 0.00175).abs() < 1e-9,
        "MTD aggregate sums it; got {mtd}"
    );
}

/// No `session_id` in the payload → nothing is written, status skipped.
#[test]
fn run_impl_main_skips_when_no_session_id() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let (value, code) = run_impl_main(r#"{}"#, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["reason"], "no_session_id");
}

/// An unsafe `session_id` (path separator) fails `cost_file_path`'s
/// validator → nothing is written, status skipped.
#[test]
fn run_impl_main_skips_when_session_id_unsafe() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let (value, code) = run_impl_main(r#"{"session_id":"../escape"}"#, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["reason"], "unsafe_session_id");
}

/// A transcript path outside `<home>/.claude/projects/` is rejected
/// by the structural validator → no per-model usage → nothing is
/// written (the statusline value is left intact, not clobbered).
#[test]
fn run_impl_main_skips_when_no_priced_usage() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    // Transcript at root/session.jsonl is NOT under .claude/projects/,
    // so the structural validator rejects it and capture reads nothing.
    let bogus = root.join("session.jsonl");
    fs::write(&bogus, "{}\n").expect("write bogus");
    let stdin = stdin_json("sid-empty", Some(&bogus));
    let (value, code) = run_impl_main(&stdin, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["reason"], "no_priced_usage");
    assert!(cost_file_path(&root, "sid-empty")
        .map(|p| !p.exists())
        .unwrap_or(true));
}

/// A session whose only per-model usage is an unpriced model family
/// has no priceable cost → nothing is written (the unpriced-model
/// branch of `cost_for` contributes nothing and `any_priced` stays
/// false).
#[test]
fn run_impl_main_skips_when_only_unpriced_models() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let dir = root.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).expect("mkdir projects");
    let line = r#"{"type":"assistant","message":{"model":"gpt-4o-unpriced","role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
    let transcript = dir.join("session.jsonl");
    fs::write(&transcript, format!("{}\n", line)).expect("write transcript");
    let stdin = stdin_json("sid-unpriced", Some(&transcript));

    let (value, code) = run_impl_main(&stdin, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "skipped");
    assert_eq!(value["reason"], "no_priced_usage");
    assert!(cost_file_path(&root, "sid-unpriced")
        .map(|p| !p.exists())
        .unwrap_or(true));
}

/// A pre-existing regular cost file is overwritten with the
/// token-derived value (exercises the not-a-symlink branch).
#[test]
fn run_impl_main_overwrites_existing_regular_cost_file() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = priced_transcript(&root, "proj", 100, 50);
    let path = cost_file_path(&root, "sid-over").expect("cost path");
    fs::create_dir_all(path.parent().unwrap()).expect("mkdir cost dir");
    fs::write(&path, "999.0\n").expect("seed stale value");

    let stdin = stdin_json("sid-over", Some(&transcript));
    let (value, _) = run_impl_main(&stdin, &root, &root);
    assert_eq!(value["status"], "ok");
    let parsed: f64 = fs::read_to_string(&path).unwrap().trim().parse().unwrap();
    assert!(
        (parsed - 0.00175).abs() < 1e-9,
        "stale value overwritten; got {parsed}"
    );
}

/// A pre-existing symlink at the cost-file path is removed before the
/// write so `fs::write` cannot follow it to an arbitrary target
/// (exercises the symlink branch of the symlink-safe write).
#[test]
#[cfg(unix)]
fn run_impl_main_replaces_symlink_cost_file() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = priced_transcript(&root, "proj", 100, 50);
    let path = cost_file_path(&root, "sid-link").expect("cost path");
    fs::create_dir_all(path.parent().unwrap()).expect("mkdir cost dir");
    let target = root.join("symlink-target");
    fs::write(&target, "should not be followed").expect("write target");
    std::os::unix::fs::symlink(&target, &path).expect("create symlink");

    let stdin = stdin_json("sid-link", Some(&transcript));
    let (value, _) = run_impl_main(&stdin, &root, &root);
    assert_eq!(value["status"], "ok");
    // The symlink was replaced by a fresh regular file; the target is
    // untouched.
    assert!(!fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    let parsed: f64 = fs::read_to_string(&path).unwrap().trim().parse().unwrap();
    assert!((parsed - 0.00175).abs() < 1e-9);
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "should not be followed"
    );
}

/// When the cost-file path is occupied by a directory, `fs::write`
/// fails and the subcommand reports an error rather than panicking.
#[test]
fn run_impl_main_returns_error_when_write_fails() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = priced_transcript(&root, "proj", 100, 50);
    let path = cost_file_path(&root, "sid-dir").expect("cost path");
    // Occupy the cost-file path with a directory so fs::write errors.
    fs::create_dir_all(&path).expect("mkdir at cost-file path");

    let stdin = stdin_json("sid-dir", Some(&transcript));
    let (value, code) = run_impl_main(&stdin, &root, &root);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason"], "write_failed");
}

// --- subprocess (real binary) ---

/// End-to-end: the compiled `write-session-cost` subcommand reads the
/// hook stdin payload, captures the session's token usage from the
/// transcript, and writes the token-derived cost file.
#[test]
fn write_session_cost_subcommand_writes_cost_file() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let transcript = priced_transcript(&root, "proj", 100, 50);
    let stdin = stdin_json("sid-subproc", Some(&transcript));

    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .arg("write-session-cost")
        .current_dir(&root)
        .env("HOME", &root)
        .env_remove("FLOW_CI_RUNNING")
        .env("GH_TOKEN", "invalid")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn write-session-cost");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let path = cost_file_path(&root, "sid-subproc").expect("cost path");
    let parsed: f64 = fs::read_to_string(&path)
        .expect("cost file written")
        .trim()
        .parse()
        .expect("single finite float");
    assert!((parsed - 0.00175).abs() < 1e-9, "got {parsed}");
}
