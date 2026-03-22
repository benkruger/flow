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


def _run_hook(command, cwd=None):
    """Run the hook script as a subprocess with the given command.

    Returns (exit_code, stderr).
    """
    hook_input = json.dumps({"tool_input": {"command": command}})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
        cwd=cwd,
    )
    return result.returncode, result.stderr.strip()


SAMPLE_SETTINGS = {
    "permissions": {
        "allow": [
            "Bash(git status)",
            "Bash(git diff *)",
            "Bash(bin/*)",
            "Bash(*bin/flow *)",
        ],
        "deny": [],
    }
}


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


def test_validate_blocks_pipe():
    mod = _load_module()
    allowed, message = mod.validate("git show HEAD:file.py | sed 's/foo/bar/'")
    assert allowed is False
    assert "Compound commands" in message
    assert "separate Bash calls" in message


def test_validate_blocks_or_operator():
    mod = _load_module()
    allowed, message = mod.validate("bin/ci || echo failed")
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


# --- Blanket restore tests ---


def test_validate_blocks_git_restore_dot():
    mod = _load_module()
    allowed, message = mod.validate("git restore .")
    assert allowed is False
    assert "git restore ." in message
    assert "individually" in message


def test_validate_allows_git_restore_specific_file():
    mod = _load_module()
    allowed, message = mod.validate("git restore lib/foo.py")
    assert allowed is True
    assert message == ""


# --- Whitelist validation tests ---


def test_whitelist_allows_matching_command():
    mod = _load_module()
    allowed, message = mod.validate("git status", settings=SAMPLE_SETTINGS)
    assert allowed is True
    assert message == ""


def test_whitelist_allows_glob_match():
    mod = _load_module()
    allowed, message = mod.validate("git diff HEAD", settings=SAMPLE_SETTINGS)
    assert allowed is True
    assert message == ""


def test_whitelist_allows_bin_glob():
    mod = _load_module()
    allowed, message = mod.validate("bin/ci", settings=SAMPLE_SETTINGS)
    assert allowed is True


def test_whitelist_allows_leading_glob():
    mod = _load_module()
    allowed, message = mod.validate(
        "/usr/local/bin/flow ci", settings=SAMPLE_SETTINGS
    )
    assert allowed is True


