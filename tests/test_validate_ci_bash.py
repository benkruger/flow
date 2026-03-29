"""Tests for lib/validate-ci-bash.py — PreToolUse hook validator."""

import json
import subprocess
import sys
from pathlib import Path

from conftest import LIB_DIR

sys.path.insert(0, str(LIB_DIR))
from importlib.util import module_from_spec, spec_from_file_location

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
            "Bash(*bin/*)",
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


# --- Git diff with file-path arguments tests ---


def test_validate_blocks_git_diff_with_file_args():
    """git diff with -- file separator is blocked (sub-agents should use Read)."""
    mod = _load_module()
    allowed, message = mod.validate("git diff origin/main..HEAD -- file.py")
    assert allowed is False
    assert "BLOCKED" in message
    assert "Read" in message


def test_validate_blocks_git_diff_head_with_file_args():
    """git diff HEAD -- path is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git diff HEAD -- src/lib/foo.py")
    assert allowed is False
    assert "BLOCKED" in message
    assert "Read" in message


def test_validate_blocks_git_diff_cached_with_file_args():
    """git diff --cached -- file is blocked."""
    mod = _load_module()
    allowed, message = mod.validate("git diff --cached -- file.py")
    assert allowed is False
    assert "BLOCKED" in message
    assert "Read" in message


def test_validate_allows_git_diff_without_file_args():
    """git diff without -- file separator is allowed."""
    mod = _load_module()
    allowed, message = mod.validate("git diff origin/main..HEAD")
    assert allowed is True
    assert message == ""


def test_validate_allows_git_diff_stat():
    """git diff --stat is allowed (--stat is a flag, not a file separator)."""
    mod = _load_module()
    allowed, message = mod.validate("git diff --stat")
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
    allowed, message = mod.validate("/usr/local/bin/flow ci", settings=SAMPLE_SETTINGS)
    assert allowed is True


def test_whitelist_allows_chmod_absolute_path():
    mod = _load_module()
    settings = {
        "permissions": {
            "allow": ["Bash(chmod +x *)"],
            "deny": [],
        }
    }
    allowed, message = mod.validate(
        "chmod +x /Users/ben/code/hh/.worktrees/feature/bin/qa",
        settings=settings,
    )
    assert allowed is True
    assert message == ""


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


# --- flow_active parameter tests ---


def test_flow_active_false_allows_unlisted_command():
    """When flow_active=False, unlisted commands pass through (no whitelist)."""
    mod = _load_module()
    allowed, message = mod.validate("npm test", settings=SAMPLE_SETTINGS, flow_active=False)
    assert allowed is True
    assert message == ""


def test_flow_active_true_blocks_unlisted_command():
    """When flow_active=True, unlisted commands are blocked (whitelist enforced)."""
    mod = _load_module()
    allowed, message = mod.validate("npm test", settings=SAMPLE_SETTINGS, flow_active=True)
    assert allowed is False
    assert "not in allow list" in message


def test_flow_active_false_still_blocks_compound():
    """Layers 1-5 enforced regardless of flow_active."""
    mod = _load_module()
    allowed, message = mod.validate("git status && git diff", settings=SAMPLE_SETTINGS, flow_active=False)
    assert allowed is False
    assert "Compound commands" in message


def test_flow_active_false_still_blocks_file_read():
    """File-read commands blocked even when flow_active=False."""
    mod = _load_module()
    allowed, message = mod.validate("cat README.md", settings=SAMPLE_SETTINGS, flow_active=False)
    assert allowed is False
    assert "Read" in message


def test_flow_active_false_still_blocks_deny():
    """Deny list enforced even when flow_active=False."""
    mod = _load_module()
    allowed, message = mod.validate("git rebase main", settings=DENY_SETTINGS, flow_active=False)
    assert allowed is False
    assert "deny" in message.lower()


def test_flow_active_false_still_blocks_redirect():
    """Redirection blocked even when flow_active=False."""
    mod = _load_module()
    allowed, message = mod.validate("git log > /tmp/out.txt", settings=SAMPLE_SETTINGS, flow_active=False)
    assert allowed is False
    assert "redirection" in message.lower()


def test_flow_active_default_is_true():
    """Default flow_active=True preserves backward compat — unlisted blocked."""
    mod = _load_module()
    allowed, message = mod.validate("npm test", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "not in allow list" in message


def test_compound_blocked_before_whitelist():
    """Compound commands are caught by fast-path before whitelist check."""
    mod = _load_module()
    allowed, message = mod.validate("git status && git diff", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "Compound commands" in message


def test_file_read_blocked_before_whitelist():
    """File-read commands are caught by fast-path before whitelist check."""
    mod = _load_module()
    allowed, message = mod.validate("cat README.md", settings=SAMPLE_SETTINGS)
    assert allowed is False
    assert "Read" in message


def test_find_settings_and_root(tmp_path, monkeypatch):
    """_find_settings_and_root finds settings.json and returns project root."""
    mod = _load_module()
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    # Nested subdir — should find settings.json in parent
    subdir = tmp_path / "a" / "b"
    subdir.mkdir(parents=True)

    monkeypatch.chdir(subdir)
    result_settings, result_root = mod._find_settings_and_root()
    assert result_settings is not None
    assert result_settings["permissions"]["allow"] == ["Bash(git status)"]
    assert result_root == tmp_path.resolve()


def test_find_settings_and_root_missing(tmp_path, monkeypatch):
    """_find_settings_and_root returns (None, None) when no settings.json."""
    mod = _load_module()

    monkeypatch.chdir(tmp_path)
    result_settings, result_root = mod._find_settings_and_root()
    assert result_settings is None
    assert result_root is None


def test_find_settings_and_root_invalid(tmp_path, monkeypatch):
    """_find_settings_and_root returns (None, None) for invalid JSON."""
    mod = _load_module()
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir()
    (claude_dir / "settings.json").write_text("not valid json {{{")

    monkeypatch.chdir(tmp_path)
    result_settings, result_root = mod._find_settings_and_root()
    assert result_settings is None
    assert result_root is None


def test_find_settings_and_root_returns_parent_of_claude_dir(tmp_path, monkeypatch):
    """Project root is the directory containing .claude/, not .claude/ itself."""
    mod = _load_module()
    project = tmp_path / "myproject"
    project.mkdir()
    claude_dir = project / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": []}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))

    monkeypatch.chdir(project)
    _, result_root = mod._find_settings_and_root()
    # Root should be the project dir, not .claude/
    assert result_root == project.resolve()
    assert result_root.name == "myproject"


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


def test_hook_exit_2_for_git_diff_with_file_args():
    """Full subprocess test: git diff with -- file args is blocked."""
    code, stderr = _run_hook("git diff HEAD -- file.py")
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


def test_hook_subprocess_whitelist_block(git_repo):
    """Full subprocess test: command blocked by whitelist when flow is active."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    # State file makes flow active — whitelist enforced
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{}")

    code, stderr = _run_hook("curl http://example.com", cwd=str(git_repo))
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
    allowed, message = mod.validate("git checkout feature-branch", settings=DENY_SETTINGS)
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_blocks_force_push():
    """git push --force matches deny pattern."""
    mod = _load_module()
    allowed, message = mod.validate("git push --force origin main", settings=DENY_SETTINGS)
    assert allowed is False
    assert "deny" in message.lower()


