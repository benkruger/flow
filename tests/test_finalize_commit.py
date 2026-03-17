"""Tests for lib/finalize-commit.py — commit, cleanup, pull, push."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, call

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("finalize-commit")


# --- finalize_commit unit tests ---


def _make_result(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=[], returncode=returncode, stdout=stdout, stderr=stderr,
    )


def test_happy_path(tmp_path):
    """Commit + pull + push all succeed, message file cleaned up."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),                         # git commit
        _make_result(),                         # git pull
        _make_result(),                         # git push
        _make_result(stdout="abc123\n"),         # git rev-parse HEAD
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {"status": "ok", "sha": "abc123"}
    assert not msg_file.exists()


def test_commit_failure(tmp_path):
    """Commit fails — error returned, message file cleaned up."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(1, stderr="nothing to commit"),    # git commit
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {"status": "error", "step": "commit", "message": "nothing to commit"}
    assert not msg_file.exists()


def test_pull_conflict(tmp_path):
    """Pull fails with merge conflicts — conflict files listed."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    porcelain = "UU file1.py\nAA file2.py\n"
    responses = [
        _make_result(),                                          # git commit
        _make_result(1, stderr="CONFLICT"),                      # git pull
        _make_result(stdout=porcelain),                          # git status --porcelain
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {"status": "conflict", "files": ["file1.py", "file2.py"]}
    assert not msg_file.exists()


def test_pull_error_non_conflict(tmp_path):
    """Pull fails without conflicts — error returned."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),                                     # git commit
        _make_result(1, stderr="Could not resolve host"),   # git pull
        _make_result(stdout=""),                             # git status --porcelain (clean)
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {"status": "error", "step": "pull", "message": "Could not resolve host"}


def test_push_failure(tmp_path):
    """Push fails — error returned after successful commit and pull."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),                                 # git commit
        _make_result(),                                 # git pull
        _make_result(1, stderr="permission denied"),    # git push
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {"status": "error", "step": "push", "message": "permission denied"}


def test_message_file_missing_ok(tmp_path):
    """Missing message file during cleanup does not crash."""
    msg_file = tmp_path / ".flow-commit-msg"
    # Don't create the file — simulate it already being gone

    responses = [
        _make_result(),                         # git commit (succeeds despite file arg)
        _make_result(),                         # git pull
        _make_result(),                         # git push
        _make_result(stdout="def456\n"),         # git rev-parse HEAD
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result["status"] == "ok"


def test_correct_git_commands(tmp_path):
    """Verifies the exact git commands called in order."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),
        _make_result(),
        _make_result(),
        _make_result(stdout="abc123\n"),
    ]

    with patch("subprocess.run", side_effect=responses) as mock_run:
        _mod.finalize_commit(str(msg_file), "feat-branch")

    assert mock_run.call_args_list == [
        call(["git", "commit", "-F", str(msg_file)], capture_output=True, text=True, timeout=30),
        call(["git", "pull", "origin", "feat-branch"], capture_output=True, text=True, timeout=60),
        call(["git", "push"], capture_output=True, text=True, timeout=60),
        call(["git", "rev-parse", "HEAD"], capture_output=True, text=True, timeout=30),
    ]


def test_rev_parse_failure(tmp_path):
    """Rev-parse fails — ok with warning, empty SHA."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),                                 # git commit
        _make_result(),                                 # git pull
        _make_result(),                                 # git push
        _make_result(returncode=1, stderr="bad HEAD"),  # git rev-parse HEAD
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "ok",
        "sha": "",
        "warning": "commit succeeded but SHA retrieval failed",
    }


def test_commit_timeout(tmp_path):
    """Commit times out — error with step and timeout message."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired("git", 30)):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "error",
        "step": "commit",
        "message": "git commit timed out after 30s",
    }
    assert not msg_file.exists()


def test_pull_timeout(tmp_path):
    """Pull times out — error with step and timeout message."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),  # git commit succeeds
        subprocess.TimeoutExpired("git", 60),  # git pull times out
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "error",
        "step": "pull",
        "message": "git pull timed out after 60s",
    }


def test_push_timeout(tmp_path):
    """Push times out — error with step and timeout message."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),  # git commit
        _make_result(),  # git pull
        subprocess.TimeoutExpired("git", 60),  # git push times out
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "error",
        "step": "push",
        "message": "git push timed out after 60s",
    }


def test_rev_parse_timeout(tmp_path):
    """Rev-parse times out — ok with warning since commit succeeded."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),  # git commit
        _make_result(),  # git pull
        _make_result(),  # git push
        subprocess.TimeoutExpired("git", 30),  # git rev-parse times out
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "ok",
        "sha": "",
        "warning": "commit succeeded but SHA retrieval timed out",
    }


def test_status_porcelain_timeout(tmp_path):
    """Status-porcelain times out during pull failure — falls through to pull error."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),                                     # git commit
        _make_result(1, stderr="Could not resolve host"),   # git pull fails
        subprocess.TimeoutExpired("git", 30),               # git status --porcelain times out
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result == {
        "status": "error",
        "step": "pull",
        "message": "Could not resolve host",
    }


def test_dd_conflict_detected(tmp_path):
    """DD (both deleted) status is recognized as a conflict."""
    msg_file = tmp_path / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    responses = [
        _make_result(),
        _make_result(1, stderr="CONFLICT"),
        _make_result(stdout="DD deleted.py\n"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.finalize_commit(str(msg_file), "my-branch")

    assert result["status"] == "conflict"
    assert result["files"] == ["deleted.py"]


# --- CLI integration ---


def test_cli_happy_path(git_repo, branch, tmp_path):
    """Full subprocess run: commit, pull, push in a real git repo."""
    bare = tmp_path / "bare.git"
    subprocess.run(
        ["git", "init", "--bare", str(bare)],
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "remote", "add", "origin", str(bare)],
        capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "-u", "origin", branch],
        capture_output=True, check=True,
    )

    (git_repo / "test.txt").write_text("hello")
    subprocess.run(
        ["git", "-C", str(git_repo), "add", "-A"],
        capture_output=True, check=True,
    )

    msg_file = git_repo / ".flow-commit-msg"
    msg_file.write_text("Test commit via CLI.")

    script = Path(__file__).resolve().parent.parent / "lib" / "finalize-commit.py"
    result = subprocess.run(
        [sys.executable, str(script), ".flow-commit-msg", branch],
        capture_output=True, text=True,
        cwd=str(git_repo),
    )

    output = json.loads(result.stdout)
    assert result.returncode == 0
    assert output["status"] == "ok"
    assert len(output["sha"]) >= 7
    assert not msg_file.exists()


def test_cli_commit_failure(git_repo, branch):
    """Commit failure exits with returncode 1."""
    msg_file = git_repo / ".flow-commit-msg"
    msg_file.write_text("Test commit.")

    # Nothing staged, so commit will fail
    script = Path(__file__).resolve().parent.parent / "lib" / "finalize-commit.py"
    result = subprocess.run(
        [sys.executable, str(script), ".flow-commit-msg", branch],
        capture_output=True, text=True,
        cwd=str(git_repo),
    )

    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert output["step"] == "commit"
    assert not msg_file.exists()


def test_cli_missing_args():
    """Missing arguments exits with error JSON."""
    script = Path(__file__).resolve().parent.parent / "lib" / "finalize-commit.py"
    result = subprocess.run(
        [sys.executable, str(script)],
        capture_output=True, text=True,
    )

    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert output["step"] == "args"
