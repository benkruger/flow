//! End-to-end staging-trunked lifecycle smoke test.
//!
//! Primes a fixture repo whose default branch is `staging` (no
//! `main` branch on the bare remote) and drives a representative
//! slice of phase commands through the compiled `flow-rs` binary,
//! asserting that each command honors `state["base_branch"]` and
//! never falls back to a hardcoded `origin/main`. This is the
//! integration-level lock-in for the read-side architecture
//! introduced by issue #1275: each per-component test
//! (`tests/git.rs::read_base_branch_*`,
//! `tests/start_gate.rs::test_base_branch_from_state_used_for_git_pull`,
//! `tests/check_freshness.rs::check_freshness_uses_base_branch_from_state`,
//! `tests/cleanup.rs::cleanup_pulls_base_branch_from_state`,
//! `tests/complete_preflight.rs::complete_preflight_merge_base_uses_base_branch_from_state`,
//! `tests/base_branch_cmd.rs`) proves a single read site honors
//! `base_branch`; this lifecycle test proves the cross-phase
//! composition does, so a future regression that re-introduces a
//! hardcoded `origin/main` in any new phase callsite fails this
//! test even when the per-component tests still pass.
//!
//! The fixture pattern follows
//! `tests/commands/init_state.rs::base_branch_matches_current_branch_at_invocation`
//! (checkout `staging` before driving the binary) extended with
//! the bare-remote setup from
//! `tests/common/mod.rs::create_git_repo_with_remote` so that
//! `git fetch origin staging` actually succeeds against the
//! fixture's remote.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

mod common;

const BRANCH: &str = "staging-feature";
const BASE_BRANCH: &str = "staging";

