"""Shared fixtures for FLOW plugin tests."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
HOOKS_DIR = REPO_ROOT / "hooks"
LIB_DIR = REPO_ROOT / "lib"
SKILLS_DIR = REPO_ROOT / "skills"
DOCS_DIR = REPO_ROOT / "docs"
BIN_DIR = REPO_ROOT / "bin"

sys.path.insert(0, str(LIB_DIR))
from flow_utils import PHASE_NAMES


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
        f"    {LIB_DIR}\n"
    )
    fd, config_path = tempfile.mkstemp(suffix=".ini", prefix="cov_subprocess_")
    os.write(fd, config.encode())
    os.close(fd)

    os.environ["COVERAGE_PROCESS_START"] = config_path
    yield
    os.environ.pop("COVERAGE_PROCESS_START", None)
    os.unlink(config_path)


@pytest.fixture(scope="session")
def _git_repo_template(tmp_path_factory):
    """Create a git repo template once per worker for copying."""
    template = tmp_path_factory.mktemp("git-template")
    subprocess.run(
        ["git", "init"], cwd=template,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.email", "test@test.com"], cwd=template,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "Test"], cwd=template,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "commit.gpgsign", "false"], cwd=template,
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"], cwd=template,
        capture_output=True, check=True,
    )
    return template


@pytest.fixture
def git_repo(_git_repo_template, tmp_path):
    """Copy the template git repo for per-test isolation."""
    repo = tmp_path / "repo"
    shutil.copytree(_git_repo_template, repo)
    return repo


@pytest.fixture
def branch(git_repo):
    """Return the current branch name of the git repo."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


@pytest.fixture
def state_dir(git_repo):
    """Create .flow-states/ inside the git repo."""
    d = git_repo / ".flow-states"
    d.mkdir(parents=True)
    return d


def make_state(current_phase=1, phase_statuses=None, mode=None, framework="rails"):
    """Build a minimal state dict.

    phase_statuses is a dict like {1: "complete", 2: "in_progress"}.
    Unspecified phases default to "pending".
    mode is an optional string ("light") added to the top-level state.
    framework is "rails" or "python" (default "rails").
    """
    phase_statuses = phase_statuses or {}
    phases = {}
    for i in range(1, 10):
        status = phase_statuses.get(i, "pending")
        phases[str(i)] = {
            "name": PHASE_NAMES[i],
            "status": status,
            "started_at": None,
            "completed_at": None,
            "session_started_at": "2026-01-01T00:00:00Z" if status == "in_progress" else None,
            "cumulative_seconds": 0,
            "visit_count": 1 if status in ("complete", "in_progress") else 0,
        }
    state = {
        "feature": "Test Feature",
        "branch": "test-feature",
        "worktree": ".worktrees/test-feature",
        "pr_number": 1,
        "pr_url": "https://github.com/test/test/pull/1",
        "started_at": "2026-01-01T00:00:00Z",
        "current_phase": current_phase,
        "framework": framework,
        "notes": [],
        "phases": phases,
    }
    if mode is not None:
        state["mode"] = mode
    return state


def write_state(state_dir, branch, state_dict):
    """Write a JSON state file for the given branch."""
    path = state_dir / f"{branch}.json"
    path.write_text(json.dumps(state_dict, indent=2))
    return path