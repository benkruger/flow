"""Tests for hooks/start-setup.py — the consolidated Start phase setup script."""

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import HOOKS_DIR

SCRIPT = str(HOOKS_DIR / "start-setup.py")

# Import start-setup.py for in-process unit tests of edge cases
_spec = importlib.util.spec_from_file_location(
    "start_setup", HOOKS_DIR / "start-setup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


@pytest.fixture
def git_repo_with_remote(tmp_path):
    """Create a git repo with a bare remote (origin) for push/pull/PR tests."""
    bare = tmp_path / "bare.git"
    repo = tmp_path / "repo"

    # Create bare remote
    subprocess.run(
        ["git", "init", "--bare", "-b", "main", str(bare)],
        capture_output=True, check=True,
    )

    # Create working repo
    subprocess.run(
        ["git", "clone", str(bare), str(repo)],
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.email", "test@test.com"],
        cwd=str(repo), capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "Test"],
        cwd=str(repo), capture_output=True, check=True,
    )
    # Initial commit so main branch exists
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=str(repo), capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "push", "-u", "origin", "main"],
        cwd=str(repo), capture_output=True, check=True,
    )
    return repo


def _run(cwd, feature_name, env_extra=None):
    """Run start-setup.py with a feature name inside the given directory."""
    env = os.environ.copy()
    if env_extra:
        env.update(env_extra)
    result = subprocess.run(
        [sys.executable, SCRIPT, feature_name],
        capture_output=True, text=True, cwd=str(cwd), env=env,
    )
    return result


def _run_no_gh(cwd, feature_name):
    """Run start-setup.py with gh stubbed out (no GitHub API)."""
    env = os.environ.copy()
    # Create a stub gh that returns a fake PR URL
    stub_dir = cwd / ".stub-bin"
    stub_dir.mkdir(exist_ok=True)
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'echo "https://github.com/test/repo/pull/42"\n'
    )
    gh_stub.chmod(0o755)
    env["PATH"] = f"{stub_dir}:{env['PATH']}"
    result = subprocess.run(
        [sys.executable, SCRIPT, feature_name],
        capture_output=True, text=True, cwd=str(cwd), env=env,
    )
    return result


# --- Branch name derivation ---


def test_branch_name_from_feature(git_repo_with_remote):
    """Feature words joined with hyphens, lowercased."""
    result = _run_no_gh(git_repo_with_remote, "Invoice Pdf Export")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["branch"] == "invoice-pdf-export"


def test_branch_name_truncated_at_32_chars(git_repo_with_remote):
    """Branch names exceeding 32 chars are truncated at last whole word."""
    result = _run_no_gh(git_repo_with_remote, "this is a very long feature name that exceeds limit")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert len(data["branch"]) <= 32
    assert not data["branch"].endswith("-")
    assert data["branch"] == "this-is-a-very-long-feature-name"


def test_branch_name_exactly_32_chars(git_repo_with_remote):
    """Branch name exactly 32 chars is not truncated."""
    result = _run_no_gh(git_repo_with_remote, "abcdefgh abcdefgh abcdefgh abcde")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["branch"] == "abcdefgh-abcdefgh-abcdefgh-abcde"


# --- Feature name title-casing ---


def test_feature_name_title_cased(git_repo_with_remote):
    """Feature name in output is title-cased."""
    result = _run_no_gh(git_repo_with_remote, "invoice pdf export")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["feature"] == "Invoice Pdf Export"


# --- Happy path output ---


def test_happy_path_returns_ok_json(git_repo_with_remote):
    """Successful run returns JSON with status, worktree, feature, branch."""
    result = _run_no_gh(git_repo_with_remote, "test feature")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["worktree"] == ".worktrees/test-feature"
    assert data["feature"] == "Test Feature"
    assert data["branch"] == "test-feature"
    assert "pr_url" in data
    assert "pr_number" in data


# --- Git pull failure ---


def test_git_pull_failure_returns_error(tmp_path):
    """When git pull fails (no remote), returns error JSON."""
    subprocess.run(
        ["git", "init"], cwd=tmp_path, capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.email", "test@test.com"],
        cwd=tmp_path, capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "Test"],
        cwd=tmp_path, capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=tmp_path, capture_output=True, check=True,
    )
    result = _run(tmp_path, "test feature")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert data["step"] == "git_pull"


# --- Settings merge ---


