"""Tests for lib/prime-project.py — FLOW priming marker in project CLAUDE.md."""

import importlib.util
import json
import sys

import pytest
from conftest import FRAMEWORKS_DIR, LIB_DIR

_spec = importlib.util.spec_from_file_location("prime_project", LIB_DIR / "prime-project.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

MARKER_BEGIN = "<!-- FLOW:BEGIN -->"
MARKER_END = "<!-- FLOW:END -->"


def test_inserts_priming_content_into_existing_claude_md(tmp_path):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n\nExisting content.\n")
    result = _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    assert result["status"] == "ok"
    content = claude_md.read_text()
    assert MARKER_BEGIN in content
    assert MARKER_END in content
    assert "Existing content." in content
    assert "Architecture Patterns" in content


def test_idempotent_replacement(tmp_path):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n\nExisting content.\n")
    _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    first_content = claude_md.read_text()
    _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    second_content = claude_md.read_text()
    assert first_content == second_content


def test_replaces_content_when_switching_framework(tmp_path):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n")
    _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    assert "Rails Conventions" in claude_md.read_text()
    _mod.prime(str(tmp_path), "python", str(FRAMEWORKS_DIR))
    content = claude_md.read_text()
    assert "Python Conventions" in content
    assert "Rails Conventions" not in content


def test_error_when_no_claude_md(tmp_path):
    result = _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    assert result["status"] == "error"
    assert "CLAUDE.md" in result["message"]


def test_error_when_invalid_framework(tmp_path):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n")
    result = _mod.prime(str(tmp_path), "nonexistent", str(FRAMEWORKS_DIR))
    assert result["status"] == "error"


def test_preserves_content_before_and_after_markers(tmp_path):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n\nBefore.\n\nAfter marker stuff.\n")
    _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    content = claude_md.read_text()
    assert content.startswith("# My Project\n\nBefore.\n\nAfter marker stuff.\n")
    assert MARKER_BEGIN in content
    assert MARKER_END in content


def test_blank_line_after_begin_marker(tmp_path):
    """FLOW:BEGIN marker must be followed by a blank line (MD022 compliance)."""
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n")
    _mod.prime(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    content = claude_md.read_text()
    assert f"{MARKER_BEGIN}\n\n" in content, "Expected blank line after <!-- FLOW:BEGIN --> for MD022 compliance"


def test_main_success(tmp_path, capsys, monkeypatch):
    claude_md = tmp_path / "CLAUDE.md"
    claude_md.write_text("# My Project\n")
    monkeypatch.setattr(
        sys,
        "argv",
        ["prime-project", str(tmp_path), "--framework", "rails"],
    )
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


def test_main_missing_args(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["prime-project"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_main_missing_framework(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["prime-project", str(tmp_path)])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_main_prime_error_exits_with_1(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(
        sys,
        "argv",
        ["prime-project", str(tmp_path), "--framework", "rails"],
    )
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
