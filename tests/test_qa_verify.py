"""Tests for lib/qa-verify.py — verify QA assertions after a completed flow."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("qa-verify")


# --- verify checks ---


def test_verify_all_pass(tmp_path):
    """Verify passes when cleanup is complete and PR is merged."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    assert result["status"] == "ok"
    assert all(c["passed"] for c in result["checks"])


def test_verify_leftover_state_file(tmp_path):
    """Verify fails when state files remain after Complete."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "leftover.json").write_text(
        json.dumps(
            {
                "branch": "leftover",
            }
        )
    )

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    state_check = [c for c in result["checks"] if "state" in c["name"].lower()]
    assert len(state_check) >= 1
    assert not state_check[0]["passed"]


def test_verify_leftover_worktree(tmp_path):
    """Verify fails when worktrees remain after Complete."""
    wt_dir = tmp_path / ".worktrees" / "some-feature"
    wt_dir.mkdir(parents=True)

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    wt_check = [c for c in result["checks"] if "worktree" in c["name"].lower()]
    assert len(wt_check) >= 1
    assert not wt_check[0]["passed"]


def test_verify_no_merged_pr(tmp_path):
    """Verify fails when no PR has been merged."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    pr_check = [c for c in result["checks"] if "PR" in c["name"]]
    assert len(pr_check) >= 1
    assert not pr_check[0]["passed"]


def test_verify_pr_fetch_failure(tmp_path):
    """Verify reports PR fetch failure."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="",
            stderr="not found",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    pr_check = [c for c in result["checks"] if "PR" in c["name"]]
    assert len(pr_check) >= 1
    assert not pr_check[0]["passed"]


def test_verify_no_flow_states_dir(tmp_path):
    """Verify passes cleanup check when .flow-states/ doesn't exist."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    state_check = [c for c in result["checks"] if "state" in c["name"].lower()]
    assert len(state_check) >= 1
    assert state_check[0]["passed"]


def test_verify_excludes_orchestrate_files(tmp_path):
    """Verify ignores orchestrate JSON files in .flow-states/."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "orchestrate-queue.json").write_text("{}")

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    state_check = [c for c in result["checks"] if "state" in c["name"].lower()]
    assert state_check[0]["passed"]


def test_verify_excludes_phases_files(tmp_path):
    """Verify ignores frozen phases JSON files in .flow-states/."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "feature-phases.json").write_text("{}")

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps([{"number": 1}]),
            stderr="",
        )
        result = _mod.verify("python", "owner/repo", str(tmp_path))

    state_check = [c for c in result["checks"] if "state" in c["name"].lower()]
    assert state_check[0]["passed"]


# --- CLI ---


def test_main_success():
    """main() prints JSON on success."""
    with (
        patch.object(_mod, "verify") as mock_verify,
        patch("sys.argv", ["qa-verify", "--framework", "rails", "--repo", "owner/repo"]),
    ):
        mock_verify.return_value = {
            "status": "ok",
            "checks": [],
        }
        _mod.main()

    mock_verify.assert_called_once()


def test_cli_missing_repo(monkeypatch):
    """Missing --repo exits with error."""
    monkeypatch.setattr("sys.argv", ["qa-verify"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code != 0