def test_deny_blocks_hard_reset():
    """git reset --hard matches deny pattern."""
    mod = _load_module()
    allowed, message = mod.validate("git reset --hard HEAD~1", settings=DENY_SETTINGS)
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


# --- Branch detection tests ---


def test_detect_branch_from_cwd_worktree(tmp_path):
    """Extracts branch name from .worktrees/<branch>/ in CWD."""
    mod = _load_module()
    worktree_dir = tmp_path / "project" / ".worktrees" / "my-feature"
    worktree_dir.mkdir(parents=True)

    import os

    old_cwd = os.getcwd()
    try:
        os.chdir(worktree_dir)
        result = mod._detect_branch_from_cwd()
        assert result == "my-feature"
    finally:
        os.chdir(old_cwd)


def test_detect_branch_from_cwd_worktree_subdir(tmp_path):
    """Extracts branch from .worktrees/<branch>/subdir/ path."""
    mod = _load_module()
    subdir = tmp_path / "project" / ".worktrees" / "fix-login" / "src" / "lib"
    subdir.mkdir(parents=True)

    import os

    old_cwd = os.getcwd()
    try:
        os.chdir(subdir)
        result = mod._detect_branch_from_cwd()
        assert result == "fix-login"
    finally:
        os.chdir(old_cwd)


def test_detect_branch_from_cwd_non_worktree(tmp_path, monkeypatch):
    """Falls back to git branch --show-current when not in a worktree."""
    mod = _load_module()
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(mod.subprocess, "run", lambda *a, **kw: type("R", (), {"stdout": "main\n", "returncode": 0})())
    result = mod._detect_branch_from_cwd()
    assert result == "main"


def test_detect_branch_from_cwd_detached_head(tmp_path, monkeypatch):
    """Returns None when git returns empty (detached HEAD)."""
    mod = _load_module()
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(mod.subprocess, "run", lambda *a, **kw: type("R", (), {"stdout": "\n", "returncode": 0})())
    result = mod._detect_branch_from_cwd()
    assert result is None


def test_detect_branch_from_cwd_git_fails(tmp_path, monkeypatch):
    """Returns None when git subprocess fails."""
    mod = _load_module()
    monkeypatch.chdir(tmp_path)

    def fail(*a, **kw):
        raise OSError("git not found")

    monkeypatch.setattr(mod.subprocess, "run", fail)
    result = mod._detect_branch_from_cwd()
    assert result is None


# --- _is_flow_active() tests ---


