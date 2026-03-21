"""Tests for lib/stop-continue.py — Stop hook continuation script."""

import importlib.util
import json
import subprocess
import sys

import pytest

from conftest import LIB_DIR, make_state, write_state
from flow_utils import format_tab_title

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


# --- format_tab_title tests ---


class TestFormatTabTitle:
    def _state(self, phase, **kwargs):
        """Build a minimal state dict for title testing."""
        state = {"current_phase": phase, "branch": "test-feature"}
        state.update(kwargs)
        return state

    def test_phase_1_start(self):
        title = format_tab_title(self._state("flow-start"))
        assert title == "Flow: Phase 1: Start \u2014 Test Feature"

    def test_phase_2_plan(self):
        title = format_tab_title(self._state("flow-plan"))
        assert title == "Flow: Phase 2: Plan \u2014 Test Feature"

    def test_phase_3_code(self):
        title = format_tab_title(self._state("flow-code"))
        assert title == "Flow: Phase 3: Code \u2014 Test Feature"

    def test_phase_4_code_review(self):
        title = format_tab_title(self._state("flow-code-review"))
        assert title == "Flow: Phase 4: Code Review \u2014 Test Feature"

    def test_phase_5_learn(self):
        title = format_tab_title(self._state("flow-learn"))
        assert title == "Flow: Phase 5: Learn \u2014 Test Feature"

    def test_phase_6_complete(self):
        title = format_tab_title(self._state("flow-complete"))
        assert title == "Flow: Phase 6: Complete \u2014 Test Feature"

    def test_code_with_task(self):
        title = format_tab_title(self._state("flow-code", code_task=2))
        assert title == "Flow: Phase 3: Code (task 2) \u2014 Test Feature"

    def test_code_with_task_zero(self):
        """code_task=0 means no task started — no step info."""
        title = format_tab_title(self._state("flow-code", code_task=0))
        assert title == "Flow: Phase 3: Code \u2014 Test Feature"

    def test_code_with_string_task(self):
        """Non-integer code_task is ignored — no step info."""
        title = format_tab_title(self._state("flow-code", code_task="2"))
        assert title == "Flow: Phase 3: Code \u2014 Test Feature"

    def test_code_review_with_step(self):
        title = format_tab_title(self._state("flow-code-review", code_review_step=2))
        assert title == "Flow: Phase 4: Code Review (step 2/4) \u2014 Test Feature"

    def test_code_review_with_step_zero(self):
        """code_review_step=0 means not started — no step info."""
        title = format_tab_title(self._state("flow-code-review", code_review_step=0))
        assert title == "Flow: Phase 4: Code Review \u2014 Test Feature"

    def test_code_review_with_step_four(self):
        """code_review_step=4 means all done — no step info."""
        title = format_tab_title(self._state("flow-code-review", code_review_step=4))
        assert title == "Flow: Phase 4: Code Review \u2014 Test Feature"

    def test_missing_current_phase(self):
        assert format_tab_title({"branch": "test-feature"}) is None

    def test_missing_branch(self):
        assert format_tab_title({"current_phase": "flow-code"}) is None

    def test_unknown_phase_key(self):
        assert format_tab_title(self._state("flow-unknown")) is None

    def test_feature_name_from_branch(self):
        """Branch name is title-cased into the feature name."""
        title = format_tab_title(self._state("flow-start", branch="invoice-pdf-export"))
        assert title == "Flow: Phase 1: Start \u2014 Invoice Pdf Export"


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

        written = []
        fake_tty = type("FakeTTY", (), {
            "write": lambda self, data: written.append(data),
            "__enter__": lambda self: self,
            "__exit__": lambda self, *a: None,
        })()

        original_open = open

        def mock_open(path, *args, **kwargs):
            if path == "/dev/tty":
                return fake_tty
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        _mod.set_tab_title()

        assert len(written) == 1
        assert written[0] == "\033]0;Flow: Phase 3: Code \u2014 Test Feature\007"

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

    def test_no_state_file_no_error(self, git_repo, monkeypatch):
        """No state file — function returns silently."""
        monkeypatch.chdir(git_repo)
        _mod.set_tab_title()

    def test_no_branch_no_error(self, tmp_path, monkeypatch):
        """Not in a git repo — function returns silently."""
        monkeypatch.chdir(tmp_path)
        _mod.set_tab_title()

    def test_unknown_phase_no_write(self, git_repo, state_dir, branch, monkeypatch):
        """State file with unknown phase — format_tab_title returns None, no tty write."""
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["current_phase"] = "flow-unknown"
        write_state(state_dir, branch, state)

        written = []
        original_open = open

        def mock_open(path, *args, **kwargs):
            if path == "/dev/tty":
                written.append("opened")
                raise AssertionError("Should not open /dev/tty")
            return original_open(path, *args, **kwargs)

        monkeypatch.setattr("builtins.open", mock_open)
        _mod.set_tab_title()
        assert len(written) == 0
