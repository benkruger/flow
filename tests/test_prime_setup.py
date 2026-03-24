"""Tests for lib/prime-setup.py — the prime phase setup script."""

import importlib.util
import json
import os
import subprocess
import sys
from unittest.mock import patch

import pytest

from conftest import FRAMEWORKS_DIR, LIB_DIR

# Import prime-setup.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "prime_setup", LIB_DIR / "prime-setup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run_main(monkeypatch, capsys, project_root, framework="rails",
              skills_json=None, commit_format=None, plugin_root=None):
    """Run prime-setup main() in-process, return (exit_code, stdout_text)."""
    argv = ["prime-setup"]
    if project_root is not None:
        argv.append(str(project_root))
    if framework:
        argv.extend(["--framework", framework])
    if skills_json is not None:
        argv.extend(["--skills-json", skills_json])
    if commit_format is not None:
        argv.extend(["--commit-format", commit_format])
    if plugin_root is not None:
        argv.extend(["--plugin-root", plugin_root])
    monkeypatch.setattr("sys.argv", argv)
    exit_code = 0
    try:
        _mod.main()
    except SystemExit as e:
        exit_code = e.code
    captured = capsys.readouterr()
    return exit_code, captured.out


# --- CLI behavior ---


def test_missing_args_returns_error(monkeypatch, capsys):
    monkeypatch.setattr("sys.argv", ["prime-setup"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_invalid_project_root_returns_error(tmp_path, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, tmp_path / "nonexistent")
    assert exit_code == 1
    data = json.loads(stdout)
    assert data["status"] == "error"


def test_happy_path_returns_ok(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo)
    assert exit_code == 0
    data = json.loads(stdout)
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


def _write_settings(tmp_path, settings):
    """Write a settings.json file with the given content."""
    settings_dir = tmp_path / ".claude"
    settings_dir.mkdir(parents=True, exist_ok=True)
    (settings_dir / "settings.json").write_text(json.dumps(settings))


# --- Pattern subsumption (in-process) ---


def test_broad_pattern_subsumes_narrow(tmp_path):
    """Bash(git *) should prevent appending Bash(git add *) etc."""
    _write_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    # Specific git entries should NOT appear because Bash(git *) subsumes them
    assert "Bash(git add *)" not in allow
    assert "Bash(git commit *)" not in allow
    assert "Bash(git diff *)" not in allow
    assert "Bash(git push *)" not in allow
    # Non-git entries should still appear
    assert "Bash(cd *)" in allow
    assert "Agent(flow:ci-fixer)" in allow


def test_broad_gh_pattern_subsumes_narrow(tmp_path):
    """Bash(gh pr *) should prevent appending Bash(gh pr create *) etc."""
    _write_settings(tmp_path, {"permissions": {"allow": ["Bash(gh pr *)"]}})
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    assert "Bash(gh pr create *)" not in allow
    assert "Bash(gh pr edit *)" not in allow
    # gh issue entries are NOT subsumed by gh pr *
    assert "Bash(gh issue *)" in allow


def test_narrow_does_not_subsume_broad(tmp_path):
    """Bash(git add *) should NOT prevent appending Bash(git *)."""
    _write_settings(tmp_path, {"permissions": {"allow": ["Bash(git add *)"]}})
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    # The broad pattern from UNIVERSAL_ALLOW is not subsumed by the narrow one
    assert "Bash(git *)" not in _mod.UNIVERSAL_ALLOW  # git * isn't in UNIVERSAL
    # But git add * IS in UNIVERSAL_ALLOW and should be deduped by exact match
    assert allow.count("Bash(git add *)") == 1


def test_cross_type_no_subsumption(tmp_path):
    """Non-Bash entries should not subsume Bash entries."""
    _write_settings(tmp_path, {"permissions": {"allow": ["Agent(*)"]}})
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    # Bash entries should still be added despite Agent(*)
    assert "Bash(git add *)" in allow


def test_non_wildcard_no_subsumption(tmp_path):
    """Bash(git status) should not subsume Bash(git add *)."""
    _write_settings(tmp_path, {"permissions": {"allow": ["Bash(git status)"]}})
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    assert "Bash(git add *)" in allow


def test_derived_permissions_subsumed(tmp_path):
    """Broad pattern should subsume derived permissions too."""
    (tmp_path / "MyApp.xcodeproj").mkdir()
    # Bash(killall *) would subsume Bash(killall MyApp)
    _write_settings(tmp_path, {"permissions": {"allow": ["Bash(killall *)"]}})
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow = settings["permissions"]["allow"]
    assert "Bash(killall MyApp)" not in allow


def test_is_subsumed_malformed_candidate():
    """Non-permission-format strings return False (not subsumed)."""
    assert not _mod._is_subsumed("plain-string", {"Bash(git *)"})


def test_is_subsumed_skips_exact_match():
    """Exact match is handled by set membership, not subsumption."""
    # Entry present in both candidate and existing — skip self-comparison
    assert not _mod._is_subsumed("Bash(git add *)", {"Bash(git add *)"})


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
        ".claude/scheduled_tasks.lock\n"
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
    monkeypatch.setattr(sys, "argv", ["prime-setup", str(git_repo), "--framework", "rails"])
    monkeypatch.setattr(sys, "stdout", captured)
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(captured.getvalue())
    assert data["status"] == "error"
    assert "test error" in data["message"]


# --- Framework argument ---


def test_missing_framework_returns_error(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework=None)
    assert exit_code == 1
    data = json.loads(stdout)
    assert data["status"] == "error"
    assert "framework" in data["message"].lower()


def test_invalid_framework_returns_error(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="django")
    assert exit_code == 1
    data = json.loads(stdout)
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


def test_settings_has_all_allow_entries_ios(tmp_path):
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    expected = _mod.UNIVERSAL_ALLOW + _load_framework_permissions("ios")
    for entry in expected:
        assert entry in settings["permissions"]["allow"]


def test_flow_json_includes_framework_ios(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "ios")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["framework"] == "ios"


def test_ios_framework_excludes_rails_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("rails"):
        assert entry not in settings["permissions"]["allow"]


def test_ios_framework_excludes_python_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("python"):
        assert entry not in settings["permissions"]["allow"]


def test_rails_framework_excludes_ios_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "rails")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("ios"):
        assert entry not in settings["permissions"]["allow"]


def test_python_framework_excludes_ios_permissions(tmp_path):
    _mod.merge_settings(tmp_path, "python")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    for entry in _load_framework_permissions("ios"):
        assert entry not in settings["permissions"]["allow"]


def test_framework_output_in_ok_response(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="python")
    assert exit_code == 0
    data = json.loads(stdout)
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


def test_universal_allow_includes_chmod():
    assert "Bash(chmod +x bin/*)" in _mod.UNIVERSAL_ALLOW


def test_universal_allow_includes_claude_plugin_commands():
    assert "Bash(claude plugin list)" in _mod.UNIVERSAL_ALLOW
    assert "Bash(claude plugin marketplace add *)" in _mod.UNIVERSAL_ALLOW
    assert "Bash(claude plugin install *)" in _mod.UNIVERSAL_ALLOW


def test_gh_entries_grouped_in_universal_allow():
    """All gh entries in UNIVERSAL_ALLOW must be contiguous (no gaps)."""
    indices = [
        i for i, entry in enumerate(_mod.UNIVERSAL_ALLOW)
        if entry.startswith("Bash(gh ")
    ]
    assert len(indices) >= 2, "Expected multiple gh entries"
    assert indices[-1] - indices[0] + 1 == len(indices), (
        f"gh entries are not contiguous: indices {indices}"
    )


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


def test_happy_path_stores_config_hash(git_repo, monkeypatch, capsys):
    """main() computes and stores config_hash in .flow.json."""
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo)
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert "config_hash" in data
    assert len(data["config_hash"]) == 12