def test_is_flow_active_with_state_file(tmp_path):
    """Returns True when state file exists for the branch."""
    mod = _load_module()
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "my-feature.json").write_text("{}")
    assert mod._is_flow_active("my-feature", tmp_path) is True


def test_is_flow_active_no_state_file(tmp_path):
    """Returns False when state file does not exist."""
    mod = _load_module()
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    assert mod._is_flow_active("my-feature", tmp_path) is False


def test_is_flow_active_no_branch():
    """Returns False when branch is None (detached HEAD)."""
    mod = _load_module()
    assert mod._is_flow_active(None, Path("/some/path")) is False


def test_is_flow_active_no_project_root():
    """Returns False when project_root is None."""
    mod = _load_module()
    assert mod._is_flow_active("my-feature", None) is False


def test_is_flow_active_no_flow_states_dir(tmp_path):
    """Returns False when .flow-states/ directory doesn't exist."""
    mod = _load_module()
    assert mod._is_flow_active("my-feature", tmp_path) is False


# --- Flow detection subprocess integration tests ---


def test_hook_subprocess_flow_active_blocks(git_repo):
    """Subprocess: settings + state file → unlisted command blocked."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{}")

    code, stderr = _run_hook("npm test", cwd=str(git_repo))
    assert code == 2
    assert "not in allow list" in stderr


def test_hook_subprocess_no_flow_allows(git_repo):
    """Subprocess: settings + no state file → unlisted command allowed."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    # No .flow-states/ — flow not active

    code, stderr = _run_hook("npm test", cwd=str(git_repo))
    assert code == 0
    assert stderr == ""


def test_hook_subprocess_worktree_flow_active_blocks(tmp_path):
    """Subprocess: worktree CWD + state file → unlisted command blocked."""
    project = tmp_path / "project"
    project.mkdir()
    claude_dir = project / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    state_dir = project / ".flow-states"
    state_dir.mkdir()
    (state_dir / "my-feature.json").write_text("{}")
    worktree_dir = project / ".worktrees" / "my-feature"
    worktree_dir.mkdir(parents=True)

    code, stderr = _run_hook("npm test", cwd=str(worktree_dir))
    assert code == 2
    assert "not in allow list" in stderr


def test_hook_subprocess_worktree_no_flow_allows(tmp_path):
    """Subprocess: worktree CWD + no state file → unlisted command allowed."""
    project = tmp_path / "project"
    project.mkdir()
    claude_dir = project / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    # No .flow-states/ — flow not active
    worktree_dir = project / ".worktrees" / "my-feature"
    worktree_dir.mkdir(parents=True)

    code, stderr = _run_hook("npm test", cwd=str(worktree_dir))
    assert code == 0
    assert stderr == ""


# --- run_in_background blocking tests ---


def _run_hook_background(command, run_in_background=True, cwd=None):
    """Run the hook script with run_in_background in tool_input.

    Returns (exit_code, stderr).
    """
    tool_input = {"command": command, "run_in_background": run_in_background}
    hook_input = json.dumps({"tool_input": tool_input})
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=hook_input,
        capture_output=True,
        text=True,
        cwd=cwd,
    )
    return result.returncode, result.stderr.strip()


def test_hook_blocks_background_when_flow_active(git_repo):
    """Subprocess: flow-active + run_in_background=true → exit 2."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{}")

    code, stderr = _run_hook_background("git status", run_in_background=True, cwd=str(git_repo))
    assert code == 2
    assert "BLOCKED" in stderr
    assert "run_in_background" in stderr


def test_hook_allows_background_when_no_flow(git_repo):
    """Subprocess: no flow + run_in_background=true → exit 0."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    # No .flow-states/ — flow not active

    code, stderr = _run_hook_background("git status", run_in_background=True, cwd=str(git_repo))
    assert code == 0
    assert stderr == ""


def test_hook_allows_foreground_when_flow_active(git_repo):
    """Subprocess: flow-active + run_in_background=false → exit 0."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{}")

    code, stderr = _run_hook_background("git status", run_in_background=False, cwd=str(git_repo))
    assert code == 0
    assert stderr == ""


def test_hook_allows_missing_background_key(git_repo):
    """Subprocess: flow-active + no run_in_background key → exit 0."""
    claude_dir = git_repo / ".claude"
    claude_dir.mkdir()
    settings = {"permissions": {"allow": ["Bash(git status)"]}}
    (claude_dir / "settings.json").write_text(json.dumps(settings))
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{}")

    # Use _run_hook which does NOT include run_in_background in tool_input
    code, stderr = _run_hook("git status", cwd=str(git_repo))
    assert code == 0
    assert stderr == ""


def test_hook_allows_background_no_settings(tmp_path):
    """Subprocess: no settings.json + run_in_background=true → exit 0."""
    # No .claude/settings.json — no flow detection possible
    code, stderr = _run_hook_background("git status", run_in_background=True, cwd=str(tmp_path))
    assert code == 0
    assert stderr == ""
