"""Tests for lib/ci.py — the bin/flow ci subcommand."""

import json
import os
import subprocess
import sys

import pytest

from conftest import LIB_DIR


@pytest.fixture
def ci_project(git_repo):
    """Create a project with a passing bin/ci inside a git repo.

    Commits bin/ci so the working tree is clean — tests that need a dirty
    tree add their own untracked files.
    """
    bin_dir = git_repo / "bin"
    bin_dir.mkdir()
    (bin_dir / "ci").write_text("#!/usr/bin/env bash\nexit 0\n")
    (bin_dir / "ci").chmod(0o755)
    subprocess.run(["git", "add", "-A"], cwd=str(git_repo), check=True,
                   capture_output=True)
    subprocess.run(["git", "commit", "-m", "add bin/ci"], cwd=str(git_repo),
                   check=True, capture_output=True)
    return git_repo


def _run(project_dir, args=None, extra_env=None):
    """Run lib/ci.py inside the given project directory."""
    env = os.environ.copy()
    env.pop("FLOW_CI_RUNNING", None)
    if extra_env:
        env.update(extra_env)
    cmd = [sys.executable, str(LIB_DIR / "ci.py")]
    if args:
        cmd.extend(args)
    result = subprocess.run(
        cmd, capture_output=True, text=True,
        cwd=str(project_dir), env=env,
    )
    return result


def _parse(result):
    """Parse JSON from the last line of stdout."""
    lines = result.stdout.strip().splitlines()
    return json.loads(lines[-1])