/// Create a bare repo whose default branch is `staging` (no `main`
/// remote ref) and a working clone with origin pointing at it. The
/// working clone is on `staging` after the initial commit + push.
/// Returns the canonicalized working repo path.
fn create_staging_trunked_repo(parent: &Path) -> PathBuf {
    let bare = parent.join("bare.git");
    let repo = parent.join("repo");

    let run = |args: &[&str], cwd: Option<&Path>| {
        let mut cmd = Command::new("git");
        cmd.args(args);
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        let output = cmd.output().expect("git spawn failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // Bare repo with default-branch staging. The bare's HEAD points
    // at refs/heads/staging but staging itself has no commits until
    // the first push lands.
    run(
        &["init", "--bare", "-b", BASE_BRANCH, &bare.to_string_lossy()],
        None,
    );
    // Clone the (empty) bare. No -b flag because no branches exist
    // yet; the local clone has no branches either at this point.
    run(
        &["clone", &bare.to_string_lossy(), &repo.to_string_lossy()],
        None,
    );
    for (key, val) in [
        ("user.email", "test@test.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
        ("init.defaultBranch", BASE_BRANCH),
    ] {
        run(&["config", key, val], Some(&repo));
    }
    // The first commit on a fresh clone creates the branch named by
    // HEAD. With init.defaultBranch=staging set above, this produces
    // a local `staging` branch with one commit. Push -u creates
    // the remote `staging` ref on the bare.
    run(&["checkout", "-b", BASE_BRANCH], Some(&repo));
    run(&["commit", "--allow-empty", "-m", "init"], Some(&repo));
    run(&["push", "-u", "origin", BASE_BRANCH], Some(&repo));

    repo.canonicalize().expect("canonicalize repo")
}

/// Write a state file at `<repo>/.flow-states/<BRANCH>/state.json`
/// with `base_branch` set to `staging`. Mirrors the minimal shape
/// the per-phase commands read.
fn write_staging_state(repo: &Path) {
    let dir = repo.join(".flow-states").join(BRANCH);
    fs::create_dir_all(&dir).unwrap();
    let state = json!({
        "schema_version": 1,
        "branch": BRANCH,
        "base_branch": BASE_BRANCH,
        "current_phase": "flow-code",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "in_progress"},
        },
    });
    fs::write(
        dir.join("state.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    )
    .unwrap();
}

/// Spawn `flow-rs <args>` with subprocess hygiene per
/// `.claude/rules/subprocess-test-hygiene.md` (clear FLOW_CI_RUNNING
/// and FLOW_SIMULATE_BRANCH, neutralize GH_TOKEN/HOME so the child
/// cannot make real GitHub or dotfile calls).
fn run_flow_rs(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(args)
        .current_dir(repo)
        .env("GH_TOKEN", "invalid")
        .env("HOME", repo)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .expect("spawn flow-rs")
}

/// Force-resolve the absolute repo path the child will see. macOS
/// tempdirs live under `/var/folders/...` (a symlink to
/// `/private/var/folders/...`), and `Command::current_dir` resolves
/// the symlink before exec — so the child's `current_dir()` returns
/// the canonical form. Tests that compare paths must canonicalize
/// at fixture-construction time, per
/// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
/// Canonicalization".
fn assert_no_origin_main(haystack: &str, context: &str) {
    assert!(
        !haystack.contains("origin/main"),
        "{}: stderr/stdout must not reference `origin/main` on a \
         staging-trunked fixture — that would prove a phase command \
         still hardcodes `main` instead of reading `base_branch`. \
         Got: {}",
        context,
        haystack
    );
}

/// Drive `bin/flow base-branch` and assert it returns the
/// state-file value (`"staging\n"`). This is the skill-side single
/// source of truth — if it returned `"main\n"` here, every Phase
/// 4/5/6 SKILL.md that interpolates `<base_branch>` would target
/// the wrong branch.
#[test]
fn staging_lifecycle_base_branch_subcommand_returns_staging() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_staging_trunked_repo(dir.path());
    write_staging_state(&repo);

    let output = run_flow_rs(&repo, &["base-branch", "--branch", BRANCH]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, format!("{}\n", BASE_BRANCH));

    // Lock-in: nothing in the output references origin/main.
    assert_no_origin_main(&stdout, "base-branch stdout");
    assert_no_origin_main(
        &String::from_utf8_lossy(&output.stderr),
        "base-branch stderr",
    );
}

/// Drive `bin/flow check-freshness` against the staging-trunked
/// fixture and assert it operates on `origin/staging` — succeeds
/// (up_to_date) because the bare remote has staging, and the
/// JSON output never references origin/main.
#[test]
fn staging_lifecycle_check_freshness_targets_staging() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_staging_trunked_repo(dir.path());
    write_staging_state(&repo);

    let state_file = repo
        .join(".flow-states")
        .join(BRANCH)
        .join("state.json")
        .to_string_lossy()
        .to_string();
    let output = run_flow_rs(&repo, &["check-freshness", "--state-file", &state_file]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The fixture is on staging at HEAD with origin/staging pointing
    // at the same commit, so the freshness check returns
    // up_to_date. The success path proves `git fetch origin
    // <base_branch>` and `git merge-base --is-ancestor origin/<base_branch>
    // HEAD` both used `staging`, not `main`.
    let last_json = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    let value: Value = serde_json::from_str(last_json)
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_json));
    assert_eq!(
        value["status"], "up_to_date",
        "check-freshness on staging-trunked fixture must succeed; got: {}",
        value
    );

    // Lock-in.
    assert_no_origin_main(&stdout, "check-freshness stdout");
    assert_no_origin_main(&stderr, "check-freshness stderr");
}

/// Drive `bin/flow cleanup --pull` against the staging-trunked
/// fixture. With no worktree to remove (we never created one),
/// cleanup is a no-op for most steps but `--pull` runs
/// `git pull origin <base_branch>`. The pull succeeds against the
/// staging-keyed remote — and the steps map's git_pull entry must
/// be `"pulled"`, not a `failed: ... origin/main ...` message that
/// would prove the cleanup still hardcoded main.
#[test]
fn staging_lifecycle_cleanup_pull_targets_staging() {
    let dir = tempfile::tempdir().unwrap();
    let repo = create_staging_trunked_repo(dir.path());
    write_staging_state(&repo);

    let output = run_flow_rs(
        &repo,
        &[
            "cleanup",
            &repo.to_string_lossy(),
            "--branch",
            BRANCH,
            "--worktree",
            ".worktrees/no-such-worktree",
            "--pull",
        ],
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "cleanup must succeed even with no worktree to remove; \
         stderr: {}",
        stderr
    );

    let last_json = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    let value: Value = serde_json::from_str(last_json)
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_json));
    assert_eq!(
        value["status"], "ok",
        "cleanup --pull on staging-trunked fixture must report ok overall; got: {}",
        value
    );
    let git_pull = value["steps"]["git_pull"].as_str().unwrap_or("<missing>");
    assert_eq!(
        git_pull, "pulled",
        "cleanup --pull must successfully pull origin/<base_branch> on a \
         staging-trunked fixture (proves the value flowed through to \
         `git pull origin staging`); got: {}",
        git_pull
    );

    // Lock-in.
    assert_no_origin_main(&stdout, "cleanup stdout");
    assert_no_origin_main(&stderr, "cleanup stderr");
}

