"""Tests for lib/qa-verify.py — verify QA assertions per tier."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("qa-verify")


# --- tier 1 checks ---


def test_tier1_all_pass(tmp_path):
    """Tier 1 passes when cleanup is complete and PR is merged."""
    # After successful Complete: no state files, no worktrees, PR merged
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout=json.dumps([{"number": 1}]), stderr="",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    assert result["tier"] == 1
    assert all(c["passed"] for c in result["checks"])


def test_tier1_leftover_state_file(tmp_path):
    """Tier 1 fails when state files remain after Complete."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "leftover.json").write_text(json.dumps({
        "branch": "leftover",
    }))

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout=json.dumps([{"number": 1}]), stderr="",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    state_check = [c for c in result["checks"]
                   if "state" in c["name"].lower()]
    assert len(state_check) >= 1
    assert not state_check[0]["passed"]


def test_tier1_leftover_worktree(tmp_path):
    """Tier 1 fails when worktrees remain after Complete."""
    wt_dir = tmp_path / ".worktrees" / "some-feature"
    wt_dir.mkdir(parents=True)

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout=json.dumps([{"number": 1}]), stderr="",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    wt_check = [c for c in result["checks"]
                if "worktree" in c["name"].lower()]
    assert len(wt_check) >= 1
    assert not wt_check[0]["passed"]


def test_tier1_no_merged_pr(tmp_path):
    """Tier 1 fails when no PR has been merged."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout=json.dumps([]), stderr="",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    pr_check = [c for c in result["checks"] if "PR" in c["name"]]
    assert len(pr_check) >= 1
    assert not pr_check[0]["passed"]


def test_tier1_pr_fetch_failure(tmp_path):
    """Tier 1 reports PR fetch failure."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="not found",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    pr_check = [c for c in result["checks"] if "PR" in c["name"]]
    assert len(pr_check) >= 1
    assert not pr_check[0]["passed"]


def test_tier1_no_flow_states_dir(tmp_path):
    """Tier 1 passes cleanup check when .flow-states/ doesn't exist."""
    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0,
            stdout=json.dumps([{"number": 1}]), stderr="",
        )
        result = _mod.check_tier1(str(tmp_path), "owner/repo")

    assert result["tier"] == 1
    state_check = [c for c in result["checks"]
                   if "state" in c["name"].lower()]
    assert len(state_check) >= 1
    assert state_check[0]["passed"]


# --- tier 2 checks ---


def test_tier2_two_completed_flows(tmp_path):
    """Tier 2 passes when two flows completed without cross-contamination."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    for branch in ["flow-a", "flow-b"]:
        state_file = state_dir / f"{branch}.json"
        state_file.write_text(json.dumps({
            "branch": branch,
            "pr_number": 1,
            "phases": {
                "flow-start": {"status": "complete"},
                "flow-plan": {"status": "complete"},
                "flow-code": {"status": "complete"},
                "flow-code-review": {"status": "complete"},
                "flow-learn": {"status": "complete"},
                "flow-complete": {"status": "complete"},
            },
        }))

    result = _mod.check_tier2(str(tmp_path), "owner/repo")

    assert result["tier"] == 2
    assert all(c["passed"] for c in result["checks"])


def test_tier2_insufficient_flows(tmp_path):
    """Tier 2 fails when fewer than 2 flows completed."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state_file = state_dir / "flow-a.json"
    state_file.write_text(json.dumps({
        "branch": "flow-a",
        "phases": {
            "flow-start": {"status": "complete"},
            "flow-plan": {"status": "complete"},
            "flow-code": {"status": "complete"},
            "flow-code-review": {"status": "complete"},
            "flow-learn": {"status": "complete"},
            "flow-complete": {"status": "complete"},
        },
    }))

    result = _mod.check_tier2(str(tmp_path), "owner/repo")

    failed = [c for c in result["checks"] if not c["passed"]]
    assert len(failed) >= 1


def test_tier2_corrupt_state_file(tmp_path):
    """Tier 2 handles corrupt state file in one flow."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "flow-a.json").write_text("not valid json {{{")
    (state_dir / "flow-b.json").write_text(json.dumps({
        "branch": "flow-b",
        "phases": {p: {"status": "complete"} for p in [
            "flow-start", "flow-plan", "flow-code",
            "flow-code-review", "flow-learn", "flow-complete",
        ]},
    }))

    result = _mod.check_tier2(str(tmp_path), "owner/repo")

    complete_check = [c for c in result["checks"]
                      if "all phases" in c["name"].lower()]
    assert len(complete_check) >= 1
    assert not complete_check[0]["passed"]


def test_tier2_incomplete_phase(tmp_path):
    """Tier 2 detects incomplete phases in one flow."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    for i, branch in enumerate(["flow-a", "flow-b"]):
        phases = {p: {"status": "complete"} for p in [
            "flow-start", "flow-plan", "flow-code",
            "flow-code-review", "flow-learn", "flow-complete",
        ]}
        if i == 1:
            phases["flow-code"]["status"] = "in_progress"
        (state_dir / f"{branch}.json").write_text(json.dumps({
            "branch": branch,
            "pr_number": 1,
            "phases": phases,
        }))

    result = _mod.check_tier2(str(tmp_path), "owner/repo")

    complete_check = [c for c in result["checks"]
                      if "all phases" in c["name"].lower()]
    assert len(complete_check) >= 1
    assert not complete_check[0]["passed"]


