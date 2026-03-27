"""Tests for bin/ci — the project CI runner."""

import os
import shutil
import subprocess
import sys

import pytest
from conftest import BIN_DIR, REPO_ROOT


@pytest.fixture
def ci_project(tmp_path):
    """Create a minimal project layout that bin/ci can run against.

    bin/ci computes REPO_ROOT from $(dirname "$0")/.., so placing it at
    <tmp>/bin/ci makes it run pytest against <tmp>/tests/.
    Includes a .venv/bin/python3 wrapper that delegates to the test-runner
    python so pytest is available.

    IMPORTANT: Uses a wrapper script, NOT a symlink. write_text() on a
    symlink follows it and overwrites the target — which would corrupt
    the real python binary.
    """
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "ci").write_text((BIN_DIR / "ci").read_text())
    (bin_dir / "ci").chmod(0o755)
    (tmp_path / "README.md").write_text("# Test\n")
    shutil.copy(REPO_ROOT / ".pymarkdown.yml", tmp_path / ".pymarkdown.yml")
    shutil.copy(REPO_ROOT / "ruff.toml", tmp_path / "ruff.toml")
    (tmp_path / "lib").mkdir()
    (tmp_path / "tests").mkdir()
    venv_bin = tmp_path / ".venv" / "bin"
    venv_bin.mkdir(parents=True)
    wrapper = venv_bin / "python3"
    wrapper.write_text(f'#!/usr/bin/env bash\nexec {sys.executable} "$@"\n')
    wrapper.chmod(0o755)
    return tmp_path


def _run(project_dir, extra_env=None):
    """Run bin/ci inside the given project directory."""
    env = {k: v for k, v in os.environ.items() if k != "COVERAGE_PROCESS_START"}
    if extra_env:
        env.update(extra_env)
    result = subprocess.run(
        ["bash", str(project_dir / "bin" / "ci")],
        capture_output=True,
        text=True,
        cwd=str(project_dir),
        env=env,
    )
    return result


def test_exits_0_when_pytest_passes(ci_project):
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")
    result = _run(ci_project)
    assert result.returncode == 0


def test_exits_nonzero_when_pytest_fails(ci_project):
    (ci_project / "tests" / "test_fail.py").write_text("def test_bad():\n    assert False\n")
    result = _run(ci_project)
    assert result.returncode != 0


def test_uses_venv_python_when_available(ci_project):
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")
    fake_python = ci_project / ".venv" / "bin" / "python3"
    fake_python.write_text("#!/usr/bin/env bash\necho VENV_MARKER\nexit 0\n")
    fake_python.chmod(0o755)
    result = _run(ci_project)
    assert "VENV_MARKER" in result.stdout


def test_runs_ruff_check_and_format(ci_project):
    """bin/ci runs ruff check and ruff format --check before pytest."""
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")
    fake_python = ci_project / ".venv" / "bin" / "python3"
    fake_python.write_text('#!/usr/bin/env bash\necho "RUFF_MARKER: $*"\nexit 0\n')
    fake_python.chmod(0o755)
    result = _run(ci_project)
    # Verify ruff check and ruff format are both invoked via $PYTHON -m
    assert "RUFF_MARKER: -m ruff check lib/ tests/" in result.stdout
    assert "RUFF_MARKER: -m ruff format --check lib/ tests/" in result.stdout


def test_falls_back_to_system_python_when_no_venv(ci_project):
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")
    shutil.rmtree(ci_project / ".venv")
    local_bin = ci_project / "local_bin"
    local_bin.mkdir()
    wrapper = local_bin / "python3"
    wrapper.write_text(f'#!/usr/bin/env bash\nexec {sys.executable} "$@"\n')
    wrapper.chmod(0o755)
    result = _run(ci_project, extra_env={"PATH": f"{local_bin}:{os.environ['PATH']}"})
    assert result.returncode == 0
