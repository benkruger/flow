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
    wrapper.write_text(f'#!/usr/bin/env bash\nexec {sys.executable} "$@"\n')
    wrapper.chmod(0o755)
    return tmp_path


def _run(project_dir, *args, extra_env=None):
    """Run bin/test inside the given project directory."""
    env = {k: v for k, v in os.environ.items() if k != "COVERAGE_PROCESS_START"}
    if extra_env:
        env.update(extra_env)
    result = subprocess.run(
        ["bash", str(project_dir / "bin" / "test"), *args],
        capture_output=True,
        text=True,
        cwd=str(project_dir),
        env=env,
    )
    return result


def test_exits_0_when_pytest_passes(test_project):
    (test_project / "tests" / "test_pass.py").write_text("def test_ok(): assert True\n")
    result = _run(test_project, "tests/")
    assert result.returncode == 0


def test_exits_nonzero_when_pytest_fails(test_project):
    (test_project / "tests" / "test_fail.py").write_text("def test_bad(): assert False\n")
    result = _run(test_project, "tests/")
    assert result.returncode != 0


def test_passes_arguments_through(test_project):
    (test_project / "tests" / "test_pass.py").write_text("def test_ok(): assert True\ndef test_also(): assert True\n")
    result = _run(test_project, "tests/", "-v")
    assert result.returncode == 0
    assert "test_ok" in result.stdout
    assert "test_also" in result.stdout


def test_rust_flag_runs_cargo_test(test_project, tmp_path):
    """bin/test --rust runs cargo test instead of pytest."""
    mock_bin = tmp_path / "mock_bin"
    mock_bin.mkdir()
    cargo = mock_bin / "cargo"
    cargo.write_text('#!/usr/bin/env bash\necho "CARGO_RUST_MARKER: $*"\nexit 0\n')
    cargo.chmod(0o755)

    result = _run(test_project, "--rust", extra_env={"PATH": f"{mock_bin}:{os.environ['PATH']}"})
    assert result.returncode == 0
    assert "CARGO_RUST_MARKER: test" in result.stdout


def test_rust_flag_passes_extra_args(test_project, tmp_path):
    """bin/test --rust passes remaining args to cargo test."""
    mock_bin = tmp_path / "mock_bin"
    mock_bin.mkdir()
    cargo = mock_bin / "cargo"
    cargo.write_text('#!/usr/bin/env bash\necho "CARGO_ARGS: $*"\nexit 0\n')
    cargo.chmod(0o755)

    env = {"PATH": f"{mock_bin}:{os.environ['PATH']}"}
    result = _run(test_project, "--rust", "my_test_name", "--", "--nocapture", extra_env=env)
    assert result.returncode == 0
    assert "CARGO_ARGS: test my_test_name -- --nocapture" in result.stdout


def test_passes_no_cov_flag(test_project):
    """bin/test must always pass --no-cov so coverage is skipped."""
    (test_project / "tests" / "test_pass.py").write_text("def test_ok(): assert True\n")
    _run(test_project, "tests/", "-v", "--collect-only")
    assert "--no-cov" in (test_project / "bin" / "test").read_text()
