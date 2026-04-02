"""Tests for lib/cleanup.py — the cleanup orchestrator."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, PHASE_ORDER, make_state, write_state

SCRIPT = str(LIB_DIR / "cleanup.py")

# Import cleanup.py for in-process unit tests
_spec = importlib.util.spec_from_file_location("cleanup", LIB_DIR / "cleanup.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(project_root, branch, worktree, pr=None):
    """Run cleanup.py via subprocess."""
    args = [sys.executable, SCRIPT, str(project_root), "--branch", branch, "--worktree", worktree]
    if pr:
        args.extend(["--pr", str(pr)])
    result = subprocess.run(args, capture_output=True, text=True)
    return result


def _setup_feature(git_repo, branch="test-feature"):
    """Create a worktree and state file for testing cleanup."""
    # Create worktree
    wt_rel = f".worktrees/{branch}"
    subprocess.run(
        ["git", "worktree", "add", wt_rel, "-b", branch],
        cwd=str(git_repo),
        capture_output=True,
        check=True,
    )

    # Create state file
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    state = make_state(current_phase="flow-complete", phase_statuses={k: "complete" for k in PHASE_ORDER})
    state["branch"] = branch
    state["worktree"] = wt_rel
    write_state(state_dir, branch, state)

    # Create log file
    (state_dir / f"{branch}.log").write_text("test log\n")

    return wt_rel


# --- CLI behavior ---


def test_missing_args_returns_error():
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0


def test_invalid_project_root_returns_error(tmp_path):
    result = _run(tmp_path / "nonexistent", "branch", ".worktrees/branch")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


# --- Cleanup mode (no --delete-remote, no --pr) ---


def test_cleanup_removes_worktree(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["steps"]["worktree"] == "removed"
    assert not (git_repo / wt_rel).exists()


def test_cleanup_deletes_state_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["state_file"] == "deleted"
    assert not (git_repo / ".flow-states" / "test-feature.json").exists()


def test_cleanup_deletes_log_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["log_file"] == "deleted"
    assert not (git_repo / ".flow-states" / "test-feature.log").exists()


def test_cleanup_deletes_plan_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    plan = git_repo / ".flow-states" / "test-feature-plan.md"
    plan.write_text("# Plan\n")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["plan_file"] == "deleted"
    assert not plan.exists()


def test_cleanup_skips_missing_plan_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["plan_file"] == "skipped"


def test_cleanup_deletes_dag_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    dag = git_repo / ".flow-states" / "test-feature-dag.md"
    dag.write_text("# DAG\n")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["dag_file"] == "deleted"
    assert not dag.exists()


def test_cleanup_skips_missing_dag_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["dag_file"] == "skipped"


def test_cleanup_deletes_frozen_phases_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    # Create frozen phases file
    frozen = git_repo / ".flow-states" / "test-feature-phases.json"
    frozen.write_text('{"phases": {}, "order": []}')
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["frozen_phases"] == "deleted"
    assert not frozen.exists()


def test_cleanup_skips_missing_frozen_phases(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["frozen_phases"] == "skipped"


def test_cleanup_skips_pr_by_default(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["pr_close"] == "skipped"


def test_cleanup_deletes_ci_sentinel(git_repo):
    wt_rel = _setup_feature(git_repo)
    sentinel = git_repo / ".flow-states" / "test-feature-ci-passed"
    sentinel.write_text("snapshot\n")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["ci_sentinel"] == "deleted"
    assert not sentinel.exists()


def test_cleanup_skips_missing_ci_sentinel(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["ci_sentinel"] == "skipped"


def test_cleanup_deletes_timings_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    timings = git_repo / ".flow-states" / "test-feature-timings.md"
    timings.write_text("| Phase | Duration |\n")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["timings_file"] == "deleted"
    assert not timings.exists()


def test_cleanup_skips_missing_timings_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["timings_file"] == "skipped"


def test_cleanup_deletes_closed_issues_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    closed = git_repo / ".flow-states" / "test-feature-closed-issues.json"
    closed.write_text('[{"number": 42, "url": "https://github.com/t/t/issues/42"}]')
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["closed_issues_file"] == "deleted"
    assert not closed.exists()


def test_cleanup_skips_missing_closed_issues_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["closed_issues_file"] == "skipped"


def test_cleanup_deletes_issues_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    issues = git_repo / ".flow-states" / "test-feature-issues.md"
    issues.write_text("| Label | Title |\n")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["issues_file"] == "deleted"
    assert not issues.exists()


def test_cleanup_skips_missing_issues_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["issues_file"] == "skipped"


def test_cleanup_full_happy_path(git_repo):
    """Single invocation asserts all 10 step results, return code, status,
    and all 3 filesystem effects (worktree, state file, log file)."""
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"

    # All 10 step results
    assert data["steps"]["pr_close"] == "skipped"
    assert data["steps"]["worktree"] == "removed"
    assert data["steps"]["remote_branch"].startswith("failed:")  # no remote
    assert data["steps"]["local_branch"] == "deleted"
    assert data["steps"]["state_file"] == "deleted"
    assert data["steps"]["plan_file"] == "skipped"
    assert data["steps"]["dag_file"] == "skipped"
    assert data["steps"]["log_file"] == "deleted"
    assert data["steps"]["ci_sentinel"] == "skipped"
    assert data["steps"]["timings_file"] == "skipped"
    assert data["steps"]["closed_issues_file"] == "skipped"
    assert data["steps"]["issues_file"] == "skipped"

    # All 3 filesystem effects
    assert not (git_repo / wt_rel).exists()
    assert not (git_repo / ".flow-states" / "test-feature.json").exists()
    assert not (git_repo / ".flow-states" / "test-feature.log").exists()


# --- Missing resources ---


def test_cleanup_skips_missing_worktree(git_repo):
    _setup_feature(git_repo)
    # Remove worktree before cleanup
    subprocess.run(
        ["git", "worktree", "remove", ".worktrees/test-feature", "--force"],
        cwd=str(git_repo),
        capture_output=True,
    )
    result = _run(git_repo, "test-feature", ".worktrees/test-feature")
    data = json.loads(result.stdout)
    assert data["steps"]["worktree"] == "skipped"


def test_cleanup_skips_missing_state_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    (git_repo / ".flow-states" / "test-feature.json").unlink()
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["state_file"] == "skipped"


def test_cleanup_skips_missing_log_file(git_repo):
    wt_rel = _setup_feature(git_repo)
    (git_repo / ".flow-states" / "test-feature.log").unlink()
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["log_file"] == "skipped"


# --- Abort mode (--delete-remote --pr) ---


def test_cleanup_always_deletes_local_branch(git_repo):
    """Branch deletion happens without --delete-remote flag."""
    wt_rel = _setup_feature(git_repo)
    # Remove worktree first so branch can be deleted
    subprocess.run(
        ["git", "worktree", "remove", wt_rel, "--force"],
        cwd=str(git_repo),
        capture_output=True,
        check=True,
    )
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["local_branch"] == "deleted"


def test_cleanup_always_attempts_remote_branch(git_repo):
    """Remote branch deletion is attempted without --delete-remote flag."""
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    # No remote configured, so push --delete will fail
    assert data["steps"]["remote_branch"].startswith("failed:")


def test_abort_deletes_local_branch(git_repo):
    wt_rel = _setup_feature(git_repo)
    # Remove worktree first so branch can be deleted
    subprocess.run(
        ["git", "worktree", "remove", wt_rel, "--force"],
        cwd=str(git_repo),
        capture_output=True,
        check=True,
    )
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["local_branch"] == "deleted"


def test_abort_remote_branch_fails_gracefully(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    # No remote configured, so push --delete will fail
    assert data["steps"]["remote_branch"].startswith("failed:")


def test_abort_pr_close_fails_gracefully(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel, pr=999)
    data = json.loads(result.stdout)
    # No GitHub remote configured, so gh pr close will fail
    assert data["steps"]["pr_close"].startswith("failed:")


# --- In-process tests ---


def test_run_cmd_handles_exception(monkeypatch):
    def _raise(*args, **kwargs):
        raise OSError("command not found")

    monkeypatch.setattr(subprocess, "run", _raise)
    ok, output = _mod._run_cmd(["fake"], ".")
    assert not ok
    assert "command not found" in output


def test_state_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    state_file = git_repo / ".flow-states" / "test-feature.json"
    original_unlink = state_file.unlink.__func__

    call_count = 0

    def _fail_first_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_first_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["state_file"].startswith("failed:")


def test_log_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    log_file = git_repo / ".flow-states" / "test-feature.log"
    original_unlink = log_file.unlink.__func__

    call_count = 0

    def _fail_second_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 2:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_second_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["log_file"].startswith("failed:")


def test_frozen_phases_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    frozen = git_repo / ".flow-states" / "test-feature-phases.json"
    frozen.write_text('{"phases": {}, "order": []}')
    original_unlink = frozen.unlink.__func__

    call_count = 0

    def _fail_third_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 3:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_third_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["frozen_phases"].startswith("failed:")


def test_ci_sentinel_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    sentinel = git_repo / ".flow-states" / "test-feature-ci-passed"
    sentinel.write_text("snapshot\n")
    original_unlink = sentinel.unlink.__func__

    call_count = 0

    # state_file=1, log_file=2, ci_sentinel=3 (frozen_phases skipped — no file)
    def _fail_third_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 3:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_third_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["ci_sentinel"].startswith("failed:")


def test_timings_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    timings = git_repo / ".flow-states" / "test-feature-timings.md"
    timings.write_text("| Phase | Duration |\n")
    original_unlink = timings.unlink.__func__

    call_count = 0

    # state_file=1, log_file=2, timings=3 (frozen_phases + ci_sentinel skipped)
    def _fail_third_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 3:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_third_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["timings_file"].startswith("failed:")


def test_plan_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    plan = git_repo / ".flow-states" / "test-feature-plan.md"
    plan.write_text("# Plan\n")
    original_unlink = plan.unlink.__func__

    call_count = 0

    # state_file=1, plan_file=2
    def _fail_second_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 2:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_second_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["plan_file"].startswith("failed:")


def test_dag_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    dag = git_repo / ".flow-states" / "test-feature-dag.md"
    dag.write_text("# DAG\n")
    original_unlink = dag.unlink.__func__

    call_count = 0

    # state_file=1, dag_file=2 (plan_file skipped — no file)
    def _fail_second_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 2:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_second_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["dag_file"].startswith("failed:")


def test_closed_issues_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    closed = git_repo / ".flow-states" / "test-feature-closed-issues.json"
    closed.write_text('[{"number": 42}]')
    original_unlink = closed.unlink.__func__

    call_count = 0

    # state_file=1, log_file=2, closed_issues=3 (frozen_phases + ci_sentinel + timings skipped)
    def _fail_third_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 3:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_third_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["closed_issues_file"].startswith("failed:")


def test_issues_file_unlink_failure(git_repo, monkeypatch):
    wt_rel = _setup_feature(git_repo)
    issues = git_repo / ".flow-states" / "test-feature-issues.md"
    issues.write_text("| Label | Title |\n")
    original_unlink = issues.unlink.__func__

    call_count = 0

    # state_file=1, log_file=2, issues=3 (frozen_phases + ci_sentinel + timings + closed_issues skipped)
    def _fail_third_unlink(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 3:
            raise PermissionError("no permission")
        return original_unlink(self, *args, **kwargs)

    from pathlib import PosixPath

    monkeypatch.setattr(PosixPath, "unlink", _fail_third_unlink)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["issues_file"].startswith("failed:")


# --- tmp/ directory cleanup ---


def test_cleanup_removes_worktree_tmp_in_flow_repo(git_repo):
    """tmp/ in worktree is removed when flow-phases.json exists."""
    wt_rel = _setup_feature(git_repo)
    # Mark as FLOW repo
    (git_repo / "flow-phases.json").write_text("{}")
    # Create tmp/ inside the worktree
    wt_tmp = git_repo / wt_rel / "tmp"
    wt_tmp.mkdir()
    (wt_tmp / "release-notes-v1.0.md").write_text("notes")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["worktree_tmp"] == "removed"


def test_cleanup_skips_tmp_without_flow_phases(git_repo):
    """tmp/ in worktree is skipped when flow-phases.json does not exist."""
    wt_rel = _setup_feature(git_repo)
    # No flow-phases.json — not a FLOW repo
    wt_tmp = git_repo / wt_rel / "tmp"
    wt_tmp.mkdir()
    (wt_tmp / "some-file.txt").write_text("data")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["worktree_tmp"] == "skipped"


def test_cleanup_skips_missing_worktree_tmp(git_repo):
    """No tmp/ in worktree reports skipped."""
    wt_rel = _setup_feature(git_repo)
    (git_repo / "flow-phases.json").write_text("{}")
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["worktree_tmp"] == "skipped"


def test_cleanup_tmp_rmtree_failure(git_repo, monkeypatch):
    """rmtree failure is reported gracefully."""
    import shutil

    wt_rel = _setup_feature(git_repo)
    (git_repo / "flow-phases.json").write_text("{}")
    wt_tmp = git_repo / wt_rel / "tmp"
    wt_tmp.mkdir()
    (wt_tmp / "file.txt").write_text("data")

    original_rmtree = shutil.rmtree

    def _fail_rmtree(path, *args, **kwargs):
        if str(path).endswith("/tmp"):
            raise PermissionError("no permission")
        return original_rmtree(path, *args, **kwargs)

    monkeypatch.setattr(shutil, "rmtree", _fail_rmtree)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel)
    assert steps["worktree_tmp"].startswith("failed:")


# --- --pull flag tests ---


def test_pull_flag_runs_git_pull(git_repo):
    """--pull flag causes git pull origin main after worktree removal."""
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    # Without --pull, no git_pull step
    assert "git_pull" not in data["steps"]


def test_pull_flag_present_runs_pull(git_repo):
    """--pull flag present adds git_pull to steps dict."""
    wt_rel = _setup_feature(git_repo)
    args = [sys.executable, SCRIPT, str(git_repo), "--branch", "test-feature", "--worktree", wt_rel, "--pull"]
    result = subprocess.run(args, capture_output=True, text=True)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert "git_pull" in data["steps"]
    # No remote configured, so pull will fail — but it should be reported
    assert data["steps"]["git_pull"].startswith("failed:")


def test_pull_flag_in_process(git_repo):
    """In-process cleanup with pull=True adds git_pull step."""
    wt_rel = _setup_feature(git_repo)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel, pull=True)
    assert "git_pull" in steps
    # No remote, so pull fails gracefully
    assert steps["git_pull"].startswith("failed:")


def test_pull_false_no_git_pull_step(git_repo):
    """In-process cleanup with pull=False has no git_pull step."""
    wt_rel = _setup_feature(git_repo)
    steps = _mod.cleanup(git_repo, "test-feature", wt_rel, pull=False)
    assert "git_pull" not in steps