def test_settings_created_when_missing(git_repo_with_remote):
    """Creates .claude/settings.json if it does not exist."""
    _run_no_gh(git_repo_with_remote, "test feature")
    settings_path = git_repo_with_remote / ".claude" / "settings.json"
    assert settings_path.exists()
    data = json.loads(settings_path.read_text())
    assert "permissions" in data
    assert "allow" in data["permissions"]
    assert "deny" in data["permissions"]


def test_settings_merge_preserves_existing(git_repo_with_remote):
    """Merging into existing settings preserves original entries."""
    claude_dir = git_repo_with_remote / ".claude"
    claude_dir.mkdir()
    existing = {
        "permissions": {
            "allow": ["Bash(my-custom-command)"],
            "deny": ["Bash(rm -rf *)"],
        },
        "defaultMode": "plan",
    }
    (claude_dir / "settings.json").write_text(json.dumps(existing, indent=2))

    _run_no_gh(git_repo_with_remote, "test feature")

    data = json.loads((claude_dir / "settings.json").read_text())
    assert "Bash(my-custom-command)" in data["permissions"]["allow"]
    assert "Bash(rm -rf *)" in data["permissions"]["deny"]
    assert "Bash(bin/ci)" in data["permissions"]["allow"]
    assert data["defaultMode"] == "plan"


def test_settings_no_duplicates_on_rerun(git_repo_with_remote):
    """Running twice does not duplicate permission entries."""
    _run_no_gh(git_repo_with_remote, "feature one")
    _run_no_gh(git_repo_with_remote, "feature two")

    data = json.loads((git_repo_with_remote / ".claude" / "settings.json").read_text())
    allow = data["permissions"]["allow"]
    assert len(allow) == len(set(allow))


def test_settings_does_not_override_existing_default_mode(git_repo_with_remote):
    """Does not override an existing defaultMode."""
    claude_dir = git_repo_with_remote / ".claude"
    claude_dir.mkdir()
    existing = {
        "permissions": {"allow": []},
        "defaultMode": "bypassPermissions",
    }
    (claude_dir / "settings.json").write_text(json.dumps(existing, indent=2))

    _run_no_gh(git_repo_with_remote, "test feature")

    data = json.loads((claude_dir / "settings.json").read_text())
    assert data["defaultMode"] == "bypassPermissions"


# --- Git exclude ---


def test_git_exclude_adds_entries(git_repo_with_remote):
    """Appends .flow-states/ and .worktrees/ to info/exclude."""
    _run_no_gh(git_repo_with_remote, "test feature")

    git_dir = git_repo_with_remote / ".git"
    exclude_path = git_dir / "info" / "exclude"
    content = exclude_path.read_text()
    assert ".flow-states/" in content
    assert ".worktrees/" in content


def test_git_exclude_no_duplicates(git_repo_with_remote):
    """Running twice does not duplicate exclude entries."""
    _run_no_gh(git_repo_with_remote, "feature one")

    git_dir = git_repo_with_remote / ".git"
    exclude_path = git_dir / "info" / "exclude"
    content_before = exclude_path.read_text()
    count_before = content_before.count(".flow-states/")

    _run_no_gh(git_repo_with_remote, "feature two")
    content_after = exclude_path.read_text()
    count_after = content_after.count(".flow-states/")

    assert count_after == count_before


# --- Worktree creation ---


def test_worktree_created(git_repo_with_remote):
    """Worktree directory is created at .worktrees/<branch>."""
    _run_no_gh(git_repo_with_remote, "test feature")
    wt_path = git_repo_with_remote / ".worktrees" / "test-feature"
    assert wt_path.is_dir()


# --- State file creation ---


def test_state_file_created(git_repo_with_remote):
    """State file created at .flow-states/<branch>.json."""
    _run_no_gh(git_repo_with_remote, "test feature")
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    assert state_path.exists()
    data = json.loads(state_path.read_text())
    assert data["feature"] == "Test Feature"
    assert data["branch"] == "test-feature"
    assert data["worktree"] == ".worktrees/test-feature"
    assert data["current_phase"] == 1
    assert data["notes"] == []


def test_state_file_has_all_8_phases(git_repo_with_remote):
    """State file must have all 8 phases with correct names."""
    _run_no_gh(git_repo_with_remote, "test feature")
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())

    expected_names = {
        "1": "Start", "2": "Research", "3": "Design", "4": "Plan",
        "5": "Code", "6": "Review", "7": "Reflect", "8": "Cleanup",
    }
    assert len(data["phases"]) == 8
    for num, name in expected_names.items():
        assert data["phases"][num]["name"] == name


