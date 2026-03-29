"""Tests for lib/clear-blocked.py — PostToolUse hook for AskUserQuestion."""

import importlib.util
import json
import os
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = LIB_DIR / "clear-blocked.py"

_spec = importlib.util.spec_from_file_location("clear_blocked", SCRIPT)
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

    def test_no_branch_returns_early(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "current_branch", lambda: None)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        _mod.clear_blocked({})

        # _blocked should still be present — early return skipped the mutation
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" in updated

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


# --- Tab title reassertion tests ---


def _mock_tty(monkeypatch):
    """Set up a fake /dev/tty and return the list that captures writes."""
    written = []
    fake_tty = type(
        "FakeTTY",
        (),
        {
            "write": lambda self, data: written.append(data),
            "__enter__": lambda self: self,
            "__exit__": lambda self, *a: None,
        },
    )()

    original_open = open

    def mock_open(path, *args, **kwargs):
        if str(path) == "/dev/tty":
            return fake_tty
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr("builtins.open", mock_open)
    return written


class TestTabTitleReassert:
    def test_writes_tab_sequences_after_clearing(self, git_repo, state_dir, branch, monkeypatch):
        """clear_blocked writes tab title escape sequences after clearing _blocked."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        written = _mock_tty(monkeypatch)
        _mod.clear_blocked({})

        # _blocked cleared
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

        # Tab sequences written — must include title escape
        assert len(written) == 1
        assert "\033]1;" in written[0]

    def test_writes_tab_sequences_on_main_fallback(self, git_repo, state_dir, monkeypatch):
        """On main with another branch's state file → tab title written via fallback."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["branch"] = "some-feature"
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, "some-feature", state)

        # current_branch() returns "main" — no main.json, should fallback
        written = _mock_tty(monkeypatch)
        _mod.clear_blocked({})

        # Tab sequences written with title from the fallback state
        assert len(written) == 1
        assert "\033]1;" in written[0]
        assert "Some Feature" in written[0]

    def test_tab_sequences_fail_open(self, git_repo, state_dir, branch, monkeypatch):
        """If tty write fails, clear_blocked still clears _blocked without error."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        original_open = open

        def mock_open(path, *args, **kwargs):
            if str(path) == "/dev/tty":
                raise OSError("No tty available")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)

        # Should not raise
        _mod.clear_blocked({})

        # _blocked still cleared despite tty failure
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the clear-blocked hook script as a subprocess."""
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

    def test_bash_tool_clears_blocked(self, git_repo, state_dir, branch):
        """PostToolUse on Bash clears _blocked (expanded matcher coverage)."""
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"tool_name": "Bash"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

    def test_edit_tool_clears_blocked(self, git_repo, state_dir, branch):
        """PostToolUse on Edit clears _blocked (expanded matcher coverage)."""
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"tool_name": "Edit"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

    def test_write_tool_clears_blocked(self, git_repo, state_dir, branch):
        """PostToolUse on Write clears _blocked (expanded matcher coverage)."""
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"tool_name": "Write"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated
