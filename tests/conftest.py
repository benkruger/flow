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
FRAMEWORKS_DIR = REPO_ROOT / "frameworks"

sys.path.insert(0, str(LIB_DIR))
from flow_utils import PHASE_NAMES, PHASE_ORDER


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
    config_path = template / ".git" / "config"
    with open(config_path, "a") as f:
        f.write(
            "[user]\n\temail = test@test.com\n\tname = Test\n"
            "[commit]\n\tgpgsign = false\n"
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
    head = (git_repo / ".git" / "HEAD").read_text().strip()
    return head.removeprefix("ref: refs/heads/")


@pytest.fixture
def state_dir(git_repo):
    """Create .flow-states/ inside the git repo."""
    d = git_repo / ".flow-states"
    d.mkdir(parents=True)
    return d


@pytest.fixture
def target_project(git_repo):
    """Simulate a target project where FLOW is installed.

    Uses a non-bash bin/ci (Python with shebang) to catch hardcoded
    bash assumptions. Has no bin/flow — target projects never have it.
    This fixture exists because the FLOW repo itself is Python with bash
    scripts, making it the worst possible test environment for a
    multi-framework plugin.
    """
    bin_dir = git_repo / "bin"
    bin_dir.mkdir()
    (bin_dir / "ci").write_text(
        "#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n"
    )
    (bin_dir / "ci").chmod(0o755)
    subprocess.run(["git", "add", "-A"], cwd=str(git_repo), check=True,
                   capture_output=True)
    subprocess.run(["git", "commit", "-m", "add bin/ci"], cwd=str(git_repo),
                   check=True, capture_output=True)
    return git_repo


def make_state(current_phase="flow-start", phase_statuses=None, framework="rails"):
    """Build a minimal state dict.

    phase_statuses is a dict like {"flow-start": "complete", "flow-plan": "in_progress"}.
    Unspecified phases default to "pending".
    framework is "rails" or "python" (default "rails").
    """
    phase_statuses = phase_statuses or {}
    phases = {}
    for key in PHASE_ORDER:
        status = phase_statuses.get(key, "pending")
        phases[key] = {
            "name": PHASE_NAMES[key],
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
        "files": {
            "plan": None,
            "dag": None,
            "log": ".flow-states/test-feature.log",
            "state": ".flow-states/test-feature.json",
        },
        "plan_file": None,
        "session_id": None,
        "transcript_path": None,
        "notes": [],
        "prompt": "test feature",
        "phases": phases,
        "issues_filed": [],
    }
    return state


def write_state(state_dir, branch, state_dict):
    """Write a JSON state file for the given branch."""
    path = state_dir / f"{branch}.json"
    path.write_text(json.dumps(state_dict, indent=2))
    return path
