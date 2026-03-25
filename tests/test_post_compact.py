"""Tests for lib/post-compact.py — PostCompact hook script."""

import importlib.util
import json
import os
import subprocess
import sys

import pytest

from conftest import LIB_DIR, make_state, write_state

SCRIPT = LIB_DIR / "post-compact.py"

_spec = importlib.util.spec_from_file_location(
    "post_compact", SCRIPT
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- In-process tests ---


class TestCaptureCompactData:
    def test_writes_summary_and_cwd(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "in_progress",
        })
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "User was writing tests for webhook handler.",
            "cwd": "/Users/ben/code/myapp",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["compact_summary"] == "User was writing tests for webhook handler."
        assert updated["compact_cwd"] == "/Users/ben/code/myapp"

    def test_increments_compact_count_from_zero(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "Working on feature.",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["compact_count"] == 1

    def test_increments_compact_count_from_existing(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["compact_count"] = 3
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "Another compaction.",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["compact_count"] == 4

    def test_summary_only_no_cwd(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "Just a summary.",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["compact_summary"] == "Just a summary."
        assert "compact_cwd" not in updated

    def test_empty_summary_still_writes_cwd_and_count(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "",
            "cwd": "/some/path",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "compact_summary" not in updated
        assert updated["compact_cwd"] == "/some/path"
        assert updated["compact_count"] == 1

    def test_no_compact_summary_key_skips(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)
        original = (state_dir / f"{branch}.json").read_text()

        _mod.capture_compact_data({"cwd": "/some/path"})

        assert (state_dir / f"{branch}.json").read_text() == original

    def test_no_state_dir(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)

        _mod.capture_compact_data({
            "compact_summary": "Summary.",
        })

    def test_no_state_file(self, git_repo, state_dir, monkeypatch):
        monkeypatch.chdir(git_repo)

        _mod.capture_compact_data({
            "compact_summary": "Summary.",
        })

    def test_corrupt_state_file(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        _mod.capture_compact_data({
            "compact_summary": "Summary.",
        })

    def test_not_in_git_repo(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        _mod.capture_compact_data({
            "compact_summary": "Summary.",
        })

    def test_preserves_existing_state_fields(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "in_progress",
        })
        state["session_id"] = "existing-session"
        state["notes"] = [{"note": "a correction"}]
        write_state(state_dir, branch, state)

        _mod.capture_compact_data({
            "compact_summary": "Summary.",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["session_id"] == "existing-session"
        assert updated["notes"] == [{"note": "a correction"}]
        assert updated["compact_summary"] == "Summary."


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the PostCompact hook script as a subprocess."""
    env = os.environ.copy()
    env.pop("FLOW_SIMULATE_BRANCH", None)
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_data,
        capture_output=True,
        text=True,
        cwd=str(cwd) if cwd else None,
        env=env,
    )
    return result.returncode, result.stdout.strip()


class TestSubprocess:
    def test_happy_path_writes_state(self, git_repo, state_dir, branch):
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)

        stdin = json.dumps({
            "compact_summary": "Working on tests.",
            "cwd": "/Users/ben/code/myapp",
            "trigger": "manual",
        })
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["compact_summary"] == "Working on tests."
        assert updated["compact_cwd"] == "/Users/ben/code/myapp"
        assert updated["compact_count"] == 1

    def test_malformed_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("not json at all", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_state_dir_exits_zero(self, git_repo):
        stdin = json.dumps({"compact_summary": "Summary."})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""
