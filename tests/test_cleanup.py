"""Tests for lib/cleanup.py — the cleanup orchestrator."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "cleanup.py")

# Import cleanup.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "cleanup", LIB_DIR / "cleanup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(project_root, branch, worktree, pr=None, delete_remote=False):
    """Run cleanup.py via subprocess."""
    args = [sys.executable, SCRIPT, str(project_root),
            "--branch", branch, "--worktree", worktree]
    if pr:
        args.extend(["--pr", str(pr)])
    if delete_remote:
        args.append("--delete-remote")
    result = subprocess.run(args, capture_output=True, text=True)
    return result


def _setup_feature(git_repo, branch="test-feature"):
    """Create a worktree and state file for testing cleanup."""
    # Create worktree
    wt_rel = f".worktrees/{branch}"
    subprocess.run(
        ["git", "worktree", "add", wt_rel, "-b", branch],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    # Create state file
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    state = make_state(current_phase=8, phase_statuses={
        i: "complete" for i in range(1, 9)
    })
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
        capture_output=True, text=True,
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


def test_cleanup_skips_pr_and_remote_by_default(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)
    data = json.loads(result.stdout)
    assert data["steps"]["pr_close"] == "skipped"
    assert data["steps"]["remote_branch"] == "skipped"
    assert data["steps"]["local_branch"] == "skipped"


def test_cleanup_full_happy_path(git_repo):
    """Single invocation asserts all 6 step results, return code, status,
    and all 3 filesystem effects (worktree, state file, log file)."""
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel)

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"

    # All 6 step results
    assert data["steps"]["pr_close"] == "skipped"
    assert data["steps"]["worktree"] == "removed"
    assert data["steps"]["remote_branch"] == "skipped"
    assert data["steps"]["local_branch"] == "skipped"
    assert data["steps"]["state_file"] == "deleted"
    assert data["steps"]["log_file"] == "deleted"

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
        cwd=str(git_repo), capture_output=True,
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


def test_abort_deletes_local_branch(git_repo):
    wt_rel = _setup_feature(git_repo)
    # Remove worktree first so branch can be deleted
    subprocess.run(
        ["git", "worktree", "remove", wt_rel, "--force"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    result = _run(git_repo, "test-feature", wt_rel, delete_remote=True)
    data = json.loads(result.stdout)
    assert data["steps"]["local_branch"] == "deleted"


def test_abort_remote_branch_fails_gracefully(git_repo):
    wt_rel = _setup_feature(git_repo)
    result = _run(git_repo, "test-feature", wt_rel, delete_remote=True)
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
