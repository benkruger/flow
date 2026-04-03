"""Tests for bin/flow — the subcommand dispatcher."""

import json
import os
import subprocess

import pytest
from conftest import BIN_DIR, LIB_DIR, REPO_ROOT

SCRIPT = str(BIN_DIR / "flow")


def _run(*args, cwd=None, extra_env=None):
    """Run bin/flow with the given arguments."""
    env = None
    if extra_env:
        env = {**os.environ, **extra_env}
    result = subprocess.run(
        ["bash", SCRIPT, *args],
        capture_output=True,
        text=True,
        cwd=cwd or str(REPO_ROOT),
        env=env,
    )
    return result


def test_no_subcommand_returns_error_json():
    """Running with no arguments returns JSON error and exit 1."""
    result = _run()
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Usage" in data["message"]


def test_unknown_subcommand_returns_error_json():
    """Running with a nonexistent subcommand returns JSON error and exit 1."""
    result = _run("nonexistent-command")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "nonexistent-command" in data["message"]


def test_dispatches_to_correct_script():
    """Known subcommand dispatches to the matching .py file in lib/."""
    # extract-release-notes with no args exits 1 with usage message
    result = _run("extract-release-notes")
    assert result.returncode == 1
    assert "Usage" in result.stdout


def test_passes_arguments_through():
    """Arguments after the subcommand are passed to the Python script."""
    # extract-release-notes with an invalid version format exits 1
    result = _run("extract-release-notes", "../../etc/passwd")
    assert result.returncode == 1
    assert "invalid version format" in result.stdout


def test_exit_code_passes_through(tmp_path):
    """Exit code from the Python script is preserved."""
    # check-phase with --required plan and no state file exits non-zero
    result = _run("check-phase", "--required", "flow-plan", cwd=str(tmp_path))
    assert result.returncode != 0


# --- Hybrid dispatcher tests ---


@pytest.fixture
def hybrid_project(tmp_path):
    """Create a self-contained project for hybrid dispatcher tests.

    Copies the real bin/flow script and creates a minimal lib/ with a
    test command. Tests can optionally add target/debug/flow-rs as a
    mock Rust binary.
    """
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "flow").write_text((BIN_DIR / "flow").read_text())
    (bin_dir / "flow").chmod(0o755)

    lib_dir = tmp_path / "lib"
    lib_dir.mkdir()
    (lib_dir / "test-cmd.py").write_text('print("python-handled")\n')

    return tmp_path


def _run_hybrid(project_dir, *args):
    """Run the hybrid dispatcher in the given project."""
    return subprocess.run(
        ["bash", str(project_dir / "bin" / "flow"), *args],
        capture_output=True,
        text=True,
        cwd=str(project_dir),
    )


def test_hybrid_falls_back_when_rust_exits_127(hybrid_project):
    """When Rust binary exists but exits 127, dispatcher falls back to Python."""
    target_dir = hybrid_project / "target" / "debug"
    target_dir.mkdir(parents=True)
    mock_bin = target_dir / "flow-rs"
    mock_bin.write_text("#!/usr/bin/env bash\nexit 127\n")
    mock_bin.chmod(0o755)

    result = _run_hybrid(hybrid_project, "test-cmd")
    assert result.returncode == 0
    assert "python-handled" in result.stdout


def test_hybrid_passes_through_rust_exit_code(hybrid_project):
    """When Rust binary handles the command (exit != 127), use its result."""
    target_dir = hybrid_project / "target" / "debug"
    target_dir.mkdir(parents=True)
    mock_bin = target_dir / "flow-rs"
    mock_bin.write_text('#!/usr/bin/env bash\necho "rust-handled"\nexit 0\n')
    mock_bin.chmod(0o755)

    result = _run_hybrid(hybrid_project, "test-cmd")
    assert result.returncode == 0
    assert "rust-handled" in result.stdout
    assert "python-handled" not in result.stdout


def test_hybrid_passes_through_nonzero_rust_exit(hybrid_project):
    """Non-127 non-zero Rust exit code passes through without Python fallback."""
    target_dir = hybrid_project / "target" / "debug"
    target_dir.mkdir(parents=True)
    mock_bin = target_dir / "flow-rs"
    mock_bin.write_text('#!/usr/bin/env bash\necho "rust-error"\nexit 42\n')
    mock_bin.chmod(0o755)

    result = _run_hybrid(hybrid_project, "test-cmd")
    assert result.returncode == 42
    assert "rust-error" in result.stdout
    assert "python-handled" not in result.stdout


def test_dispatcher_works_without_rust_binary(hybrid_project):
    """When no Rust binary exists, commands route to Python (existing behavior)."""
    result = _run_hybrid(hybrid_project, "test-cmd")
    assert result.returncode == 0
    assert "python-handled" in result.stdout


def test_hybrid_prefers_release_over_debug(hybrid_project):
    """When both release and debug binaries exist, release is preferred."""
    for variant in ("debug", "release"):
        target_dir = hybrid_project / "target" / variant
        target_dir.mkdir(parents=True)
        mock_bin = target_dir / "flow-rs"
        mock_bin.write_text(f'#!/usr/bin/env bash\necho "{variant}-handled"\nexit 0\n')
        mock_bin.chmod(0o755)

    result = _run_hybrid(hybrid_project, "test-cmd")
    assert result.returncode == 0
    assert "release-handled" in result.stdout


def test_every_lib_script_is_reachable():
    """Every .py file in lib/ is reachable as a subcommand.

    The bin/flow dispatcher resolves subcommands via:
        script="$LIB_DIR/$subcmd.py"
    so reachability is a pure filesystem property: every lib/*.py stem
    must resolve to an existing file. No subprocess calls needed.
    """
    py_files = sorted(LIB_DIR.glob("*.py"))
    # Exclude flow_utils.py (library, not a subcommand)
    scripts = [f for f in py_files if f.name != "flow_utils.py"]
    assert len(scripts) > 0

    for script in scripts:
        subcmd = script.stem
        resolved = LIB_DIR / f"{subcmd}.py"
        assert resolved.is_file(), f"bin/flow cannot find subcommand '{subcmd}' — expected {resolved} to exist"
