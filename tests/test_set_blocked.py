"""Tests for lib/set-blocked.py — PermissionRequest hook for blocked detection."""

import json
import os
import subprocess
import sys

import pytest
from conftest import LIB_DIR, make_state, write_state

SCRIPT = LIB_DIR / "set-blocked.py"


def _load_module():
    """Load set-blocked as a module for in-process testing."""
    from importlib.util import module_from_spec, spec_from_file_location

    spec = spec_from_file_location("set_blocked_mod", SCRIPT)
    mod = module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _run_hook(git_repo, stdin_json=None):
    """Run the hook script as a subprocess in the given git repo."""
    if stdin_json is None:
        stdin_json = json.dumps({"tool_name": "Bash"})
    env = os.environ.copy()
    env.pop("FLOW_SIMULATE_BRANCH", None)
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_json,
        capture_output=True,
        text=True,
        cwd=str(git_repo),
        env=env,
    )
    return result.returncode, result.stdout.strip(), result.stderr.strip()


# --- In-process set_blocked() tests ---


def test_set_blocked_sets_timestamp(state_dir, branch):
    """set_blocked writes _blocked timestamp to state file."""
    mod = _load_module()
    state = make_state(current_phase="flow-code")
    path = write_state(state_dir, branch, state)
    mod.set_blocked(str(path))
    updated = json.loads(path.read_text())
    assert "_blocked" in updated
    assert isinstance(updated["_blocked"], str)
    assert len(updated["_blocked"]) > 0


def test_set_blocked_no_state_file(tmp_path):
    """set_blocked does nothing when state file does not exist."""
    mod = _load_module()
    mod.set_blocked(str(tmp_path / "nonexistent.json"))
    # Should not raise


def test_set_blocked_none_path():
    """set_blocked does nothing when path is None."""
    mod = _load_module()
    mod.set_blocked(None)
    # Should not raise


def test_set_blocked_corrupt_state(tmp_path):
    """set_blocked fails open on corrupt JSON."""
    mod = _load_module()
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("{bad json")
    mod.set_blocked(str(bad_file))
    # Should not raise


def test_set_blocked_preserves_other_fields(state_dir, branch):
    """set_blocked preserves existing state fields."""
    mod = _load_module()
    state = make_state(current_phase="flow-code")
    state["session_id"] = "existing-session"
    state["notes"] = [{"note": "a correction"}]
    path = write_state(state_dir, branch, state)
    mod.set_blocked(str(path))
    updated = json.loads(path.read_text())
    assert updated["session_id"] == "existing-session"
    assert updated["notes"] == [{"note": "a correction"}]
    assert "_blocked" in updated


def test_set_blocked_overwrites_existing(state_dir, branch):
    """set_blocked overwrites an existing _blocked timestamp."""
    mod = _load_module()
    state = make_state(current_phase="flow-code")
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    path = write_state(state_dir, branch, state)
    mod.set_blocked(str(path))
    updated = json.loads(path.read_text())
    assert "_blocked" in updated
    assert updated["_blocked"] != "2026-01-01T10:00:00-08:00"


# --- Subprocess (full hook) tests ---


def test_hook_sets_blocked_with_state_file(git_repo, state_dir, branch):
    """Hook writes _blocked to state file when invoked."""
    state = make_state(current_phase="flow-code")
    path = write_state(state_dir, branch, state)
    code, stdout, stderr = _run_hook(git_repo)
    assert code == 0
    assert stdout == ""
    updated = json.loads(path.read_text())
    assert "_blocked" in updated
    assert isinstance(updated["_blocked"], str)


def test_hook_no_state_file_exits_zero(git_repo):
    """Hook exits 0 when no state file exists."""
    code, stdout, stderr = _run_hook(git_repo)
    assert code == 0
    assert stdout == ""


def test_hook_outside_git_repo_exits_zero(tmp_path):
    """Hook exits 0 when running outside a git repo."""
    empty = tmp_path / "not-a-repo"
    empty.mkdir()
    code, stdout, stderr = _run_hook(empty)
    assert code == 0
    assert stdout == ""


def test_hook_malformed_stdin_exits_zero(git_repo):
    """Hook exits 0 on malformed stdin."""
    code, stdout, stderr = _run_hook(git_repo, stdin_json="not json")
    assert code == 0


def test_hook_empty_stdin_exits_zero(git_repo):
    """Hook exits 0 on empty stdin."""
    code, stdout, stderr = _run_hook(git_repo, stdin_json="")
    assert code == 0


# --- In-process main() error path ---


def test_main_project_root_error_exits_zero(monkeypatch):
    """main() fails open when project_root raises after branch resolution."""
    import io

    import flow_utils

    monkeypatch.setattr("sys.stdin", io.StringIO("{}"))

    monkeypatch.setattr(flow_utils, "current_branch", lambda: "test-branch")

    def raise_error():
        raise OSError("git not found")

    monkeypatch.setattr(flow_utils, "project_root", raise_error)

    with pytest.raises(SystemExit) as exc_info:
        _load_module().main()
    assert exc_info.value.code == 0
