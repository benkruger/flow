"""Tests for lib/init-setup.py — the init phase setup script."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "init-setup.py")

# Import init-setup.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "init_setup", LIB_DIR / "init-setup.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(project_root):
    """Run init-setup.py via subprocess."""
    result = subprocess.run(
        [sys.executable, SCRIPT, str(project_root)],
        capture_output=True, text=True,
    )
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


# --- Settings merge ---


def test_creates_settings_from_scratch(git_repo):
    _run(git_repo)
    settings_path = git_repo / ".claude" / "settings.json"
    assert settings_path.exists()
    settings = json.loads(settings_path.read_text())
    assert "permissions" in settings
    assert "allow" in settings["permissions"]
    assert "deny" in settings["permissions"]


def test_settings_has_all_allow_entries(git_repo):
    _run(git_repo)
    settings = json.loads((git_repo / ".claude" / "settings.json").read_text())
    for entry in _mod.FLOW_ALLOW:
        assert entry in settings["permissions"]["allow"]


def test_settings_has_all_deny_entries(git_repo):
    _run(git_repo)
    settings = json.loads((git_repo / ".claude" / "settings.json").read_text())
    for entry in _mod.FLOW_DENY:
        assert entry in settings["permissions"]["deny"]


def test_settings_sets_default_mode(git_repo):
    _run(git_repo)
    settings = json.loads((git_repo / ".claude" / "settings.json").read_text())
    assert settings["permissions"]["defaultMode"] == "acceptEdits"


def test_settings_preserves_existing_entries(git_repo):
    settings_dir = git_repo / ".claude"
    settings_dir.mkdir()
    existing = {
        "permissions": {
            "allow": ["Bash(custom command)"],
            "deny": ["Bash(custom deny)"],
        }
    }
    (settings_dir / "settings.json").write_text(json.dumps(existing))

    _run(git_repo)

    settings = json.loads((settings_dir / "settings.json").read_text())
    assert "Bash(custom command)" in settings["permissions"]["allow"]
    assert "Bash(custom deny)" in settings["permissions"]["deny"]


def test_settings_preserves_existing_default_mode(git_repo):
    settings_dir = git_repo / ".claude"
    settings_dir.mkdir()
    existing = {
        "permissions": {
            "allow": [],
            "deny": [],
            "defaultMode": "plan",
        }
    }
    (settings_dir / "settings.json").write_text(json.dumps(existing))

    _run(git_repo)

    settings = json.loads((settings_dir / "settings.json").read_text())
    assert settings["permissions"]["defaultMode"] == "plan"


def test_settings_no_duplicate_entries(git_repo):
    _run(git_repo)
    _run(git_repo)

    settings = json.loads((git_repo / ".claude" / "settings.json").read_text())
    allow_list = settings["permissions"]["allow"]
    assert len(allow_list) == len(set(allow_list))
    deny_list = settings["permissions"]["deny"]
    assert len(deny_list) == len(set(deny_list))


# --- Version marker ---


def test_version_marker_created(git_repo):
    _run(git_repo)
    flow_json = git_repo / ".flow.json"
    assert flow_json.exists()
    data = json.loads(flow_json.read_text())
    assert "flow_version" in data


def test_version_marker_matches_plugin_version(git_repo):
    _run(git_repo)
    flow_data = json.loads((git_repo / ".flow.json").read_text())
    assert flow_data["flow_version"] == _mod._plugin_version()


# --- Git exclude ---


def test_git_exclude_updated(git_repo):
    result = _run(git_repo)
    data = json.loads(result.stdout)
    assert data["exclude_updated"] is True

    # Find the exclude file
    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    exclude_path = git_repo / git_common / "info" / "exclude"
    content = exclude_path.read_text()
    assert ".flow-states/" in content
    assert ".worktrees/" in content


def test_git_exclude_idempotent(git_repo):
    _run(git_repo)
    _run(git_repo)

    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    exclude_path = git_repo / git_common / "info" / "exclude"
    content = exclude_path.read_text()
    assert content.count(".flow-states/") == 1
    assert content.count(".worktrees/") == 1


def test_git_exclude_preserves_existing_content(git_repo):
    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    info_dir = git_repo / git_common / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    exclude_path = info_dir / "exclude"
    exclude_path.write_text("*.log\n")

    _run(git_repo)

    content = exclude_path.read_text()
    assert "*.log" in content
    assert ".flow-states/" in content


def test_git_exclude_not_updated_when_already_present(git_repo):
    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    info_dir = git_repo / git_common / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    (info_dir / "exclude").write_text(".flow-states/\n.worktrees/\n")

    result = _run(git_repo)
    data = json.loads(result.stdout)
    assert data["exclude_updated"] is False


# --- In-process tests ---


def test_merge_settings_empty_project(tmp_path):
    _mod.merge_settings(tmp_path)
    settings = json.loads((tmp_path / ".claude" / "settings.json").read_text())
    assert len(settings["permissions"]["allow"]) == len(_mod.FLOW_ALLOW)
    assert len(settings["permissions"]["deny"]) == len(_mod.FLOW_DENY)


def test_update_git_exclude_no_git(tmp_path):
    result = _mod.update_git_exclude(tmp_path)
    assert result is False


def test_update_git_exclude_adds_newline_if_missing(git_repo):
    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    info_dir = git_repo / git_common / "info"
    info_dir.mkdir(parents=True, exist_ok=True)
    (info_dir / "exclude").write_text("*.tmp")  # No trailing newline

    _mod.update_git_exclude(git_repo)

    content = (info_dir / "exclude").read_text()
    assert "*.tmp\n.flow-states/" in content


def test_update_git_exclude_creates_file_when_missing(git_repo):
    git_common = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=str(git_repo), capture_output=True, text=True, check=True,
    ).stdout.strip()
    info_dir = git_repo / git_common / "info"
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
        _mod, "_plugin_version",
        lambda: (_ for _ in ()).throw(RuntimeError("test error")),
    )
    import io
    captured = io.StringIO()
    monkeypatch.setattr(sys, "argv", [SCRIPT, str(git_repo)])
    monkeypatch.setattr(sys, "stdout", captured)
    with __import__("pytest").raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(captured.getvalue())
    assert data["status"] == "error"
    assert "test error" in data["message"]
