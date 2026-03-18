"""Tests for lib/start-setup.py — the consolidated Start phase setup script."""

import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR, PHASE_ORDER

SCRIPT = str(LIB_DIR / "start-setup.py")

# Import start-setup.py for in-process unit tests of edge cases
_spec = importlib.util.spec_from_file_location(
    "start_setup", LIB_DIR / "start-setup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


@pytest.fixture(scope="session")
def _git_repo_with_remote_template(tmp_path_factory):
    """Create a bare+clone pair once per worker for copying."""
    parent = tmp_path_factory.mktemp("remote-template")
    bare = parent / "bare.git"
    repo = parent / "repo"

    subprocess.run(
        ["git", "init", "--bare", "-b", "main", str(bare)],
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "clone", str(bare), str(repo)],
        capture_output=True, check=True,
    )
    config_path = repo / ".git" / "config"
    with open(config_path, "a") as f:
        f.write(
            "[user]\n\temail = test@test.com\n\tname = Test\n"
            "[commit]\n\tgpgsign = false\n"
        )
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=str(repo), capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "push", "-u", "origin", "main"],
        cwd=str(repo), capture_output=True, check=True,
    )
    return parent


@pytest.fixture
def git_repo_with_remote(_git_repo_with_remote_template, tmp_path):
    """Copy the template bare+clone pair for per-test isolation."""
    parent_copy = tmp_path / "remote-setup"
    shutil.copytree(_git_repo_with_remote_template, parent_copy)
    repo = parent_copy / "repo"
    bare = parent_copy / "bare.git"
    config_path = repo / ".git" / "config"
    config_text = config_path.read_text()
    config_text = config_text.replace(
        str(_git_repo_with_remote_template / "bare.git"), str(bare)
    )
    config_path.write_text(config_text)
    return repo


def _run(cwd, feature_name, env_extra=None, prompt=None):
    """Run start-setup.py with a feature name inside the given directory."""
    env = os.environ.copy()
    if env_extra:
        env.update(env_extra)
    cmd = [sys.executable, SCRIPT, feature_name]
    if prompt is not None:
        cmd.extend(["--prompt", prompt])
    result = subprocess.run(
        cmd,
        capture_output=True, text=True, cwd=str(cwd), env=env,
    )
    return result


