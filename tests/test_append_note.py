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


def _run(note_text, note_type=None, cwd=None):
    """Run append-note.py via subprocess with --note and optional --type."""
    cmd = [sys.executable, SCRIPT, "--note", note_text]
    if note_type:
        cmd.extend(["--type", note_type])
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


# --- CLI behavior ---


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


def test_happy_path_returns_ok(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    write_state(state_dir, branch, state)
    result = _run("Always merge, never rebase", note_type="correction", cwd=git_repo)
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["note_count"] == 1


def test_note_written_to_state_file(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    path = write_state(state_dir, branch, state)
    _run("Always merge, never rebase", note_type="correction", cwd=git_repo)

    updated = json.loads(path.read_text())
    assert len(updated["notes"]) == 1
    note = updated["notes"][0]
    assert note["phase"] == 2
    assert note["phase_name"] == "Plan"
    assert note["type"] == "correction"
    assert note["note"] == "Always merge, never rebase"
    assert "T" in note["timestamp"]  # ISO 8601 format


def test_multiple_notes_append(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    write_state(state_dir, branch, state)
    _run("First note", note_type="correction", cwd=git_repo)
    _run("Second note", note_type="learning", cwd=git_repo)

    result = _run("Third note", note_type="correction", cwd=git_repo)
    data = json.loads(result.stdout)
    assert data["note_count"] == 3


def test_type_defaults_to_correction(state_dir, git_repo):
    branch = _get_branch(git_repo)
    state = make_state(current_phase=3, phase_statuses={1: "complete", 2: "complete", 3: "in_progress"})
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
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
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
    state = {"feature": "Test", "branch": "test", "current_phase": 1}
    state_path.write_text(json.dumps(state))

    result = _mod.append_note(state_path, 1, "correction", "test note")
    assert len(result["notes"]) == 1


def test_append_note_preserves_existing_notes(tmp_path):
    state_path = tmp_path / "state.json"
    state = make_state(current_phase=1, phase_statuses={1: "in_progress"})
    state["notes"] = [{"phase": 1, "note": "existing"}]
    state_path.write_text(json.dumps(state))

    result = _mod.append_note(state_path, 1, "learning", "new note")
    assert len(result["notes"]) == 2
    assert result["notes"][0]["note"] == "existing"
    assert result["notes"][1]["note"] == "new note"
