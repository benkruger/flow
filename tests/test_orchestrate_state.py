"""Tests for lib/orchestrate-state.py — manages orchestration queue state."""

import json
from unittest.mock import patch

from conftest import import_lib


def _sample_queue():
    """Build a sample issue queue for tests."""
    return [
        {"issue_number": 42, "title": "Add PDF export"},
        {"issue_number": 43, "title": "Fix login timeout"},
        {"issue_number": 44, "title": "Refactor auth middleware"},
    ]


def _write_queue_file(tmp_path, issues):
    """Write a queue file for CLI tests."""
    queue_file = tmp_path / "queue.json"
    queue_file.write_text(json.dumps(issues))
    return queue_file


# --- In-process tests: create ---


def test_create_state(tmp_path):
    """Creates orchestrate.json with queue, started_at, no completed_at."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue = _sample_queue()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        result = mod.create_state(queue, str(state_dir))

    assert result["status"] == "ok"

    state_path = state_dir / "orchestrate.json"
    assert state_path.exists()
    state = json.loads(state_path.read_text())

    assert state["started_at"] == "2026-03-20T22:00:00-07:00"
    assert state["completed_at"] is None
    assert state["current_index"] is None
    assert len(state["queue"]) == 3
    assert state["queue"][0]["issue_number"] == 42
    assert state["queue"][0]["status"] == "pending"
    assert state["queue"][0]["started_at"] is None
    assert state["queue"][0]["completed_at"] is None
    assert state["queue"][0]["outcome"] is None
    assert state["queue"][0]["pr_url"] is None
    assert state["queue"][0]["branch"] is None
    assert state["queue"][0]["reason"] is None


def test_create_state_empty_queue(tmp_path):
    """Creates state with empty queue when no decomposed issues found."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        result = mod.create_state([], str(state_dir))

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["queue"] == []


def test_create_state_already_exists_in_progress(tmp_path):
    """Errors when orchestrate.json exists without completed_at."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    existing = {
        "started_at": "2026-03-20T20:00:00-07:00",
        "completed_at": None,
        "queue": [],
        "current_index": 0,
    }
    (state_dir / "orchestrate.json").write_text(json.dumps(existing))

    result = mod.create_state(_sample_queue(), str(state_dir))

    assert result["status"] == "error"
    assert "already in progress" in result["message"]


def test_create_state_overwrites_completed(tmp_path):
    """Overwrites existing state that has completed_at set."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    existing = {
        "started_at": "2026-03-20T20:00:00-07:00",
        "completed_at": "2026-03-20T21:00:00-07:00",
        "queue": [],
        "current_index": None,
    }
    (state_dir / "orchestrate.json").write_text(json.dumps(existing))

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        result = mod.create_state(_sample_queue(), str(state_dir))

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["started_at"] == "2026-03-20T22:00:00-07:00"
    assert len(state["queue"]) == 3


def test_create_state_creates_directory(tmp_path):
    """Creates .flow-states/ directory if it does not exist."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        result = mod.create_state(_sample_queue(), str(state_dir))

    assert result["status"] == "ok"
    assert (state_dir / "orchestrate.json").exists()


# --- In-process tests: start_issue ---


def test_start_issue(tmp_path):
    """Sets current_index and marks issue as in_progress with started_at."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    with patch.object(mod, "now", return_value="2026-03-20T22:05:00-07:00"):
        result = mod.start_issue(state_path, 0)

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["current_index"] == 0
    assert state["queue"][0]["status"] == "in_progress"
    assert state["queue"][0]["started_at"] == "2026-03-20T22:05:00-07:00"


def test_start_issue_out_of_range(tmp_path):
    """Errors when index is out of range."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    result = mod.start_issue(state_path, 10)

    assert result["status"] == "error"
    assert "out of range" in result["message"]


# --- In-process tests: record_outcome ---


def test_record_outcome_completed(tmp_path):
    """Marks issue as completed with PR URL and branch."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    with patch.object(mod, "now", return_value="2026-03-20T22:05:00-07:00"):
        mod.start_issue(state_path, 0)

    with patch.object(mod, "now", return_value="2026-03-20T23:00:00-07:00"):
        result = mod.record_outcome(
            state_path,
            0,
            "completed",
            pr_url="https://github.com/test/test/pull/100",
            branch="add-pdf-export",
        )

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["queue"][0]["status"] == "completed"
    assert state["queue"][0]["outcome"] == "completed"
    assert state["queue"][0]["completed_at"] == "2026-03-20T23:00:00-07:00"
    assert state["queue"][0]["pr_url"] == "https://github.com/test/test/pull/100"
    assert state["queue"][0]["branch"] == "add-pdf-export"


