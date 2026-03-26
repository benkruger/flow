"""Tests for lib/stop-continue.py — Stop hook continuation script."""

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import LIB_DIR, make_state, write_state
from flow_utils import (
    format_tab_color,
    format_tab_title,
    write_tab_sequences,
)

SCRIPT = LIB_DIR / "stop-continue.py"

_spec = importlib.util.spec_from_file_location(
    "stop_continue", SCRIPT
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _mock_tty(monkeypatch):
    """Set up a fake /dev/tty and return the list that captures writes."""
    written = []
    fake_tty = type("FakeTTY", (), {
        "write": lambda self, data: written.append(data),
        "__enter__": lambda self: self,
        "__exit__": lambda self, *a: None,
    })()

    original_open = open

    def mock_open(path, *args, **kwargs):
        if str(path) == "/dev/tty":
            return fake_tty
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr("builtins.open", mock_open)
    return written


# --- In-process tests ---


class TestCaptureSessionId:
    def test_updates_state_file(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)

        _mod.capture_session_id({
            "session_id": "abc123",
            "transcript_path": "/path/to/transcript.jsonl",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["session_id"] == "abc123"
        assert updated["transcript_path"] == "/path/to/transcript.jsonl"

    def test_skips_when_already_set(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-start")
        state["session_id"] = "abc123"
        write_state(state_dir, branch, state)
        state_path = state_dir / f"{branch}.json"
        original_content = state_path.read_text()

        _mod.capture_session_id({"session_id": "abc123"})

        assert state_path.read_text() == original_content

    def test_no_state_file(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)

        # Should not raise
        _mod.capture_session_id({"session_id": "abc123"})

    def test_no_session_id_in_input(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)
        state_path = state_dir / f"{branch}.json"
        original_content = state_path.read_text()

        _mod.capture_session_id({})

        assert state_path.read_text() == original_content

    def test_no_branch(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        # Should not raise when not in a git repo
        _mod.capture_session_id({"session_id": "abc123"})

    def test_corrupt_state_file(self, git_repo, state_dir, branch, monkeypatch, capsys):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        # Should not raise on corrupt state file
        _mod.capture_session_id({"session_id": "abc123"})

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] capture_session_id error:" in captured.err

    def test_updates_transcript_path(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)

        _mod.capture_session_id({
            "session_id": "xyz789",
            "transcript_path": "/home/user/.claude/projects/abc/xyz789.jsonl",
        })

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["transcript_path"] == "/home/user/.claude/projects/abc/xyz789.jsonl"


class TestCheckContinue:
    def test_flag_set_blocks_and_clears(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code-review",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "complete",
                "flow-code-review": "in_progress",
            },
        )
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is True
        assert skill_name == "simplify"
        assert context is None

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""

    def test_flag_empty_allows(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code-review")
        state["_continue_pending"] = ""
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

    def test_flag_absent_allows(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code-review")
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

    def test_no_state_file_allows(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

    def test_no_branch_allows(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

    def test_corrupt_state_file_allows(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

    def test_context_returned_when_present(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-learn",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "complete",
                "flow-code-review": "complete",
                "flow-learn": "in_progress",
            },
        )
        state["_continue_pending"] = "commit"
        state["_continue_context"] = "Set learn_step=5, then self-invoke flow:flow-learn --continue-step."
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is True
        assert skill_name == "commit"
        assert context == "Set learn_step=5, then self-invoke flow:flow-learn --continue-step."

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""
        assert updated["_continue_context"] == ""

    def test_context_absent_returns_none(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is True
        assert skill_name == "commit"
        assert context is None

    def test_context_empty_returns_none(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        state["_continue_context"] = ""
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is True
        assert skill_name == "commit"
        assert context is None

    def test_context_without_pending_allows(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_context"] = "Stale context from a previous invocation."
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None


# --- Error reporting tests ---


class TestCheckContinueErrorReporting:
    def test_stderr_on_mutate_state_error(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When mutate_state raises, stderr contains the error diagnostic."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        def raise_error(*args, **kwargs):
            raise json.JSONDecodeError("Expecting value", "", 0)

        monkeypatch.setattr(_mod, "mutate_state", raise_error)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] check_continue error:" in captured.err
        assert "Expecting value" in captured.err

    def test_log_file_written_on_error(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When mutate_state raises and branch is known, error is logged to the log file."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        def raise_error(*args, **kwargs):
            raise RuntimeError("disk full")

        monkeypatch.setattr(_mod, "mutate_state", raise_error)

        _mod.check_continue()

        log_path = state_dir / f"{branch}.log"
        assert log_path.exists()
        log_content = log_path.read_text()
        assert "[stop-continue] check_continue error:" in log_content
        assert "disk full" in log_content

    def test_no_crash_when_branch_unknown(self, tmp_path, monkeypatch, capsys):
        """When current_branch raises (extreme edge case), stderr is written but no log crash."""
        monkeypatch.chdir(tmp_path)

        monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)

        def raise_error():
            raise OSError("git binary missing")

        monkeypatch.setattr(_mod, "current_branch", raise_error)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None
        assert context is None

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] check_continue error:" in captured.err
        assert "git binary missing" in captured.err

    def test_log_failure_does_not_mask_original_error(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When file logging itself fails, the original stderr diagnostic still appears."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        def raise_mutate(*args, **kwargs):
            raise RuntimeError("original error")

        def raise_now():
            raise OSError("clock broken")

        monkeypatch.setattr(_mod, "mutate_state", raise_mutate)
        monkeypatch.setattr(_mod, "now", raise_now)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        captured = capsys.readouterr()
        assert "original error" in captured.err

        log_path = state_dir / f"{branch}.log"
        log_content = log_path.read_text() if log_path.exists() else ""
        assert "[stop-continue] check_continue error:" not in log_content

    def test_subprocess_corrupt_state_produces_stderr(self, git_repo, state_dir, branch):
        """Subprocess: corrupt state file with _continue_pending produces stderr diagnostic."""
        (state_dir / f"{branch}.json").write_text("{bad json")

        stdin = json.dumps({})
        exit_code, stdout, stderr = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""
        assert "[FLOW stop-continue] check_continue error:" in stderr


# --- Decision logging tests ---


class TestCheckContinueDecisionLogging:
    """Tests that check_continue logs meaningful decisions (block/session-mismatch)."""

    def test_block_logs_decision_to_stderr(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When flag is honored, stderr contains blocking decision."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        _mod.check_continue()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] blocking: pending=commit" in captured.err

    def test_block_logs_decision_to_log_file(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When flag is honored, decision is written to flow log file."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        _mod.check_continue()

        log_path = state_dir / f"{branch}.log"
        assert log_path.exists()
        log_content = log_path.read_text()
        assert "[stop-continue] blocking: pending=commit" in log_content

    def test_session_mismatch_logs_decision_to_stderr(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When session isolation clears flag, stderr contains session mismatch."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["session_id"] = "old-session"
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        _mod.check_continue({"session_id": "new-session"})

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] session mismatch" in captured.err
        assert "old-session" in captured.err
        assert "new-session" in captured.err
        assert "simplify" in captured.err

    def test_session_mismatch_logs_decision_to_log_file(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When session isolation clears flag, decision is written to flow log file."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["session_id"] = "old-session"
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        _mod.check_continue({"session_id": "new-session"})

        log_path = state_dir / f"{branch}.log"
        assert log_path.exists()
        log_content = log_path.read_text()
        assert "[stop-continue] session mismatch" in log_content
        assert "simplify" in log_content

    def test_no_pending_does_not_log_decision(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When _continue_pending is empty, no decision log line."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = ""
        write_state(state_dir, branch, state)

        _mod.check_continue()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue]" not in captured.err

    def test_block_log_file_failure_does_not_propagate(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When log file write fails, stderr diagnostic is still preserved."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        def raise_now():
            raise OSError("clock broken")

        monkeypatch.setattr(_mod, "now", raise_now)

        _mod.check_continue()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] blocking: pending=commit" in captured.err
        log_path = state_dir / f"{branch}.log"
        log_content = log_path.read_text() if log_path.exists() else ""
        assert "[stop-continue] blocking:" not in log_content

    def test_no_state_file_does_not_log_decision(self, git_repo, monkeypatch, capsys):
        """When no state file exists, no decision log line."""
        monkeypatch.chdir(git_repo)

        _mod.check_continue()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue]" not in captured.err

    def test_subprocess_block_logs_to_stderr(self, git_repo, state_dir, branch):
        """Subprocess: blocking decision appears in stderr."""
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout, stderr = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert "blocking: pending=commit" in stderr


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the Stop hook script as a subprocess.

    Returns (exit_code, stdout, stderr).
    """
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_data,
        capture_output=True,
        text=True,
        cwd=str(cwd) if cwd else None,
    )
    return result.returncode, result.stdout.strip(), result.stderr.strip()


class TestSubprocess:
    def test_flag_set_outputs_block_json(self, git_repo, state_dir, branch):
        state = make_state(
            current_phase="flow-code-review",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "complete",
                "flow-code-review": "in_progress",
            },
        )
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        output = json.loads(stdout)
        assert output["decision"] == "block"
        assert "simplify" in output["reason"]

    def test_flag_empty_no_output(self, git_repo, state_dir, branch):
        state = make_state(current_phase="flow-code-review")
        state["_continue_pending"] = ""
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_malformed_stdin_no_output(self, git_repo):
        exit_code, stdout, _ = _run_hook("not json at all", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_state_dir_no_output(self, git_repo):
        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_context_included_in_block_reason(self, git_repo, state_dir, branch):
        state = make_state(
            current_phase="flow-learn",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "complete",
                "flow-code-review": "complete",
                "flow-learn": "in_progress",
            },
        )
        state["_continue_pending"] = "commit"
        state["_continue_context"] = "Set learn_step=5, then self-invoke flow:flow-learn --continue-step."
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        output = json.loads(stdout)
        assert output["decision"] == "block"
        assert "Next steps:" in output["reason"]
        assert "learn_step=5" in output["reason"]

    def test_no_context_uses_generic_reason(self, git_repo, state_dir, branch):
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        output = json.loads(stdout)
        assert output["decision"] == "block"
        assert "Resume the parent skill instructions" in output["reason"]

    def test_main_passes_stdin_to_capture(self, git_repo, state_dir, branch):
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)

        stdin = json.dumps({
            "session_id": "from-stdin-test",
            "transcript_path": "/path/to/from-stdin.jsonl",
        })
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["session_id"] == "from-stdin-test"
        assert updated["transcript_path"] == "/path/to/from-stdin.jsonl"


# --- Session isolation tests ---


class TestSessionIsolation:
    def test_stale_session_clears_flag(self, git_repo, state_dir, branch, monkeypatch):
        """Flag set by old session → check_continue with new session_id clears it."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code-review", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "complete", "flow-code-review": "in_progress",
        })
        state["session_id"] = "old-session"
        state["_continue_pending"] = "simplify"
        state["_continue_context"] = "Resume at step 2."
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue({"session_id": "new-session"})

        assert should_block is False
        assert skill_name is None
        assert context is None

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""
        assert updated["_continue_context"] == ""

    def test_matching_session_fires_flag(self, git_repo, state_dir, branch, monkeypatch):
        """Flag set by same session → check_continue blocks."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code-review", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "complete", "flow-code-review": "in_progress",
        })
        state["session_id"] = "same-session"
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue({"session_id": "same-session"})

        assert should_block is True
        assert skill_name == "simplify"

    def test_missing_state_session_fires_flag(self, git_repo, state_dir, branch, monkeypatch):
        """State has no session_id (old state file) → backward compat, flag fires."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue({"session_id": "any"})

        assert should_block is True
        assert skill_name == "commit"

    def test_missing_hook_session_fires_flag(self, git_repo, state_dir, branch, monkeypatch):
        """Hook has no session_id → backward compat, flag fires."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["session_id"] = "abc123"
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue({})

        assert should_block is True
        assert skill_name == "commit"

    def test_no_state_file_on_main_allows(self, git_repo, state_dir, monkeypatch):
        """On main with only feature-branch.json → no exact match, allows stop."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, "feature-branch", state)

        should_block, skill_name, context = _mod.check_continue()

        assert should_block is False
        assert skill_name is None

    def test_subprocess_stale_session_no_block(self, git_repo, state_dir, branch):
        """Subprocess: stale session_id → no block output."""
        state = make_state(current_phase="flow-code-review", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "complete", "flow-code-review": "in_progress",
        })
        state["session_id"] = "old-session"
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"session_id": "new-session"})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_main_reorder_capture_after_check(self, git_repo, state_dir, branch):
        """After main(): stale flag cleared AND session_id updated to new (proves check before capture)."""
        state = make_state(current_phase="flow-code-review", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "complete", "flow-code-review": "in_progress",
        })
        state["session_id"] = "old-session"
        state["_continue_pending"] = "simplify"
        write_state(state_dir, branch, state)

        stdin = json.dumps({"session_id": "new-session"})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""
        assert updated["session_id"] == "new-session"


# --- check_qa_pending tests ---


class TestCheckQaPending:
    def test_blocks_when_file_exists(self, git_repo):
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        (state_dir / "qa-pending.json").write_text(json.dumps({
            "_continue_context": "Return to FLOW repo and verify.",
        }))

        should_block, context = _mod.check_qa_pending(root=git_repo)

        assert should_block is True
        assert context == "Return to FLOW repo and verify."

    def test_allows_when_no_file(self, git_repo):
        should_block, context = _mod.check_qa_pending(root=git_repo)

        assert should_block is False
        assert context is None

    def test_allows_when_empty_context(self, git_repo):
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        (state_dir / "qa-pending.json").write_text(json.dumps({
            "_continue_context": "",
        }))

        should_block, context = _mod.check_qa_pending(root=git_repo)

        assert should_block is False
        assert context is None

    def test_allows_when_corrupt_json(self, git_repo):
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        (state_dir / "qa-pending.json").write_text("{bad json")

        should_block, context = _mod.check_qa_pending(root=git_repo)

        assert should_block is False
        assert context is None

    def test_does_not_delete_file(self, git_repo):
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        qa_path = state_dir / "qa-pending.json"
        qa_path.write_text(json.dumps({
            "_continue_context": "Verify results.",
        }))

        _mod.check_qa_pending(root=git_repo)

        assert qa_path.exists()

    def test_default_root_resolution(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        (state_dir / "qa-pending.json").write_text(json.dumps({
            "_continue_context": "Verify results.",
        }))

        should_block, context = _mod.check_qa_pending()

        assert should_block is True
        assert context == "Verify results."

    def test_subprocess_qa_fallback_blocks(self, git_repo):
        """main() blocks via qa-pending fallback when no branch state file."""
        state_dir = git_repo / ".flow-states"
        state_dir.mkdir(exist_ok=True)
        (state_dir / "qa-pending.json").write_text(json.dumps({
            "_continue_context": "Return to FLOW repo and verify.",
        }))

        stdin = json.dumps({})
        exit_code, stdout, _ = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        output = json.loads(stdout)
        assert output["decision"] == "block"
        assert "Return to FLOW repo and verify." in output["reason"]


# --- set_tab_title tests ---


class TestSetTabTitle:
    def test_writes_escape_sequence_to_tty(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, branch, state)

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title()

        r, g, b = format_tab_color(state)
        assert len(written) == 1
        expected = (
            f"\033]6;1;bg;red;brightness;{r}\007"
            f"\033]6;1;bg;green;brightness;{g}\007"
            f"\033]6;1;bg;blue;brightness;{b}\007"
            f"\033]1;Test Feature \u2014 P3: Code\007"
        )
        assert written[0] == expected

    def test_color_override_from_flow_json(self, git_repo, state_dir, branch, monkeypatch):
        """When .flow.json has tab_color, use override instead of hash."""
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, branch, state)
        (git_repo / ".flow.json").write_text(json.dumps({"tab_color": [99, 88, 77]}))

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title()

        assert len(written) == 1
        assert "\033]6;1;bg;red;brightness;99\007" in written[0]
        assert "\033]6;1;bg;green;brightness;88\007" in written[0]
        assert "\033]6;1;bg;blue;brightness;77\007" in written[0]

    def test_color_write_failure_silent(self, git_repo, state_dir, branch, monkeypatch):
        """If tty write raises mid-sequence, no exception propagates."""
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, branch, state)

        call_count = 0

        class FailingTTY:
            def write(self_tty, data):
                nonlocal call_count
                call_count += 1
                raise OSError("tty write failed")

            def __enter__(self_tty):
                return self_tty

            def __exit__(self_tty, *a):
                return None

        original_open = open

        def mock_open(path, *args, **kwargs):
            if path == "/dev/tty":
                return FailingTTY()
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        _mod.set_tab_title()
        assert call_count >= 1

    def test_oserror_silently_caught(self, git_repo, state_dir, branch, monkeypatch):
        """OSError from /dev/tty is caught silently."""
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, branch, state)

        original_open = open

        def mock_open(path, *args, **kwargs):
            if path == "/dev/tty":
                raise OSError("No tty available")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        # Should not raise
        _mod.set_tab_title()

    def test_no_state_file_writes_color_only(self, git_repo, state_dir, monkeypatch):
        """No state file but detect_repo returns a repo — write color only, no title."""
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "detect_repo", lambda: "test/test")

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title()

        assert len(written) == 1
        r, g, b = format_tab_color(repo="test/test")
        expected = (
            f"\033]6;1;bg;red;brightness;{r}\007"
            f"\033]6;1;bg;green;brightness;{g}\007"
            f"\033]6;1;bg;blue;brightness;{b}\007"
        )
        assert written[0] == expected
        # No title escape in the output
        assert "\033]1;" not in written[0]

    def test_no_state_file_no_repo_no_override_no_write(self, git_repo, state_dir, monkeypatch):
        """No state file, no repo, no override — no tty write."""
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "detect_repo", lambda: None)

        written = []
        original_open = open

        def mock_open(path, *args, **kwargs):
            if str(path) == "/dev/tty":
                written.append("opened")
                raise AssertionError("Should not open /dev/tty")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        _mod.set_tab_title()
        assert len(written) == 0

    def test_no_state_file_with_flow_json_override(self, git_repo, state_dir, monkeypatch):
        """No state file but .flow.json has tab_color — use override color."""
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "detect_repo", lambda: None)
        (git_repo / ".flow.json").write_text(json.dumps({"tab_color": [50, 60, 70]}))

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title()

        assert len(written) == 1
        assert "\033]6;1;bg;red;brightness;50\007" in written[0]
        assert "\033]6;1;bg;green;brightness;60\007" in written[0]
        assert "\033]6;1;bg;blue;brightness;70\007" in written[0]

    def test_no_state_file_no_error(self, git_repo, monkeypatch):
        """No state file, no repo — function returns silently."""
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "detect_repo", lambda: None)
        _mod.set_tab_title()

    def test_no_branch_no_error(self, tmp_path, monkeypatch):
        """Not in a git repo — function returns silently."""
        monkeypatch.chdir(tmp_path)
        _mod.set_tab_title()

    def test_unknown_phase_color_still_written(self, git_repo, state_dir, branch, monkeypatch):
        """State file with unknown phase — no title, but color still written."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["current_phase"] = "flow-unknown"
        write_state(state_dir, branch, state)

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title()

        assert len(written) == 1
        r, g, b = format_tab_color(state)
        expected = (
            f"\033]6;1;bg;red;brightness;{r}\007"
            f"\033]6;1;bg;green;brightness;{g}\007"
            f"\033]6;1;bg;blue;brightness;{b}\007"
        )
        assert written[0] == expected
        assert "\033]1;" not in written[0]


# --- set_tab_title error logging tests ---


class TestSetTabTitleErrorLogging:
    def test_error_logged_to_stderr(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """Corrupt state file → stderr contains error diagnostic."""
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        _mod.set_tab_title()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] set_tab_title error:" in captured.err

    def test_error_logged_to_log_file(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """Corrupt state file → error logged to .flow-states/<branch>.log."""
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        _mod.set_tab_title()

        log_path = state_dir / f"{branch}.log"
        assert log_path.exists()
        log_content = log_path.read_text()
        assert "[stop-continue] set_tab_title error:" in log_content

    def test_log_failure_does_not_propagate(self, git_repo, state_dir, branch, monkeypatch, capsys):
        """When both the main operation and log writing fail, no exception propagates."""
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        def raise_now():
            raise OSError("clock broken")

        monkeypatch.setattr(_mod, "now", raise_now)

        # Should not raise
        _mod.set_tab_title()

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] set_tab_title error:" in captured.err


# --- Hoisted root/branch parameter tests ---


class TestCheckContinueWithParams:
    """Tests that check_continue accepts root and branch params directly."""

    def test_uses_passed_root_and_branch(self, git_repo, state_dir, branch):
        """When root and branch are passed, function uses them without subprocess."""
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        write_state(state_dir, branch, state)

        # Call from tmp_path (not git_repo) to prove no subprocess is used
        should_block, skill_name, context = _mod.check_continue(
            hook_input=None, root=git_repo, branch=branch
        )

        assert should_block is True
        assert skill_name == "commit"

    def test_no_state_file_with_params(self, git_repo, branch):
        """When root/branch point to nonexistent state file, allows stop."""
        should_block, skill_name, context = _mod.check_continue(
            hook_input=None, root=git_repo, branch=branch
        )

        assert should_block is False
        assert skill_name is None

    def test_none_branch_param_allows(self, git_repo):
        """When branch param is None, allows stop."""
        should_block, skill_name, context = _mod.check_continue(
            hook_input=None, root=git_repo, branch=None
        )

        assert should_block is False

    def test_none_branch_does_not_modify_state(
        self, git_repo, state_dir, branch, monkeypatch,
    ):
        """When branch=None is passed explicitly, state file must not be touched.

        Regression: branch=None conflated with 'auto-detect' caused
        current_branch() to leak to the host repo. On main, the host branch
        matched the fixture branch, so the function found and modified the
        state file. On feature branches the names diverged and the test
        passed by accident. monkeypatch.chdir ensures current_branch()
        returns the fixture's branch, making this test fail regardless of
        which host branch is running.
        """
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_continue_pending"] = "commit"
        state["_continue_context"] = "Self-invoke flow:flow-code."
        write_state(state_dir, branch, state)
        original = (state_dir / f"{branch}.json").read_text()

        should_block, skill_name, context = _mod.check_continue(
            hook_input=None, root=git_repo, branch=None
        )

        assert should_block is False
        assert (state_dir / f"{branch}.json").read_text() == original


class TestCaptureSessionIdWithParams:
    """Tests that capture_session_id accepts root and branch params directly."""

    def test_uses_passed_root_and_branch(self, git_repo, state_dir, branch):
        """When root and branch are passed, function uses them without subprocess."""
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)

        _mod.capture_session_id(
            {"session_id": "via-params", "transcript_path": "/p.jsonl"},
            root=git_repo, branch=branch,
        )

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["session_id"] == "via-params"
        assert updated["transcript_path"] == "/p.jsonl"

    def test_none_branch_param_skips(
        self, git_repo, state_dir, branch, monkeypatch,
    ):
        """When branch param is None, function returns without error.

        Uses monkeypatch.chdir so current_branch() would resolve to the
        fixture branch if accidentally called — catches regressions
        regardless of host branch.
        """
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-start")
        write_state(state_dir, branch, state)
        original = (state_dir / f"{branch}.json").read_text()

        _mod.capture_session_id(
            {"session_id": "via-params"},
            root=git_repo, branch=None,
        )

        assert (state_dir / f"{branch}.json").read_text() == original


class TestSetTabTitleWithParams:
    """Tests that set_tab_title accepts root and branch params directly."""

    def test_uses_passed_root_and_branch(self, git_repo, state_dir, branch, monkeypatch):
        """When root and branch are passed, function uses them without subprocess."""
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, branch, state)

        written = _mock_tty(monkeypatch)
        _mod.set_tab_title(root=git_repo, branch=branch)

        assert len(written) == 1
        assert "\033]1;" in written[0]

    def test_none_branch_returns_silently(self, git_repo, monkeypatch):
        """When branch param is None, function returns without error."""
        monkeypatch.setattr(_mod, "detect_repo", lambda: None)
        _mod.set_tab_title(root=git_repo, branch=None)


# --- write_tab_sequences tests ---


class TestWriteTabSequences:
    """Tests for flow_utils.write_tab_sequences — shared tab escape writer."""

    def test_writes_color_and_title_with_state(self, tmp_path, monkeypatch):
        """State dict with phase/branch/repo writes color + title to /dev/tty."""
        monkeypatch.chdir(tmp_path)
        written = _mock_tty(monkeypatch)

        state = {
            "current_phase": "flow-code",
            "branch": "test-feature",
            "repo": "test/test",
            "prompt": "test feature",
        }
        write_tab_sequences(state)

        assert len(written) == 1
        r, g, b = format_tab_color(state)
        assert f"\033]6;1;bg;red;brightness;{r}\007" in written[0]
        assert f"\033]6;1;bg;green;brightness;{g}\007" in written[0]
        assert f"\033]6;1;bg;blue;brightness;{b}\007" in written[0]
        title = format_tab_title(state)
        assert f"\033]1;{title}\007" in written[0]

    def test_writes_color_only_with_repo(self, tmp_path, monkeypatch):
        """repo kwarg without state writes only color sequences, no title."""
        monkeypatch.chdir(tmp_path)
        written = _mock_tty(monkeypatch)

        write_tab_sequences(repo="test/test")

        assert len(written) == 1
        r, g, b = format_tab_color(repo="test/test")
        assert f"\033]6;1;bg;red;brightness;{r}\007" in written[0]
        assert "\033]1;" not in written[0]

    def test_reads_flow_json_override(self, tmp_path, monkeypatch):
        """.flow.json with tab_color uses the override color."""
        monkeypatch.chdir(tmp_path)
        (tmp_path / ".flow.json").write_text(json.dumps({"tab_color": [99, 88, 77]}))
        written = _mock_tty(monkeypatch)

        state = {
            "current_phase": "flow-code",
            "branch": "test-feature",
            "repo": "test/test",
            "prompt": "test feature",
        }
        write_tab_sequences(state)

        assert len(written) == 1
        assert "\033]6;1;bg;red;brightness;99\007" in written[0]
        assert "\033]6;1;bg;green;brightness;88\007" in written[0]
        assert "\033]6;1;bg;blue;brightness;77\007" in written[0]

    def test_reads_flow_json_from_root(self, tmp_path, monkeypatch):
        """root kwarg directs .flow.json reading to the root path."""
        monkeypatch.chdir(tmp_path)
        subdir = tmp_path / "subdir"
        subdir.mkdir()
        (subdir / ".flow.json").write_text(json.dumps({"tab_color": [10, 20, 30]}))
        written = _mock_tty(monkeypatch)

        write_tab_sequences(repo="test/test", root=subdir)

        assert len(written) == 1
        assert "\033]6;1;bg;red;brightness;10\007" in written[0]

    def test_no_state_no_repo_no_write(self, tmp_path, monkeypatch):
        """No state, no repo — no /dev/tty open at all."""
        monkeypatch.chdir(tmp_path)
        opened = []
        original_open = open

        def mock_open(path, *args, **kwargs):
            if str(path) == "/dev/tty":
                opened.append("tty")
                raise AssertionError("Should not open /dev/tty")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        write_tab_sequences()
        assert len(opened) == 0

    def test_state_with_unknown_phase_writes_color_only(self, tmp_path, monkeypatch):
        """State with unrecognized phase writes color, no title."""
        monkeypatch.chdir(tmp_path)
        written = _mock_tty(monkeypatch)

        state = {
            "current_phase": "flow-unknown",
            "branch": "test-feature",
            "repo": "test/test",
        }
        write_tab_sequences(state)

        assert len(written) == 1
        assert "\033]1;" not in written[0]
        r, g, b = format_tab_color(state)
        assert f"\033]6;1;bg;red;brightness;{r}\007" in written[0]

    def test_missing_flow_json_uses_hash_color(self, tmp_path, monkeypatch):
        """No .flow.json file — uses hash-based color, no override."""
        monkeypatch.chdir(tmp_path)
        written = _mock_tty(monkeypatch)

        write_tab_sequences(repo="test/test")

        assert len(written) == 1
        r, g, b = format_tab_color(repo="test/test")
        assert f"\033]6;1;bg;red;brightness;{r}\007" in written[0]

    def test_raises_on_tty_error(self, tmp_path, monkeypatch):
        """OSError from /dev/tty propagates — callers handle errors."""
        monkeypatch.chdir(tmp_path)
        original_open = open

        def mock_open(path, *args, **kwargs):
            if str(path) == "/dev/tty":
                raise OSError("No tty available")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)

        with pytest.raises(OSError, match="No tty available"):
            write_tab_sequences(repo="test/test")


class TestMainErrorHandling:
    """Tests that main() handles project_root/current_branch failures gracefully."""

    def test_project_root_failure_allows_stop(self, monkeypatch):
        """When project_root raises, main exits 0 with no output (fail-open)."""
        def raise_error():
            raise OSError("git not found")

        monkeypatch.setattr(_mod, "project_root", raise_error)

        import io
        monkeypatch.setattr("sys.stdin", io.StringIO("{}"))

        _mod.main()
        # If we get here without exception, fail-open works

    def test_no_branch_allows_stop(self, monkeypatch, capsys):
        """When current_branch returns None, main exits with no output."""
        monkeypatch.setattr(_mod, "project_root", lambda: Path("/tmp"))
        monkeypatch.setattr(_mod, "current_branch", lambda: None)

        import io
        monkeypatch.setattr("sys.stdin", io.StringIO("{}"))

        _mod.main()

        captured = capsys.readouterr()
        assert captured.out == ""


class TestClearBlocked:
    def test_clears_blocked_on_stop(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_blocked"] = "2026-01-01T10:00:00-08:00"
        write_state(state_dir, branch, state)

        _mod.clear_blocked(root=git_repo, branch=branch)

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_blocked" not in updated

    def test_no_blocked_flag_noop(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        path = write_state(state_dir, branch, state)
        original = path.read_text()

        _mod.clear_blocked(root=git_repo, branch=branch)

        assert path.read_text() == original

    def test_no_state_file_noop(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)
        # Should not raise
        _mod.clear_blocked(root=git_repo, branch="nonexistent")

    def test_no_branch_noop(self):
        # Should not raise when branch is None
        _mod.clear_blocked(root=Path("/tmp"), branch=None)

    def test_corrupt_state_file_noop(self, git_repo, state_dir, branch, monkeypatch, capsys):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        _mod.clear_blocked(root=git_repo, branch=branch)

        captured = capsys.readouterr()
        assert "[FLOW stop-continue] clear_blocked error:" in captured.err