def test_whitelist_blocks_unmatched_command():
    mod = _load_module()
    allowed, message = mod.validate("curl http://example.com", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "not in allow list" in message
    assert "curl http://example.com" in message


def test_whitelist_blocks_rm_rf():
    mod = _load_module()
    allowed, message = mod.validate("rm -rf /", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "not in allow list" in message


def test_whitelist_skipped_when_no_settings():
    """When settings=None, whitelist check is skipped — command passes."""
    mod = _load_module()
    allowed, message = mod.validate("curl http://example.com", settings=None)
    assert allowed is True
    assert message == ""


def test_whitelist_skipped_when_empty_allow():
    """When allow list is empty, whitelist is not enforced."""
    mod = _load_module()
    settings = {"permissions": {"allow": []}}
    allowed, message = mod.validate("curl http://example.com", settings=settings)
    assert allowed is True


def test_compound_blocked_before_whitelist():
    """Compound commands are caught by fast-path before whitelist check."""
    mod = _load_module()
    allowed, message = mod.validate(
        "git status && git diff", settings=SAMPLE_SETTINGS
    )
    assert allowed is False
    assert "Compound commands" in message


def test_file_read_blocked_before_whitelist():
    """File-read commands are caught by fast-path before whitelist check."""
    mod = _load_module()
    allowed, message = mod.validate("cat README.md", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "Read" in message


def test_find_settings_json(tmp_path):
    """_find_settings_json finds settings.json walking up from CWD."""
    mod = _load_module()
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    # Nested subdir — should find settings.json in parent
    subdir = tmp_path / "a" / "b"
    subdir.mkdir(parents=True)

    import os
    old_cwd = os.getcwd()
    try:
        os.chdir(subdir)
        result = mod._find_settings_json()
        assert result is not None
        assert result["permissions"]["allow"] == ["Bash(git status)"]
    finally:
        os.chdir(old_cwd)


def test_find_settings_json_missing(tmp_path):
    """_find_settings_json returns None when no settings.json exists."""
    mod = _load_module()

    import os
    old_cwd = os.getcwd()
    try:
        os.chdir(tmp_path)
        result = mod._find_settings_json()
        assert result is None
    finally:
        os.chdir(old_cwd)


def test_find_settings_json_invalid(tmp_path):
    """_find_settings_json returns None when settings.json is invalid JSON."""
    mod = _load_module()
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    (claude_dir / "settings.json").write_text("not valid json {{{")

    import os
    old_cwd = os.getcwd()
    try:
        os.chdir(tmp_path)
        result = mod._find_settings_json()
        assert result is None
    finally:
        os.chdir(old_cwd)


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


def test_hook_exit_2_for_blocked_pipe():
    code, stderr = _run_hook("git show HEAD:file.py | sed 's/foo/bar/'")
    assert code == 2
    assert "BLOCKED" in stderr


def test_hook_exit_2_for_git_restore_dot():
    """git restore . is blocked by the hook."""
    code, stderr = _run_hook("git restore .")
    assert code == 2
    assert "BLOCKED" in stderr
    assert "individually" in stderr


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


def test_hook_subprocess_whitelist_block(tmp_path):
    """Full subprocess test: command blocked by whitelist."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    code, stderr = _run_hook("curl http://example.com", cwd=str(tmp_path))
    assert code == 2
    assert "not in allow list" in stderr


def test_hook_subprocess_whitelist_allow(tmp_path):
    """Full subprocess test: command allowed by whitelist."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    code, stderr = _run_hook("git status", cwd=str(tmp_path))
    assert code == 0
    assert stderr == ""


def test_hook_subprocess_no_settings(tmp_path):
    """Full subprocess test: no settings.json means fall through."""
    code, stderr = _run_hook("curl http://example.com", cwd=str(tmp_path))
    assert code == 0
    assert stderr == ""


# --- Deny-list validation tests ---


DENY_SETTINGS = {
    "permissions": {
        "allow": [
            "Bash(git *)",
        ],
        "deny": [
            "Bash(git rebase *)",
            "Bash(git push --force *)",
            "Bash(git push -f *)",
            "Bash(git reset --hard *)",
            "Bash(git stash *)",
            "Bash(git checkout *)",
            "Bash(git clean *)",
        ],
    }
}


def test_deny_blocks_matching_command():
    """Command matching a deny pattern is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git rebase main", settings=DENY_SETTINGS)
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_overrides_allow():
    """Command matching both allow and deny is blocked — deny wins."""
    mod = _load_module()
    allowed, message = mod.validate(
        "git checkout feature-branch", settings=DENY_SETTINGS
    )
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_blocks_force_push():
    """git push --force matches deny pattern."""
    mod = _load_module()
    allowed, message = mod.validate(
        "git push --force origin main", settings=DENY_SETTINGS
    )
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_blocks_hard_reset():
    """git reset --hard matches deny pattern."""
    mod = _load_module()
    allowed, message = mod.validate(
        "git reset --hard HEAD~1", settings=DENY_SETTINGS
    )
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_allows_non_matching_command():
    """Command matching allow but not deny passes through."""
    mod = _load_module()
    allowed, message = mod.validate("git status", settings=DENY_SETTINGS)
    assert allowed is True
    assert message == ""


def test_deny_skipped_when_no_settings():
    """When settings=None, deny check is skipped."""
    mod = _load_module()
    allowed, message = mod.validate("git rebase main", settings=None)
    assert allowed is True
    assert message == ""


def test_deny_skipped_when_empty_deny():
    """When deny list is empty, no deny blocking occurs."""
    mod = _load_module()
    settings = {
        "permissions": {
            "allow": ["Bash(git status)"],
            "deny": [],
        }
    }
    allowed, message = mod.validate("git status", settings=settings)
    assert allowed is True
    assert message == ""


def test_deny_skipped_when_no_deny_key():
    """When permissions has no deny key, deny check is skipped."""
    mod = _load_module()
    settings = {
        "permissions": {
            "allow": ["Bash(git status)"],
        }
    }
    allowed, message = mod.validate("git status", settings=settings)
    assert allowed is True
    assert message == ""


def test_deny_runs_before_allow():
    """Deny check runs before allow check — denied command never reaches allow."""
    mod = _load_module()
    settings = {
        "permissions": {
            "allow": ["Bash(git stash *)"],
            "deny": ["Bash(git stash *)"],
        }
    }
    allowed, message = mod.validate("git stash save", settings=settings)
    assert allowed is False
    assert "deny" in message.lower()


def test_hook_subprocess_deny_block(tmp_path):
    """Full subprocess test: command blocked by deny list."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {
        "permissions": {
            "allow": ["Bash(git *)"],
            "deny": ["Bash(git rebase *)"],
        }
    }
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    code, stderr = _run_hook("git rebase main", cwd=str(tmp_path))
    assert code == 2
    assert "deny" in stderr.lower()


def test_hook_subprocess_deny_allows_safe_command(tmp_path):
    """Full subprocess test: safe command passes when deny list is present."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {
        "permissions": {
            "allow": ["Bash(git *)"],
            "deny": ["Bash(git rebase *)"],
        }
    }
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    code, stderr = _run_hook("git status", cwd=str(tmp_path))
    assert code == 0
    assert stderr == ""


# --- Redirect blocking tests ---


def test_validate_blocks_redirect_output():
    """Shell output redirection (>) is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git show HEAD:file.py > /tmp/out.py")
    assert allowed is False
    assert "Read tool" in message
    assert "Write tool" in message


def test_validate_blocks_redirect_append():
    """Shell append redirection (>>) is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git log >> /tmp/out.txt")
    assert allowed is False
    assert "redirection" in message.lower()


def test_validate_blocks_redirect_stderr():
    """Stderr redirection (2>) is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git status 2> /tmp/err.txt")
    assert allowed is False
    assert "redirection" in message.lower()


def test_validate_blocks_redirect_no_space():
    """Redirection without spaces (command>file) is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git show HEAD:file.py>/tmp/out.py")
    assert allowed is False
    assert "redirection" in message.lower()


def test_validate_allows_no_redirect():
    """Commands without > pass through (e.g. git diff --diff-filter=M)."""
    mod = _load_module()
    allowed, message = mod.validate("git diff --diff-filter=M")
    assert allowed is True
    assert message == ""


def test_validate_allows_arrow_in_flag():
    """Commands with => in flags are not blocked (lookbehind guards)."""
    mod = _load_module()
    allowed, message = mod.validate("git log --format=>%s")
    assert allowed is True
    assert message == ""


def test_hook_exit_2_for_blocked_redirect():
    """Full subprocess test: redirect blocked by hook."""
    code, stderr = _run_hook("git show HEAD:file.py > /tmp/out.py")
    assert code == 2
    assert "BLOCKED" in stderr
