"""Tests for bin/test — the pytest wrapper script."""

import os
import subprocess
import sys

import pytest

from conftest import BIN_DIR


@pytest.fixture
def test_project(tmp_path):
    """Create a minimal project layout that bin/test can run against."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "test").write_text((BIN_DIR / "test").read_text())
    (bin_dir / "test").chmod(0o755)
    (tmp_path / "tests").mkdir()
    venv_bin = tmp_path / ".venv" / "bin"
    venv_bin.mkdir(parents=True)
    wrapper = venv_bin / "python3"
    wrapper.write_text(f"#!/usr/bin/env bash\nexec {sys.executable} \"$@\"\n")
    wrapper.chmod(0o755)
    return tmp_path


def _run(project_dir, *args, extra_env=None):
    """Run bin/test inside the given project directory."""
    env = {k: v for k, v in os.environ.items() if k != "COVERAGE_PROCESS_START"}
    if extra_env:
        env.update(extra_env)
    result = subprocess.run(
        ["bash", str(project_dir / "bin" / "test"), *args],
        capture_output=True, text=True, cwd=str(project_dir), env=env,
    )
    return result


def test_exits_0_when_pytest_passes(test_project):
    (test_project / "tests" / "test_pass.py").write_text(
        "def test_ok(): assert True\n"
    )
    result = _run(test_project, "tests/")
    assert result.returncode == 0


def test_exits_nonzero_when_pytest_fails(test_project):
    (test_project / "tests" / "test_fail.py").write_text(
        "def test_bad(): assert False\n"
    )
    result = _run(test_project, "tests/")
    assert result.returncode != 0


def test_passes_arguments_through(test_project):
    (test_project / "tests" / "test_pass.py").write_text(
        "def test_ok(): assert True\ndef test_also(): assert True\n"
    )
    result = _run(test_project, "tests/", "-v")
    assert result.returncode == 0
    assert "test_ok" in result.stdout
    assert "test_also" in result.stdout