def test_record_outcome_failed(tmp_path):
    """Marks issue as failed with reason."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    with patch.object(mod, "now", return_value="2026-03-20T22:05:00-07:00"):
        mod.start_issue(state_path, 1)

    with patch.object(mod, "now", return_value="2026-03-20T23:00:00-07:00"):
        result = mod.record_outcome(
            state_path,
            1,
            "failed",
            reason="CI failed after 3 attempts",
        )

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["queue"][1]["status"] == "failed"
    assert state["queue"][1]["outcome"] == "failed"
    assert state["queue"][1]["reason"] == "CI failed after 3 attempts"


def test_record_outcome_out_of_range(tmp_path):
    """Errors when index is out of range."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    result = mod.record_outcome(state_path, 10, "completed")

    assert result["status"] == "error"
    assert "out of range" in result["message"]


# --- In-process tests: complete ---


def test_complete(tmp_path):
    """Sets completed_at on the orchestrate state."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    with patch.object(mod, "now", return_value="2026-03-21T06:00:00-07:00"):
        result = mod.complete_orchestration(state_path)

    assert result["status"] == "ok"
    state = json.loads((state_dir / "orchestrate.json").read_text())
    assert state["completed_at"] == "2026-03-21T06:00:00-07:00"


# --- In-process tests: read ---


def test_read_state(tmp_path):
    """Returns current state as JSON."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")
    result = mod.read_state(state_path)

    assert result["status"] == "ok"
    assert result["state"]["started_at"] == "2026-03-20T22:00:00-07:00"
    assert len(result["state"]["queue"]) == 3


def test_read_state_missing(tmp_path):
    """Errors when no state file exists."""
    mod = import_lib("orchestrate-state.py")
    state_path = str(tmp_path / ".flow-states" / "orchestrate.json")

    result = mod.read_state(state_path)

    assert result["status"] == "error"
    assert "not found" in result["message"]


# --- In-process tests: next_issue ---


def test_next_issue(tmp_path):
    """Returns the next pending issue index."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")

    result = mod.next_issue(state_path)
    assert result["status"] == "ok"
    assert result["index"] == 0
    assert result["issue_number"] == 42
    assert result["title"] == "Add PDF export"


def test_next_issue_skips_completed(tmp_path):
    """Skips completed and failed issues, returns next pending."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state(_sample_queue(), str(state_dir))

    state_path = str(state_dir / "orchestrate.json")

    with patch.object(mod, "now", return_value="2026-03-20T22:05:00-07:00"):
        mod.start_issue(state_path, 0)
        mod.record_outcome(state_path, 0, "completed")

    result = mod.next_issue(state_path)
    assert result["status"] == "ok"
    assert result["index"] == 1
    assert result["issue_number"] == 43


def test_next_issue_all_done(tmp_path):
    """Returns done status when all issues processed."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()

    with patch.object(mod, "now", return_value="2026-03-20T22:00:00-07:00"):
        mod.create_state([{"issue_number": 42, "title": "One issue"}], str(state_dir))

    state_path = str(state_dir / "orchestrate.json")

    with patch.object(mod, "now", return_value="2026-03-20T22:05:00-07:00"):
        mod.start_issue(state_path, 0)
        mod.record_outcome(state_path, 0, "completed")

    result = mod.next_issue(state_path)
    assert result["status"] == "done"


# --- CLI integration tests ---


def test_cli_create(tmp_path, monkeypatch, capsys):
    """CLI --create with --queue-file creates state."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert (state_dir / "orchestrate.json").exists()


def test_cli_start_issue(tmp_path, monkeypatch, capsys):
    """CLI --start-issue marks queue item as in_progress."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--start-issue",
            "0",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


def test_cli_record_outcome(tmp_path, monkeypatch, capsys):
    """CLI --record-outcome records result for queue item."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--start-issue",
            "0",
            "--state-file",
            state_path,
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--record-outcome",
            "0",
            "--outcome",
            "completed",
            "--pr-url",
            "https://github.com/test/test/pull/100",
            "--branch",
            "add-pdf-export",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


