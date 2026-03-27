"""Tests for lib/qa-mode.py — manage dev-mode plugin_root redirection."""

import json

import pytest
from conftest import import_lib


def _write_flow_json(path, data):
    """Write a .flow.json file."""
    path.write_text(json.dumps(data) + "\n")


def _read_flow_json(path):
    """Read and parse .flow.json."""
    return json.loads(path.read_text())


# --- start() ---


def test_start_happy_path(tmp_path):
    """start() saves backup and redirects plugin_root."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "framework": "python",
            "plugin_root": "/original/cache/path",
        },
    )

    result = mod.start(flow_json, local_source)

    assert result["status"] == "ok"
    assert result["plugin_root"] == str(local_source)
    assert result["backup"] == "/original/cache/path"

    data = _read_flow_json(flow_json)
    assert data["plugin_root"] == str(local_source)
    assert data["plugin_root_backup"] == "/original/cache/path"


def test_start_missing_flow_json(tmp_path):
    """start() returns error when .flow.json does not exist."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"

    result = mod.start(flow_json, local_source)

    assert result["status"] == "error"
    assert "not found" in result["message"].lower() or "does not exist" in result["message"].lower()


def test_start_missing_plugin_root(tmp_path):
    """start() returns error when .flow.json has no plugin_root."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "framework": "python",
        },
    )

    result = mod.start(flow_json, local_source)

    assert result["status"] == "error"
    assert "plugin_root" in result["message"]


def test_start_already_in_dev_mode(tmp_path):
    """start() returns error when plugin_root_backup already exists."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/some/path",
            "plugin_root_backup": "/original/path",
        },
    )

    result = mod.start(flow_json, local_source)

    assert result["status"] == "error"
    assert "already" in result["message"].lower() or "dev mode" in result["message"].lower()


def test_start_invalid_local_path_not_exists(tmp_path):
    """start() returns error when local source path doesn't exist."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/original/path",
        },
    )

    result = mod.start(flow_json, tmp_path / "nonexistent")

    assert result["status"] == "error"
    assert "not found" in result["message"].lower() or "does not exist" in result["message"].lower()


def test_start_invalid_local_path_no_bin_flow(tmp_path):
    """start() returns error when local source has no bin/flow."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/original/path",
        },
    )

    result = mod.start(flow_json, local_source)

    assert result["status"] == "error"
    assert "bin/flow" in result["message"]


def test_start_preserves_other_keys(tmp_path):
    """start() preserves all existing .flow.json keys."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    original = {
        "flow_version": "0.39.0",
        "framework": "python",
        "config_hash": "abc123",
        "setup_hash": "def456",
        "commit_format": "conventional",
        "plugin_root": "/original/cache/path",
        "skills": {"flow-start": {"continue": "auto"}},
    }
    _write_flow_json(flow_json, original)

    mod.start(flow_json, local_source)

    data = _read_flow_json(flow_json)
    assert data["flow_version"] == "0.39.0"
    assert data["framework"] == "python"
    assert data["config_hash"] == "abc123"
    assert data["setup_hash"] == "def456"
    assert data["commit_format"] == "conventional"
    assert data["skills"] == {"flow-start": {"continue": "auto"}}
    assert data["plugin_root"] == str(local_source)
    assert data["plugin_root_backup"] == "/original/cache/path"


# --- stop() ---


def test_stop_happy_path(tmp_path):
    """stop() restores plugin_root from backup and removes backup key."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/local/dev/path",
            "plugin_root_backup": "/original/cache/path",
        },
    )

    result = mod.stop(flow_json)

    assert result["status"] == "ok"
    assert result["restored"] == "/original/cache/path"

    data = _read_flow_json(flow_json)
    assert data["plugin_root"] == "/original/cache/path"
    assert "plugin_root_backup" not in data


def test_stop_not_in_dev_mode(tmp_path):
    """stop() returns error when plugin_root_backup doesn't exist."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/some/path",
        },
    )

    result = mod.stop(flow_json)

    assert result["status"] == "error"
    assert "not in dev mode" in result["message"].lower() or "backup" in result["message"].lower()


def test_stop_missing_flow_json(tmp_path):
    """stop() returns error when .flow.json does not exist."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    result = mod.stop(flow_json)

    assert result["status"] == "error"
    assert "not found" in result["message"].lower() or "does not exist" in result["message"].lower()


def test_stop_preserves_other_keys(tmp_path):
    """stop() preserves all existing .flow.json keys except backup."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "framework": "python",
            "config_hash": "abc123",
            "plugin_root": "/local/dev/path",
            "plugin_root_backup": "/original/cache/path",
            "skills": {"flow-code": {"commit": "auto"}},
        },
    )

    mod.stop(flow_json)

    data = _read_flow_json(flow_json)
    assert data["flow_version"] == "0.39.0"
    assert data["framework"] == "python"
    assert data["config_hash"] == "abc123"
    assert data["skills"] == {"flow-code": {"commit": "auto"}}
    assert data["plugin_root"] == "/original/cache/path"
    assert "plugin_root_backup" not in data


# --- CLI integration ---


def test_cli_start(tmp_path, monkeypatch, capsys):
    """CLI --start --local-path produces correct JSON output."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    local_source = tmp_path / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/original/path",
        },
    )

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--start",
            "--local-path",
            str(local_source),
            "--flow-json",
            str(flow_json),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 0

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "ok"
    assert output["plugin_root"] == str(local_source)


def test_cli_stop(tmp_path, monkeypatch, capsys):
    """CLI --stop produces correct JSON output."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/dev/path",
            "plugin_root_backup": "/original/path",
        },
    )

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--stop",
            "--flow-json",
            str(flow_json),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 0

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "ok"
    assert output["restored"] == "/original/path"


def test_cli_start_error(tmp_path, monkeypatch, capsys):
    """CLI --start returns error JSON on failure."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--start",
            "--local-path",
            "/nonexistent",
            "--flow-json",
            str(flow_json),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "error"


def test_cli_stop_error(tmp_path, monkeypatch, capsys):
    """CLI --stop returns error JSON on failure."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--stop",
            "--flow-json",
            str(flow_json),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "error"


def test_cli_start_missing_local_path(tmp_path, monkeypatch, capsys):
    """CLI --start without --local-path returns error."""
    mod = import_lib("qa-mode.py")
    flow_json = tmp_path / ".flow.json"
    _write_flow_json(flow_json, {"flow_version": "0.39.0", "plugin_root": "/p"})

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--start",
            "--flow-json",
            str(flow_json),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "error"
    assert "--local-path" in output["message"]


def test_cli_default_flow_json(git_repo, monkeypatch, capsys):
    """CLI uses project_root()/.flow.json when --flow-json is omitted."""
    mod = import_lib("qa-mode.py")
    flow_json = git_repo / ".flow.json"
    local_source = git_repo / "flow-source"
    local_source.mkdir()
    (local_source / "bin").mkdir()
    (local_source / "bin" / "flow").write_text("#!/bin/bash\n")

    _write_flow_json(
        flow_json,
        {
            "flow_version": "0.39.0",
            "plugin_root": "/original/path",
        },
    )

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr(
        "sys.argv",
        [
            "qa-mode",
            "--start",
            "--local-path",
            str(local_source),
        ],
    )
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 0

    output = json.loads(capsys.readouterr().out.strip())
    assert output["status"] == "ok"

    data = _read_flow_json(flow_json)
    assert data["plugin_root"] == str(local_source)
