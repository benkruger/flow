"""Tests for lib/stop-continue.py — Stop hook continuation script."""

import importlib.util
import json
import subprocess
import sys

import pytest

from conftest import LIB_DIR, make_state, write_state

SCRIPT = LIB_DIR / "stop-continue.py"

_spec = importlib.util.spec_from_file_location(
    "stop_continue", SCRIPT
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


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

    def test_corrupt_state_file(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        # Should not raise on corrupt state file
        _mod.capture_session_id({"session_id": "abc123"})

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


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the Stop hook script as a subprocess.

    Returns (exit_code, stdout).
    """
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=stdin_data,
        capture_output=True,
        text=True,
        cwd=str(cwd) if cwd else None,
    )
    return result.returncode, result.stdout.strip()


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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        output = json.loads(stdout)
        assert output["decision"] == "block"
        assert "simplify" in output["reason"]

    def test_flag_empty_no_output(self, git_repo, state_dir, branch):
        state = make_state(current_phase="flow-code-review")
        state["_continue_pending"] = ""
        write_state(state_dir, branch, state)

        stdin = json.dumps({})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_malformed_stdin_no_output(self, git_repo):
        exit_code, stdout = _run_hook("not json at all", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_state_dir_no_output(self, git_repo):
        stdin = json.dumps({})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

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
        write_state(state_dir, branch, state)

        should_block, skill_name, context = _mod.check_continue({"session_id": "new-session"})

        assert should_block is False
        assert skill_name is None
        assert context is None

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""

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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

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
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_continue_pending"] == ""
        assert updated["session_id"] == "new-session"