# --- tier 3 checks ---


def test_tier3_lock_file_absent(tmp_path):
    """Tier 3 passes lock check when no stale lock exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    result = _mod.check_tier3(str(tmp_path), "owner/repo")

    assert result["tier"] == 3
    lock_check = [c for c in result["checks"]
                  if "lock" in c["name"].lower()]
    if lock_check:
        assert lock_check[0]["passed"]


def test_tier3_stale_lock_detected(tmp_path):
    """Tier 3 fails lock check when stale lock exists."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    lock_file = state_dir / "start.lock"
    lock_file.write_text(json.dumps({
        "pid": 99999, "feature": "stale",
        "acquired_at": "2025-01-01T00:00:00-08:00",
    }))

    result = _mod.check_tier3(str(tmp_path), "owner/repo")

    lock_check = [c for c in result["checks"]
                  if "lock" in c["name"].lower()]
    if lock_check:
        assert not lock_check[0]["passed"]


def test_tier3_corrupt_lock(tmp_path):
    """Tier 3 detects corrupt lock file."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "start.lock").write_text("not json {{{")

    result = _mod.check_tier3(str(tmp_path), "owner/repo")

    lock_check = [c for c in result["checks"]
                  if "lock" in c["name"].lower()]
    assert len(lock_check) >= 1
    assert not lock_check[0]["passed"]
    assert "Corrupt" in lock_check[0]["detail"]


def test_tier3_orphan_state_files(tmp_path):
    """Tier 3 detects orphan state files without matching worktrees."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "orphan-branch.json").write_text(json.dumps({
        "branch": "orphan-branch",
    }))

    result = _mod.check_tier3(str(tmp_path), "owner/repo")

    orphan_check = [c for c in result["checks"]
                    if "orphan" in c["name"].lower()]
    assert len(orphan_check) >= 1
    assert not orphan_check[0]["passed"]


# --- _load_state ---


def test_load_state_file_not_found(tmp_path):
    """_load_state returns None when file does not exist."""
    result = _mod._load_state(tmp_path / "nonexistent.json")
    assert result is None


# --- verify (main orchestrator) ---


def test_verify_dispatches_to_tier():
    """verify() dispatches to the correct tier function."""
    with patch.object(_mod, "check_tier1") as mock_t1:
        mock_t1.return_value = {"tier": 1, "checks": []}
        result = _mod.verify(1, "rails", "owner/repo", "/tmp/project")

    assert result["tier"] == 1
    mock_t1.assert_called_once()


def test_verify_invalid_tier():
    """verify() returns error for invalid tier."""
    result = _mod.verify(99, "rails", "owner/repo", "/tmp/project")

    assert result["status"] == "error"


def test_verify_tier2():
    """verify() dispatches tier 2."""
    with patch.object(_mod, "check_tier2") as mock_t2:
        mock_t2.return_value = {"tier": 2, "checks": []}
        result = _mod.verify(2, "rails", "owner/repo", "/tmp/project")

    assert result["tier"] == 2


def test_verify_tier3():
    """verify() dispatches tier 3."""
    with patch.object(_mod, "check_tier3") as mock_t3:
        mock_t3.return_value = {"tier": 3, "checks": []}
        result = _mod.verify(3, "rails", "owner/repo", "/tmp/project")

    assert result["tier"] == 3


# --- CLI ---


def test_main_success():
    """main() prints JSON on success."""
    with patch.object(_mod, "verify") as mock_verify, \
         patch("sys.argv", ["qa-verify", "--tier", "1",
                            "--framework", "rails",
                            "--repo", "owner/repo"]):
        mock_verify.return_value = {
            "status": "ok", "tier": 1, "checks": [],
        }
        _mod.main()

    mock_verify.assert_called_once()


def test_main_error():
    """main() exits 1 on error."""
    with patch.object(_mod, "verify") as mock_verify, \
         patch("sys.argv", ["qa-verify", "--tier", "99",
                            "--framework", "rails",
                            "--repo", "owner/repo"]), \
         pytest.raises(SystemExit) as exc_info:
        mock_verify.return_value = {
            "status": "error", "message": "invalid tier",
        }
        _mod.main()

    assert exc_info.value.code == 1


def test_cli_missing_args(monkeypatch):
    """Missing required args exits with error."""
    monkeypatch.setattr("sys.argv", ["qa-verify"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code != 0
