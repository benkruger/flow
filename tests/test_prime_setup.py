"""Tests for lib/prime-setup.py — the prime phase setup script."""

import importlib.util
import json
import os
import subprocess
import sys

from conftest import FRAMEWORKS_DIR, LIB_DIR

SCRIPT = str(LIB_DIR / "prime-setup.py")

# Import prime-setup.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "prime_setup", LIB_DIR / "prime-setup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(project_root, framework="rails", skills_json=None, commit_format=None):
    """Run prime-setup.py via subprocess."""
    cmd = [sys.executable, SCRIPT, str(project_root)]
    if framework:
        cmd.extend(["--framework", framework])
    if skills_json is not None:
        cmd.extend(["--skills-json", skills_json])
    if commit_format is not None:
        cmd.extend(["--commit-format", commit_format])
    result = subprocess.run(cmd, capture_output=True, text=True)
    return result


# --- CLI behavior ---


def test_missing_args_returns_error():
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True,
    )
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


def test_invalid_project_root_returns_error(tmp_path):
    result = _run(tmp_path / "nonexistent")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"


def test_happy_path_returns_ok(git_repo):
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["settings_merged"] is True
    assert data["version_marker"] is True


# --- Settings merge (in-process) ---


def test_creates_settings_from_scratch(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings_path = tmp_path / ".claude" / "settings.json"
    assert settings_path.exists()
    settings = json.loads(settings_path.read_text())
    assert "permissions" in settings
    assert "allow" in settings["permissions"]
    assert "deny" in settings["permissions"]


def _load_framework_permissions(framework):
    """Load expected permissions from frameworks/<name>/permissions.json."""
    permissions_path = FRAMEWORKS_DIR / framework / "permissions.json"
    return json.loads(permissions_path.read_text())["allow"]


def test_settings_has_all_allow_entries_rails(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    expected = _mod.UNIVERSAL_ALLOW + _load_framework_permissions("rails")
    for entry in expected:
        assert entry in settings["permissions"]["allow"]


def test_settings_has_all_allow_entries_python(tmp_path):
    _mod.merge_settings(tmp_path, "python")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    expected = _mod.UNIVERSAL_ALLOW + _load_framework_permissions("python")
    for entry in expected:
        assert entry in settings["permissions"]["allow"]


def test_settings_has_all_deny_entries(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _mod.FLOW_DENY:
        assert entry in settings["permissions"]["deny"]


def test_settings_sets_default_mode(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    assert settings["permissions"]["defaultMode"] == "acceptEdits"


def test_settings_preserves_existing_entries(tmp_path):
    settings_dir = tmp_path / ".claude"
    settings_dir.mkdir()
    existing = {
        "permissions": {
            "allow": ["Bash(custom command)"],
            "deny": ["Bash(custom deny)"],
        }
    }
    (settings_dir / "settings.json").write_text(json.dumps(existing))

    _mod.merge_settings(tmp_path, "rails")

    settings = json.loads((settings_dir / "settings.json").read_text())
    assert "Bash(custom command)" in settings["permissions"]["allow"]
    assert "Bash(custom deny)" in settings["permissions"]["deny"]


def test_settings_overrides_existing_default_mode(tmp_path, capsys):
    """FLOW always sets defaultMode to acceptEdits, even if project had plan."""
    settings_dir = tmp_path / ".claude"
    settings_dir.mkdir()
    existing = {
        "permissions": {
            "allow": [],
            "deny": [],
            "defaultMode": "plan",
        }
    }
    (settings_dir / "settings.json").write_text(json.dumps(existing))

    _mod.merge_settings(tmp_path, "rails")

    settings = json.loads((settings_dir / "settings.json").read_text())
    assert settings["permissions"]["defaultMode"] == "acceptEdits"
    captured = capsys.readouterr()
    assert "overriding" in captured.err.lower()


def test_settings_no_duplicate_entries(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    _mod.merge_settings(tmp_path, "rails")

    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow_list = settings["permissions"]["allow"]
    assert len(allow_list) == len(set(allow_list))
    deny_list = settings["permissions"]["deny"]
    assert len(deny_list) == len(set(deny_list))


# --- Version marker (in-process) ---


def test_version_marker_created(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    flow_json = tmp_path / ".flow.json"
    assert flow_json.exists()
    data = json.loads(flow_json.read_text())
    assert "flow_version" in data


def test_version_marker_matches_plugin_version(tmp_path):
    version = _mod._plugin_version()
    _mod.write_version_marker(tmp_path, version, "rails")
    flow_data = json.loads((tmp_path / ".flow.json").read_text())
    assert flow_data["flow_version"] == version


def test_settings_file_has_trailing_newline(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    content = (tmp_path / ".claude" / "settings.json").read_text()
    assert content.endswith("\n")


def test_version_marker_has_trailing_newline(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    content = (tmp_path / ".flow.json").read_text()
    assert content.endswith("\n")


# --- Git exclude (in-process) ---


def test_git_exclude_updated(git_repo):
    updated = _mod.update_git_exclude(git_repo)
    assert updated is True

    exclude_path = git_repo / ".git" / "info" / "exclude"
    content = exclude_path.read_text()
    assert ".flow-states/" in content
    assert ".worktrees/" in content


def test_git_exclude_idempotent(git_repo):
    _mod.update_git_exclude(git_repo)
    _mod.update_git_exclude(git_repo)

    exclude_path = git_repo / ".git" / "info" / "exclude"
    content = exclude_path.read_text()
    assert content.count(".flow-states/") == 1
    assert content.count(".worktrees/") == 1


def test_git_exclude_preserves_existing_content(git_repo):
    info_dir = git_repo / ".git" / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    exclude_path = info_dir / "exclude"
    exclude_path.write_text("*.log\n")

    _mod.update_git_exclude(git_repo)

    content = exclude_path.read_text()
    assert "*.log" in content
    assert ".flow-states/" in content


def test_git_exclude_not_updated_when_already_present(git_repo):
    info_dir = git_repo / ".git" / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    (info_dir / "exclude").write_text(
        ".flow-states/\n.worktrees/\n.flow.json\nbin/dependencies\n"
    )

    updated = _mod.update_git_exclude(git_repo)
    assert updated is False


# --- In-process tests ---


def test_merge_settings_empty_project_rails(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    expected = _mod.UNIVERSAL_ALLOW + _load_framework_permissions("rails")
    assert len(settings["permissions"]["allow"]) == len(expected)
    assert len(settings["permissions"]["deny"]) == len(_mod.FLOW_DENY)


def test_merge_settings_empty_project_python(tmp_path):
    _mod.merge_settings(tmp_path, "python")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    expected = _mod.UNIVERSAL_ALLOW + _load_framework_permissions("python")
    assert len(settings["permissions"]["allow"]) == len(expected)
    assert len(settings["permissions"]["deny"]) == len(_mod.FLOW_DENY)


def test_update_git_exclude_no_git(tmp_path):
    result = _mod.update_git_exclude(tmp_path)
    assert result is False


def test_update_git_exclude_adds_newline_if_missing(git_repo):
    info_dir = git_repo / ".git" / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    (info_dir / "exclude").write_text("*.tmp")  # No trailing newline

    _mod.update_git_exclude(git_repo)

    content = (info_dir / "exclude").read_text()
    assert "*.tmp\n.flow-states/" in content


def test_update_git_exclude_creates_file_when_missing(git_repo):
    info_dir = git_repo / ".git" / "info"
    exclude_path = info_dir / "exclude"
    if exclude_path.exists():
        exclude_path.unlink()

    _mod.update_git_exclude(git_repo)

    assert exclude_path.exists()
    content = exclude_path.read_text()
    assert ".flow-states/" in content
    assert ".worktrees/" in content


def test_main_exception_returns_error(git_repo, monkeypatch):
    monkeypatch.setattr(
        _mod, "_plugin_json",
        lambda: (_ for _ in ()).throw(RuntimeError("test error")),
    )
    import io
    captured = io.StringIO()
    monkeypatch.setattr(sys, "argv", [SCRIPT, str(git_repo), "--framework", "rails"])
    monkeypatch.setattr(sys, "stdout", captured)
    with __import__("pytest").raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(captured.getvalue())
    assert data["status"] == "error"
    assert "test error" in data["message"]


# --- Framework argument ---


def test_missing_framework_returns_error(git_repo):
    result = _run(git_repo, framework=None)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "framework" in data["message"].lower()


def test_invalid_framework_returns_error(git_repo):
    result = _run(git_repo, framework="django")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "framework" in data["message"].lower()


def test_flow_json_includes_framework_rails(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["framework"] == "rails"


def test_flow_json_includes_framework_python(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "python")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["framework"] == "python"


def test_rails_framework_excludes_python_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("python"):
        assert entry not in settings["permissions"]["allow"]


def test_python_framework_excludes_rails_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "python")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("rails"):
        assert entry not in settings["permissions"]["allow"]


def test_framework_output_in_ok_response(git_repo):
    result = _run(git_repo, framework="python")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["framework"] == "python"


def test_rerun_preserves_framework(tmp_path):
    version = _mod._plugin_version()
    _mod.merge_settings(tmp_path, "rails")
    _mod.write_version_marker(tmp_path, version, "rails")
    _mod.merge_settings(tmp_path, "rails")
    _mod.write_version_marker(tmp_path, version, "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["framework"] == "rails"
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow_list = settings["permissions"]["allow"]
    assert len(allow_list) == len(set(allow_list))


# --- Skills dict in .flow.json ---


def test_version_marker_without_skills_has_no_skills_key(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "python")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "skills" not in data


def test_version_marker_with_skills_dict(tmp_path):
    skills = {
        "flow-start": "manual",
        "flow-code": "manual",
        "flow-code-review": "manual",
        "flow-learn": "auto",
        "flow-commit": "manual",
        "flow-abort": "auto",
        "flow-complete": "auto",
    }
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "python", skills=skills)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["skills"] == skills


def test_version_marker_with_empty_skills_dict(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails", skills={})
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["skills"] == {}


def test_universal_allow_includes_bin_glob():
    assert "Bash(bin/*)" in _mod.UNIVERSAL_ALLOW


def test_universal_allow_includes_claude_plugin_commands():
    assert "Bash(claude plugin list)" in _mod.UNIVERSAL_ALLOW
    assert "Bash(claude plugin marketplace add *)" in _mod.UNIVERSAL_ALLOW
    assert "Bash(claude plugin install *)" in _mod.UNIVERSAL_ALLOW


def test_permissions_loaded_from_framework_directory(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    assert "Bash(bin/rails test *)" in settings["permissions"]["allow"]


def test_load_framework_permissions_returns_empty_for_unknown():
    result = _mod._load_framework_permissions("nonexistent")
    assert result == []


# --- Config hash ---


def test_compute_config_hash_returns_12_hex_chars():
    result = _mod.compute_config_hash("rails")
    assert isinstance(result, str)
    assert len(result) == 12
    assert all(c in "0123456789abcdef" for c in result)


def test_compute_config_hash_is_deterministic():
    hash1 = _mod.compute_config_hash("rails")
    hash2 = _mod.compute_config_hash("rails")
    assert hash1 == hash2


def test_compute_config_hash_differs_by_framework():
    rails_hash = _mod.compute_config_hash("rails")
    python_hash = _mod.compute_config_hash("python")
    assert rails_hash != python_hash


def test_version_marker_with_config_hash(tmp_path):
    _mod.write_version_marker(
        tmp_path, _mod._plugin_version(), "rails", config_hash="abc123def456",
    )
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["config_hash"] == "abc123def456"


def test_version_marker_without_config_hash_has_no_key(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "config_hash" not in data


def test_config_hash_includes_setup_epoch():
    """Changing SETUP_EPOCH changes the config hash."""
    original_epoch = _mod.SETUP_EPOCH
    hash_before = _mod.compute_config_hash("rails")

    _mod.SETUP_EPOCH = original_epoch + 1
    try:
        hash_after = _mod.compute_config_hash("rails")
    finally:
        _mod.SETUP_EPOCH = original_epoch

    assert hash_before != hash_after


def test_happy_path_stores_config_hash(git_repo):
    """main() computes and stores config_hash in .flow.json."""
    result = _run(git_repo)
    assert result.returncode == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert "config_hash" in data
    assert len(data["config_hash"]) == 12


# --- Pre-commit hook installation ---


def test_pre_commit_hook_created(git_repo):
    _mod.install_pre_commit_hook(git_repo)
    hook_path = git_repo / ".git" / "hooks" / "pre-commit"
    assert hook_path.exists()


def test_pre_commit_hook_executable(git_repo):
    _mod.install_pre_commit_hook(git_repo)
    hook_path = git_repo / ".git" / "hooks" / "pre-commit"
    assert os.access(hook_path, os.X_OK)


def test_pre_commit_hook_content(git_repo):
    _mod.install_pre_commit_hook(git_repo)
    hook_path = git_repo / ".git" / "hooks" / "pre-commit"
    content = hook_path.read_text()
    assert ".flow-commit-msg" in content
    assert ".flow-states/" in content
    assert "exit 1" in content


def test_pre_commit_hook_idempotent(git_repo):
    _mod.install_pre_commit_hook(git_repo)
    content_first = (git_repo / ".git" / "hooks" / "pre-commit").read_text()
    _mod.install_pre_commit_hook(git_repo)
    content_second = (git_repo / ".git" / "hooks" / "pre-commit").read_text()
    assert content_first == content_second


def test_pre_commit_hook_blocks_direct_commit(git_repo, branch):
    _mod.install_pre_commit_hook(git_repo)
    # Create active FLOW state file for current branch
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    (state_dir / f"{branch}.json").write_text("{}")
    (git_repo / "test.txt").write_text("hello")
    subprocess.run(["git", "add", "test.txt"], cwd=git_repo, check=True)
    result = subprocess.run(
        ["git", "commit", "-m", "direct commit"],
        cwd=git_repo, capture_output=True, text=True,
    )
    assert result.returncode != 0
    assert "BLOCKED" in result.stderr or "BLOCKED" in result.stdout


def test_pre_commit_hook_allows_flow_commit(git_repo, branch):
    _mod.install_pre_commit_hook(git_repo)
    # Create active FLOW state file for current branch
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    (state_dir / f"{branch}.json").write_text("{}")
    (git_repo / "test.txt").write_text("hello")
    subprocess.run(["git", "add", "test.txt"], cwd=git_repo, check=True)
    (git_repo / ".flow-commit-msg").write_text("test commit message")
    result = subprocess.run(
        ["git", "commit", "-F", ".flow-commit-msg"],
        cwd=git_repo, capture_output=True, text=True,
    )
    assert result.returncode == 0


def test_pre_commit_hook_allows_commit_without_flow_state(git_repo):
    """No active FLOW feature — direct commits should succeed."""
    _mod.install_pre_commit_hook(git_repo)
    (git_repo / "test.txt").write_text("hello")
    subprocess.run(["git", "add", "test.txt"], cwd=git_repo, check=True)
    result = subprocess.run(
        ["git", "commit", "-m", "direct commit"],
        cwd=git_repo, capture_output=True, text=True,
    )
    assert result.returncode == 0


def test_pre_commit_hook_allows_commit_on_different_branch(git_repo):
    """FLOW state exists for another branch — commits on main should succeed."""
    _mod.install_pre_commit_hook(git_repo)
    state_dir = git_repo / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    (state_dir / "feature-branch.json").write_text("{}")
    (git_repo / "test.txt").write_text("hello")
    subprocess.run(["git", "add", "test.txt"], cwd=git_repo, check=True)
    result = subprocess.run(
        ["git", "commit", "-m", "direct commit"],
        cwd=git_repo, capture_output=True, text=True,
    )
    assert result.returncode == 0


# --- commit_format in version marker ---


def test_version_marker_with_commit_format(tmp_path):
    _mod.write_version_marker(
        tmp_path, _mod._plugin_version(), "rails", commit_format="full",
    )
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["commit_format"] == "full"


def test_version_marker_without_commit_format_has_no_key(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "commit_format" not in data


def test_version_marker_title_only_format(tmp_path):
    _mod.write_version_marker(
        tmp_path, _mod._plugin_version(), "python", commit_format="title-only",
    )
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["commit_format"] == "title-only"


# --- CLI --skills-json and --commit-format ---


def test_cli_skills_json_written_to_flow_json(git_repo):
    skills = {"flow-start": {"continue": "manual"}, "flow-abort": "auto"}
    result = _run(git_repo, skills_json=json.dumps(skills))
    assert result.returncode == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["skills"] == skills


def test_cli_commit_format_written_to_flow_json(git_repo):
    result = _run(git_repo, commit_format="title-only")
    assert result.returncode == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["commit_format"] == "title-only"


def test_cli_skills_json_and_commit_format_together(git_repo):
    skills = {"flow-code": {"commit": "auto", "continue": "auto"}}
    result = _run(
        git_repo, skills_json=json.dumps(skills), commit_format="full",
    )
    assert result.returncode == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["skills"] == skills
    assert data["commit_format"] == "full"


def test_cli_invalid_skills_json_returns_error(git_repo):
    result = _run(git_repo, skills_json="not valid json")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "skills-json" in data["message"].lower()


def test_cli_ignores_unknown_args(git_repo):
    cmd = [sys.executable, SCRIPT, str(git_repo), "--framework", "rails", "--unknown"]
    result = subprocess.run(cmd, capture_output=True, text=True)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"


# --- Consolidated prime-project and create-dependencies ---


def test_cli_primes_project_claude_md(git_repo):
    (git_repo / "CLAUDE.md").write_text("# Project\n")
    result = _run(git_repo, framework="rails")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["prime_project"] == "ok"
    content = (git_repo / "CLAUDE.md").read_text()
    assert "<!-- FLOW:BEGIN -->" in content


def test_cli_prime_project_error_does_not_block_success(git_repo):
    result = _run(git_repo, framework="rails")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["prime_project"] == "error"
    assert data["status"] == "ok"


def test_cli_creates_bin_dependencies(git_repo):
    result = _run(git_repo, framework="rails")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["dependencies"] == "ok"
    assert (git_repo / "bin" / "dependencies").exists()


def test_cli_dependencies_skipped_when_exists(git_repo):
    bin_dir = git_repo / "bin"
    bin_dir.mkdir()
    (bin_dir / "dependencies").write_text("#!/bin/bash\ncustom\n")
    result = _run(git_repo, framework="rails")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["dependencies"] == "skipped"
    assert (bin_dir / "dependencies").read_text() == "#!/bin/bash\ncustom\n"