def _branch_name(project_dir):
    """Get the current branch name in the project directory."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=str(project_dir), capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


def test_runs_ci_and_creates_sentinel(ci_project):
    branch = _branch_name(ci_project)
    result = _run(ci_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    assert output["skipped"] is False
    sentinel = ci_project / ".flow-states" / f"{branch}-ci-passed"
    assert sentinel.exists()


def test_runs_ci_even_with_sentinel(ci_project):
    branch = _branch_name(ci_project)
    sentinel = ci_project / ".flow-states" / f"{branch}-ci-passed"
    sentinel.parent.mkdir(parents=True, exist_ok=True)
    sentinel.touch()
    result = _run(ci_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is False


def test_if_dirty_skips_when_sentinel_and_clean(ci_project):
    # Exclude .flow-states from git (as real projects do via .git/info/exclude)
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Run CI once to create sentinel with current snapshot
    first = _run(ci_project)
    assert first.returncode == 0
    # Now --if-dirty should skip — nothing changed
    result = _run(ci_project, args=["--if-dirty"])
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is True
    assert "no changes" in output["reason"]


def test_if_dirty_runs_when_no_sentinel(ci_project):
    result = _run(ci_project, args=["--if-dirty"])
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is False


def test_if_dirty_runs_when_dirty(ci_project):
    # Exclude .flow-states from git (as real projects do via .git/info/exclude)
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Run CI once to create sentinel with current snapshot
    first = _run(ci_project)
    assert first.returncode == 0
    # Add a file so the tree snapshot changes
    (ci_project / "untracked.txt").write_text("dirty\n")
    result = _run(ci_project, args=["--if-dirty"])
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is False


def test_if_dirty_skips_after_commit(ci_project):
    """After committing, --if-dirty still skips because HEAD hash is in snapshot."""
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Create a new file and commit it
    (ci_project / "feature.py").write_text("# new feature\n")
    subprocess.run(["git", "add", "-A"], cwd=str(ci_project), check=True,
                   capture_output=True)
    subprocess.run(["git", "commit", "-m", "add feature"], cwd=str(ci_project),
                   check=True, capture_output=True)
    # Run CI — creates sentinel with post-commit snapshot
    first = _run(ci_project)
    assert first.returncode == 0
    assert _parse(first)["skipped"] is False
    # Run CI again with --if-dirty — should skip (HEAD unchanged, tree clean)
    second = _run(ci_project, args=["--if-dirty"])
    assert second.returncode == 0
    output = _parse(second)
    assert output["skipped"] is True
    assert "no changes" in output["reason"]


def test_runs_without_branch_detection(ci_project):
    """Detached HEAD: CI runs but no sentinel is created."""
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=str(ci_project), capture_output=True, text=True, check=True,
    ).stdout.strip()
    subprocess.run(
        ["git", "checkout", head],
        cwd=str(ci_project), capture_output=True, check=True,
    )
    result = _run(ci_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is False
    # No sentinel created — no branch to name it after
    flow_states = ci_project / ".flow-states"
    if flow_states.exists():
        assert not list(flow_states.glob("*-ci-passed"))


def test_ci_failure_exits_1_and_removes_sentinel(ci_project):
    branch = _branch_name(ci_project)
    sentinel = ci_project / ".flow-states" / f"{branch}-ci-passed"
    sentinel.parent.mkdir(parents=True, exist_ok=True)
    sentinel.touch()
    (ci_project / "bin" / "ci").write_text("#!/usr/bin/env bash\nexit 1\n")
    result = _run(ci_project)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert not sentinel.exists()


def test_ci_failure_without_sentinel(ci_project):
    branch = _branch_name(ci_project)
    (ci_project / "bin" / "ci").write_text("#!/usr/bin/env bash\nexit 1\n")
    result = _run(ci_project)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    sentinel = ci_project / ".flow-states" / f"{branch}-ci-passed"
    assert not sentinel.exists()


def test_runs_non_bash_ci_script(target_project):
    """ci.py must not force bash — target projects may use Ruby, Python, etc."""
    result = _run(target_project)
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"


def test_non_bash_ci_with_if_dirty(target_project):
    """--if-dirty works with non-bash CI scripts too."""
    result = _run(target_project, args=["--if-dirty"])
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    assert output["skipped"] is False


def test_non_bash_ci_failure(target_project):
    """Non-bash CI script that fails is detected correctly."""
    (target_project / "bin" / "ci").write_text(
        "#!/usr/bin/env python3\nimport sys\nsys.exit(1)\n"
    )
    result = _run(target_project)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"


def test_missing_bin_ci_exits_1(git_repo):
    result = _run(git_repo)
    assert result.returncode == 1
    output = _parse(result)
    assert output["status"] == "error"
    assert "not found" in output["message"]


def test_recursion_guard(ci_project):
    result = _run(ci_project, extra_env={"FLOW_CI_RUNNING": "1"})
    assert result.returncode == 0
    output = _parse(result)
    assert output["skipped"] is True
    assert "recursion" in output["reason"]


def test_branch_flag_uses_specified_sentinel(ci_project):
    """--branch flag creates sentinel named after the specified branch."""
    result = _run(ci_project, args=["--branch", "other-feature"])
    assert result.returncode == 0
    output = _parse(result)
    assert output["status"] == "ok"
    sentinel = ci_project / ".flow-states" / "other-feature-ci-passed"
    assert sentinel.exists()


def test_detects_tracked_file_content_change(ci_project):
    """Editing an already-modified tracked file must change the snapshot."""
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Create and commit a tracked file
    (ci_project / "app.py").write_text("version = 1\n")
    subprocess.run(["git", "add", "-A"], cwd=str(ci_project), check=True,
                   capture_output=True)
    subprocess.run(["git", "commit", "-m", "add app"], cwd=str(ci_project),
                   check=True, capture_output=True)
    # Modify the tracked file (status: M)
    (ci_project / "app.py").write_text("version = 2\n")
    # Run CI — creates sentinel with "version = 2" content
    first = _run(ci_project, args=["--if-dirty"])
    assert first.returncode == 0
    assert _parse(first)["skipped"] is False
    # Modify again — still M status, but different content
    (ci_project / "app.py").write_text("version = 3\n")
    # Must NOT skip — content changed even though status is the same
    second = _run(ci_project, args=["--if-dirty"])
    assert second.returncode == 0
    assert _parse(second)["skipped"] is False


def test_detects_untracked_file_content_change(ci_project):
    """Editing an untracked file must change the snapshot."""
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Create an untracked file
    (ci_project / "notes.txt").write_text("draft 1\n")
    # Run CI — creates sentinel with "draft 1" content
    first = _run(ci_project, args=["--if-dirty"])
    assert first.returncode == 0
    assert _parse(first)["skipped"] is False
    # Modify untracked file — still ?? status, but different content
    (ci_project / "notes.txt").write_text("draft 2\n")
    # Must NOT skip — content changed
    second = _run(ci_project, args=["--if-dirty"])
    assert second.returncode == 0
    assert _parse(second)["skipped"] is False


def test_detects_staged_content_change(ci_project):
    """Re-staging a file with different content must change the snapshot."""
    exclude = ci_project / ".git" / "info" / "exclude"
    exclude.parent.mkdir(parents=True, exist_ok=True)
    exclude.write_text(".flow-states/\n")
    # Create a file, stage it
    (ci_project / "config.py").write_text("setting = 'a'\n")
    subprocess.run(["git", "add", "config.py"], cwd=str(ci_project),
                   check=True, capture_output=True)
    # Run CI — creates sentinel
    first = _run(ci_project, args=["--if-dirty"])
    assert first.returncode == 0
    assert _parse(first)["skipped"] is False
    # Replace content and re-stage — status stays "A" but content differs
    (ci_project / "config.py").write_text("setting = 'b'\n")
    subprocess.run(["git", "add", "config.py"], cwd=str(ci_project),
                   check=True, capture_output=True)
    # Must NOT skip — staged content changed
    second = _run(ci_project, args=["--if-dirty"])
    assert second.returncode == 0
    assert _parse(second)["skipped"] is False
