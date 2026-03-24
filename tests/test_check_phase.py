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


# --- Basic behavior (in-process main() — covers main() paths) ---


def test_phase_1_always_exits_0(git_repo, monkeypatch):
    """Phase 1 has no prerequisites — always allowed."""
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-start"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0


def test_detached_head_exits_1(git_repo, monkeypatch, capsys):
    """Detached HEAD with no state files should block with a clear message."""
    # Detach HEAD by checking out a specific commit
    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "Could not determine current git branch" in capsys.readouterr().out


def test_no_state_file_exits_1(git_repo, monkeypatch, capsys):
    """No state file for the current branch should block."""
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "/flow:flow-start" in capsys.readouterr().out


def test_corrupt_json_exits_1(tmp_path, monkeypatch):
    """Corrupt JSON state file should block with parse error message."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "test-branch.json").write_text("{bad json")

    captured = io.StringIO()
    monkeypatch.setattr(_mod, "resolve_branch", lambda override=None: ("test-branch", []))
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    monkeypatch.setattr(sys, "argv", [SCRIPT, "--required", "flow-plan"])
    monkeypatch.setattr(sys, "stdout", captured)

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "Could not read state file" in captured.getvalue()


def test_previous_phase_pending_blocks(git_repo, state_dir, branch, monkeypatch, capsys):
    """Previous phase 'pending' blocks entry (covers print+exit path in main)."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "pending"})
    write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    output = capsys.readouterr().out
    assert "BLOCKED" in output
    assert "pending" in output


# --- Phase status checks (in-process) ---


def test_previous_phase_in_progress_blocks():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "in_progress"})
    allowed, output = _mod.check_phase(state, "flow-plan")
    assert not allowed
    assert "BLOCKED" in output
    assert "in_progress" in output


def test_previous_phase_complete_allows():
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
    allowed, output = _mod.check_phase(state, "flow-plan")
    assert allowed


def test_sequential_chain_phase_4_with_1_to_3_complete():
    state = make_state(
        current_phase="flow-code-review",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "complete"},
    )
    allowed, output = _mod.check_phase(state, "flow-code-review")
    assert allowed


# --- Re-entry (in-process) ---


def test_re_entering_completed_phase_shows_note():
    """Re-entering a completed phase should return allowed=True with a note."""
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete"},
    )
    state["phases"]["flow-plan"]["visit_count"] = 2
    allowed, output = _mod.check_phase(state, "flow-plan")
    assert allowed
    assert "previously completed" in output
    assert "2 visit(s)" in output


def test_first_visit_no_previously_completed_message():
    """First visit to a pending phase should not show 'previously completed'."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
    allowed, output = _mod.check_phase(state, "flow-plan")
    assert allowed
    assert "previously completed" not in output


def test_phase_5_requires_phase_4_complete():
    """Phase 5 (Learn) requires phase 4 (Code Review) to be complete."""
    state = make_state(
        current_phase="flow-learn",
        phase_statuses={
            "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
            "flow-code-review": "pending",
        },
    )
    allowed, output = _mod.check_phase(state, "flow-learn")
    assert not allowed
    assert "Phase 4" in output


def test_phase_6_requires_phase_5_complete():
    """Phase 6 (Complete) requires phase 5 (Learn) to be complete."""
    state = make_state(
        current_phase="flow-complete",
        phase_statuses={
            "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
            "flow-code-review": "complete", "flow-learn": "pending",
        },
    )
    allowed, output = _mod.check_phase(state, "flow-complete")
    assert not allowed
    assert "Phase 5" in output


def test_missing_phases_key_blocks():
    """State file with no 'phases' key should block (defaults to pending)."""
    state = {"branch": "test", "current_phase": "flow-plan"}
    allowed, output = _mod.check_phase(state, "flow-plan")
    assert not allowed
    assert "BLOCKED" in output


def test_blocked_message_includes_correct_command():
    """Blocked message should include the correct /flow:X command."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "pending",
    })
    allowed, output = _mod.check_phase(state, "flow-code-review")
    assert not allowed
    assert "/flow:flow-code" in output


def test_invalid_phase_name_raises():
    """An unrecognized phase name should raise ValueError."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    with pytest.raises(ValueError):
        _mod.check_phase(state, "nonexistent")


# --- Frozen config (in-process) ---


def test_check_phase_uses_frozen_config():
    """check_phase uses phase_config tuple when provided."""
    custom_order = ["flow-start", "flow-plan", "flow-code-review"]
    custom_names = {"flow-start": "Start", "flow-plan": "Plan", "flow-code-review": "Review"}
    custom_numbers = {"flow-start": 1, "flow-plan": 2, "flow-code-review": 3}
    custom_commands = {"flow-start": "/t:a", "flow-plan": "/t:b", "flow-code-review": "/t:c"}
    config = (custom_order, custom_names, custom_numbers, custom_commands)

    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
    })
    allowed, output = _mod.check_phase(state, "flow-code-review", phase_config=config)
    assert allowed


def test_check_phase_frozen_config_uses_correct_predecessor():
    """check_phase with phase_config uses the config's predecessor, not the default."""
    custom_order = ["flow-start", "flow-code", "flow-plan"]
    custom_names = {"flow-start": "Start", "flow-code": "Code", "flow-plan": "Plan"}
    custom_numbers = {"flow-start": 1, "flow-code": 2, "flow-plan": 3}
    custom_commands = {"flow-start": "/t:a", "flow-code": "/t:b", "flow-plan": "/t:c"}
    config = (custom_order, custom_names, custom_numbers, custom_commands)

    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-code": "pending",
    })
    # In default PHASE_ORDER, flow-plan's predecessor is flow-start (complete).
    # In custom order, flow-plan's predecessor is flow-code (pending) → blocked.
    allowed, output = _mod.check_phase(state, "flow-plan", phase_config=config)
    assert not allowed
    assert "BLOCKED" in output


def test_cli_uses_frozen_phases_file(git_repo, state_dir, branch, monkeypatch):
    """CLI loads frozen phases file when it exists."""
    import shutil
    source = LIB_DIR.parent / "flow-phases.json"
    frozen = state_dir / f"{branch}-phases.json"
    state_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(source), str(frozen))

    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0


def test_worktree_finds_state_in_main_repo(git_repo, state_dir, monkeypatch):
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
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, "feature-branch", state)

    monkeypatch.chdir(wt_path)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0


# --- --branch flag (in-process main()) ---


def test_cli_branch_flag_uses_specified_state_file(git_repo, state_dir, monkeypatch):
    """--branch flag finds the state file for a different branch."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, "other-feature", state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan", "--branch", "other-feature"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0


def test_error_ambiguous_multiple_state_files(git_repo, state_dir, monkeypatch, capsys):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete"})
        write_state(state_dir, name, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", [SCRIPT, "--required", "flow-plan"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    output = capsys.readouterr().out
    assert "Multiple active features" in output
    assert "feat-a" in output
    assert "feat-b" in output
