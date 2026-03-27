"""Tests for lib/append-note.py — the note appender."""

import importlib.util
import json
import subprocess

import pytest
from conftest import LIB_DIR, make_state, write_state

# Import append-note.py for in-process unit tests
_spec = importlib.util.spec_from_file_location("append_note", LIB_DIR / "append-note.py")
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _get_branch(git_repo):
    """Get the current branch name from a git repo."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        capture_output=True,
        text=True,
        cwd=str(git_repo),
    )
    return result.stdout.strip()


# --- CLI behavior (in-process main()) ---


def test_no_branch_returns_error(tmp_path, monkeypatch, capsys):
    """Running outside a git repo (no branch) returns an error."""
    monkeypatch.delenv("FLOW_SIMULATE_BRANCH", raising=False)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "test note"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "branch" in data["message"]


def test_no_state_file_returns_no_state(git_repo, monkeypatch, capsys):
    """Running with no state file returns no_state."""
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "test note"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
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


def test_type_defaults_to_correction(state_dir, git_repo, monkeypatch, capsys):
    """Type defaults to correction when --type is not specified."""
    branch = _get_branch(git_repo)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    path = write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "Default type note"])
    _mod.main()

    updated = json.loads(path.read_text())
    assert updated["notes"][0]["type"] == "correction"


def test_invalid_type_rejected(monkeypatch):
    """Invalid --type is rejected by argparse."""
    monkeypatch.setattr("sys.argv", ["append-note", "--type", "invalid", "--note", "test"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code != 0


def test_corrupt_state_file_returns_error(state_dir, git_repo, monkeypatch, capsys):
    """Corrupt state file returns a read error."""
    branch = _get_branch(git_repo)
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "test"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_write_failure_returns_error(state_dir, git_repo, monkeypatch, capsys):
    """Read-only state file returns a write error."""
    branch = _get_branch(git_repo)
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    path = write_state(state_dir, branch, state)
    path.chmod(0o444)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "test note"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    path.chmod(0o644)
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
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


# --- --branch flag (in-process main()) ---


def test_cli_branch_flag_uses_specified_state_file(state_dir, git_repo, monkeypatch, capsys):
    """--branch flag finds the state file for a different branch."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "in_progress",
        },
    )
    write_state(state_dir, "other-feature", state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "Branch test note", "--branch", "other-feature"])
    _mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["note_count"] == 1


def test_error_ambiguous_multiple_state_files(state_dir, git_repo, monkeypatch, capsys):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(
            current_phase="flow-code",
            phase_statuses={
                "flow-start": "complete",
                "flow-plan": "complete",
                "flow-code": "in_progress",
            },
        )
        write_state(state_dir, name, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["append-note", "--note", "test note"])
    with pytest.raises(SystemExit) as exc_info:
        _mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "Multiple" in data["message"]
    assert sorted(data["candidates"]) == ["feat-a", "feat-b"]