# --- Setup hash ---


def test_compute_setup_hash_returns_12_hex_chars():
    result = _mod.compute_setup_hash()
    assert isinstance(result, str)
    assert len(result) == 12
    assert all(c in "0123456789abcdef" for c in result)


def test_compute_setup_hash_is_deterministic():
    hash1 = _mod.compute_setup_hash()
    hash2 = _mod.compute_setup_hash()
    assert hash1 == hash2


def test_compute_setup_hash_matches_file_content():
    """Verify the hash is derived from the actual file bytes."""
    import hashlib
    from pathlib import Path
    content = (LIB_DIR / "prime-setup.py").read_bytes()
    expected = hashlib.sha256(content).hexdigest()[:12]
    assert _mod.compute_setup_hash() == expected


def test_version_marker_with_setup_hash(tmp_path):
    _mod.write_version_marker(
        tmp_path, _mod._plugin_version(), "rails", setup_hash="abc123def456",
    )
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["setup_hash"] == "abc123def456"


def test_version_marker_without_setup_hash_has_no_key(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "setup_hash" not in data


def test_happy_path_stores_setup_hash(git_repo, monkeypatch, capsys):
    """main() computes and stores setup_hash in .flow.json."""
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo)
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert "setup_hash" in data
    assert len(data["setup_hash"]) == 12