def test_cli_complete(tmp_path, monkeypatch, capsys):
    """CLI --complete sets completed_at."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--complete",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"


def test_cli_read(tmp_path, monkeypatch, capsys):
    """CLI --read returns current state."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--read",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert "state" in data


def test_cli_next(tmp_path, monkeypatch, capsys):
    """CLI --next returns next pending issue."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--next",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["index"] == 0


def test_cli_read_missing_state(tmp_path, monkeypatch, capsys):
    """CLI --read with nonexistent file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--read",
            "--state-file",
            str(tmp_path / "missing.json"),
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


# --- In-process tests: missing state file error paths ---


def test_start_issue_missing_state(tmp_path):
    """start_issue returns error when state file does not exist."""
    mod = import_lib("orchestrate-state.py")
    result = mod.start_issue(str(tmp_path / "missing.json"), 0)
    assert result["status"] == "error"
    assert "not found" in result["message"]


def test_record_outcome_missing_state(tmp_path):
    """record_outcome returns error when state file does not exist."""
    mod = import_lib("orchestrate-state.py")
    result = mod.record_outcome(str(tmp_path / "missing.json"), 0, "completed")
    assert result["status"] == "error"
    assert "not found" in result["message"]


def test_complete_missing_state(tmp_path):
    """complete_orchestration returns error when state file does not exist."""
    mod = import_lib("orchestrate-state.py")
    result = mod.complete_orchestration(str(tmp_path / "missing.json"))
    assert result["status"] == "error"
    assert "not found" in result["message"]


def test_next_issue_missing_state(tmp_path):
    """next_issue returns error when state file does not exist."""
    mod = import_lib("orchestrate-state.py")
    result = mod.next_issue(str(tmp_path / "missing.json"))
    assert result["status"] == "error"
    assert "not found" in result["message"]


# --- CLI missing argument error paths ---


def test_cli_create_missing_queue_file(tmp_path, monkeypatch, capsys):
    """CLI --create without --queue-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr("sys.argv", ["orchestrate-state", "--create"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--queue-file" in data["message"]


def test_cli_start_issue_missing_state_file(monkeypatch, capsys):
    """CLI --start-issue without --state-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr("sys.argv", ["orchestrate-state", "--start-issue", "0"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--state-file" in data["message"]


def test_cli_record_outcome_missing_state_file(monkeypatch, capsys):
    """CLI --record-outcome without --state-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--record-outcome",
            "0",
            "--outcome",
            "completed",
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--state-file" in data["message"]


def test_cli_record_outcome_missing_outcome(tmp_path, monkeypatch, capsys):
    """CLI --record-outcome without --outcome returns error."""
    mod = import_lib("orchestrate-state.py")
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    queue_file = _write_queue_file(tmp_path, _sample_queue())

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--create",
            "--queue-file",
            str(queue_file),
            "--state-dir",
            str(state_dir),
        ],
    )
    mod.main()
    capsys.readouterr()  # discard setup output

    state_path = str(state_dir / "orchestrate.json")
    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--record-outcome",
            "0",
            "--state-file",
            state_path,
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--outcome" in data["message"]


def test_cli_complete_missing_state_file(monkeypatch, capsys):
    """CLI --complete without --state-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr("sys.argv", ["orchestrate-state", "--complete"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--state-file" in data["message"]


def test_cli_read_missing_state_file(monkeypatch, capsys):
    """CLI --read without --state-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr("sys.argv", ["orchestrate-state", "--read"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--state-file" in data["message"]


def test_cli_next_missing_state_file(monkeypatch, capsys):
    """CLI --next without --state-file returns error."""
    mod = import_lib("orchestrate-state.py")
    monkeypatch.setattr("sys.argv", ["orchestrate-state", "--next"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "--state-file" in data["message"]


def test_cli_exception_handling(tmp_path, monkeypatch, capsys):
    """CLI handles unexpected exceptions gracefully."""
    mod = import_lib("orchestrate-state.py")
    bad_file = tmp_path / "bad.json"
    bad_file.write_text("{corrupt json")

    monkeypatch.setattr(
        "sys.argv",
        [
            "orchestrate-state",
            "--start-issue",
            "0",
            "--state-file",
            str(bad_file),
        ],
    )
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
