"""Tests for lib/validate-worktree-paths.py — PreToolUse hook for file tools."""

import json
import subprocess
import sys
from importlib.util import module_from_spec, spec_from_file_location

from conftest import LIB_DIR

SCRIPT = LIB_DIR / "validate-worktree-paths.py"


def _load_module():
    """Load validate-worktree-paths as a module for in-process testing."""
    spec = spec_from_file_location("validate_worktree_paths", SCRIPT)
    mod = module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _run_hook(tool_input, cwd=None):
    """Run the hook script as a subprocess.

    Returns (exit_code, stderr).
    """
    hook_input = json.dumps({"tool_input": tool_input})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
        cwd=cwd,
    )
    return result.returncode, result.stderr.strip()


# --- In-process validate() tests ---


def test_allows_when_not_in_worktree():
    mod = _load_module()
    cwd = "/Users/ben/code/flow"
    file_path = "/Users/ben/code/flow/lib/foo.py"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is True
    assert message == ""


def test_allows_file_inside_worktree():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/code/flow/.worktrees/my-feature/lib/foo.py"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is True
    assert message == ""


def test_blocks_main_repo_path_from_worktree():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/code/flow/lib/foo.py"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is False
    assert "BLOCKED" in message
    assert cwd in message


def test_allows_flow_states_path():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/code/flow/.flow-states/my-feature.json"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is True
    assert message == ""


def test_allows_home_directory_paths():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/.claude/plans/some-plan.md"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is True
    assert message == ""


def test_allows_plugin_cache_paths():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/.claude/plugins/cache/flow/0.28.5/skills/flow-code/SKILL.md"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is True
    assert message == ""


def test_error_message_includes_corrected_path():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    file_path = "/Users/ben/code/flow/skills/flow-prime/SKILL.md"
    allowed, message = mod.validate(file_path, cwd)
    assert allowed is False
    corrected = "/Users/ben/code/flow/.worktrees/my-feature/skills/flow-prime/SKILL.md"
    assert corrected in message
    assert file_path in message


def test_allows_empty_file_path():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    allowed, message = mod.validate("", cwd)
    assert allowed is True


def test_allows_worktree_root_path_exactly():
    mod = _load_module()
    cwd = "/Users/ben/code/flow/.worktrees/my-feature"
    allowed, message = mod.validate(cwd, cwd)
    assert allowed is True


def test_get_file_path_prefers_file_path():
    mod = _load_module()
    tool_input = {"file_path": "/some/path.py", "path": "/other/path"}
    assert mod.get_file_path(tool_input) == "/some/path.py"


def test_get_file_path_falls_back_to_path():
    mod = _load_module()
    tool_input = {"path": "/some/dir"}
    assert mod.get_file_path(tool_input) == "/some/dir"


def test_get_file_path_returns_empty_for_neither():
    mod = _load_module()
    tool_input = {"command": "something"}
    assert mod.get_file_path(tool_input) == ""


# --- Subprocess (full hook) tests ---


def test_hook_exit_0_when_not_in_worktree(tmp_path):
    code, stderr = _run_hook(
        {"file_path": "/some/project/lib/foo.py"},
        cwd=str(tmp_path),
    )
    assert code == 0
    assert stderr == ""


def test_hook_exit_0_for_invalid_json():
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input="not json",
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0


def test_hook_exit_0_for_missing_tool_input():
    hook_input = json.dumps({"other": "data"})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0


def test_hook_exit_0_for_empty_file_path():
    hook_input = json.dumps({"tool_input": {"file_path": ""}})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0


def test_hook_blocks_edit_with_main_repo_path(tmp_path):
    worktree = tmp_path / "project" / ".worktrees" / "feat"
    worktree.mkdir(parents=True)
    project_root = str(tmp_path / "project")
    code, stderr = _run_hook(
        {"file_path": f"{project_root}/lib/foo.py"},
        cwd=str(worktree),
    )
    assert code == 2
    assert "BLOCKED" in stderr


def test_hook_allows_glob_with_worktree_path(tmp_path):
    worktree = tmp_path / "project" / ".worktrees" / "feat"
    worktree.mkdir(parents=True)
    code, stderr = _run_hook(
        {"path": f"{worktree}/lib"},
        cwd=str(worktree),
    )
    assert code == 0


def test_hook_blocks_grep_with_main_repo_path(tmp_path):
    worktree = tmp_path / "project" / ".worktrees" / "feat"
    worktree.mkdir(parents=True)
    project_root = str(tmp_path / "project")
    code, stderr = _run_hook(
        {"path": f"{project_root}/lib"},
        cwd=str(worktree),
    )
    assert code == 2
    assert "BLOCKED" in stderr
