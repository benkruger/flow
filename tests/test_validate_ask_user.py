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
