"""Tests for lib/validate-ask-user.py — PreToolUse hook for AskUserQuestion."""

import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

from importlib.util import spec_from_file_location, module_from_spec

SCRIPT = LIB_DIR / "validate-ask-user.py"


def _load_module():
    """Load validate-ask-user as a module for in-process testing."""
    spec = spec_from_file_location("validate_ask_user", SCRIPT)
    mod = module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _run_hook(git_repo, stdin_json=None):
    """Run the hook script as a subprocess in the given git repo.

    Returns (exit_code, stderr).
    """
    if stdin_json is None:
        stdin_json = json.dumps({"tool_input": {}})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_json,
        capture_output=True,
        text=True,
        cwd=str(git_repo),
    )
    return result.returncode, result.stderr.strip()


# --- In-process validate() tests ---


def test_validate_allows_no_state_file(tmp_path):
    mod = _load_module()
    allowed, message = mod.validate(str(tmp_path / "nonexistent.json"))
    assert allowed is True
    assert message == ""


def test_validate_allows_none_state_path():
    mod = _load_module()
    allowed, message = mod.validate(None)
    assert allowed is True
    assert message == ""


def test_validate_allows_invalid_json(tmp_path):
    mod = _load_module()
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("not json at all")
    allowed, message = mod.validate(str(bad_file))
    assert allowed is True
    assert message == ""


def test_validate_allows_no_auto_continue(state_dir, branch):
    mod = _load_module()
    state = make_state(current_phase="flow-start")
    path = write_state(state_dir, branch, state)
    allowed, message = mod.validate(str(path))
    assert allowed is True
    assert message == ""


def test_validate_allows_empty_auto_continue(state_dir, branch):
    mod = _load_module()
    state = make_state(current_phase="flow-start")
    state["_auto_continue"] = ""
    path = write_state(state_dir, branch, state)
    allowed, message = mod.validate(str(path))
    assert allowed is True
    assert message == ""


def test_validate_blocks_when_auto_continue_set(state_dir, branch):
    mod = _load_module()
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "complete"},
    )
    state["_auto_continue"] = "/flow:flow-plan"
    path = write_state(state_dir, branch, state)
    allowed, message = mod.validate(str(path))
    assert allowed is False
    assert "BLOCKED" in message
    assert "/flow:flow-plan" in message


def test_validate_blocks_with_different_command(state_dir, branch):
    mod = _load_module()
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
        },
    )
    state["_auto_continue"] = "/flow:flow-code-review"
    path = write_state(state_dir, branch, state)
    allowed, message = mod.validate(str(path))
    assert allowed is False
    assert "/flow:flow-code-review" in message


# --- Subprocess (full hook) tests ---


def test_hook_allows_no_state_file(git_repo):
    code, stderr = _run_hook(git_repo)
    assert code == 0
    assert stderr == ""


def test_hook_allows_without_auto_continue(git_repo, state_dir, branch):
    state = make_state(current_phase="flow-plan")
    write_state(state_dir, branch, state)
    code, stderr = _run_hook(git_repo)
    assert code == 0
    assert stderr == ""


def test_hook_blocks_with_auto_continue(git_repo, state_dir, branch):
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "complete"},
    )
    state["_auto_continue"] = "/flow:flow-plan"
    write_state(state_dir, branch, state)
    code, stderr = _run_hook(git_repo)
    assert code == 2
    assert "BLOCKED" in stderr
    assert "/flow:flow-plan" in stderr


def test_hook_allows_invalid_json_stdin(git_repo):
    code, stderr = _run_hook(git_repo, stdin_json="not json")
    assert code == 0


def test_hook_allows_outside_git_repo(tmp_path):
    """Running outside a git repo — branch/root detection fails, allow through."""
    empty = tmp_path / "not-a-repo"
    empty.mkdir()
    code, stderr = _run_hook(empty)
    assert code == 0


# --- In-process helper function tests ---


def test_project_root_returns_none_on_git_failure(tmp_path, monkeypatch):
    """_project_root returns None when git worktree list fails."""
    mod = _load_module()
    monkeypatch.chdir(tmp_path)
    result = mod._project_root()
    assert result is None


def test_project_root_returns_none_when_no_worktree_line(monkeypatch):
    """_project_root returns None when output has no worktree line."""
    mod = _load_module()
    import subprocess as sp

    class FakeResult:
        returncode = 0
        stdout = "bare\n"

    monkeypatch.setattr(sp, "run", lambda *a, **kw: FakeResult())
    result = mod._project_root()
    assert result is None


# --- In-process write_blocked() tests ---


def test_write_blocked_sets_timestamp(state_dir, branch):
    """write_blocked writes _blocked timestamp to state file."""
    mod = _load_module()
    state = make_state(current_phase="flow-code")
    path = write_state(state_dir, branch, state)
    mod.write_blocked(str(path))
    updated = json.loads(path.read_text())
    assert "_blocked" in updated
    assert isinstance(updated["_blocked"], str)
    assert len(updated["_blocked"]) > 0


def test_write_blocked_no_state_file(tmp_path):
    """write_blocked does nothing when state file does not exist."""
    mod = _load_module()
    mod.write_blocked(str(tmp_path / "nonexistent.json"))
    # Should not raise


def test_write_blocked_none_path():
    """write_blocked does nothing when path is None."""
    mod = _load_module()
    mod.write_blocked(None)
    # Should not raise


def test_write_blocked_corrupt_state(tmp_path):
    """write_blocked fails open on corrupt JSON."""
    mod = _load_module()
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("{bad json")
    mod.write_blocked(str(bad_file))
    # Should not raise


def test_write_blocked_preserves_other_fields(state_dir, branch):
    """write_blocked preserves existing state fields."""
    mod = _load_module()
    state = make_state(current_phase="flow-code")
    state["session_id"] = "existing-session"
    state["notes"] = [{"note": "a correction"}]
    path = write_state(state_dir, branch, state)
    mod.write_blocked(str(path))
    updated = json.loads(path.read_text())
    assert updated["session_id"] == "existing-session"
    assert updated["notes"] == [{"note": "a correction"}]
    assert "_blocked" in updated


# --- Subprocess write_blocked tests ---


def test_hook_writes_blocked_on_allow(git_repo, state_dir, branch):
    """Hook writes _blocked to state file when allowing through (no auto-continue)."""
    state = make_state(current_phase="flow-code")
    path = write_state(state_dir, branch, state)
    code, stderr = _run_hook(git_repo)
    assert code == 0
    updated = json.loads(path.read_text())
    assert "_blocked" in updated
    assert isinstance(updated["_blocked"], str)


def test_hook_does_not_write_blocked_when_blocking(git_repo, state_dir, branch):
    """Hook does not write _blocked when blocking due to auto-continue."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    state["_auto_continue"] = "/flow:flow-plan"
    path = write_state(state_dir, branch, state)
    code, stderr = _run_hook(git_repo)
    assert code == 2
    updated = json.loads(path.read_text())
    assert "_blocked" not in updated
