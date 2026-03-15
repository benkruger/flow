"""Tests for lib/sweep-update.py — updates issue status in sweep state."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "sweep-update.py")


def _import_module():
    """Import sweep-update.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "sweep_update", LIB_DIR / "sweep-update.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _make_sweep(issues=None):
    """Build a minimal sweep state dict."""
    return {
        "started_at": "2026-03-15T10:00:00-07:00",
        "status": "in_progress",
        "concurrency_limit": 3,
        "issues": issues or [],
    }


def _make_issue(number, title, status="queued"):
    """Build a minimal issue entry."""
    return {
        "number": number,
        "title": title,
        "status": status,
        "branch": None,
        "worktree": None,
        "pr_number": None,
        "pr_url": None,
        "agent_name": f"worker-{number}",
        "started_at": None,
        "completed_at": None,
        "error": None,
    }


# --- In-process tests ---


def test_update_issue_status(tmp_path):
    """update_issue changes the status field."""
    mod = _import_module()
    sweep = _make_sweep(issues=[_make_issue(42, "Test issue")])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 42, "in_progress")

    assert result is not None
    assert result["issues"][0]["status"] == "in_progress"
    assert result["issues"][0]["started_at"] is not None


def test_update_issue_pr_fields(tmp_path):
    """update_issue sets pr_number and pr_url."""
    mod = _import_module()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test", status="in_progress"),
    ])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(
        sweep_path, 42, "complete",
        pr_url="https://github.com/test/repo/pull/99",
        pr_number=99,
    )

    assert result["issues"][0]["pr_url"] == "https://github.com/test/repo/pull/99"
    assert result["issues"][0]["pr_number"] == 99
    assert result["issues"][0]["completed_at"] is not None


def test_update_nonexistent_issue(tmp_path):
    """update_issue returns None for missing issue."""
    mod = _import_module()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 999, "in_progress")

    assert result is None


def test_update_sets_completed_at_on_complete(tmp_path):
    """Completing an issue sets completed_at timestamp."""
    mod = _import_module()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test", status="in_progress"),
    ])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 42, "complete")

    assert result["issues"][0]["completed_at"] is not None
    assert "T" in result["issues"][0]["completed_at"]


def test_update_sets_completed_at_on_failed(tmp_path):
    """Failing an issue sets completed_at timestamp."""
    mod = _import_module()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test", status="in_progress"),
    ])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(
        sweep_path, 42, "failed", error="maxTurns exhausted",
    )

    assert result["issues"][0]["completed_at"] is not None
    assert result["issues"][0]["error"] == "maxTurns exhausted"


def test_update_sets_started_at_on_in_progress(tmp_path):
    """Starting an issue sets started_at if not already set."""
    mod = _import_module()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 42, "in_progress")

    assert result["issues"][0]["started_at"] is not None


def test_update_does_not_overwrite_started_at(tmp_path):
    """Re-entering in_progress does not overwrite existing started_at."""
    mod = _import_module()
    issue = _make_issue(42, "Test", status="in_progress")
    issue["started_at"] = "2026-03-15T09:00:00-07:00"
    sweep = _make_sweep(issues=[issue])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 42, "in_progress")

    assert result["issues"][0]["started_at"] == "2026-03-15T09:00:00-07:00"


def test_all_done_sets_sweep_status_complete(tmp_path):
    """When all issues are complete or failed, sweep status becomes complete."""
    mod = _import_module()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test A", status="complete"),
        _make_issue(43, "Test B", status="in_progress"),
    ])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 43, "complete")

    assert result["status"] == "complete"


def test_mixed_done_keeps_sweep_in_progress(tmp_path):
    """When some issues are still queued, sweep stays in_progress."""
    mod = _import_module()
    sweep = _make_sweep(issues=[
        _make_issue(42, "Test A", status="in_progress"),
        _make_issue(43, "Test B", status="queued"),
    ])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(sweep_path, 42, "complete")

    assert result["status"] == "in_progress"


def test_persists_to_disk(tmp_path):
    """update_issue writes the updated state back to disk."""
    mod = _import_module()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    mod.update_issue(sweep_path, 42, "in_progress")

    on_disk = json.loads(sweep_path.read_text())
    assert on_disk["issues"][0]["status"] == "in_progress"


def test_update_branch_and_worktree(tmp_path):
    """update_issue sets branch and worktree fields."""
    mod = _import_module()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    sweep_path = tmp_path / "sweep.json"
    sweep_path.write_text(json.dumps(sweep))

    result = mod.update_issue(
        sweep_path, 42, "in_progress",
        branch="sweep/42", worktree=".worktrees/sweep-42",
    )

    assert result["issues"][0]["branch"] == "sweep/42"
    assert result["issues"][0]["worktree"] == ".worktrees/sweep-42"


# --- CLI behavior (subprocess) ---


def test_cli_no_sweep_file(git_repo):
    """Running with no sweep.json returns no_sweep."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--issue", "42", "--status", "in_progress"],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "no_sweep"


def test_cli_happy_path(git_repo):
    """Full CLI round-trip: write sweep, run CLI, verify output."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    sweep = _make_sweep(issues=[_make_issue(42, "Test issue")])
    (state_dir / "sweep.json").write_text(json.dumps(sweep))

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--issue", "42", "--status", "in_progress",
         "--branch", "sweep/42"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["issue"] == 42
    assert data["new_status"] == "in_progress"

    on_disk = json.loads((state_dir / "sweep.json").read_text())
    assert on_disk["issues"][0]["status"] == "in_progress"
    assert on_disk["issues"][0]["branch"] == "sweep/42"


def test_cli_issue_not_found(git_repo):
    """Updating nonexistent issue returns error."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    (state_dir / "sweep.json").write_text(json.dumps(sweep))

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--issue", "999", "--status", "in_progress"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "#999" in data["message"]


def test_cli_write_failure(git_repo):
    """Read-only sweep.json returns a write error."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    sweep = _make_sweep(issues=[_make_issue(42, "Test")])
    sweep_file = state_dir / "sweep.json"
    sweep_file.write_text(json.dumps(sweep))
    sweep_file.chmod(0o444)

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--issue", "42", "--status", "in_progress"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    sweep_file.chmod(0o644)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Failed to update" in data["message"]


def test_cli_corrupt_file(git_repo):
    """Corrupt sweep.json returns read error."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "sweep.json").write_text("{bad json")

    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--issue", "42", "--status", "in_progress"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]
