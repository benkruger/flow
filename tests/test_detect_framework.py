"""Tests for lib/detect-framework.py — data-driven framework auto-detection."""

import importlib.util
import json
import sys

import pytest
from conftest import FRAMEWORKS_DIR, LIB_DIR

_spec = importlib.util.spec_from_file_location("detect_framework", LIB_DIR / "detect-framework.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


@pytest.fixture
def project(tmp_path):
    """Create a temporary project directory."""
    return tmp_path / "project"


def test_detects_rails_when_gemfile_exists(project):
    project.mkdir()
    (project / "Gemfile").write_text("source 'https://rubygems.org'\n")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "rails" in names


def test_detects_python_when_pyproject_exists(project):
    project.mkdir()
    (project / "pyproject.toml").write_text("[project]\n")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "python" in names


def test_detects_python_when_requirements_txt_exists(project):
    project.mkdir()
    (project / "requirements.txt").write_text("flask\n")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "python" in names


def test_detects_both_when_gemfile_and_pyproject_exist(project):
    project.mkdir()
    (project / "Gemfile").write_text("")
    (project / "pyproject.toml").write_text("")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "rails" in names
    assert "python" in names


def test_detects_ios_when_xcodeproj_exists(project):
    project.mkdir()
    (project / "MyApp.xcodeproj").mkdir()
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "ios" in names


def test_detects_ios_with_glob_pattern(project):
    project.mkdir()
    (project / "AnotherApp.xcodeproj").mkdir()
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    ios = [f for f in result if f["name"] == "ios"]
    assert len(ios) == 1
    assert ios[0]["display_name"] == "iOS"


def test_detects_go_when_go_mod_exists(project):
    project.mkdir()
    (project / "go.mod").write_text("module example.com/myapp\n\ngo 1.21\n")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    names = [f["name"] for f in result]
    assert "go" in names


def test_detects_go_display_name(project):
    project.mkdir()
    (project / "go.mod").write_text("module example.com/myapp\n\ngo 1.21\n")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    go = [f for f in result if f["name"] == "go"]
    assert len(go) == 1
    assert go[0]["display_name"] == "Go"


def test_detects_nothing_when_no_marker_files(project):
    project.mkdir()
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    assert result == []


def test_result_includes_display_name(project):
    project.mkdir()
    (project / "Gemfile").write_text("")
    result = _mod.detect(str(project), str(FRAMEWORKS_DIR))
    rails = [f for f in result if f["name"] == "rails"][0]
    assert rails["display_name"] == "Rails"


def test_lists_available_frameworks(project):
    project.mkdir()
    available = _mod.available_frameworks(str(FRAMEWORKS_DIR))
    names = [f["name"] for f in available]
    assert "rails" in names
    assert "python" in names
    assert "ios" in names
    assert "go" in names


def test_frameworks_dir_returns_valid_path():
    result = _mod._frameworks_dir()
    assert result.is_dir()
    assert result.name == "frameworks"


def test_detect_with_default_frameworks_dir(project):
    project.mkdir()
    (project / "Gemfile").write_text("")
    result = _mod.detect(str(project))
    names = [f["name"] for f in result]
    assert "rails" in names


def test_available_frameworks_with_default_dir():
    available = _mod.available_frameworks()
    names = [f["name"] for f in available]
    assert "rails" in names
    assert "python" in names
    assert "ios" in names
    assert "go" in names


def test_main_detects_framework(tmp_path, capsys, monkeypatch):
    project = tmp_path / "project"
    project.mkdir()
    (project / "Gemfile").write_text("")
    monkeypatch.setattr(sys, "argv", ["detect-framework", str(project)])
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    names = [f["name"] for f in data["detected"]]
    assert "rails" in names


def test_main_missing_args(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["detect-framework"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_main_invalid_project_root(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["detect-framework", "/nonexistent/path"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