/// Drive `bin/flow complete-preflight` on the staging-trunked fixture
/// with `gh pr view` stubbed to return `OPEN`. The OPEN arm dispatches
/// to `merge_main(base_branch)` which runs `git fetch origin
/// <base_branch>` and `git merge-base --is-ancestor origin/<base_branch>
/// HEAD`. Both must operate on `origin/staging` — never `origin/main`
/// — and the JSON response must report `"merge": "clean"` because the
/// fixture's HEAD is already at origin/staging. Locks in the
/// complete-preflight read-side after the issue #1275 architecture
/// landed; complements the per-component
/// `tests/complete_preflight.rs::complete_preflight_merge_base_uses_base_branch_from_state`
/// with a real-git lifecycle assertion.
#[test]
fn staging_lifecycle_complete_preflight_targets_staging() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let repo = create_staging_trunked_repo(&parent);
    write_staging_state(&repo);

    // Stub gh on PATH so `gh pr view` returns OPEN. Real git runs;
    // git fetch / merge-base operate against the staging-keyed bare
    // remote created above.
    let stubs = parent.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let gh_script = "#!/bin/sh\n\
        case \"$1 $2\" in\n\
            \"pr view\")\n\
                printf '%s' 'OPEN'\n\
                exit 0\n\
                ;;\n\
            *)\n\
                exit 0\n\
                ;;\n\
        esac\n";
    let gh_path = stubs.join("gh");
    fs::write(&gh_path, gh_script).unwrap();
    fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

    // Stub bin/flow phase-transition so the preflight's internal
    // recursion call returns success without spawning the test
    // binary again under different env conditions.
    let flow_stub_dir = parent.join("bin-flow-stub");
    fs::create_dir_all(&flow_stub_dir).unwrap();
    let flow_stub = flow_stub_dir.join("flow");
    let flow_script = "#!/bin/sh\n\
        case \"$1\" in\n\
            phase-transition)\n\
                printf '%s' '{\"status\":\"ok\"}'\n\
                exit 0\n\
                ;;\n\
            *)\n\
                exit 0\n\
                ;;\n\
        esac\n";
    fs::write(&flow_stub, flow_script).unwrap();
    fs::set_permissions(&flow_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stubs.display(), path);

    let output = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["complete-preflight", "--branch", BRANCH, "--auto"])
        .current_dir(&repo)
        .env("PATH", new_path)
        .env("FLOW_BIN_PATH", &flow_stub)
        .env("HOME", &repo)
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .output()
        .expect("spawn flow-rs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let last_json = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout; stdout={}", stdout));
    let value: Value = serde_json::from_str(last_json)
        .unwrap_or_else(|e| panic!("JSON parse failed: {} (line: {:?})", e, last_json));

    // Fixture's HEAD already points at origin/staging — git merge-base
    // succeeds, so merge_main returns "clean". A "merged" result would
    // mean origin had advanced (it can't on this fixture); a
    // "conflict"/"error" would mean fetch/merge-base targeted the
    // wrong ref.
    assert_eq!(
        value["status"], "ok",
        "complete-preflight on staging-trunked fixture must report ok; \
         got: {}",
        value
    );
    assert_eq!(
        value["merge"], "clean",
        "complete-preflight must report merge=clean (HEAD == \
         origin/staging on the fixture); got: {}",
        value
    );

    // Lock-in: nothing in stdout/stderr references origin/main.
    assert_no_origin_main(&stdout, "complete-preflight stdout");
    assert_no_origin_main(&stderr, "complete-preflight stderr");
}
