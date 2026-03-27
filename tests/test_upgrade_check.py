"""Tests for lib/upgrade-check.py — GitHub release version check."""

import json
import os
import stat
import subprocess
import sys
from pathlib import Path

from conftest import LIB_DIR

SCRIPT = str(LIB_DIR / "upgrade-check.py")


def _current_plugin_version():
    """Read the current version from plugin.json."""
    plugin_path = Path(__file__).resolve().parent.parent / ".claude-plugin" / "plugin.json"
    return json.loads(plugin_path.read_text())["version"]


def _make_fake_gh(tmp_path, stdout="", exit_code=0):
    """Create a fake gh script that echoes controlled output."""
    fake_gh = tmp_path / "gh"
    fake_gh.write_text(f"#!/usr/bin/env bash\necho '{stdout}'\nexit {exit_code}\n")
    fake_gh.chmod(fake_gh.stat().st_mode | stat.S_IEXEC)
    return str(tmp_path)


def _run(gh_dir=None, extra_env=None):
    """Run upgrade-check.py with an optional fake gh on PATH."""
    env = os.environ.copy()
    if gh_dir is not None:
        env["PATH"] = gh_dir + os.pathsep + env.get("PATH", "")
    if extra_env:
        env.update(extra_env)
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True,
        text=True,
        env=env,
    )
    return result


def test_current_version(tmp_path):
    """Returns 'current' when installed version matches latest release."""
    version = _current_plugin_version()
    gh_dir = _make_fake_gh(tmp_path, stdout=f"v{version}")
    result = _run(gh_dir=gh_dir)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "current"
    assert data["installed"] == version


def test_upgrade_available(tmp_path):
    """Returns 'upgrade_available' when a newer version exists."""
    version = _current_plugin_version()
    parts = version.split(".")
    newer = f"{parts[0]}.{int(parts[1]) + 1}.{parts[2]}"
    gh_dir = _make_fake_gh(tmp_path, stdout=f"v{newer}")
    result = _run(gh_dir=gh_dir)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "upgrade_available"
    assert data["installed"] == version
    assert data["latest"] == newer


def test_gh_not_found(tmp_path):
    """Returns 'unknown' when gh CLI is not on PATH."""
    env = os.environ.copy()
    env["PATH"] = str(tmp_path)
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "not found" in data["reason"]


def test_network_failure(tmp_path):
    """Returns 'unknown' when gh returns non-zero exit code."""
    gh_dir = _make_fake_gh(tmp_path, stdout="", exit_code=1)
    result = _run(gh_dir=gh_dir)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "failed" in data["reason"]


def test_no_releases(tmp_path):
    """Returns 'unknown' when gh returns empty output."""
    gh_dir = _make_fake_gh(tmp_path, stdout="")
    result = _run(gh_dir=gh_dir)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "No releases" in data["reason"]


def test_malformed_tag(tmp_path):
    """Returns 'unknown' when gh returns unparseable version."""
    gh_dir = _make_fake_gh(tmp_path, stdout="not-a-version")
    result = _run(gh_dir=gh_dir)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "parse" in data["reason"].lower()


def test_no_repository_url(tmp_path):
    """Returns 'unknown' when plugin.json has no GitHub repository URL."""
    fake_plugin = tmp_path / "plugin.json"
    fake_plugin.write_text(json.dumps({"version": "1.0.0"}))
    result = _run(extra_env={"FLOW_PLUGIN_JSON": str(fake_plugin)})
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "repository" in data["reason"].lower()


def test_timeout(tmp_path):
    """Returns 'unknown' when gh takes too long."""
    fake_gh = tmp_path / "gh"
    fake_gh.write_text("#!/usr/bin/env bash\nsleep 10\necho 'v1.0.0'\n")
    fake_gh.chmod(fake_gh.stat().st_mode | stat.S_IEXEC)
    result = _run(
        gh_dir=str(tmp_path),
        extra_env={"FLOW_UPGRADE_TIMEOUT": "1"},
    )
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "unknown"
    assert "timed out" in data["reason"]
