"""Tests for lib/create-dependencies.py — copy framework dependency template."""

import importlib.util
import json
import os
import stat
import sys

import pytest

from conftest import FRAMEWORKS_DIR, LIB_DIR

_spec = importlib.util.spec_from_file_location(
    "create_dependencies", LIB_DIR / "create-dependencies.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def test_creates_bin_dependencies_from_template(tmp_path):
    result = _mod.create(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    assert result["status"] == "ok"
    dependencies = tmp_path / "bin" / "dependencies"
    assert dependencies.exists()
    content = dependencies.read_text()
    assert "bundle update --all" in content


def test_created_file_is_executable(tmp_path):
    _mod.create(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    dependencies = tmp_path / "bin" / "dependencies"
    file_stat = dependencies.stat()
    assert file_stat.st_mode & stat.S_IXUSR


def test_skips_if_bin_dependencies_already_exists(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    existing = bin_dir / "dependencies"
    existing.write_text("#!/usr/bin/env bash\n# custom\n")
    result = _mod.create(str(tmp_path), "rails", str(FRAMEWORKS_DIR))
    assert result["status"] == "skipped"
    assert existing.read_text() == "#!/usr/bin/env bash\n# custom\n"


def test_creates_bin_directory_if_missing(tmp_path):
    _mod.create(str(tmp_path), "python", str(FRAMEWORKS_DIR))
    assert (tmp_path / "bin" / "dependencies").exists()


def test_error_when_invalid_framework(tmp_path):
    result = _mod.create(str(tmp_path), "nonexistent", str(FRAMEWORKS_DIR))
    assert result["status"] == "error"


def test_python_template_content(tmp_path):
    _mod.create(str(tmp_path), "python", str(FRAMEWORKS_DIR))
    content = (tmp_path / "bin" / "dependencies").read_text()
    assert ".venv/bin/pip" in content


def test_main_success(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(
        sys, "argv",
        ["create-dependencies", str(tmp_path), "--framework", "rails"],
    )
    _mod.main()
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


def test_main_missing_args(capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["create-dependencies"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_main_missing_framework(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["create-dependencies", str(tmp_path)])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_main_error_exits_with_1(tmp_path, capsys, monkeypatch):
    monkeypatch.setattr(
        sys, "argv",
        ["create-dependencies", str(tmp_path), "--framework", "nonexistent"],
    )
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