# --- Plugin root in version marker ---


def test_version_marker_with_plugin_root(tmp_path):
    _mod.write_version_marker(
        tmp_path, _mod._plugin_version(), "rails",
        plugin_root="/some/cache/path",
    )
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["plugin_root"] == "/some/cache/path"


def test_version_marker_without_plugin_root_has_no_key(tmp_path):
    _mod.write_version_marker(tmp_path, _mod._plugin_version(), "rails")
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "plugin_root" not in data


# --- Shared script installation helper ---


def test_install_script_creates_executable_file(tmp_path):
    target_dir = tmp_path / "subdir"
    content = "#!/bin/bash\necho hi\n"
    _mod._install_script(target_dir, "my-script", content)
    script = target_dir / "my-script"
    assert target_dir.is_dir()
    assert script.exists()
    assert script.read_text() == content
    assert os.access(script, os.X_OK)


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


# --- Derived permissions ---


def test_derive_permissions_ios_xcodeproj(tmp_path):
    (tmp_path / "MyApp.xcodeproj").mkdir()
    result = _mod._derive_permissions(tmp_path, "ios")
    assert "Bash(killall MyApp)" in result


def test_derive_permissions_no_xcodeproj(tmp_path):
    result = _mod._derive_permissions(tmp_path, "ios")
    assert result == []


def test_derive_permissions_rails_has_none(tmp_path):
    result = _mod._derive_permissions(tmp_path, "rails")
    assert result == []


def test_derive_permissions_unknown_framework(tmp_path):
    result = _mod._derive_permissions(tmp_path, "nonexistent")
    assert result == []


def test_derived_permissions_merged_into_settings(tmp_path):
    (tmp_path / "SaltedKitchen.xcodeproj").mkdir()
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    assert "Bash(killall SaltedKitchen)" in settings["permissions"]["allow"]


def test_derived_permissions_no_duplicates(tmp_path):
    (tmp_path / "SaltedKitchen.xcodeproj").mkdir()
    _mod.merge_settings(tmp_path, "ios")
    _mod.merge_settings(tmp_path, "ios")
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    allow_list = settings["permissions"]["allow"]
    assert allow_list.count("Bash(killall SaltedKitchen)") == 1


def test_derived_permissions_not_in_config_hash(tmp_path):
    (tmp_path / "SomeApp.xcodeproj").mkdir()
    hash_value = _mod.compute_config_hash("ios")
    assert isinstance(hash_value, str)
    assert len(hash_value) == 12


def test_cli_derives_permissions_from_project(git_repo, monkeypatch, capsys):
    (git_repo / "TestApp.xcodeproj").mkdir()
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="ios")
    assert exit_code == 0
    settings = json.loads(
        (git_repo / ".claude" / "settings.json").read_text()
    )
    assert "Bash(killall TestApp)" in settings["permissions"]["allow"]


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


# --- Global launcher ---


