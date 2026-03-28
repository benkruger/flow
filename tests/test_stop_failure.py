"""Tests for lib/stop-failure.py — StopFailure hook script."""

import json
import os
import subprocess
import sys

from conftest import LIB_DIR, import_lib, make_state, write_state

SCRIPT = LIB_DIR / "stop-failure.py"

_mod = import_lib("stop-failure.py")


# --- In-process tests ---


class TestCaptureFailureData:
    def test_writes_failure_data(self, git_repo, state_dir, branch, monkeypatch):
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

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        failure = updated["_last_failure"]
        assert failure["type"] == "rate_limit"
        assert failure["message"] == "429 Too Many Requests"
        assert "timestamp" in failure

    def test_no_error_type_key_skips(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)
        original = (state_dir / f"{branch}.json").read_text()

        _mod.capture_failure_data({"error_message": "some error"})

        assert (state_dir / f"{branch}.json").read_text() == original

    def test_no_state_dir(self, git_repo, monkeypatch):
        monkeypatch.chdir(git_repo)

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

    def test_no_state_file(self, git_repo, state_dir, monkeypatch):
        monkeypatch.chdir(git_repo)

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

    def test_corrupt_state_file(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        (state_dir / f"{branch}.json").write_text("{bad json")

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

    def test_no_branch_returns_early(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        monkeypatch.setattr(_mod, "resolve_branch", lambda override=None: (None, []))
        state = make_state(current_phase="flow-code")
        write_state(state_dir, branch, state)

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert "_last_failure" not in updated

    def test_not_in_git_repo(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        _mod.capture_failure_data(
            {
                "error_type": "auth_failure",
                "error_message": "Invalid API key",
            }
        )

    def test_preserves_existing_state_fields(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        state["session_id"] = "existing-session"
        state["notes"] = [{"note": "a correction"}]
        write_state(state_dir, branch, state)

        _mod.capture_failure_data(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["session_id"] == "existing-session"
        assert updated["notes"] == [{"note": "a correction"}]
        assert updated["_last_failure"]["type"] == "rate_limit"

    def test_overwrites_previous_failure(self, git_repo, state_dir, branch, monkeypatch):
        monkeypatch.chdir(git_repo)
        state = make_state(current_phase="flow-code")
        state["_last_failure"] = {
            "type": "old_error",
            "message": "Old message",
            "timestamp": "2026-01-01T00:00:00-08:00",
        }
        write_state(state_dir, branch, state)

        _mod.capture_failure_data(
            {
                "error_type": "network_timeout",
                "error_message": "Connection timed out",
            }
        )

        updated = json.loads((state_dir / f"{branch}.json").read_text())
        assert updated["_last_failure"]["type"] == "network_timeout"
        assert updated["_last_failure"]["message"] == "Connection timed out"


# --- Subprocess integration tests ---


def _run_hook(stdin_data, cwd=None):
    """Run the StopFailure hook script as a subprocess."""
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

        stdin = json.dumps(
            {
                "error_type": "rate_limit",
                "error_message": "429 Too Many Requests",
            }
        )
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        updated = json.loads((state_dir / f"{branch}.json").read_text())
        failure = updated["_last_failure"]
        assert failure["type"] == "rate_limit"
        assert failure["message"] == "429 Too Many Requests"
        assert "timestamp" in failure

    def test_malformed_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("not json at all", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_state_dir_exits_zero(self, git_repo):
        stdin = json.dumps({"error_type": "rate_limit", "error_message": "429"})
        exit_code, stdout = _run_hook(stdin, cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""

    def test_no_stdin_exits_zero(self, git_repo):
        exit_code, stdout = _run_hook("", cwd=git_repo)

        assert exit_code == 0
        assert stdout == ""
