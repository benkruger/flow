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


def test_runs_cargo_test_when_cargo_toml_exists(ci_project, tmp_path):
    """bin/ci runs cargo test when Cargo.toml exists in the project root."""
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")
    (ci_project / "Cargo.toml").write_text('[package]\nname = "test"\nversion = "0.1.0"\n')

    mock_bin = tmp_path / "mock_bin"
    mock_bin.mkdir()
    cargo = mock_bin / "cargo"
    cargo.write_text('#!/usr/bin/env bash\necho "CARGO_TEST_MARKER: $*"\nexit 0\n')
    cargo.chmod(0o755)

    result = _run(ci_project, extra_env={"PATH": f"{mock_bin}:{os.environ['PATH']}"})
    assert result.returncode == 0
    assert "CARGO_TEST_MARKER: test" in result.stdout


def test_skips_cargo_when_no_cargo_toml(ci_project, tmp_path):
    """bin/ci does not run cargo when no Cargo.toml exists."""
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")

    mock_bin = tmp_path / "mock_bin"
    mock_bin.mkdir()
    cargo = mock_bin / "cargo"
    cargo.write_text('#!/usr/bin/env bash\necho "CARGO_SHOULD_NOT_RUN"\nexit 1\n')
    cargo.chmod(0o755)

    result = _run(ci_project, extra_env={"PATH": f"{mock_bin}:{os.environ['PATH']}"})
    assert result.returncode == 0
    assert "CARGO_SHOULD_NOT_RUN" not in result.stdout


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


def test_cleans_stale_pycache_before_pytest(ci_project):
    """bin/ci removes __pycache__ dirs (excl. .venv) before running pytest."""
    # Create a stale .pyc in lib/__pycache__/ (no corresponding .py)
    pycache = ci_project / "lib" / "__pycache__"
    pycache.mkdir()
    stale_pyc = pycache / "deleted_module.cpython-314.pyc"
    stale_pyc.write_bytes(b"\x00")

    # Also create a __pycache__ in tests/ to confirm it's cleaned too
    test_pycache = ci_project / "tests" / "__pycache__"
    test_pycache.mkdir()
    stale_test_pyc = test_pycache / "test_deleted.cpython-314-pytest-9.0.2.pyc"
    stale_test_pyc.write_bytes(b"\x00")

    # Write a test that passes
    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")

    result = _run(ci_project)
    assert result.returncode == 0

    # Stale .pyc files should be gone after bin/ci ran.
    # pytest may recreate tests/__pycache__/ during collection, so check
    # the stale FILE rather than the directory.
    assert not stale_pyc.exists(), "stale lib .pyc should be cleaned by bin/ci"
    assert not stale_test_pyc.exists(), "stale test .pyc should be cleaned by bin/ci"


def test_pycache_cleanup_preserves_venv(ci_project):
    """bin/ci must NOT clean __pycache__ inside .venv/."""
    venv_pycache = ci_project / ".venv" / "lib" / "python3" / "__pycache__"
    venv_pycache.mkdir(parents=True)
    venv_marker = venv_pycache / "venv_module.cpython-314.pyc"
    venv_marker.write_bytes(b"\x00")

    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")

    result = _run(ci_project)
    assert result.returncode == 0
    assert venv_marker.exists(), ".venv __pycache__ must be preserved"


def test_pycache_cleanup_preserves_venv_from_subdirectory(ci_project):
    """bin/ci preserves .venv/__pycache__ even when invoked from a subdirectory.

    The find command must use $REPO_ROOT (absolute) rather than '.' (CWD-
    relative), otherwise running bin/ci from a subdirectory would cause
    the prune to miss the project-root .venv and rm its __pycache__.
    """
    venv_pycache = ci_project / ".venv" / "lib" / "python3" / "__pycache__"
    venv_pycache.mkdir(parents=True)
    venv_marker = venv_pycache / "venv_pkg.cpython-314.pyc"
    venv_marker.write_bytes(b"\x00")

    (ci_project / "tests" / "test_pass.py").write_text("def test_ok():\n    assert True\n")

    # Invoke bin/ci with cwd set to the lib subdirectory — relative '.' in
    # the find command would resolve to ci_project/lib, not ci_project.
    env = {k: v for k, v in os.environ.items() if k != "COVERAGE_PROCESS_START"}
    result = subprocess.run(
        ["bash", str(ci_project / "bin" / "ci")],
        capture_output=True,
        text=True,
        cwd=str(ci_project / "lib"),
        env=env,
    )
    # bin/ci may fail for other reasons from a subdirectory (pymarkdown/ruff
    # use relative paths too). The assertion here is specifically that the
    # .venv __pycache__ survives whatever bin/ci does — the find must never
    # delete it regardless of CWD.
    assert venv_marker.exists(), (
        ".venv __pycache__ was deleted when bin/ci ran from a subdirectory. "
        "find must use absolute $REPO_ROOT for both the search root and "
        f"the prune path. bin/ci stderr: {result.stderr}"
    )
