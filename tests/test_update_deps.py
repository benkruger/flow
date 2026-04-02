"""Tests for lib/update-deps.py — the bin/flow update-deps subcommand."""

import json
import os
import subprocess
import sys

from conftest import LIB_DIR


def _run(project_dir, extra_env=None):
    """Run lib/update-deps.py inside the given project directory."""
    env = os.environ.copy()
    env.pop("FLOW_UPDATE_DEPS_TIMEOUT", None)
    if extra_env:
        env.update(extra_env)
    result = subprocess.run(
        [sys.executable, str(LIB_DIR / "update-deps.py")],
        capture_output=True,
        text=True,
        cwd=str(project_dir),
        env=env,
    )
    return result


def _parse(result):
    """Parse JSON from the last line of stdout."""
    lines = result.stdout.strip().splitlines()
    return json.loads(lines[-1])


def _add_deps_script(project_dir, script_body):
    """Create bin/dependencies with the given body."""
    bin_dir = project_dir / "bin"
    bin_dir.mkdir(exist_ok=True)
    deps = bin_dir / "dependencies"
    deps.write_text(f"#!/usr/bin/env bash\n{script_body}\n")
    deps.chmod(0o755)


def test_skipped_when_no_bin_dependencies(target_project):
    """No bin/dependencies file → skipped."""
    assert not (target_project / "bin" / "dependencies").exists()
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "skipped"
    assert "not found" in output["reason"]


def test_no_changes_after_run(target_project):
    """bin/dependencies exists but produces no file changes → ok, changes=false."""
    _add_deps_script(target_project, "# no-op")
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    assert output["changes"] is False


def test_changes_after_run(target_project):
    """bin/dependencies touches a file → ok, changes=true."""
    _add_deps_script(target_project, 'echo "updated" > deps.lock')
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    assert output["changes"] is True


def test_error_when_deps_fails(target_project):
    """bin/dependencies exits non-zero → error."""
    _add_deps_script(target_project, "exit 1")
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert "failed" in output["message"].lower() or "exit" in output["message"].lower()


def test_timeout_reports_error(target_project):
    """bin/dependencies that hangs → error with timeout message."""
    _add_deps_script(target_project, "sleep 300")
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project, extra_env={"FLOW_UPDATE_DEPS_TIMEOUT": "1"})
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert "timed out" in output["message"].lower()


def test_non_bash_deps_script(target_project):
    """bin/dependencies can be a Python script, not just bash."""
    deps = target_project / "bin" / "dependencies"
    deps.parent.mkdir(exist_ok=True)
    deps.write_text('#!/usr/bin/env python3\nfrom pathlib import Path\nPath("py-deps.lock").write_text("v1")\n')
    deps.chmod(0o755)
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    assert output["changes"] is True


def test_non_executable_deps_reports_error(target_project):
    """bin/dependencies exists but is not executable → error."""
    deps = target_project / "bin" / "dependencies"
    deps.parent.mkdir(exist_ok=True)
    deps.write_text("#!/usr/bin/env bash\necho ok\n")
    deps.chmod(0o644)
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert "executed" in output["message"].lower()


def test_deps_is_directory_reports_skipped(target_project):
    """bin/dependencies is a directory, not a file → skipped."""
    deps_dir = target_project / "bin" / "dependencies"
    deps_dir.mkdir(parents=True, exist_ok=True)
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "skipped"


def test_invalid_timeout_env_var_reports_error(target_project):
    """Non-numeric FLOW_UPDATE_DEPS_TIMEOUT → error."""
    _add_deps_script(target_project, "# no-op")
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project, extra_env={"FLOW_UPDATE_DEPS_TIMEOUT": "notanumber"})
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"


def test_git_status_failure_reports_error(tmp_path):
    """git status fails (not a git repo) → error."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    deps = bin_dir / "dependencies"
    deps.write_text("#!/usr/bin/env bash\n# no-op\n")
    deps.chmod(0o755)
    result = _run(tmp_path)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert "git status" in output["message"].lower()


def test_deps_stdout_does_not_corrupt_json(target_project):
    """bin/dependencies that prints to stdout does not break JSON parsing."""
    _add_deps_script(target_project, 'echo "Installing dependencies..."')
    subprocess.run(["git", "add", "-A"], cwd=str(target_project), check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "add deps"], cwd=str(target_project), check=True, capture_output=True)
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
