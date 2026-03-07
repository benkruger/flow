"""Tests for lib/check-phase.py — the phase entry guard."""

import importlib.util
import io
import subprocess
import sys

import pytest

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "check-phase.py")

# Import check-phase.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "check_phase_mod", LIB_DIR / "check-phase.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(git_repo, phase, state_dir=None):
    """Run check-phase.py --required <phase> inside the given git repo."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--required", phase],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


# --- Basic behavior (subprocess — covers main() paths) ---


def test_phase_1_always_exits_0(git_repo):
    """Phase 1 has no prerequisites — always allowed."""
    result = _run(git_repo, "start")
    assert result.returncode == 0


def test_detached_head_exits_1(git_repo):
    """Detached HEAD (no branch) should block with a clear message."""
    # Detach HEAD by checking out a specific commit
    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    result = _run(git_repo, "plan")
    assert result.returncode == 1
    assert "Could not determine current git branch" in result.stdout


def test_no_state_file_exits_1(git_repo):
    """No state file for the current branch should block."""
    result = _run(git_repo, "plan")
    assert result.returncode == 1
    assert "/flow:start" in result.stdout


def test_corrupt_json_exits_1(tmp_path, monkeypatch):
    """Corrupt JSON state file should block with parse error message."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "test-branch.json").write_text("{bad json")

    captured = io.StringIO()
    monkeypatch.setattr(_mod, "current_branch", lambda: "test-branch")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    monkeypatch.setattr(sys, "argv", [SCRIPT, "--required", "plan"])
    monkeypatch.setattr(sys, "stdout", captured)

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "Could not read state file" in captured.getvalue()


def test_previous_phase_pending_blocks(git_repo, state_dir, branch):
    """Previous phase 'pending' blocks entry (covers print+exit path in main)."""
    state = make_state(current_phase="plan", phase_statuses={"start": "pending"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, "plan")
    assert result.returncode == 1
    assert "BLOCKED" in result.stdout
    assert "pending" in result.stdout


# --- Phase status checks (in-process) ---


def test_previous_phase_in_progress_blocks():
    state = make_state(current_phase="plan", phase_statuses={"start": "in_progress"})
    allowed, output = _mod.check_phase(state, "plan")
    assert not allowed
    assert "BLOCKED" in output
    assert "in_progress" in output


def test_previous_phase_complete_allows():
    state = make_state(current_phase="plan", phase_statuses={"start": "complete"})
    allowed, output = _mod.check_phase(state, "plan")
    assert allowed


def test_sequential_chain_phase_4_with_1_to_3_complete():
    state = make_state(
        current_phase="simplify",
        phase_statuses={"start": "complete", "plan": "complete", "code": "complete"},
    )
    allowed, output = _mod.check_phase(state, "simplify")
    assert allowed


# --- Re-entry (in-process) ---


def test_re_entering_completed_phase_shows_note():
    """Re-entering a completed phase should return allowed=True with a note."""
    state = make_state(
        current_phase="plan",
        phase_statuses={"start": "complete", "plan": "complete"},
    )
    state["phases"]["plan"]["visit_count"] = 2
    allowed, output = _mod.check_phase(state, "plan")
    assert allowed
    assert "previously completed" in output
    assert "2 visit(s)" in output


def test_first_visit_no_previously_completed_message():
    """First visit to a pending phase should not show 'previously completed'."""
    state = make_state(current_phase="plan", phase_statuses={"start": "complete"})
    allowed, output = _mod.check_phase(state, "plan")
    assert allowed
    assert "previously completed" not in output


def test_phase_6_requires_phase_5_complete():
    """Phase 6 (Security) requires phase 5 (Review) to be complete."""
    state = make_state(
        current_phase="security",
        phase_statuses={
            "start": "complete", "plan": "complete", "code": "complete",
            "simplify": "complete", "review": "pending",
        },
    )
    allowed, output = _mod.check_phase(state, "security")
    assert not allowed
    assert "Phase 5" in output


def test_phase_7_requires_phase_6_complete():
    """Phase 7 (Learning) requires phase 6 (Security) to be complete."""
    state = make_state(
        current_phase="learning",
        phase_statuses={
            "start": "complete", "plan": "complete", "code": "complete",
            "simplify": "complete", "review": "complete", "security": "pending",
        },
    )
    allowed, output = _mod.check_phase(state, "learning")
    assert not allowed
    assert "Phase 6" in output


def test_phase_8_requires_phase_7_complete():
    """Phase 8 (Cleanup) requires phase 7 (Learning) to be complete."""
    state = make_state(
        current_phase="cleanup",
        phase_statuses={
            "start": "complete", "plan": "complete", "code": "complete",
            "simplify": "complete", "review": "complete", "security": "complete",
            "learning": "pending",
        },
    )
    allowed, output = _mod.check_phase(state, "cleanup")
    assert not allowed
    assert "Phase 7" in output


def test_missing_phases_key_blocks():
    """State file with no 'phases' key should block (defaults to pending)."""
    state = {"feature": "Test", "branch": "test", "current_phase": "plan"}
    allowed, output = _mod.check_phase(state, "plan")
    assert not allowed
    assert "BLOCKED" in output


def test_blocked_message_includes_correct_command():
    """Blocked message should include the correct /flow:X command."""
    state = make_state(current_phase="simplify", phase_statuses={
        "start": "complete", "plan": "complete", "code": "pending",
    })
    allowed, output = _mod.check_phase(state, "simplify")
    assert not allowed
    assert "/flow:code" in output


def test_invalid_phase_name_raises():
    """An unrecognized phase name should raise ValueError."""
    state = make_state(current_phase="start", phase_statuses={"start": "complete"})
    with pytest.raises(ValueError):
        _mod.check_phase(state, "nonexistent")


# --- Worktree resolution (subprocess) ---


def test_worktree_finds_state_in_main_repo(git_repo, state_dir):
    """Running from a worktree should find state files in the main repo."""
    # Create a branch for the worktree
    subprocess.run(
        ["git", "branch", "feature-branch"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    # Create a worktree
    wt_path = git_repo / "wt"
    subprocess.run(
        ["git", "worktree", "add", str(wt_path), "feature-branch"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    # Write state file in main repo for the feature-branch
    state = make_state(current_phase="plan", phase_statuses={"start": "complete"})
    write_state(state_dir, "feature-branch", state)

    result = _run(wt_path, "plan")
    assert result.returncode == 0
