"""Tests for lib/check-phase.py — the phase entry guard."""

import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "check-phase.py")


def _run(git_repo, phase, state_dir=None):
    """Run check-phase.py --required <phase> inside the given git repo."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--required", str(phase)],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


# --- Basic behavior ---


def test_phase_1_always_exits_0(git_repo):
    """Phase 1 has no prerequisites — always allowed."""
    result = _run(git_repo, 1)
    assert result.returncode == 0


def test_detached_head_exits_1(git_repo):
    """Detached HEAD (no branch) should block with a clear message."""
    # Detach HEAD by checking out a specific commit
    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "Could not determine current git branch" in result.stdout


def test_no_state_file_exits_1(git_repo):
    """No state file for the current branch should block."""
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "/flow:start" in result.stdout


def test_corrupt_json_exits_1(git_repo, state_dir):
    """Corrupt JSON state file should block with parse error message."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    (state_dir / f"{branch}.json").write_text("{bad json")
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "Could not read state file" in result.stdout


# --- Phase status checks ---


def test_previous_phase_pending_blocks(git_repo, state_dir):
    """Previous phase 'pending' should block entry."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=2, phase_statuses={1: "pending"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "BLOCKED" in result.stdout
    assert "pending" in result.stdout


def test_previous_phase_in_progress_blocks(git_repo, state_dir):
    """Previous phase 'in_progress' should also block."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=2, phase_statuses={1: "in_progress"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "BLOCKED" in result.stdout
    assert "in_progress" in result.stdout


def test_previous_phase_complete_allows(git_repo, state_dir):
    """Previous phase 'complete' should allow entry."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=2, phase_statuses={1: "complete"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 0


def test_sequential_chain_phase_5_with_1_to_4_complete(git_repo, state_dir):
    """Phase 5 entry should work when phases 1-4 are complete."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(
        current_phase=5,
        phase_statuses={1: "complete", 2: "complete", 3: "complete", 4: "complete"},
    )
    write_state(state_dir, branch, state)
    result = _run(git_repo, 5)
    assert result.returncode == 0


# --- Re-entry ---


def test_re_entering_completed_phase_shows_note(git_repo, state_dir):
    """Re-entering a completed phase should exit 0 with a 'previously completed' note."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(
        current_phase=2,
        phase_statuses={1: "complete", 2: "complete"},
    )
    state["phases"]["2"]["visit_count"] = 2
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 0
    assert "previously completed" in result.stdout
    assert "2 visit(s)" in result.stdout


def test_first_visit_no_previously_completed_message(git_repo, state_dir):
    """First visit to a pending phase should not show 'previously completed'."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=2, phase_statuses={1: "complete"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 0
    assert "previously completed" not in result.stdout


def test_phase_8_requires_phase_7_complete(git_repo, state_dir):
    """Phase 8 should follow the same rules — phase 7 must be complete."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(
        current_phase=8,
        phase_statuses={
            1: "complete", 2: "complete", 3: "complete", 4: "complete",
            5: "complete", 6: "complete", 7: "pending",
        },
    )
    write_state(state_dir, branch, state)
    result = _run(git_repo, 8)
    assert result.returncode == 1
    assert "Phase 7" in result.stdout


# --- Worktree resolution ---


def test_missing_phases_key_blocks(git_repo, state_dir):
    """State file with no 'phases' key should block (defaults to pending)."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = {"feature": "Test", "branch": branch, "current_phase": 2}
    write_state(state_dir, branch, state)
    result = _run(git_repo, 2)
    assert result.returncode == 1
    assert "BLOCKED" in result.stdout


def test_blocked_message_includes_correct_command(git_repo, state_dir):
    """Blocked message should include the correct /flow:X command for the missing phase."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=4, phase_statuses={
        1: "complete", 2: "complete", 3: "pending",
    })
    write_state(state_dir, branch, state)
    result = _run(git_repo, 4)
    assert result.returncode == 1
    assert "/flow:design" in result.stdout


def test_phase_0_blocks(git_repo, state_dir):
    """Phase 0 is invalid — should block because phase -1 doesn't exist."""
    branch = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    state = make_state(current_phase=1, phase_statuses={1: "complete"})
    write_state(state_dir, branch, state)
    result = _run(git_repo, 0)
    assert result.returncode == 1


# --- Worktree resolution ---


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
    state = make_state(current_phase=2, phase_statuses={1: "complete"})
    write_state(state_dir, "feature-branch", state)

    result = _run(wt_path, 2)
    assert result.returncode == 0


