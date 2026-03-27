"""Tests for lib/write-rule.py — write content to a target file path."""

import json
import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

import importlib

_mod = importlib.import_module("write-rule")


# --- read_content_file ---


def test_read_content_file_happy_path(tmp_path):
    """Reads file content and deletes the file."""
    content_file = tmp_path / "content.md"
    content_file.write_text("# My Rule\n\nDo the thing.\n")

    content, error = _mod.read_content_file(str(content_file))

    assert content == "# My Rule\n\nDo the thing.\n"
    assert error is None
    assert not content_file.exists()


def test_read_content_file_missing_file(tmp_path):
    """Returns error when file does not exist."""
    missing = tmp_path / "nonexistent.md"

    content, error = _mod.read_content_file(str(missing))

    assert content is None
    assert "Could not read content file" in error


def test_read_content_file_delete_fails_silently(tmp_path, monkeypatch):
    """Continues without error when file deletion fails after reading."""
    content_file = tmp_path / "content.md"
    content_file.write_text("rule text")

    original_remove = os.remove

    def failing_remove(path):
        if str(path) == str(content_file):
            raise OSError("permission denied")
        original_remove(path)

    monkeypatch.setattr("os.remove", failing_remove)

    content, error = _mod.read_content_file(str(content_file))

    assert content == "rule text"
    assert error is None


# --- write_rule ---


def test_write_rule_happy_path(tmp_path):
    """Writes content to the target path."""
    target = tmp_path / "rules" / "topic.md"
    target.parent.mkdir()

    ok, error = _mod.write_rule(str(target), "# Topic\n\nRule text.\n")

    assert ok is True
    assert error is None
    assert target.read_text() == "# Topic\n\nRule text.\n"


def test_write_rule_creates_parent_dirs(tmp_path):
    """Creates parent directories when they do not exist."""
    target = tmp_path / "deep" / "nested" / "dir" / "rule.md"

    ok, error = _mod.write_rule(str(target), "content")

    assert ok is True
    assert error is None
    assert target.read_text() == "content"


def test_write_rule_write_error(tmp_path):
    """Returns error when target path is unwritable."""
    target = tmp_path / "readonly" / "rule.md"
    target.parent.mkdir()
    target.parent.chmod(0o444)

    ok, error = _mod.write_rule(str(target), "content")

    assert ok is False
    assert "Could not write" in error

    # Restore permissions for cleanup
    target.parent.chmod(0o755)


def test_write_rule_makedirs_error(tmp_path, monkeypatch):
    """Returns error when directory creation fails."""
    target = tmp_path / "deep" / "rule.md"

    original_makedirs = os.makedirs

    def failing_makedirs(path, **kwargs):
        if "deep" in str(path):
            raise OSError("permission denied")
        original_makedirs(path, **kwargs)

    monkeypatch.setattr("os.makedirs", failing_makedirs)

    ok, error = _mod.write_rule(str(target), "content")

    assert ok is False
    assert "Could not create directories" in error


def test_write_rule_overwrites_existing(tmp_path):
    """Overwrites existing file content."""
    target = tmp_path / "rule.md"
    target.write_text("old content")

    ok, error = _mod.write_rule(str(target), "new content")

    assert ok is True
    assert error is None
    assert target.read_text() == "new content"


# --- CLI integration ---


def test_cli_happy_path(tmp_path, monkeypatch, capsys):
    """CLI writes content file to target and outputs JSON."""
    content_file = tmp_path / "content.md"
    content_file.write_text("# Rule\n\nDo it.\n")
    target = tmp_path / ".claude" / "rules" / "topic.md"

    monkeypatch.setattr("sys.argv", [
        "write-rule.py",
        "--path", str(target),
        "--content-file", str(content_file),
    ])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "ok"
    assert output["path"] == str(target)
    assert target.read_text() == "# Rule\n\nDo it.\n"
    assert not content_file.exists()


def test_cli_missing_content_file(tmp_path, monkeypatch, capsys):
    """CLI exits non-zero when content file does not exist."""
    target = tmp_path / "rule.md"

    monkeypatch.setattr("sys.argv", [
        "write-rule.py",
        "--path", str(target),
        "--content-file", str(tmp_path / "nonexistent.md"),
    ])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code == 1
    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"
    assert "Could not read content file" in output["message"]


def test_cli_write_error(tmp_path, monkeypatch, capsys):
    """CLI exits non-zero when target path is unwritable."""
    content_file = tmp_path / "content.md"
    content_file.write_text("content")
    readonly_dir = tmp_path / "readonly"
    readonly_dir.mkdir()
    readonly_dir.chmod(0o444)
    target = readonly_dir / "rule.md"

    monkeypatch.setattr("sys.argv", [
        "write-rule.py",
        "--path", str(target),
        "--content-file", str(content_file),
    ])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()

    assert exc_info.value.code == 1
    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "error"
    assert "Could not write" in output["message"]

    # Restore permissions for cleanup
    readonly_dir.chmod(0o755)
