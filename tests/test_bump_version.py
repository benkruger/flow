"""Tests for lib/bump-version.py."""

import importlib.util
import json
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR

# Import the hyphenated module directly
_spec = importlib.util.spec_from_file_location(
    "bump_version", LIB_DIR / "bump-version.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
validate_version = _mod.validate_version
read_current_version = _mod.read_current_version
bump_json = _mod.bump_json
bump_skill = _mod.bump_skill


@pytest.fixture
def fake_repo(tmp_path):
    """Create a minimal repo structure with plugin.json, marketplace.json, and skills."""
    plugin_dir = tmp_path / ".claude-plugin"
    plugin_dir.mkdir()

    plugin_json = plugin_dir / "plugin.json"
    plugin_json.write_text(json.dumps({
        "name": "flow",
        "version": "1.0.0",
    }, indent=2))

    marketplace_json = plugin_dir / "marketplace.json"
    marketplace_json.write_text(json.dumps({
        "name": "flow-marketplace",
        "metadata": {"version": "1.0.0"},
        "plugins": [{"name": "flow", "version": "1.0.0"}],
    }, indent=2))

    skills_dir = tmp_path / "skills"
    for name in ("flow-start", "flow-code"):
        skill_dir = skills_dir / name
        skill_dir.mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text(
            "# Skill\n\n"
            "```\n"
            "  FLOW v1.0.0 — Phase — STARTING\n"
            "```\n\n"
            "```\n"
            "  FLOW v1.0.0 — Phase — COMPLETE\n"
            "```\n"
        )

    release_dir = tmp_path / ".claude" / "skills" / "flow-release"
    release_dir.mkdir(parents=True)
    (release_dir / "SKILL.md").write_text(
        "# Release\n\n"
        "```\n"
        "  FLOW v1.0.0 — release — STARTING\n"
        "```\n"
    )

    return tmp_path


# --- validate_version ---


def test_validate_version_valid():
    assert validate_version("1.2.3")
    assert validate_version("0.0.0")
    assert validate_version("10.20.30")


def test_validate_version_invalid():
    assert not validate_version("v1.2.3")
    assert not validate_version("1.2")
    assert not validate_version("abc")
    assert not validate_version("../../etc/passwd")


# --- read_current_version ---


def test_read_current_version(tmp_path):
    p = tmp_path / "plugin.json"
    p.write_text(json.dumps({"version": "2.5.0"}))
    assert read_current_version(p) == "2.5.0"


# --- bump_json ---


def test_bump_json_updates_version(tmp_path):
    p = tmp_path / "test.json"
    p.write_text(json.dumps({"version": "1.0.0"}, indent=2))
    assert bump_json(p, "1.0.0", "2.0.0")
    data = json.loads(p.read_text())
    assert data["version"] == "2.0.0"


def test_bump_json_no_match(tmp_path):
    p = tmp_path / "test.json"
    p.write_text(json.dumps({"version": "3.0.0"}, indent=2))
    assert not bump_json(p, "1.0.0", "2.0.0")


def test_bump_json_multiple_version_fields(fake_repo):
    """marketplace.json has two version fields — both should update."""
    mp = fake_repo / ".claude-plugin" / "marketplace.json"
    assert bump_json(mp, "1.0.0", "2.0.0")
    data = json.loads(mp.read_text())
    assert data["metadata"]["version"] == "2.0.0"
    assert data["plugins"][0]["version"] == "2.0.0"


# --- bump_skill ---


def test_bump_skill_replaces_banners(tmp_path):
    p = tmp_path / "SKILL.md"
    p.write_text("  FLOW v1.0.0 — Start\n  FLOW v1.0.0 — End\n")
    assert bump_skill(p, "1.0.0", "2.0.0")
    text = p.read_text()
    assert "FLOW v2.0.0" in text
    assert "FLOW v1.0.0" not in text


def test_bump_skill_no_match(tmp_path):
    p = tmp_path / "SKILL.md"
    p.write_text("No version here\n")
    assert not bump_skill(p, "1.0.0", "2.0.0")


# --- CLI integration tests ---


def test_cli_successful_bump(fake_repo, monkeypatch):
    """Full bump updates all JSON fields and skill banners."""
    monkeypatch.setattr(_mod, "__file__", str(fake_repo / "hooks" / "bump-version.py"))
    monkeypatch.setattr(sys, "argv", ["bump-version.py", "2.0.0"])

    _mod.main()

    # Check plugin.json
    data = json.loads((fake_repo / ".claude-plugin" / "plugin.json").read_text())
    assert data["version"] == "2.0.0"

    # Check marketplace.json
    data = json.loads((fake_repo / ".claude-plugin" / "marketplace.json").read_text())
    assert data["metadata"]["version"] == "2.0.0"
    assert data["plugins"][0]["version"] == "2.0.0"

    # Check skill banners
    for skill_file in (fake_repo / "skills").glob("*/SKILL.md"):
        text = skill_file.read_text()
        assert "FLOW v2.0.0" in text
        assert "FLOW v1.0.0" not in text

    # Check release skill
    text = (fake_repo / ".claude" / "skills" / "flow-release" / "SKILL.md").read_text()
    assert "FLOW v2.0.0" in text
    assert "FLOW v1.0.0" not in text


def test_cli_no_arguments_exits_1(monkeypatch, capsys):
    """Running with no arguments should exit 1 with usage message."""
    monkeypatch.setattr("sys.argv", ["bump-version.py"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "Usage" in capsys.readouterr().out


def test_cli_invalid_version_exits_1(monkeypatch, capsys):
    """Running with a non-semver version should exit 1."""
    monkeypatch.setattr("sys.argv", ["bump-version.py", "v1.0.0"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "invalid version format" in capsys.readouterr().out


def test_cli_same_version_exits_1(fake_repo, monkeypatch):
    """Bumping to the current version should exit 1."""
    monkeypatch.setattr(_mod, "__file__", str(fake_repo / "hooks" / "bump-version.py"))
    monkeypatch.setattr(sys, "argv", ["bump-version.py", "1.0.0"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1


def test_cli_plugin_json_not_found(monkeypatch, tmp_path, capsys):
    """main() exits 1 when plugin.json doesn't exist."""
    monkeypatch.setattr(_mod, "__file__", str(tmp_path / "hooks" / "bump-version.py"))
    monkeypatch.setattr(sys, "argv", ["bump-version.py", "2.0.0"])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    assert "not found" in capsys.readouterr().out