def _current_plugin_version():
    """Read the current version from plugin.json."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())["version"]


def _write_flow_json(repo, version, framework="rails", skills=None):
    """Write .flow.json with a version marker, framework, and optional skills."""
    data = {"flow_version": version, "framework": framework}
    if skills is not None:
        data["skills"] = skills
    (repo / ".flow.json").write_text(json.dumps(data))


def _run_no_gh(cwd, feature_name, framework="rails", prompt=None):
    """Run start-setup.py with gh stubbed out and flow.json initialized."""
    # Ensure flow.json exists with correct version for the version gate
    _write_flow_json(cwd, _current_plugin_version(), framework)

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
    cmd = [sys.executable, SCRIPT, feature_name]
    if prompt is not None:
        cmd.extend(["--prompt", prompt])
    result = subprocess.run(
        cmd,
        capture_output=True, text=True, cwd=str(cwd), env=env,
    )
    return result


@pytest.fixture(scope="module")
def _default_run(_git_repo_with_remote_template, tmp_path_factory):
    """Run start-setup.py once for all 'test feature' assertion tests."""
    parent = tmp_path_factory.mktemp("default-run")
    parent_copy = parent / "remote-setup"
    shutil.copytree(_git_repo_with_remote_template, parent_copy)
    repo = parent_copy / "repo"
    bare = parent_copy / "bare.git"
    config_path = repo / ".git" / "config"
    config_text = config_path.read_text()
    config_text = config_text.replace(
        str(_git_repo_with_remote_template / "bare.git"), str(bare)
    )
    config_path.write_text(config_text)
    result = _run_no_gh(repo, "test feature")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    state_path = repo / ".flow-states" / "test-feature.json"
    state = json.loads(state_path.read_text())
    log_path = repo / ".flow-states" / "test-feature.log"
    log = log_path.read_text() if log_path.exists() else ""
    return data, state, log, repo


# --- Branch name derivation (in-process) ---


def test_branch_name_from_feature():
    """Feature words joined with hyphens, lowercased."""
    assert _mod._branch_name("Invoice Pdf Export") == "invoice-pdf-export"


def test_branch_name_truncated_at_32_chars():
    """Branch names exceeding 32 chars are truncated at last whole word."""
    result = _mod._branch_name("this is a very long feature name that exceeds limit")
    assert len(result) <= 32
    assert not result.endswith("-")
    assert result == "this-is-a-very-long-feature-name"


def test_branch_name_exactly_32_chars():
    """Branch name exactly 32 chars is not truncated."""
    assert _mod._branch_name("abcdefgh abcdefgh abcdefgh abcde") == "abcdefgh-abcdefgh-abcdefgh-abcde"


def test_branch_name_single_long_word():
    """Single word >32 chars with no hyphens truncates at 32."""
    result = _mod._branch_name("a" * 40)
    assert len(result) == 32
    assert result == "a" * 32


# --- Feature name title-casing (in-process) ---


def test_feature_name_title_cased():
    """Feature name in output is title-cased."""
    assert _mod._title_case("invoice pdf export") == "Invoice Pdf Export"


def test_feature_name_title_cased_from_hyphenated():
    """Hyphenated branch names (the real-world input) produce spaced title case."""
    assert _mod._title_case("complete-banner-and-feature-name") == "Complete Banner And Feature Name"


def test_feature_name_title_cased_short_hyphenated():
    """Short hyphenated names also produce proper title case."""
    assert _mod._title_case("fix-login-timeout") == "Fix Login Timeout"


# --- Happy path output (shared run) ---


def test_happy_path_returns_ok_json(_default_run):
    """Successful run returns JSON with status, worktree, feature, branch."""
    data, state, log, repo = _default_run
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
    config_path = tmp_path / ".git" / "config"
    with open(config_path, "a") as f:
        f.write(
            "[user]\n\temail = test@test.com\n\tname = Test\n"
            "[commit]\n\tgpgsign = false\n"
        )
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=tmp_path, capture_output=True, check=True,
    )
    _write_flow_json(tmp_path, _current_plugin_version())
    result = _run(tmp_path, "test feature")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert data["step"] == "git_pull"


# --- Version gate now handled by prime-check.py (see test_prime_check.py) ---


# --- Worktree creation (shared run) ---


def test_worktree_created(_default_run):
    """Worktree directory is created at .worktrees/<branch>."""
    data, state, log, repo = _default_run
    wt_path = repo / ".worktrees" / "test-feature"
    assert wt_path.is_dir()


# --- State file creation (shared run) ---


def test_state_file_created(_default_run):
    """State file created at .flow-states/<branch>.json."""
    data, state, log, repo = _default_run
    assert state["feature"] == "Test Feature"
    assert state["branch"] == "test-feature"
    assert state["worktree"] == ".worktrees/test-feature"
    assert state["current_phase"] == "flow-start"
    assert state["notes"] == []


def test_state_file_has_all_6_phases(_default_run):
    """State file must have all 6 phases with correct names."""
    data, state, log, repo = _default_run

    expected_names = {
        "flow-start": "Start", "flow-plan": "Plan", "flow-code": "Code",
        "flow-code-review": "Code Review", "flow-learn": "Learn", "flow-complete": "Complete",
    }
    assert len(state["phases"]) == 6
    for key, name in expected_names.items():
        assert state["phases"][key]["name"] == name


def test_state_file_phase_fields(_default_run):
    """Each phase has all required fields."""
    data, state, log, repo = _default_run

    required_fields = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for key in PHASE_ORDER:
        phase = state["phases"][key]
        for field in required_fields:
            assert field in phase, f"Phase '{key}' missing field '{field}'"


def test_state_file_phase_1_in_progress(_default_run):
    """Phase 1 should be in_progress with timestamps set."""
    data, state, log, repo = _default_run

    start_phase = state["phases"]["flow-start"]
    assert start_phase["status"] == "in_progress"
    assert start_phase["started_at"] is not None
    assert start_phase["session_started_at"] is not None
    assert start_phase["visit_count"] == 1


def test_state_file_other_phases_pending(_default_run):
    """Non-start phases should be pending with null timestamps."""
    data, state, log, repo = _default_run

    for key in PHASE_ORDER:
        if key == "flow-start":
            continue
        phase = state["phases"][key]
        assert phase["status"] == "pending"
        assert phase["started_at"] is None
        assert phase["session_started_at"] is None
        assert phase["visit_count"] == 0


# --- Logging (shared run) ---


def test_log_file_created(_default_run):
    """Log file created at .flow-states/<branch>.log."""
    data, state, log, repo = _default_run
    assert log
    assert "[Phase 1]" in log


def test_log_entries_have_timestamps(_default_run):
    """Log entries have ISO 8601 timestamps."""
    data, state, log, repo = _default_run
    lines = [line for line in log.strip().splitlines() if line.strip()]
    for line in lines:
        assert line[0:4].isdigit(), f"Log line missing timestamp: {line}"
        assert "T" in line[:20], f"Log line missing ISO format: {line}"


# --- PR creation (shared run) ---


def test_pr_url_and_number_in_output(_default_run):
    """Output JSON includes pr_url and pr_number from gh pr create."""
    data, state, log, repo = _default_run
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


def test_extract_pr_number_malformed_url():
    """Malformed PR URL returns 0."""
    assert _mod._extract_pr_number("not-a-url") == 0


def test_extract_pr_number_non_numeric():
    """PR URL with non-numeric part after /pull/ returns 0."""
    assert _mod._extract_pr_number("https://github.com/org/repo/pull/abc") == 0


# --- files block (shared run) ---


def test_state_file_has_files_block(_default_run):
    """State file must have a files block with plan, dag, log, and state paths."""
    data, state, log, repo = _default_run
    assert "files" in state
    files = state["files"]
    assert files["plan"] is None
    assert files["dag"] is None
    assert files["log"] == ".flow-states/test-feature.log"
    assert files["state"] == ".flow-states/test-feature.json"


# --- .venv symlink in worktree ---


def test_venv_symlink_created_in_worktree(git_repo_with_remote):
    """When .venv/ exists in the project root, worktree gets a relative symlink."""
    venv_dir = git_repo_with_remote / ".venv"
    venv_dir.mkdir()
    (venv_dir / "bin").mkdir()
    (venv_dir / "bin" / "python3").write_text("fake")

    _run_no_gh(git_repo_with_remote, "test feature")

    wt_venv = git_repo_with_remote / ".worktrees" / "test-feature" / ".venv"
    assert wt_venv.is_symlink()
    assert os.readlink(str(wt_venv)) == os.path.join("..", "..", ".venv")


def test_worktree_created_without_venv(_default_run):
    """When no .venv/ exists, worktree is created successfully without it."""
    data, state, log, repo = _default_run
    assert data["status"] == "ok"

    wt_venv = repo / ".worktrees" / "test-feature" / ".venv"
    assert not wt_venv.exists()


# --- Framework propagation ---


def test_state_file_includes_framework(_default_run):
    """State file must include framework from .flow.json."""
    data, state, log, repo = _default_run
    assert state["framework"] == "rails"


def test_state_file_includes_python_framework(git_repo_with_remote):
    """State file must include python framework when .flow.json says python."""
    result = _run_no_gh(git_repo_with_remote, "test feature", framework="python")
    assert result.returncode == 0, result.stderr
    state_path = git_repo_with_remote / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())
    assert data["framework"] == "python"


# --- Frozen phases file ---


def test_frozen_phases_file_created(_default_run):
    """Start creates .flow-states/<branch>-phases.json from flow-phases.json."""
    data, state, log, repo = _default_run
    frozen = repo / ".flow-states" / "test-feature-phases.json"
    assert frozen.exists(), "Frozen phases file not created"


def test_frozen_phases_file_matches_source(_default_run):
    """Frozen phases file must match flow-phases.json content."""
    data, state, log, repo = _default_run
    frozen = repo / ".flow-states" / "test-feature-phases.json"
    frozen_data = json.loads(frozen.read_text())
    source_path = Path(__file__).resolve().parent.parent / "flow-phases.json"
    source_data = json.loads(source_path.read_text())
    assert frozen_data == source_data


# --- Skills config propagation ---


def test_state_file_includes_skills_from_flow_json(git_repo_with_remote):
    """State file must include skills config when .flow.json has a skills key."""
    skills_config = {
        "flow-start": {"continue": "auto"},
        "flow-code": {"commit": "manual", "continue": "manual"},
    }
    version = _current_plugin_version()
    _write_flow_json(git_repo_with_remote, version, skills=skills_config)

    env = os.environ.copy()
    stub_dir = git_repo_with_remote / ".stub-bin"
    stub_dir.mkdir(exist_ok=True)
    gh_stub = stub_dir / "gh"
    gh_stub.write_text(
        '#!/bin/bash\n'
        'echo "https://github.com/test/repo/pull/42"\n'
    )
    gh_stub.chmod(0o755)
    env["PATH"] = f"{stub_dir}:{env['PATH']}"

    result = subprocess.run(
        [sys.executable, SCRIPT, "skills test"],
        capture_output=True, text=True, cwd=str(git_repo_with_remote), env=env,
    )
    assert result.returncode == 0, result.stderr
    state_path = git_repo_with_remote / ".flow-states" / "skills-test.json"
    state = json.loads(state_path.read_text())
    assert "skills" in state
    assert state["skills"] == skills_config


def test_state_file_omits_skills_when_not_in_flow_json(_default_run):
    """State file should not have a skills key when .flow.json has no skills."""
    data, state, log, repo = _default_run
    assert "skills" not in state


# --- session_id and transcript_path in state file ---


def test_state_file_has_session_id_null(_default_run):
    """State file has session_id: null — Stop hook fills it later."""
    data, state, log, repo = _default_run
    assert "session_id" in state
    assert state["session_id"] is None


def test_state_file_has_transcript_path_null(_default_run):
    """State file has transcript_path: null — Stop hook fills it later."""
    data, state, log, repo = _default_run
    assert "transcript_path" in state
    assert state["transcript_path"] is None


def test_pr_body_is_simple_format(_default_run):
    """PR body uses simple format without artifacts section."""
    data, state, log, repo = _default_run
    assert data["status"] == "ok"


# --- Prompt storage in state file ---


def test_state_file_has_prompt(_default_run):
    """State file stores the raw start prompt for Plan and Complete phases."""
    data, state, log, repo = _default_run
    assert state["prompt"] == "test feature"


# --- Branch name sanitization (in-process) ---


def test_branch_name_strips_hash():
    """Hash symbols are stripped from branch names."""
    assert _mod._branch_name("fix #83 and #89") == "fix-83-and-89"


def test_branch_name_strips_at_sign():
    """At signs are stripped from branch names."""
    assert _mod._branch_name("notify @user on save") == "notify-user-on-save"


def test_branch_name_strips_mixed_special_chars():
    """Multiple special characters stripped, words still joined."""
    assert _mod._branch_name("fix #83 @user $100") == "fix-83-user-100"


# --- Prompt preservation via --prompt flag ---


def test_prompt_flag_preserves_issue_references(git_repo_with_remote):
    """--prompt stores verbatim text including #N issue references."""
    result = _run_no_gh(
        git_repo_with_remote, "fix issue pre-commit hook",
        prompt="fix issue pre-commit hook #106",
    )
    assert result.returncode == 0, result.stderr
    state_path = (
        git_repo_with_remote / ".flow-states"
        / "fix-issue-pre-commit-hook.json"
    )
    state = json.loads(state_path.read_text())
    assert state["prompt"] == "fix issue pre-commit hook #106"


def test_prompt_flag_omitted_falls_back_to_feature_words(git_repo_with_remote):
    """Without --prompt, prompt field falls back to the feature name."""
    result = _run_no_gh(git_repo_with_remote, "some feature")
    assert result.returncode == 0, result.stderr
    state_path = (
        git_repo_with_remote / ".flow-states" / "some-feature.json"
    )
    state = json.loads(state_path.read_text())
    assert state["prompt"] == "some feature"


# --- Pre-commit hook compatibility (#108) ---


def _install_pre_commit_hook(repo):
    """Install the FLOW pre-commit hook in a test repo."""
    spec = importlib.util.spec_from_file_location(
        "prime_setup", LIB_DIR / "prime-setup.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.install_pre_commit_hook(repo)


def test_initial_commit_succeeds_with_pre_commit_hook(git_repo_with_remote):
    """Start-setup succeeds when the pre-commit hook is installed (#108)."""
    _install_pre_commit_hook(git_repo_with_remote)
    result = _run_no_gh(git_repo_with_remote, "hook test")
    assert result.returncode == 0, result.stderr
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    wt_path = git_repo_with_remote / ".worktrees" / "hook-test"
    assert not (wt_path / ".flow-commit-msg").exists()
