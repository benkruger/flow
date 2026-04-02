"""Tests for lib/validate-claude-paths.py — PreToolUse hook blocking Edit/Write on .claude/ paths."""

import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = LIB_DIR / "validate-claude-paths.py"


def _load_module():
    """Load validate-claude-paths as a module for in-process testing."""
    from importlib.util import module_from_spec, spec_from_file_location

    spec = spec_from_file_location("validate_claude_paths", SCRIPT)
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


def test_blocks_claude_rules_when_flow_active():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/rules/foo.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message
    assert "write-rule" in message


def test_blocks_claude_md_when_flow_active():
    mod = _load_module()
    allowed, message = mod.validate("/project/CLAUDE.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message
    assert "write-rule" in message


def test_allows_claude_rules_when_no_flow():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/rules/foo.md", flow_active=False)
    assert allowed is True
    assert message == ""


def test_allows_claude_md_when_no_flow():
    mod = _load_module()
    allowed, message = mod.validate("/project/CLAUDE.md", flow_active=False)
    assert allowed is True
    assert message == ""


def test_allows_unrelated_path_when_flow_active():
    mod = _load_module()
    allowed, message = mod.validate("/project/lib/foo.py", flow_active=True)
    assert allowed is True
    assert message == ""


def test_allows_claude_settings_when_flow_active():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/settings.json", flow_active=True)
    assert allowed is True
    assert message == ""


def test_allows_flow_states_path():
    mod = _load_module()
    allowed, message = mod.validate("/project/.flow-states/branch-rule-content.md", flow_active=True)
    assert allowed is True
    assert message == ""


def test_allows_empty_path():
    mod = _load_module()
    allowed, message = mod.validate("", flow_active=True)
    assert allowed is True
    assert message == ""


def test_blocks_nested_claude_rules():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/rules/subdir/deep.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message


def test_blocks_worktree_claude_rules():
    mod = _load_module()
    allowed, message = mod.validate("/project/.worktrees/feat/.claude/rules/foo.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message


def test_blocks_worktree_claude_md():
    mod = _load_module()
    allowed, message = mod.validate("/project/.worktrees/feat/CLAUDE.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message


def test_blocks_claude_skills_when_flow_active():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/skills/foo/SKILL.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message
    assert "write-rule" in message


def test_blocks_nested_claude_skills():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/skills/subdir/deep/SKILL.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message


def test_blocks_worktree_claude_skills():
    mod = _load_module()
    allowed, message = mod.validate("/project/.worktrees/feat/.claude/skills/foo/SKILL.md", flow_active=True)
    assert allowed is False
    assert "BLOCKED" in message


def test_allows_claude_skills_when_no_flow():
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/skills/foo/SKILL.md", flow_active=False)
    assert allowed is True
    assert message == ""


def test_allows_claude_settings_local():
    """settings.local.json is managed by promote-permissions, not write-rule."""
    mod = _load_module()
    allowed, message = mod.validate("/project/.claude/settings.local.json", flow_active=True)
    assert allowed is True


def test_error_message_mentions_write_rule():
    mod = _load_module()
    _, message = mod.validate("/project/.claude/rules/foo.md", flow_active=True)
    assert "write-rule" in message
    assert "--path" in message
    assert "--content-file" in message


# --- Direct _is_protected_path tests ---


def test_is_protected_path_empty():
    mod = _load_module()
    assert mod._is_protected_path("") is False


def test_is_protected_path_claude_rules():
    mod = _load_module()
    assert mod._is_protected_path("/project/.claude/rules/foo.md") is True


def test_is_protected_path_claude_md():
    mod = _load_module()
    assert mod._is_protected_path("/project/CLAUDE.md") is True


def test_is_protected_path_claude_skills():
    mod = _load_module()
    assert mod._is_protected_path("/project/.claude/skills/foo/SKILL.md") is True


def test_is_protected_path_settings():
    mod = _load_module()
    assert mod._is_protected_path("/project/.claude/settings.json") is False


# --- Direct _detect_branch_from_cwd tests ---


def test_detect_branch_from_worktree(monkeypatch, tmp_path):
    mod = _load_module()
    worktree = tmp_path / "project" / ".worktrees" / "my-feat"
    worktree.mkdir(parents=True)
    monkeypatch.chdir(worktree)
    assert mod._detect_branch_from_cwd() == "my-feat"


def test_detect_branch_non_worktree(monkeypatch, git_repo):
    """Exercises the git subprocess fallback (lines 41-48)."""
    mod = _load_module()
    monkeypatch.chdir(git_repo)
    branch = mod._detect_branch_from_cwd()
    # git_repo fixture creates a repo on 'main'
    assert branch == "main"


def test_detect_branch_git_failure(monkeypatch, tmp_path):
    """Exercises the except branch (lines 49-50) when git fails."""
    mod = _load_module()
    monkeypatch.chdir(tmp_path)  # not a git repo
    assert mod._detect_branch_from_cwd() is None


# --- Subprocess (full hook) tests ---


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
    code, stderr = _run_hook({"file_path": ""})
    assert code == 0
    assert stderr == ""


def test_hook_exit_0_for_unrelated_path(tmp_path):
    code, stderr = _run_hook(
        {"file_path": "/some/project/lib/foo.py"},
        cwd=str(tmp_path),
    )
    assert code == 0
    assert stderr == ""


def test_hook_blocks_claude_rules_with_active_flow(tmp_path):
    """When state file exists, hook blocks .claude/rules/ edits."""
    flow_states = tmp_path / ".flow-states"
    flow_states.mkdir()
    (flow_states / "my-feature.json").write_text("{}")

    worktree = tmp_path / ".worktrees" / "my-feature"
    worktree.mkdir(parents=True)

    code, stderr = _run_hook(
        {"file_path": f"{worktree}/.claude/rules/foo.md"},
        cwd=str(worktree),
    )
    assert code == 2
    assert "BLOCKED" in stderr
    assert "write-rule" in stderr


def test_hook_blocks_claude_md_with_active_flow(tmp_path):
    """When state file exists, hook blocks CLAUDE.md edits."""
    flow_states = tmp_path / ".flow-states"
    flow_states.mkdir()
    (flow_states / "my-feature.json").write_text("{}")

    worktree = tmp_path / ".worktrees" / "my-feature"
    worktree.mkdir(parents=True)

    code, stderr = _run_hook(
        {"file_path": f"{worktree}/CLAUDE.md"},
        cwd=str(worktree),
    )
    assert code == 2
    assert "BLOCKED" in stderr


def test_hook_allows_claude_rules_no_active_flow(tmp_path):
    """When no state file exists, hook allows .claude/rules/ edits."""
    code, stderr = _run_hook(
        {"file_path": f"{tmp_path}/.claude/rules/foo.md"},
        cwd=str(tmp_path),
    )
    assert code == 0
    assert stderr == ""


def test_hook_blocks_claude_skills_with_active_flow(tmp_path):
    """When state file exists, hook blocks .claude/skills/ edits."""
    flow_states = tmp_path / ".flow-states"
    flow_states.mkdir()
    (flow_states / "my-feature.json").write_text("{}")

    worktree = tmp_path / ".worktrees" / "my-feature"
    worktree.mkdir(parents=True)

    code, stderr = _run_hook(
        {"file_path": f"{worktree}/.claude/skills/foo/SKILL.md"},
        cwd=str(worktree),
    )
    assert code == 2
    assert "BLOCKED" in stderr
    assert "write-rule" in stderr


def test_hook_allows_settings_json_with_active_flow(tmp_path):
    """settings.json is not blocked — only rules/, skills/, and CLAUDE.md."""
    flow_states = tmp_path / ".flow-states"
    flow_states.mkdir()
    (flow_states / "my-feature.json").write_text("{}")

    worktree = tmp_path / ".worktrees" / "my-feature"
    worktree.mkdir(parents=True)

    code, stderr = _run_hook(
        {"file_path": f"{worktree}/.claude/settings.json"},
        cwd=str(worktree),
    )
    assert code == 0
