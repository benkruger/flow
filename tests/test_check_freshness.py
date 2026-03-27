"""Tests for lib/check-freshness.py — pre-merge freshness check."""

import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import call, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))

_mod = importlib.import_module("check-freshness")


# --- helpers ---


def _make_result(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=[],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def _make_state_file(tmp_path, retries=0):
    """Create a minimal state file with freshness_retries."""
    state = {"branch": "test", "freshness_retries": retries}
    path = tmp_path / "state.json"
    path.write_text(json.dumps(state))
    return str(path)


# --- check_freshness unit tests ---


def test_up_to_date():
    """Branch already contains origin/main — no merge needed."""
    responses = [
        _make_result(),  # git fetch origin main
        _make_result(),  # git merge-base --is-ancestor (exit 0 = ancestor)
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {"status": "up_to_date"}


def test_merged():
    """Main has new commits, merge succeeds without conflicts."""
    responses = [
        _make_result(),  # git fetch origin main
        _make_result(returncode=1),  # git merge-base (not ancestor)
        _make_result(stdout="Merge made\n"),  # git merge origin/main
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {"status": "merged"}


def test_conflict():
    """Main has new commits, merge has conflicts."""
    porcelain = "UU file1.py\nAA file2.py\n"
    responses = [
        _make_result(),  # git fetch
        _make_result(returncode=1),  # merge-base
        _make_result(returncode=1, stderr="CONFLICT"),  # git merge
        _make_result(stdout=porcelain),  # git status --porcelain
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {"status": "conflict", "files": ["file1.py", "file2.py"]}


def test_fetch_failure():
    """Fetch fails — error returned."""
    responses = [
        _make_result(returncode=1, stderr="Could not resolve host"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {
        "status": "error",
        "step": "fetch",
        "message": "Could not resolve host",
    }


def test_merge_error_non_conflict():
    """Merge fails without conflicts — error returned."""
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(returncode=1, stderr="merge failed"),  # merge
        _make_result(stdout=""),  # status (clean)
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {
        "status": "error",
        "step": "merge",
        "message": "merge failed",
    }


def test_fetch_timeout():
    """Fetch times out — error with timeout message."""
    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired("git", 60)):
        result = _mod.check_freshness()

    assert result == {
        "status": "error",
        "step": "fetch",
        "message": "git fetch timed out after 60s",
    }


def test_merge_base_timeout():
    """Merge-base times out — treated as not up-to-date, proceeds to merge."""
    responses = [
        _make_result(),  # fetch
        subprocess.TimeoutExpired("git", 30),  # merge-base timeout
        _make_result(stdout="Already up to date.\n"),  # merge succeeds
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {"status": "merged"}


def test_merge_timeout():
    """Merge times out — error with timeout message."""
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        subprocess.TimeoutExpired("git", 60),  # merge timeout
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {
        "status": "error",
        "step": "merge",
        "message": "git merge timed out after 60s",
    }


def test_status_porcelain_timeout():
    """Status times out during conflict detection — falls through to merge error."""
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(returncode=1, stderr="CONFLICT"),  # merge fails
        subprocess.TimeoutExpired("git", 30),  # status timeout
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result == {
        "status": "error",
        "step": "merge",
        "message": "CONFLICT",
    }


def test_correct_git_commands_up_to_date():
    """Verifies the exact git commands called for up-to-date path."""
    responses = [
        _make_result(),  # fetch
        _make_result(),  # merge-base
    ]

    with patch("subprocess.run", side_effect=responses) as mock_run:
        _mod.check_freshness()

    assert mock_run.call_args_list == [
        call(
            ["git", "fetch", "origin", "main"],
            capture_output=True,
            text=True,
            timeout=60,
        ),
        call(
            ["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
            capture_output=True,
            text=True,
            timeout=30,
        ),
    ]


def test_correct_git_commands_merged():
    """Verifies the exact git commands called for merged path."""
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(),  # merge
    ]

    with patch("subprocess.run", side_effect=responses) as mock_run:
        _mod.check_freshness()

    assert mock_run.call_args_list == [
        call(
            ["git", "fetch", "origin", "main"],
            capture_output=True,
            text=True,
            timeout=60,
        ),
        call(
            ["git", "merge-base", "--is-ancestor", "origin/main", "HEAD"],
            capture_output=True,
            text=True,
            timeout=30,
        ),
        call(
            ["git", "merge", "origin/main"],
            capture_output=True,
            text=True,
            timeout=60,
        ),
    ]


def test_dd_conflict_detected():
    """DD (both deleted) status is recognized as a conflict."""
    responses = [
        _make_result(),
        _make_result(returncode=1),
        _make_result(returncode=1, stderr="CONFLICT"),
        _make_result(stdout="DD deleted.py\n"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness()

    assert result["status"] == "conflict"
    assert result["files"] == ["deleted.py"]


# --- retry tracking tests ---


def test_retry_increment(tmp_path):
    """freshness_retries starts at 0, incremented to 1 after merge."""
    state_file = _make_state_file(tmp_path, retries=0)
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(),  # merge succeeds
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness(state_file=state_file)

    assert result == {"status": "merged", "retries": 1}
    state = json.loads(Path(state_file).read_text())
    assert state["freshness_retries"] == 1


def test_retry_max_reached(tmp_path):
    """freshness_retries is 3 — max_retries returned immediately."""
    state_file = _make_state_file(tmp_path, retries=3)

    # No subprocess calls should happen
    with patch("subprocess.run", side_effect=AssertionError("should not be called")):
        result = _mod.check_freshness(state_file=state_file)

    assert result == {"status": "max_retries", "retries": 3}


def test_retry_no_state_file():
    """No state file — retry tracking skipped, merge works normally."""
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(),  # merge
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness(state_file=None)

    assert result == {"status": "merged"}
    assert "retries" not in result


def test_retry_not_incremented_on_up_to_date(tmp_path):
    """up_to_date path does not increment freshness_retries."""
    state_file = _make_state_file(tmp_path, retries=1)
    responses = [
        _make_result(),  # fetch
        _make_result(),  # merge-base (ancestor)
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness(state_file=state_file)

    assert result == {"status": "up_to_date"}
    state = json.loads(Path(state_file).read_text())
    assert state["freshness_retries"] == 1  # unchanged


def test_retry_increment_on_conflict(tmp_path):
    """freshness_retries incremented on conflict too."""
    state_file = _make_state_file(tmp_path, retries=1)
    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(returncode=1, stderr="CONFLICT"),  # merge fails
        _make_result(stdout="UU conflict.py\n"),  # status
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness(state_file=state_file)

    assert result["status"] == "conflict"
    assert result["retries"] == 2
    state = json.loads(Path(state_file).read_text())
    assert state["freshness_retries"] == 2


def test_retry_missing_key_in_state(tmp_path):
    """State file exists but has no freshness_retries key — defaults to 0."""
    state = {"branch": "test"}
    path = tmp_path / "state.json"
    path.write_text(json.dumps(state))
    state_file = str(path)

    responses = [
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(),  # merge
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.check_freshness(state_file=state_file)

    assert result == {"status": "merged", "retries": 1}
    updated = json.loads(path.read_text())
    assert updated["freshness_retries"] == 1


# --- CLI integration tests ---


def test_cli_up_to_date(git_repo, tmp_path, monkeypatch, capsys):
    """Real git repo already up to date — up_to_date returned."""
    bare = tmp_path / "bare.git"
    subprocess.run(
        ["git", "init", "--bare", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "remote", "add", "origin", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "-u", "origin", "main"],
        capture_output=True,
        check=True,
    )

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["check-freshness.py"])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "up_to_date"


def test_cli_merged(git_repo, tmp_path, monkeypatch, capsys):
    """Real git repo with new commits on main — merged returned."""
    bare = tmp_path / "bare.git"
    subprocess.run(
        ["git", "init", "--bare", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "remote", "add", "origin", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "-u", "origin", "main"],
        capture_output=True,
        check=True,
    )

    # Create a feature branch
    subprocess.run(
        ["git", "-C", str(git_repo), "branch", "feature"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "feature"],
        capture_output=True,
        check=True,
    )

    # Add a commit on main (via bare remote simulation)
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "main"],
        capture_output=True,
        check=True,
    )
    (git_repo / "new_on_main.txt").write_text("new content")
    subprocess.run(
        ["git", "-C", str(git_repo), "add", "-A"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "commit", "-m", "new on main"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "origin", "main"],
        capture_output=True,
        check=True,
    )

    # Switch back to feature branch
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "feature"],
        capture_output=True,
        check=True,
    )

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["check-freshness.py"])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "merged"


def test_cli_with_state_file(git_repo, tmp_path, monkeypatch, capsys):
    """CLI with --state-file tracks retries."""
    bare = tmp_path / "bare.git"
    subprocess.run(
        ["git", "init", "--bare", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "remote", "add", "origin", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "-u", "origin", "main"],
        capture_output=True,
        check=True,
    )

    # Create a feature branch diverged from main
    subprocess.run(
        ["git", "-C", str(git_repo), "branch", "feature"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "feature"],
        capture_output=True,
        check=True,
    )

    # Advance main
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "main"],
        capture_output=True,
        check=True,
    )
    (git_repo / "main_file.txt").write_text("content")
    subprocess.run(
        ["git", "-C", str(git_repo), "add", "-A"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "commit", "-m", "advance main"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "origin", "main"],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "switch", "feature"],
        capture_output=True,
        check=True,
    )

    # Create state file
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps({"branch": "feature", "freshness_retries": 0}))

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["check-freshness.py", "--state-file", str(state_file)])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "merged"
    assert output["retries"] == 1

    state = json.loads(state_file.read_text())
    assert state["freshness_retries"] == 1


def test_cli_max_retries(tmp_path, git_repo, monkeypatch, capsys):
    """CLI with max retries returns error exit code."""
    state_file = tmp_path / "state.json"
    state_file.write_text(json.dumps({"branch": "test", "freshness_retries": 3}))

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["check-freshness.py", "--state-file", str(state_file)])

    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "max_retries"
    assert output["retries"] == 3


def test_cli_unknown_args_ignored(git_repo, tmp_path, monkeypatch, capsys):
    """Unknown CLI arguments are silently skipped."""
    bare = tmp_path / "bare.git"
    subprocess.run(
        ["git", "init", "--bare", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "remote", "add", "origin", str(bare)],
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(git_repo), "push", "-u", "origin", "main"],
        capture_output=True,
        check=True,
    )

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["check-freshness.py", "--unknown", "value"])

    _mod.main()

    output = json.loads(capsys.readouterr().out)
    assert output["status"] == "up_to_date"
