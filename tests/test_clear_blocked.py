"""Tests for lib/clear-blocked.py — PostToolUse hook for AskUserQuestion."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = LIB_DIR / "clear-blocked.py"

_spec = importlib.util.spec_from_file_location(
    "clear_blocked", SCRIPT
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- In-process tests ---


class TestClearBlocked:
    def test_clears_blocked_flag(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        _mod.clear_blocked({})

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

    def test_no_blocked_flag_noop(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        path = write_state(state_dir, branch, state)
        original = path.read_text()

        _mod.clear_blocked({})

        assert path.read_text() == original

    def test_no_state_file(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)
        # Should not raise
        _mod.clear_blocked({})

    def test_corrupt_state_file(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")
        # Should not raise
        _mod.clear_blocked({})

    def test_not_in_git_repo(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        # Should not raise
        _mod.clear_blocked({})

    def test_preserves_other_state(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        state["session_id"] = "existing-session"
        state["notes"] = [{"note": "a correction"}]
        write_state(state_dir, branch, state)

        _mod.clear_blocked({})

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated
        assert updated["session_id"] == "existing-session"
        assert updated["notes"] == [{"note": "a correction"}]


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the clear-blocked hook script as a subprocess."""
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_data,
        capture_output=True,
        text=True,
        cwd=str(cwd) if cwd else None,
    )
    return result.returncode, result.stdout.strip()


class TestSubprocess:
    def test_happy_path_clears_blocked(self, git_repo, state_dir, branch):
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"tool_name": "AskUserQuestion"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

    def test_malformed_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("not json at all", cwd=git_repo)
        assert exit_code == 0
        assert stdout == ""

    def test_no_state_dir_exits_zero(self, git_repo):
        stdin = json.dumps({"tool_name": "AskUserQuestion"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)
        assert exit_code == 0
        assert stdout == ""

    def test_no_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("", cwd=git_repo)
        assert exit_code == 0
        assert stdout == ""