def test_install_launcher_creates_file(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    _mod.install_launcher()
    launcher = tmp_path / ".local" / "bin" / "flow"
    assert launcher.exists()


def test_install_launcher_executable(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    _mod.install_launcher()
    launcher = tmp_path / ".local" / "bin" / "flow"
    assert os.access(launcher, os.X_OK)


def test_install_launcher_content(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    _mod.install_launcher()
    content = (tmp_path / ".local" / "bin" / "flow").read_text()
    assert "git rev-parse --show-toplevel" in content
    assert ".flow.json" in content
    assert "plugin_root" in content
    assert 'exec "$plugin_root/bin/flow"' in content
    assert "#!/usr/bin/env bash" in content


def test_install_launcher_idempotent(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    _mod.install_launcher()
    content_first = (tmp_path / ".local" / "bin" / "flow").read_text()
    _mod.install_launcher()
    content_second = (tmp_path / ".local" / "bin" / "flow").read_text()
    assert content_first == content_second


def test_install_launcher_creates_directory(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    assert not (tmp_path / ".local" / "bin").exists()
    _mod.install_launcher()
    assert (tmp_path / ".local" / "bin" / "flow").exists()


def test_check_launcher_path_in_path(tmp_path, monkeypatch, capsys):
    local_bin = str(tmp_path / ".local" / "bin")
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("PATH", f"{local_bin}:/usr/bin")
    _mod.check_launcher_path()
    captured = capsys.readouterr()
    assert captured.err == ""


def test_check_launcher_path_not_in_path(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("PATH", "/usr/bin:/usr/local/bin")
    _mod.check_launcher_path()
    captured = capsys.readouterr()
    assert "export PATH" in captured.err
    assert ".local/bin" in captured.err


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


def test_cli_skills_json_written_to_flow_json(git_repo, monkeypatch, capsys):
    skills = {"flow-start": {"continue": "manual"}, "flow-abort": "auto"}
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo, skills_json=json.dumps(skills),
    )
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["skills"] == skills


def test_cli_commit_format_written_to_flow_json(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo, commit_format="title-only",
    )
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["commit_format"] == "title-only"


def test_cli_skills_json_and_commit_format_together(git_repo, monkeypatch, capsys):
    skills = {"flow-code": {"commit": "auto", "continue": "auto"}}
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo,
        skills_json=json.dumps(skills), commit_format="full",
    )
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["skills"] == skills
    assert data["commit_format"] == "full"


def test_cli_invalid_skills_json_returns_error(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo, skills_json="not valid json",
    )
    assert exit_code == 1
    data = json.loads(stdout)
    assert data["status"] == "error"
    assert "skills-json" in data["message"].lower()


def test_cli_ignores_unknown_args(git_repo, monkeypatch, capsys):
    monkeypatch.setattr("sys.argv", [
        "prime-setup", str(git_repo), "--framework", "rails", "--unknown",
    ])
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


# --- CLI --plugin-root ---


def test_cli_plugin_root_written_to_flow_json(git_repo, tmp_path, monkeypatch, capsys):
    fake_home = tmp_path / "fakehome"
    fake_home.mkdir()
    monkeypatch.setenv("HOME", str(fake_home))
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo, plugin_root="/some/cache/path",
    )
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert data["plugin_root"] == "/some/cache/path"


def test_cli_launcher_installed_in_output(git_repo, tmp_path, monkeypatch, capsys):
    fake_home = tmp_path / "fakehome"
    fake_home.mkdir()
    monkeypatch.setenv("HOME", str(fake_home))
    exit_code, stdout = _run_main(
        monkeypatch, capsys, git_repo, plugin_root="/some/cache/path",
    )
    assert exit_code == 0
    data = json.loads(stdout)
    assert data["launcher_installed"] is True


def test_cli_no_plugin_root_no_launcher(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo)
    assert exit_code == 0
    data = json.loads((git_repo / ".flow.json").read_text())
    assert "plugin_root" not in data
    output = json.loads(stdout)
    assert output["launcher_installed"] is False


# --- Consolidated prime-project and create-dependencies ---


def test_cli_primes_project_claude_md(git_repo, monkeypatch, capsys):
    (git_repo / "CLAUDE.md").write_text("# Project\n")
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="rails")
    assert exit_code == 0
    data = json.loads(stdout)
    assert data["prime_project"] == "ok"
    content = (git_repo / "CLAUDE.md").read_text()
    assert "<!-- FLOW:BEGIN -->" in content


def test_cli_prime_project_error_does_not_block_success(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="rails")
    assert exit_code == 0
    data = json.loads(stdout)
    assert data["prime_project"] == "error"
    assert data["status"] == "ok"


def test_cli_creates_bin_dependencies(git_repo, monkeypatch, capsys):
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="rails")
    assert exit_code == 0
    data = json.loads(stdout)
    assert data["dependencies"] == "ok"
    assert (git_repo / "bin" / "dependencies").exists()


def test_cli_dependencies_skipped_when_exists(git_repo, monkeypatch, capsys):
    bin_dir = git_repo / "bin"
    bin_dir.mkdir()
    (bin_dir / "dependencies").write_text("#!/bin/bash\ncustom\n")
    exit_code, stdout = _run_main(monkeypatch, capsys, git_repo, framework="rails")
    assert exit_code == 0
    data = json.loads(stdout)
    assert data["dependencies"] == "skipped"
    assert (bin_dir / "dependencies").read_text() == "#!/bin/bash\ncustom\n"


# --- Slack config ---


def test_write_slack_config_both_vars_present(tmp_path):
    """Writes slack block to .flow.json when both env vars are set."""
    # Create initial .flow.json
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test-token", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout=json.dumps({"ok": True}), stderr="",
            )
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["slack"]["bot_token"] == "xoxb-test-token"
    assert data["slack"]["channel"] == "C12345"
    assert data["notify"] == "auto"


