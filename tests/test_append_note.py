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


def _run(state_path, phase, note_type, note_text):
    """Run append-note.py via subprocess."""
    result = subprocess.run(
        [sys.executable, SCRIPT, str(state_path),
         "--phase", str(phase),
         "--type", note_type,
         "--note", note_text],
        capture_output=True, text=True,
    )
    return result


# --- CLI behavior ---


def test_missing_args_returns_error():
    result = subprocess.run(
        [sys.executable, SCRIPT],
        capture_output=True, text=True,
    )
    assert result.returncode != 0


def test_nonexistent_state_returns_error(tmp_path):
    result = _run(tmp_path / "missing.json", 1, "correction", "test note")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "not found" in data["message"]


def test_happy_path_returns_ok(state_dir):
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    path = write_state(state_dir, "test-feature", state)
    result = _run(path, 2, "correction", "Always merge, never rebase")
    assert result.returncode == 0
    data = json.loads(result.stdout)
    assert data["status"] == "ok"
    assert data["note_count"] == 1


def test_note_written_to_state_file(state_dir):
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    path = write_state(state_dir, "test-feature", state)
    _run(path, 2, "correction", "Always merge, never rebase")

    updated = json.loads(path.read_text())
    assert len(updated["notes"]) == 1
    note = updated["notes"][0]
    assert note["phase"] == 2
    assert note["phase_name"] == "Research"
    assert note["type"] == "correction"
    assert note["note"] == "Always merge, never rebase"
    assert note["timestamp"].endswith("Z")


def test_multiple_notes_append(state_dir):
    state = make_state(current_phase=2, phase_statuses={1: "complete", 2: "in_progress"})
    path = write_state(state_dir, "test-feature", state)
    _run(path, 2, "correction", "First note")
    _run(path, 2, "learning", "Second note")

    result = _run(path, 3, "correction", "Third note")
    data = json.loads(result.stdout)
    assert data["note_count"] == 3


def test_invalid_type_rejected():
    result = subprocess.run(
        [sys.executable, SCRIPT, "/tmp/fake.json",
         "--phase", "1", "--type", "invalid", "--note", "test"],
        capture_output=True, text=True,
    )
    assert result.returncode != 0


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


def test_corrupt_state_file_returns_error(tmp_path):
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("{bad json")
    result = _run(bad_file, 1, "correction", "test")
    assert result.returncode == 1
    data = json.loads(result.stdout)
    assert data["status"] == "error"
    assert "Failed to append" in data["message"]
