"""Tests for bin/flow close-issue — close a single GitHub issue (Rust implementation).

All tests run via subprocess against the Rust binary.
"""

import json
import subprocess

from conftest import BIN_DIR

BIN_FLOW = str(BIN_DIR / "flow")


def _run(cwd, *args):
    """Run close-issue via bin/flow."""
    return subprocess.run(
        [BIN_FLOW, "close-issue", *args],
        capture_output=True,
        text=True,
        cwd=str(cwd),
    )


# --- CLI argument validation ---


def test_cli_requires_number_argument(target_project):
    """CLI fails when --number is not provided."""
    result = _run(target_project, "--repo", "benkruger/flow")
    assert result.returncode == 2


# --- CLI with mocked gh (gh not available in test env) ---


def test_cli_with_repo_and_number_runs(target_project):
    """CLI with --number and --repo attempts to close the issue.

    In a test environment without gh auth, this will fail with an error,
    but we verify the CLI accepts the arguments and returns structured JSON.
    """
    result = _run(target_project, "--number", "117", "--repo", "benkruger/flow")
    # gh will fail (no auth in test env), so verify structured error JSON
    output = json.loads(result.stdout)
    if result.returncode == 0:
        assert output["status"] == "ok"
    else:
        assert output["status"] == "error"


def test_cli_auto_detects_repo(target_project):
    """CLI attempts to auto-detect repo when --repo is omitted.

    The test git repo has no remote, so detection will fail with structured error.
    """
    result = _run(target_project, "--number", "117")
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Could not detect repo" in output["message"]
