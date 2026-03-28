"""Tests for lib/promote-permissions.py — merge settings.local.json into settings.json."""

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

import importlib

_mod = importlib.import_module("promote-permissions")


# --- promote_permissions ---


def _setup_settings(tmp_path, settings_data):
    """Write .claude/settings.json with given data."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir(exist_ok=True)
    settings_file = claude_dir / "settings.json"
    settings_file.write_text(json.dumps(settings_data, indent=2))
    return settings_file


def _setup_local(tmp_path, local_data):
    """Write .claude/settings.local.json with given data."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir(exist_ok=True)
    local_file = claude_dir / "settings.local.json"
    local_file.write_text(json.dumps(local_data, indent=2))
    return local_file


def test_no_local_file(tmp_path):
    """Returns skipped when settings.local.json does not exist."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "skipped"
    assert result["reason"] == "no_local_file"


def test_empty_allow_list(tmp_path):
    """Empty allow list in local still deletes local and returns ok."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": []}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == []
    assert result["already_present"] == 0
    assert not local_file.exists()


def test_new_entries_promoted(tmp_path):
    """New entries are merged into settings.json allow list."""
    settings_file = _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": ["Bash(npm run *)"], "deny": []}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == ["Bash(npm run *)"]
    assert result["already_present"] == 0
    assert not local_file.exists()

    updated = json.loads(settings_file.read_text())
    assert "Bash(npm run *)" in updated["permissions"]["allow"]
    assert "Bash(git *)" in updated["permissions"]["allow"]


def test_all_duplicates(tmp_path):
    """All entries already present — returns count, still deletes local."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)", "Bash(npm run *)"]}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)", "Bash(npm run *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == []
    assert result["already_present"] == 2
    assert not local_file.exists()


def test_mixed_new_and_existing(tmp_path):
    """Some entries are new, some are duplicates."""
    settings_file = _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)", "Bash(make *)", "Bash(curl *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert sorted(result["promoted"]) == ["Bash(curl *)", "Bash(make *)"]
    assert result["already_present"] == 1
    assert not local_file.exists()

    updated = json.loads(settings_file.read_text())
    assert len(updated["permissions"]["allow"]) == 3


def test_preserves_existing_settings(tmp_path):
    """Non-permissions keys in settings.json survive the merge."""
    settings_data = {
        "permissions": {"allow": ["Bash(git *)"], "deny": ["Bash(rm -rf *)"]},
        "attribution": {"commit": "", "pr": ""},
    }
    settings_file = _setup_settings(tmp_path, settings_data)
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(npm run *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    updated = json.loads(settings_file.read_text())
    assert updated["attribution"] == {"commit": "", "pr": ""}
    assert updated["permissions"]["deny"] == ["Bash(rm -rf *)"]


def test_deletion_verification(tmp_path):
    """settings.local.json is deleted after successful merge."""
    _setup_settings(tmp_path, {"permissions": {"allow": []}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    assert local_file.exists()
    _mod.promote_permissions(str(tmp_path))
    assert not local_file.exists()


def test_malformed_local_json(tmp_path):
    """Returns error for unparseable settings.local.json."""
    _setup_settings(tmp_path, {"permissions": {"allow": []}})
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir(exist_ok=True)
    (claude_dir / "settings.local.json").write_text("{bad json")

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "error"
    assert "settings.local.json" in result["message"]


def test_malformed_settings_json(tmp_path):
    """Returns error for unparseable settings.json."""
    claude_dir = tmp_path / ".claude"
    claude_dir.mkdir(exist_ok=True)
    (claude_dir / "settings.json").write_text("{bad json")
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "error"
    assert "settings.json" in result["message"]


def test_missing_permissions_key_in_local(tmp_path):
    """No permissions key in local — deletes local, returns ok with nothing promoted."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    local_file = _setup_local(tmp_path, {"attribution": {"commit": ""}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == []
    assert result["already_present"] == 0
    assert not local_file.exists()


def test_missing_allow_key_in_local(tmp_path):
    """permissions but no allow key in local — deletes local, returns ok."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    local_file = _setup_local(tmp_path, {"permissions": {"deny": ["Bash(rm *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == []
    assert result["already_present"] == 0
    assert not local_file.exists()


def test_settings_json_missing(tmp_path):
    """Returns error when settings.json does not exist."""
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "error"
    assert "settings.json" in result["message"]


def test_settings_json_no_permissions_key(tmp_path):
    """settings.json with no permissions key gets one created during merge."""
    settings_file = _setup_settings(tmp_path, {"attribution": {"commit": ""}})
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == ["Bash(git *)"]
    updated = json.loads(settings_file.read_text())
    assert "permissions" in updated
    assert "Bash(git *)" in updated["permissions"]["allow"]


def test_write_error(tmp_path, monkeypatch):
    """Returns error when settings.json cannot be written."""
    _setup_settings(tmp_path, {"permissions": {"allow": []}})
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    original_open = open

    def failing_open(path, *args, **kwargs):
        if "settings.json" in str(path) and "w" in args:
            raise OSError("permission denied")
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr("builtins.open", failing_open)

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "error"
    assert "Could not write settings.json" in result["message"]


def test_local_delete_fails_silently(tmp_path, monkeypatch):
    """Continues without error when settings.local.json deletion fails."""
    settings_file = _setup_settings(tmp_path, {"permissions": {"allow": []}})
    local_file = _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    original_remove = os.remove

    def failing_remove(path):
        if "settings.local.json" in str(path):
            raise OSError("permission denied")
        original_remove(path)

    monkeypatch.setattr("os.remove", failing_remove)

    result = _mod.promote_permissions(str(tmp_path))

    assert result["status"] == "ok"
    assert result["promoted"] == ["Bash(git *)"]
    assert local_file.exists()  # still exists because delete failed
    updated = json.loads(settings_file.read_text())
    assert "Bash(git *)" in updated["permissions"]["allow"]


def test_cli_integration_error_exit(tmp_path, monkeypatch, capsys):
    """CLI exits non-zero on error."""
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})

    monkeypatch.setattr(
        "sys.argv",
        [
            "promote-permissions.py",
            "--worktree-path",
            str(tmp_path),
        ],
    )

    import pytest

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code == 1
    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"


# --- CLI integration ---


def test_cli_integration_happy_path(tmp_path, monkeypatch, capsys):
    """CLI merges permissions and outputs JSON."""
    _setup_settings(tmp_path, {"permissions": {"allow": ["Bash(git *)"]}})
    _setup_local(tmp_path, {"permissions": {"allow": ["Bash(npm run *)"]}})

    monkeypatch.setattr(
        "sys.argv",
        [
            "promote-permissions.py",
            "--worktree-path",
            str(tmp_path),
        ],
    )

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "ok"
    assert output["promoted"] == ["Bash(npm run *)"]


def test_cli_integration_no_local(tmp_path, monkeypatch, capsys):
    """CLI returns skipped when no local file exists."""
    _setup_settings(tmp_path, {"permissions": {"allow": []}})

    monkeypatch.setattr(
        "sys.argv",
        [
            "promote-permissions.py",
            "--worktree-path",
            str(tmp_path),
        ],
    )

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "skipped"
    assert output["reason"] == "no_local_file"
