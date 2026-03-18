"""Tests for lib/append-note.py — the note appender."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "append-note.py")

# Import append-note.py for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "append_note", LIB_DIR / "append-note.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(note_text, note_type=None, cwd=None, branch=None):
    """Run append-note.py via subprocess with --note and optional --type."""
    cmd = [sys.executable, SCRIPT, "--note", note_text]
    if note_type:
        cmd.extend(["--type", note_type])
    if branch is not None:
        cmd.extend(["--branch", branch])
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(cwd) if cwd else None,
    )
    return result


def _get_branch(git_repo):
    """Get the current branch name from a git repo."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    return result.stdout.strip()


# --- CLI behavior (subprocess) ---


def test_no_branch_returns_error(tmp_path):
    """Running outside a git repo (no branch) returns an error."""
    result = _run("test note", cwd=tmp_path)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "branch" in data["message"]


def test_no_state_file_returns_no_state(git_repo):
    result = _run("test note", cwd=git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "no_state"


def test_happy_path_returns_ok(tmp_path):
    """append_note returns updated state with one note."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = _mod.append_note(state_path, "flow-plan", "correction", "Always merge, never rebase")

    assert len(updated["notes"]) == 1


def test_note_written_to_state_file(tmp_path):
    """append_note persists note to disk with all expected fields."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    _mod.append_note(state_path, "flow-plan", "correction", "Always merge, never rebase")

    updated = json.loads(state_path.read_text())
    assert len(updated["notes"]) == 1
    note = updated["notes"][0]
    assert note["phase"] == "flow-plan"
    assert note["phase_name"] == "Plan"
    assert note["type"] == "correction"
    assert note["note"] == "Always merge, never rebase"
    assert "T" in note["timestamp"]  # ISO 8601 format


def test_multiple_notes_append(tmp_path):
    """Three sequential append_note calls accumulate all three notes."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    _mod.append_note(state_path, "flow-plan", "correction", "First note")
    _mod.append_note(state_path, "flow-plan", "learning", "Second note")
    updated = _mod.append_note(state_path, "flow-plan", "correction", "Third note")

    assert len(updated["notes"]) == 3


def test_type_defaults_to_correction(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase="flow-code", phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"})
    path = write_state(state_dir, branch, state)
    result = _run("Default type note", cwd=git_repo)
    assert result.returncode == 0
    updated = json.loads(path.read_text())
    assert updated["notes"][0]["type"] == "correction"


def test_invalid_type_rejected():
    result = subprocess.run(
        [sys.executable, SCRIPT,
         "--type", "invalid", "--note", "test"],
        capture_output=True, text=True,
    )
    assert result.returncode != 0


def test_corrupt_state_file_returns_error(state_dir, git_repo):
    branch = _get_branch(git_repo)
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")
    result = _run("test", cwd=git_repo)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_write_failure_returns_error(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    path = write_state(state_dir, branch, state)
    path.chmod(0o444)
    result = _run("test note", cwd=git_repo)
    path.chmod(0o644)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Failed to append" in data["message"]


# --- In-process tests ---


def test_append_note_creates_notes_array_if_missing(tmp_path):
    state_path = tmp_path / "state.json"
    state = {"branch": "test", "current_phase": "flow-start"}
    state_path.write_text(json.dumps(state))

    result = _mod.append_note(state_path, "flow-start", "correction", "test note")
    assert len(result["notes"]) == 1


def test_append_note_preserves_existing_notes(tmp_path):
    state_path = tmp_path / "state.json"
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["notes"] = [{"phase": "flow-start", "note": "existing"}]
    state_path.write_text(json.dumps(state))

    result = _mod.append_note(state_path, "flow-start", "learning", "new note")
    assert len(result["notes"]) == 2
    assert result["notes"][0]["note"] == "existing"
    assert result["notes"][1]["note"] == "new note"


# --- --branch flag (subprocess) ---


def test_cli_branch_flag_uses_specified_state_file(state_dir, git_repo):
    """--branch flag finds the state file for a different branch."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "other-feature", state)
    result = _run("Branch test note", cwd=git_repo, branch="other-feature")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["note_count"] == 1


def test_error_ambiguous_multiple_state_files(state_dir, git_repo):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
        })
        write_state(state_dir, name, state)
    result = _run("test note", cwd=git_repo)
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Multiple" in data["message"]
    assert sorted(data["candidates"]) == ["feat-a", "feat-b"]
