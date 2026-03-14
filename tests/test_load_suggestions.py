"""Tests for lib/load-suggestions.py — framework permission suggestions."""

import importlib.util
import json
import sys

import pytest

from conftest import FRAMEWORKS_DIR, LIB_DIR

_spec = importlib.util.spec_from_file_location(
    "load_suggestions", LIB_DIR / "load-suggestions.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def test_ios_has_suggestions():
    result = _mod.load("ios", str(FRAMEWORKS_DIR))
    assert len(result) > 0


def test_rails_no_suggestions():
    result = _mod.load("rails", str(FRAMEWORKS_DIR))
    assert result == []


def test_python_no_suggestions():
    result = _mod.load("python", str(FRAMEWORKS_DIR))
    assert result == []


def test_unknown_framework_returns_empty():
    result = _mod.load("nonexistent", str(FRAMEWORKS_DIR))
    assert result == []


def test_suggestion_has_label_and_template():
    result = _mod.load("ios", str(FRAMEWORKS_DIR))
    for suggestion in result:
        assert "label" in suggestion
        assert "template" in suggestion


def test_ios_suggestion_template_has_placeholder():
    result = _mod.load("ios", str(FRAMEWORKS_DIR))
    templates = [s["template"] for s in result]
    assert any("{value}" in t for t in templates)


def test_cli_output_valid_json(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["load-suggestions", "ios"])
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert isinstance(data["suggestions"], list)


def test_cli_missing_args(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["load-suggestions"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_cli_unknown_framework_returns_empty(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["load-suggestions", "nonexistent"])
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["suggestions"] == []
