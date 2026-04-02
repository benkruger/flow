"""Tests for lib/complete-merge.py — the consolidated Complete phase merge script."""

import json
import subprocess
import sys
from unittest.mock import patch

from conftest import LIB_DIR, import_lib, make_state, write_state

_mod = import_lib("complete-merge.py")

SCRIPT = str(LIB_DIR / "complete-merge.py")


# --- helpers ---


def _make_result(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=[],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def _setup_merge_state(target_project, branch="test-feature", pr_number=42):
    """Create a state file ready for the merge step."""
    phase_statuses = {
        "flow-start": "complete",
        "flow-plan": "complete",
        "flow-code": "complete",
        "flow-code-review": "complete",
        "flow-learn": "complete",
    }
    state = make_state(current_phase="flow-complete", phase_statuses=phase_statuses)
    state["branch"] = branch
    state["pr_number"] = pr_number
    state["pr_url"] = f"https://github.com/test/test/pull/{pr_number}"
    state["repo"] = "test/test"
    state["prompt"] = "work on issue #100"
    state["complete_step"] = 4
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    write_state(state_dir, branch, state)
    return state


# --- complete_merge() in-process tests ---


def test_up_to_date_and_merge_succeeds(target_project, monkeypatch):
    """Branch is up-to-date, squash merge succeeds."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        # check-freshness
        _make_result(stdout='{"status": "up_to_date"}'),
        # gh pr merge --squash
        _make_result(stdout="merged"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "merged"
    assert result["pr_number"] == 42


def test_main_moved_ci_rerun(target_project, monkeypatch):
    """Main moved, check-freshness merged — returns ci_rerun."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        # check-freshness returns merged (new commits from main)
        _make_result(stdout='{"status": "merged"}'),
        # git push
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "ci_rerun"
    assert result["pushed"] is True


def test_merge_conflicts(target_project, monkeypatch):
    """Check-freshness returns conflict — returns conflict with file list."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(
            returncode=1,
            stdout='{"status": "conflict", "files": ["lib/foo.py", "lib/bar.py"]}',
        ),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "conflict"
    assert result["conflict_files"] == ["lib/foo.py", "lib/bar.py"]


def test_max_retries(target_project, monkeypatch):
    """Check-freshness returns max_retries."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(returncode=1, stdout='{"status": "max_retries", "retries": 3}'),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "max_retries"


def test_branch_protection_ci_pending(target_project, monkeypatch):
    """Merge fails with branch protection error — returns ci_pending."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        # check-freshness: up to date
        _make_result(stdout='{"status": "up_to_date"}'),
        # gh pr merge --squash fails with branch protection
        _make_result(returncode=1, stderr="base branch policy prohibits the merge"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "ci_pending"


def test_merge_fails_other_reason(target_project, monkeypatch):
    """Merge fails for unrecognized reason — returns error."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(stdout='{"status": "up_to_date"}'),
        _make_result(returncode=1, stderr="unknown merge error"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"
    assert "unknown merge error" in result["message"]


def test_check_freshness_error(target_project, monkeypatch):
    """Check-freshness returns error status."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(
            returncode=1,
            stdout='{"status": "error", "step": "fetch", "message": "network error"}',
        ),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"


def test_step_counter_set(target_project, monkeypatch):
    """complete_merge sets complete_step in state file."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(stdout='{"status": "up_to_date"}'),
        _make_result(stdout="merged"),
    ]

    with patch("subprocess.run", side_effect=responses):
        _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    state = json.loads((target_project / ".flow-states" / "test-feature.json").read_text())
    assert state["complete_step"] == 5


def test_push_failure_after_freshness_merge(target_project, monkeypatch):
    """Push failure after check-freshness merged returns error."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(stdout='{"status": "merged"}'),
        _make_result(returncode=1, stderr="remote rejected"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"
    assert "push" in result["message"].lower()


def test_check_freshness_invalid_json(target_project, monkeypatch):
    """Check-freshness returns invalid JSON."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(returncode=1, stdout="not json at all"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"


# --- CLI tests ---


def test_timeout_handling(target_project, monkeypatch):
    """Subprocess timeout is handled gracefully."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    def _timeout_run(args, **kwargs):
        raise subprocess.TimeoutExpired(cmd=args, timeout=30)

    with patch("subprocess.run", side_effect=_timeout_run):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"


def test_unknown_freshness_status(target_project, monkeypatch):
    """Unknown check-freshness status returns error."""
    monkeypatch.chdir(target_project)
    _setup_merge_state(target_project)

    responses = [
        _make_result(stdout='{"status": "unexpected_value"}'),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.complete_merge(
            pr_number=42,
            state_file=str(target_project / ".flow-states" / "test-feature.json"),
            root=target_project,
        )

    assert result["status"] == "error"
    assert "unexpected" in result["message"].lower()


# --- CLI tests ---


def test_cli_requires_args():
    """CLI without required args returns non-zero."""
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0


def test_cli_with_args(target_project):
    """CLI with required args runs (will fail on check-freshness, but parses OK)."""
    _setup_merge_state(target_project)
    result = subprocess.run(
        [
            sys.executable,
            SCRIPT,
            "--pr",
            "42",
            "--state-file",
            str(target_project / ".flow-states" / "test-feature.json"),
        ],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    # Will fail because bin/flow isn't available in the target_project, but
    # it should parse args and attempt to run — not crash on import
    assert result.returncode in (0, 1)
    data = json.loads(result.stdout.strip().splitlines()[-1])
    assert "status" in data
