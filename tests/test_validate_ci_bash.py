"""Tests for lib/validate-ci-bash.py — PreToolUse hook validator."""

import json
import subprocess
import sys

from conftest import LIB_DIR, REPO_ROOT

sys.path.insert(0, str(LIB_DIR))
from importlib.util import spec_from_file_location, module_from_spec

SCRIPT = LIB_DIR / "validate-ci-bash.py"


def _load_module():
    """Load validate-ci-bash as a module for in-process testing."""
    spec = spec_from_file_location("validate_ci_bash", SCRIPT)
    mod = module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _run_hook(command):
    """Run the hook script as a subprocess with the given command.

    Returns (exit_code, stderr).
    """
    hook_input = json.dumps({"tool_input": {"command": command}})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
    )
    return result.returncode, result.stderr.strip()


# --- In-process validate() tests ---


def test_validate_allows_bin_flow_ci():
    mod = _load_module()
    allowed, message = mod.validate("bin/flow ci")
    assert allowed is True
    assert message == ""


def test_validate_allows_bin_ci():
    mod = _load_module()
    allowed, message = mod.validate("bin/ci")
    assert allowed is True
    assert message == ""


def test_validate_allows_git_add():
    mod = _load_module()
    allowed, message = mod.validate("git add -A")
    assert allowed is True
    assert message == ""


def test_validate_allows_git_diff():
    mod = _load_module()
    allowed, message = mod.validate("git diff HEAD")
    assert allowed is True
    assert message == ""


def test_validate_blocks_compound_and():
    mod = _load_module()
    allowed, message = mod.validate("cd .worktrees/test && git status")
    assert allowed is False
    assert "Compound commands" in message
    assert "separate Bash calls" in message


def test_validate_blocks_compound_semicolon():
    mod = _load_module()
    allowed, message = mod.validate("bin/ci; echo done")
    assert allowed is False
    assert "Compound commands" in message


def test_validate_blocks_cat():
    mod = _load_module()
    allowed, message = mod.validate("cat lib/foo.py")
    assert allowed is False
    assert "Read" in message


def test_validate_blocks_grep():
    mod = _load_module()
    allowed, message = mod.validate("grep -r 'pattern' lib/")
    assert allowed is False
    assert "Grep" in message


def test_validate_blocks_rg():
    mod = _load_module()
    allowed, message = mod.validate("rg 'pattern' lib/")
    assert allowed is False
    assert "Grep" in message


def test_validate_blocks_find():
    mod = _load_module()
    allowed, message = mod.validate("find . -name '*.py'")
    assert allowed is False
    assert "Glob" in message


def test_validate_blocks_ls():
    mod = _load_module()
    allowed, message = mod.validate("ls -la lib/")
    assert allowed is False
    assert "Glob" in message


def test_validate_blocks_head():
    mod = _load_module()
    allowed, message = mod.validate("head -20 lib/foo.py")
    assert allowed is False
    assert "Read" in message


def test_validate_blocks_tail():
    mod = _load_module()
    allowed, message = mod.validate("tail -f log.txt")
    assert allowed is False
    assert "Read" in message


def test_validate_allows_empty_command():
    mod = _load_module()
    allowed, message = mod.validate("")
    assert allowed is True


# --- Subprocess (full hook) tests ---


def test_hook_exit_0_for_allowed():
    code, stderr = _run_hook("bin/flow ci")
    assert code == 0
    assert stderr == ""


def test_hook_exit_2_for_blocked_compound():
    code, stderr = _run_hook("cd foo && git status")
    assert code == 2
    assert "BLOCKED" in stderr


def test_hook_exit_2_for_blocked_file_read():
    code, stderr = _run_hook("cat README.md")
    assert code == 2
    assert "BLOCKED" in stderr


def test_hook_exit_0_for_invalid_json():
    """Invalid JSON input should allow through (exit 0)."""
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input="not json",
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0


def test_hook_exit_0_for_empty_command():
    """Empty command in valid JSON should allow through."""
    hook_input = json.dumps({"tool_input": {"command": ""}})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0


def test_hook_exit_0_for_missing_tool_input():
    """JSON without tool_input should allow through."""
    hook_input = json.dumps({"other": "data"})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
