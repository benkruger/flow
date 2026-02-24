"""Shared fixtures for FLOW plugin tests."""

import json
import os
import subprocess
import tempfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
HOOKS_DIR = REPO_ROOT / "hooks"
SKILLS_DIR = REPO_ROOT / "skills"
DOCS_DIR = REPO_ROOT / "docs"
BIN_DIR = REPO_ROOT / "bin"


@pytest.fixture(autouse=True, scope="session")
def _subprocess_coverage():
    """Route subprocess coverage data to the project root.

    Tests run Python scripts via subprocess with cwd set to temp dirs.
    Without this, coverage data files land in the temp dir and are never
    combined. This fixture writes a config with an absolute data_file
    path and sets COVERAGE_PROCESS_START so the coverage .pth hook
    activates in every subprocess.
    """
    config = (
        "[run]\n"
        f"data_file = {REPO_ROOT / '.coverage'}\n"
        "parallel = true\n"
        f"source =\n"
        f"    {HOOKS_DIR}\n"
    )
    fd, config_path = tempfile.mkstemp(suffix=".ini", prefix="cov_subprocess_")
    os.write(fd, config.encode())
    os.close(fd)

    os.environ["COVERAGE_PROCESS_START"] = config_path
    yield
    os.environ.pop("COVERAGE_PROCESS_START", None)
    os.unlink(config_path)


@pytest.fixture
def git_repo(tmp_path):
    """Create a minimal git repo with an initial commit."""
    subprocess.run(
        ["git", "init"], cwd=tmp_path,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.email", "test@test.com"], cwd=tmp_path,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "Test"], cwd=tmp_path,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"], cwd=tmp_path,
        capture_output=True, check=True,
    )
    return tmp_path


@pytest.fixture
def state_dir(git_repo):
    """Create .flow-states/ inside the git repo."""
    d = git_repo / ".flow-states"
    d.mkdir(parents=True)
    return d


def make_state(current_phase=1, phase_statuses=None):
    """Build a minimal state dict.

    phase_statuses is a dict like {1: "complete", 2: "in_progress"}.
    Unspecified phases default to "pending".
    """
    phase_statuses = phase_statuses or {}
    names = {
        1: "Start", 2: "Research", 3: "Design", 4: "Plan",
        5: "Code", 6: "Review", 7: "Reflect", 8: "Cleanup",
    }
    phases = {}
    for i in range(1, 9):
        status = phase_statuses.get(i, "pending")
        phases[str(i)] = {
            "name": names[i],
            "status": status,
            "started_at": None,
            "completed_at": None,
            "session_started_at": "2026-01-01T00:00:00Z" if status == "in_progress" else None,
            "cumulative_seconds": 0,
            "visit_count": 1 if status in ("complete", "in_progress") else 0,
        }
    return {
        "feature": "Test Feature",
        "branch": "test-feature",
        "worktree": ".worktrees/test-feature",
        "pr_number": 1,
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00Z",
        "current_phase": current_phase,
        "notes": [],
        "phases": phases,
    }


def write_state(state_dir, branch, state_dict):
    """Write a JSON state file for the given branch."""
    path = state_dir / f"{branch}.json"
    path.write_text(json.dumps(state_dict, indent=2))
    return path