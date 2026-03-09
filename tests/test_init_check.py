"""Tests for lib/init-check.py — standalone version gate."""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "init-check.py")


def _computed_config_hash(framework):
    """Compute config hash for test fixtures."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "init_setup",
        Path(__file__).resolve().parent.parent / "lib" / "init-setup.py",
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.compute_config_hash(framework)


def _current_plugin_data():
    """Read the full plugin.json."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())


def _current_plugin_version():
    """Read the current version from plugin.json."""
    return _current_plugin_data()["version"]


def _run(cwd):
    """Run init-check.py inside the given directory."""
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True, cwd=str(cwd),
    )
    return result


def test_fails_when_flow_json_missing(tmp_path):
    """init-check.py returns error when .flow.json is missing."""
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "/flow:flow-init" in data["message"]


def test_fails_when_flow_version_mismatch(tmp_path):
    """init-check.py returns error when .flow.json has wrong version."""
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": "0.0.0", "framework": "rails"})
    )
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "mismatch" in data["message"]


def test_fails_when_framework_missing(tmp_path):
    """init-check.py returns error when .flow.json has no framework field."""
    version = _current_plugin_version()
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": version})
    )
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "framework" in data["message"].lower()


def test_happy_path_returns_ok(tmp_path):
    """init-check.py returns ok with framework when everything matches."""
    version = _current_plugin_version()
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": version, "framework": "rails"})
    )
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["framework"] == "rails"


def test_happy_path_python_framework(tmp_path):
    """init-check.py returns python framework correctly."""
    version = _current_plugin_version()
    (tmp_path / ".flow.json").write_text(
        json.dumps({"flow_version": version, "framework": "python"})
    )
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["framework"] == "python"


# --- Auto-upgrade on version mismatch with matching config_hash ---


def test_auto_upgrades_when_config_hash_matches(tmp_path):
    """Version mismatch + matching hash → ok + auto_upgraded."""
    config_hash = _computed_config_hash("rails")
    (tmp_path / ".flow.json").write_text(json.dumps({
        "flow_version": "0.0.1",
        "framework": "rails",
        "config_hash": config_hash,
    }))
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["auto_upgraded"] is True
    assert data["framework"] == "rails"


def test_auto_upgrade_updates_version_in_file(tmp_path):
    """Auto-upgrade rewrites flow_version in .flow.json."""
    config_hash = _computed_config_hash("python")
    (tmp_path / ".flow.json").write_text(json.dumps({
        "flow_version": "0.0.1",
        "framework": "python",
        "config_hash": config_hash,
    }))
    _run(tmp_path)
    updated = json.loads((tmp_path / ".flow.json").read_text())
    assert updated["flow_version"] == _current_plugin_version()


def test_auto_upgrade_preserves_existing_fields(tmp_path):
    """Auto-upgrade preserves framework, skills, and config_hash."""
    config_hash = _computed_config_hash("rails")
    skills = {"flow-start": {"continue": "auto"}}
    (tmp_path / ".flow.json").write_text(json.dumps({
        "flow_version": "0.0.1",
        "framework": "rails",
        "config_hash": config_hash,
        "skills": skills,
    }))
    _run(tmp_path)
    updated = json.loads((tmp_path / ".flow.json").read_text())
    assert updated["framework"] == "rails"
    assert updated["config_hash"] == config_hash
    assert updated["skills"] == skills


def test_requires_reinit_when_config_hash_missing(tmp_path):
    """Version mismatch + no config_hash → error (backward compat)."""
    (tmp_path / ".flow.json").write_text(json.dumps({
        "flow_version": "0.0.1",
        "framework": "rails",
    }))
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "mismatch" in data["message"]


def test_requires_reinit_when_config_hash_mismatches(tmp_path):
    """Version mismatch + different hash → error (config changed)."""
    (tmp_path / ".flow.json").write_text(json.dumps({
        "flow_version": "0.0.1",
        "framework": "rails",
        "config_hash": "000000000000",
    }))
    result = _run(tmp_path)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "/flow:flow-init" in data["message"]