def test_write_slack_config_token_missing(tmp_path):
    """Removes slack block when FLOW_SLACK_TOKEN is absent."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({
            "flow_version": "0.36.2", "framework": "rails",
            "slack": {"bot_token": "old", "channel": "old"},
            "notify": "auto",
        }) + "\n"
    )
    env = {"FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env, clear=False):
        os.environ.pop("FLOW_SLACK_TOKEN", None)
        _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "slack" not in data
    assert data.get("notify") == "never"


def test_write_slack_config_channel_missing(tmp_path):
    """Removes slack block when FLOW_SLACK_CHANNEL is absent."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({
            "flow_version": "0.36.2", "framework": "rails",
            "slack": {"bot_token": "old", "channel": "old"},
            "notify": "auto",
        }) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test"}
    with patch.dict(os.environ, env, clear=False):
        os.environ.pop("FLOW_SLACK_CHANNEL", None)
        _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "slack" not in data
    assert data.get("notify") == "never"


def test_write_slack_config_both_missing(tmp_path):
    """Removes slack block and sets notify=never when both vars absent."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    with patch.dict(os.environ, {}, clear=False):
        os.environ.pop("FLOW_SLACK_TOKEN", None)
        os.environ.pop("FLOW_SLACK_CHANNEL", None)
        _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert "slack" not in data
    assert data.get("notify") == "never"


def test_write_slack_config_validates_token(tmp_path):
    """Calls auth.test to validate the Slack token."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout=json.dumps({"ok": True}), stderr="",
            )
            _mod.write_slack_config(tmp_path)
    call_args = mock_run.call_args[0][0]
    assert "auth.test" in " ".join(call_args)


def test_write_slack_config_warns_on_invalid_token(tmp_path, capsys):
    """Writes config but warns when auth.test fails."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-bad", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout=json.dumps({"ok": False, "error": "invalid_auth"}),
                stderr="",
            )
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["slack"]["bot_token"] == "xoxb-bad"
    captured = capsys.readouterr()
    assert "invalid_auth" in captured.err


def test_write_slack_config_warns_on_curl_failure(tmp_path, capsys):
    """Writes config but warns when curl fails."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=1, stdout="", stderr="Connection refused",
            )
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["slack"]["bot_token"] == "xoxb-test"
    captured = capsys.readouterr()
    assert "warning" in captured.err.lower() or "failed" in captured.err.lower()


def test_write_slack_config_auth_invalid_json(tmp_path, capsys):
    """Writes config but warns when auth.test returns invalid JSON."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout="<html>error</html>", stderr="",
            )
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["slack"]["bot_token"] == "xoxb-test"
    captured = capsys.readouterr()
    assert "invalid json" in captured.err.lower()


def test_write_slack_config_auth_timeout(tmp_path, capsys):
    """Writes config but warns when auth.test times out."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.36.2", "framework": "rails"}) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.side_effect = subprocess.TimeoutExpired(cmd="curl", timeout=10)
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["slack"]["bot_token"] == "xoxb-test"
    captured = capsys.readouterr()
    assert "timed out" in captured.err.lower()


def test_write_slack_config_no_flow_json(tmp_path):
    """Does nothing when .flow.json does not exist."""
    with patch.dict(os.environ, {"FLOW_SLACK_TOKEN": "xoxb", "FLOW_SLACK_CHANNEL": "C123"}):
        _mod.write_slack_config(tmp_path)
    assert not (tmp_path / ".flow.json").exists()


def test_write_slack_config_preserves_other_keys(tmp_path):
    """Slack config write preserves existing .flow.json keys."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({
            "flow_version": "0.36.2", "framework": "rails",
            "config_hash": "abc123", "skills": {"flow-start": "manual"},
        }) + "\n"
    )
    env = {"FLOW_SLACK_TOKEN": "xoxb-test", "FLOW_SLACK_CHANNEL": "C12345"}
    with patch.dict(os.environ, env):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout=json.dumps({"ok": True}), stderr="",
            )
            _mod.write_slack_config(tmp_path)
    data = json.loads((tmp_path / ".flow.json").read_text())
    assert data["config_hash"] == "abc123"
    assert data["skills"] == {"flow-start": "manual"}
    assert data["slack"]["bot_token"] == "xoxb-test"
