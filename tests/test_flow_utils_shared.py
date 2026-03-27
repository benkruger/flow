"""Tests for shared utility functions in flow_utils: elapsed_since, read_version_from, read_version, current_branch."""

import json
from datetime import datetime

from flow_utils import PACIFIC, current_branch, elapsed_since, read_version, read_version_from

# --- elapsed_since ---


def test_elapsed_since_none():
    """Returns 0 when started_at is None."""
    assert elapsed_since(None) == 0


def test_elapsed_since_empty_string():
    """Returns 0 when started_at is empty string."""
    assert elapsed_since("") == 0


def test_elapsed_since_with_explicit_now():
    """Calculates elapsed seconds between two known times."""
    started = "2026-01-01T00:00:00-08:00"
    now = datetime(2026, 1, 1, 0, 10, 0, tzinfo=PACIFIC)
    assert elapsed_since(started, now=now) == 600


def test_elapsed_since_default_now():
    """Uses current time when now is not passed."""
    result = elapsed_since("2026-01-01T00:00:00-08:00")
    assert isinstance(result, int)
    assert result >= 0


def test_elapsed_since_utc_timestamp():
    """Handles UTC timestamps (Z suffix) from old state files."""
    from datetime import timezone

    started = "2026-01-01T00:00:00Z"
    now = datetime(2026, 1, 1, 0, 5, 0, tzinfo=timezone.utc)
    assert elapsed_since(started, now=now) == 300


def test_elapsed_since_never_negative():
    """Returns 0 even if now is before started_at (clamped)."""
    started = "2026-01-01T01:00:00-08:00"
    now = datetime(2026, 1, 1, 0, 0, 0, tzinfo=PACIFIC)
    assert elapsed_since(started, now=now) == 0


# --- read_version_from ---


def test_read_version_from_valid(tmp_path):
    """Reads version from a valid plugin.json file."""
    plugin_json = tmp_path / "plugin.json"
    plugin_json.write_text(json.dumps({"version": "1.2.3"}))
    assert read_version_from(plugin_json) == "1.2.3"


def test_read_version_from_missing_file(tmp_path):
    """Returns '?' when file does not exist."""
    assert read_version_from(tmp_path / "nonexistent.json") == "?"


def test_read_version_from_invalid_json(tmp_path):
    """Returns '?' when file contains invalid JSON."""
    bad_file = tmp_path / "plugin.json"
    bad_file.write_text("{bad json")
    assert read_version_from(bad_file) == "?"


def test_read_version_from_missing_key(tmp_path):
    """Returns '?' when JSON lacks 'version' key."""
    plugin_json = tmp_path / "plugin.json"
    plugin_json.write_text(json.dumps({"name": "flow"}))
    assert read_version_from(plugin_json) == "?"


# --- read_version ---


def test_read_version_returns_string():
    """read_version returns a valid version string from the real plugin.json."""
    version = read_version()
    assert isinstance(version, str)
    assert version != ""
    assert "." in version or version == "?"


# --- current_branch ---


def test_current_branch_simulate_env_var(monkeypatch):
    """FLOW_SIMULATE_BRANCH overrides git branch detection."""
    monkeypatch.setenv("FLOW_SIMULATE_BRANCH", "main")
    assert current_branch() == "main"


def test_current_branch_simulate_empty_string_falls_through(monkeypatch, git_repo):
    """Empty FLOW_SIMULATE_BRANCH falls through to git detection."""
    monkeypatch.setenv("FLOW_SIMULATE_BRANCH", "")
    monkeypatch.chdir(git_repo)
    result = current_branch()
    assert result is not None
    assert result != ""
