"""Tests for bin/dependencies — the framework dependency updater."""

import os
import shutil
import subprocess

import pytest

from conftest import BIN_DIR


@pytest.fixture
def dep_project(tmp_path):
    """Create a minimal project layout that bin/dependencies can run against.

    bin/dependencies computes REPO_ROOT from $(dirname "$0")/.., so placing it at
    <tmp>/bin/dependencies makes it look for .venv at <tmp>/.venv/.
    Includes a .venv/bin/pip wrapper that echoes a marker and exits.
    """
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "dependencies").write_text((BIN_DIR / "dependencies").read_text())
    (bin_dir / "dependencies").chmod(0o755)
    (tmp_path / "requirements.txt").write_text("# test requirements\n")
    venv_bin = tmp_path / ".venv" / "bin"
    venv_bin.mkdir(parents=True)
    fake_pip = venv_bin / "pip"
    fake_pip.write_text("#!/usr/bin/env bash\necho VENV_MARKER\n")
    fake_pip.chmod(0o755)
    return tmp_path


def _run_dep(project_dir, extra_env=None):
    """Run bin/dependencies inside the given project directory."""
    env = {k: v for k, v in os.environ.items()}
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        ["bash", str(project_dir / "bin" / "dependencies")],
        capture_output=True, text=True, cwd=str(project_dir), env=env,
    )


def test_script_is_executable():
    dep = BIN_DIR / "dependencies"
    assert dep.exists(), "bin/dependencies must exist"
    assert os.access(dep, os.X_OK), "bin/dependencies must be executable"


def test_script_is_valid_bash(tmp_path):
    dep = BIN_DIR / "dependencies"
    result = subprocess.run(
        ["bash", "-n", str(dep)],
        capture_output=True, text=True,
    )
    assert result.returncode == 0, f"Syntax error: {result.stderr}"


def test_skips_when_no_requirements_or_pyproject(tmp_path):
    """Running in a directory with neither requirements.txt nor pyproject.toml exits cleanly."""
    script = tmp_path / "dependencies"
    script.write_text((BIN_DIR / "dependencies").read_text())
    script.chmod(0o755)
    result = subprocess.run(
        ["bash", str(script)],
        capture_output=True, text=True, cwd=str(tmp_path),
    )
    assert result.returncode == 0


def test_uses_venv_pip_when_available(dep_project):
    result = _run_dep(dep_project)
    assert "VENV_MARKER" in result.stdout


def test_falls_back_to_system_pip_when_no_venv(dep_project):
    shutil.rmtree(dep_project / ".venv")
    local_bin = dep_project / "local_bin"
    local_bin.mkdir()
    fake_pip = local_bin / "pip"
    fake_pip.write_text("#!/usr/bin/env bash\necho SYSTEM_PIP_MARKER\n")
    fake_pip.chmod(0o755)
    result = _run_dep(dep_project, extra_env={"PATH": f"{local_bin}:{os.environ['PATH']}"})
    assert "SYSTEM_PIP_MARKER" in result.stdout
