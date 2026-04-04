"""Tests for bin/flow close-issues — close issues from FLOW prompt (Rust implementation).

All tests run via subprocess against the Rust binary.
"""

import json
import subprocess

from conftest import BIN_DIR

BIN_FLOW = str(BIN_DIR / "flow")


def _run(cwd, *args):
    """Run close-issues via bin/flow."""
    return subprocess.run(
        [BIN_FLOW, "close-issues", *args],
        capture_output=True,
        text=True,
        cwd=str(cwd),
    )


# --- CLI argument validation ---


def test_cli_requires_state_file_argument(target_project):
    """CLI fails when --state-file is not provided."""
    result = _run(target_project)
    assert result.returncode == 2


# --- CLI with state file ---


def test_cli_no_prompt_field(target_project):
    """State file without prompt field outputs ok with empty lists."""
    state_file = target_project / "state.json"
    state_file.write_text(json.dumps({"branch": "test"}))

    result = _run(target_project, "--state-file", str(state_file))
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["closed"] == []
    assert output["failed"] == []


def test_cli_empty_prompt(target_project):
    """State file with empty prompt outputs ok with empty lists."""
    state_file = target_project / "state.json"
    state_file.write_text(json.dumps({"prompt": "", "branch": "test"}))

    result = _run(target_project, "--state-file", str(state_file))
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["closed"] == []
    assert output["failed"] == []


def test_cli_corrupt_state_file(target_project):
    """Corrupt state file returns structured error."""
    state_file = target_project / "state.json"
    state_file.write_text("{corrupt")

    result = _run(target_project, "--state-file", str(state_file))
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "state file" in output["message"].lower()


def test_cli_missing_state_file(target_project):
    """Missing state file returns structured error."""
    result = _run(target_project, "--state-file", "/nonexistent/state.json")
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "state file" in output["message"].lower()


def test_cli_with_issues_attempts_close(target_project):
    """State file with issue references attempts to close them.

    Without gh auth, the issues will fail to close, but we verify
    the output structure has closed and failed arrays.
    """
    state_file = target_project / "state.json"
    state_file.write_text(
        json.dumps(
            {
                "prompt": "fix #42 and #99",
                "repo": "test/test",
                "branch": "test",
            }
        )
    )

    result = _run(target_project, "--state-file", str(state_file))
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert "closed" in output
    assert "failed" in output
    # Both should be in failed since gh isn't authed in tests
    all_numbers = [e["number"] for e in output["closed"]] + [e["number"] for e in output["failed"]]
    assert 42 in all_numbers
    assert 99 in all_numbers


# --- Tombstone: Python files removed ---


def test_close_issues_py_removed():
    """Tombstone: lib/close-issues.py ported to Rust in PR #831. Must not return."""
    from conftest import LIB_DIR

    assert not (LIB_DIR / "close-issues.py").exists(), "lib/close-issues.py was ported to Rust and should not exist"