def test_state_file_phase_fields(git_repo_with_remote):
    """Each phase has all required fields."""
    _run_no_gh(git_repo_with_remote, "test feature")
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())

    required_fields = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for num in range(1, 9):
        phase = data["phases"][str(num)]
        for field in required_fields:
            assert field in phase, f"Phase {num} missing field '{field}'"


def test_state_file_phase_1_in_progress(git_repo_with_remote):
    """Phase 1 should be in_progress with timestamps set."""
    _run_no_gh(git_repo_with_remote, "test feature")
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())

    phase1 = data["phases"]["1"]
    assert phase1["status"] == "in_progress"
    assert phase1["started_at"] is not None
    assert phase1["session_started_at"] is not None
    assert phase1["visit_count"] == 1


def test_state_file_other_phases_pending(git_repo_with_remote):
    """Phases 2-8 should be pending with null timestamps."""
    _run_no_gh(git_repo_with_remote, "test feature")
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())

    for num in range(2, 9):
        phase = data["phases"][str(num)]
        assert phase["status"] == "pending"
        assert phase["started_at"] is None
        assert phase["session_started_at"] is None
        assert phase["visit_count"] == 0


# --- Logging ---


def test_log_file_created(git_repo_with_remote):
    """Log file created at .flow-states/<branch>.log."""
    _run_no_gh(git_repo_with_remote, "test feature")
    log_path = git_repo_with_remote / ".flow-states" / "test-feature.log"
    assert log_path.exists()
    content = log_path.read_text()
    assert "[Phase 1]" in content


def test_log_entries_have_timestamps(git_repo_with_remote):
    """Log entries have ISO 8601 timestamps."""
    _run_no_gh(git_repo_with_remote, "test feature")
    log_path = git_repo_with_remote / ".flow-states" / "test-feature.log"
    content = log_path.read_text()
    lines = [line for line in content.strip().splitlines() if line.strip()]
    for line in lines:
        assert line[0:4].isdigit(), f"Log line missing timestamp: {line}"
        assert "T" in line[:20], f"Log line missing ISO format: {line}"


# --- PR creation (stubbed) ---


def test_pr_url_and_number_in_output(git_repo_with_remote):
    """Output JSON includes pr_url and pr_number from gh pr create."""
    result = _run_no_gh(git_repo_with_remote, "test feature")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["pr_url"] == "https://github.com/test/repo/pull/42"
    assert data["pr_number"] == 42


# --- No feature name ---


def test_missing_feature_name_fails(tmp_path):
    """Running without a feature name exits with error."""
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(tmp_path),
    )
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "feature name" in data["message"].lower()


# --- In-process unit tests for edge cases ---


def test_branch_name_single_long_word():
    """Single word >32 chars with no hyphens truncates at 32."""
    result = _mod._branch_name("a" * 40)
    assert len(result) == 32
    assert result == "a" * 32


def test_settings_copied_to_worktree(git_repo_with_remote):
    """Settings file is copied into the worktree .claude/ directory."""
    _run_no_gh(git_repo_with_remote, "test feature")
    wt_settings = (git_repo_with_remote / ".worktrees" / "test-feature"
                   / ".claude" / "settings.json")
    assert wt_settings.exists()
    data = json.loads(wt_settings.read_text())
    assert "Bash(bin/ci)" in data["permissions"]["allow"]
    assert "Bash(bin/ci; *)" in data["permissions"]["allow"]


def test_worktree_settings_match_project_root(git_repo_with_remote):
    """Worktree settings must be identical to project root settings."""
    _run_no_gh(git_repo_with_remote, "test feature")
    root_settings = json.loads(
        (git_repo_with_remote / ".claude" / "settings.json").read_text()
    )
    wt_settings = json.loads(
        (git_repo_with_remote / ".worktrees" / "test-feature"
         / ".claude" / "settings.json").read_text()
    )
    assert root_settings == wt_settings


def test_extract_pr_number_malformed_url():
    """Malformed PR URL returns 0."""
    assert _mod._extract_pr_number("not-a-url") == 0


def test_extract_pr_number_non_numeric():
    """PR URL with non-numeric part after /pull/ returns 0."""
    assert _mod._extract_pr_number("https://github.com/org/repo/pull/abc") == 0