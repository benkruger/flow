"""Tests for bin/flow — the subcommand dispatcher."""

import json
import os
import subprocess

from conftest import BIN_DIR, LIB_DIR, REPO_ROOT


SCRIPT = str(BIN_DIR / "flow")


def _run(*args, cwd=None, extra_env=None):
    """Run bin/flow with the given arguments."""
    env = None
    if extra_env:
        env = {**os.environ, **extra_env}
    result = subprocess.run(
        ["bash", SCRIPT, *args],
        capture_output=True, text=True,
        cwd=cwd or str(REPO_ROOT),
        env=env,
    )
    return result


def test_no_subcommand_returns_error_json():
    """Running with no arguments returns JSON error and exit 1."""
    result = _run()
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Usage" in data["message"]


def test_unknown_subcommand_returns_error_json():
    """Running with a nonexistent subcommand returns JSON error and exit 1."""
    result = _run("nonexistent-command")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "nonexistent-command" in data["message"]


def test_dispatches_to_correct_script():
    """Known subcommand dispatches to the matching .py file in lib/."""
    # extract-release-notes with no args exits 1 with usage message
    result = _run("extract-release-notes")
    assert result.returncode == 1
    assert "Usage" in result.stdout


def test_passes_arguments_through():
    """Arguments after the subcommand are passed to the Python script."""
    # extract-release-notes with an invalid version format exits 1
    result = _run("extract-release-notes", "../../etc/passwd")
    assert result.returncode == 1
    assert "invalid version format" in result.stdout


def test_exit_code_passes_through():
    """Exit code from the Python script is preserved."""
    # check-phase with --required plan and no state file exits non-zero
    result = _run("check-phase", "--required", "plan", cwd="/tmp")
    assert result.returncode != 0


def test_every_lib_script_is_reachable():
    """Every .py file in lib/ is reachable as a subcommand."""
    py_files = sorted(LIB_DIR.glob("*.py"))
    # Exclude flow_utils.py (library, not a subcommand)
    scripts = [f for f in py_files if f.name != "flow_utils.py"]
    assert len(scripts) > 0

    for script in scripts:
        subcmd = script.stem
        # Verify the script file exists and bin/flow can find it
        # (we check by running with no args — the script should run,
        # not produce "Unknown subcommand")
        # FLOW_CI_RUNNING prevents bin/flow ci from running the full
        # test suite recursively when cwd is a repo with bin/ci.
        result = _run(subcmd, extra_env={"FLOW_CI_RUNNING": "1"})
        assert "Unknown subcommand" not in result.stdout, (
            f"bin/flow cannot find subcommand '{subcmd}' "
            f"for lib/{script.name}"
        )
