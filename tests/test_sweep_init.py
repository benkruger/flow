"""Tests for lib/sweep-init.py — creates the sweep state file."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "sweep-init.py")


def _import_module():
    """Import sweep-init.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "sweep_init", LIB_DIR / "sweep-init.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# --- In-process tests ---


def test_create_sweep_basic(tmp_path):
    """create_sweep builds correct structure with queued issues."""
    mod = _import_module()
    sweep_path = tmp_path / ".flow-states" / "sweep.json"
    issues = [
        {"number": 42, "title": "Fix login"},
        {"number": 43, "title": "Add feature"},
    ]

    result = mod.create_sweep(sweep_path, issues)

    assert result["status"] == "in_progress"
    assert result["concurrency_limit"] == 3
    assert len(result["issues"]) == 2
    assert result["issues"][0]["number"] == 42
    assert result["issues"][0]["status"] == "queued"
    assert result["issues"][0]["agent_name"] == "worker-42"
    assert result["issues"][1]["number"] == 43
    assert "T" in result["started_at"]


def test_create_sweep_custom_limit(tmp_path):
    """create_sweep respects custom concurrency limit."""
    mod = _import_module()
    sweep_path = tmp_path / ".flow-states" / "sweep.json"
    issues = [{"number": 1, "title": "Test"}]

    result = mod.create_sweep(sweep_path, issues, concurrency_limit=5)

    assert result["concurrency_limit"] == 5


def test_create_sweep_persists_to_disk(tmp_path):
    """create_sweep writes valid JSON to disk."""
    mod = _import_module()
    sweep_path = tmp_path / ".flow-states" / "sweep.json"
    issues = [{"number": 42, "title": "Test"}]

    mod.create_sweep(sweep_path, issues)

    on_disk = json.loads(sweep_path.read_text())
    assert len(on_disk["issues"]) == 1
    assert on_disk["issues"][0]["number"] == 42


def test_create_sweep_creates_parent_directory(tmp_path):
    """create_sweep creates .flow-states/ if it does not exist."""
    mod = _import_module()
    sweep_path = tmp_path / ".flow-states" / "sweep.json"
    assert not sweep_path.parent.exists()

    mod.create_sweep(sweep_path, [{"number": 1, "title": "Test"}])

    assert sweep_path.exists()


def test_issue_fields_initialized_to_null(tmp_path):
    """All optional issue fields start as null."""
    mod = _import_module()
    sweep_path = tmp_path / ".flow-states" / "sweep.json"
    issues = [{"number": 42, "title": "Test"}]

    result = mod.create_sweep(sweep_path, issues)
    issue = result["issues"][0]

    assert issue["branch"] is None
    assert issue["worktree"] is None
    assert issue["pr_number"] is None
    assert issue["pr_url"] is None
    assert issue["started_at"] is None
    assert issue["completed_at"] is None
    assert issue["error"] is None


# --- CLI behavior (subprocess) ---


def test_cli_happy_path(git_repo):
    """Full CLI round-trip: create sweep, verify output."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    issues_json = json.dumps([
        {"number": 42, "title": "Fix bug"},
        {"number": 43, "title": "Add feature"},
    ])

    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", issues_json],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["issue_count"] == 2

    on_disk = json.loads((state_dir / "sweep.json").read_text())
    assert len(on_disk["issues"]) == 2


def test_cli_custom_limit(git_repo):
    """--limit flag sets concurrency."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    issues_json = json.dumps([{"number": 1, "title": "Test"}])

    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", issues_json, "--limit", "5"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 0
    on_disk = json.loads((state_dir / "sweep.json").read_text())
    assert on_disk["concurrency_limit"] == 5


def test_cli_fails_if_exists(git_repo):
    """Refuses to overwrite existing sweep.json without --force."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "sweep.json").write_text("{}")
    issues_json = json.dumps([{"number": 1, "title": "Test"}])

    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", issues_json],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "already exists" in data["message"]


def test_cli_force_overwrites(git_repo):
    """--force overwrites existing sweep.json."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "sweep.json").write_text("{}")
    issues_json = json.dumps([{"number": 42, "title": "New"}])

    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", issues_json, "--force"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 0
    on_disk = json.loads((state_dir / "sweep.json").read_text())
    assert on_disk["issues"][0]["number"] == 42


def test_cli_write_failure(git_repo):
    """Read-only directory returns a write error."""
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    state_dir.chmod(0o444)
    issues_json = json.dumps([{"number": 42, "title": "Test"}])

    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", issues_json],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    state_dir.chmod(0o755)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Failed to create" in data["message"]


def test_cli_invalid_json(git_repo):
    """Invalid JSON for --issues returns error."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", "not json"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Invalid JSON" in data["message"]


def test_cli_empty_array(git_repo):
    """Empty issues array returns error."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--issues", "[]"],
        capture_output=True, text=True, cwd=str(git_repo),
    )

    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "non-empty" in data["message"]
