"""Tests for lib/add-issue.py — records filed issues in the state file."""

import importlib.util
import json

import pytest

from conftest import LIB_DIR, make_state, write_state


def _import_module():
    """Import add-issue.py for in-process unit tests."""
    spec = importlib.util.spec_from_file_location(
        "add_issue", LIB_DIR / "add-issue.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# --- In-process tests ---


def test_append_to_empty_issues_filed(tmp_path):
    """add_issue creates issues_filed array and appends first entry."""
    mod = _import_module()
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "complete",
        "flow-learn": "in_progress",
    })
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_issue(state_path, "Rule", "Add rule: use git -C", "https://github.com/test/test/issues/1", "flow-learn")

    assert len(updated["issues_filed"]) == 1
    issue = updated["issues_filed"][0]
    assert issue["label"] == "Rule"
    assert issue["title"] == "Add rule: use git -C"
    assert issue["url"] == "https://github.com/test/test/issues/1"
    assert issue["phase"] == "flow-learn"
    assert issue["phase_name"] == "Learn"
    assert "T" in issue["timestamp"]


def test_append_to_existing_issues_filed(tmp_path):
    """add_issue preserves existing entries and appends new one."""
    mod = _import_module()
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "complete",
        "flow-learn": "in_progress",
    })
    state["issues_filed"] = [{
        "label": "Flaky Test",
        "title": "Existing issue",
        "url": "https://github.com/test/test/issues/1",
        "phase": "flow-code",
        "phase_name": "Code",
        "timestamp": "2026-01-01T00:00:00-08:00",
    }]
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_issue(state_path, "Rule", "New rule", "https://github.com/test/test/issues/2", "flow-learn")

    assert len(updated["issues_filed"]) == 2
    assert updated["issues_filed"][0]["title"] == "Existing issue"
    assert updated["issues_filed"][1]["title"] == "New rule"


def test_creates_issues_filed_array_if_missing(tmp_path):
    """add_issue creates issues_filed key if state file lacks it."""
    mod = _import_module()
    state = {"branch": "test", "current_phase": "flow-learn"}
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    updated = mod.add_issue(state_path, "Flow", "Process gap", "https://github.com/test/flow/issues/5", "flow-learn")

    assert len(updated["issues_filed"]) == 1


def test_persists_to_disk(tmp_path):
    """add_issue writes the updated state back to disk."""
    mod = _import_module()
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    state_path = tmp_path / "state.json"
    state_path.write_text(json.dumps(state))

    mod.add_issue(state_path, "Flaky Test", "Test flakes", "https://github.com/test/test/issues/3", "flow-code")

    on_disk = json.loads(state_path.read_text())
    assert len(on_disk["issues_filed"]) == 1
    assert on_disk["issues_filed"][0]["label"] == "Flaky Test"


# --- CLI behavior (in-process main()) ---


def test_no_branch_returns_error(tmp_path, monkeypatch, capsys):
    """Running outside a git repo returns an error."""
    mod = _import_module()
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule", "--title", "test",
                                     "--url", "https://example.com", "--phase", "flow-learn"])
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "branch" in data["message"]


def test_no_state_file_returns_no_state(git_repo, monkeypatch, capsys):
    """Running with no state file returns no_state."""
    mod = _import_module()
    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule", "--title", "test",
                                     "--url", "https://example.com", "--phase", "flow-learn"])
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "no_state"


def test_cli_happy_path(state_dir, git_repo, branch, monkeypatch, capsys):
    """Full CLI round-trip: write state, run CLI, verify output."""
    mod = _import_module()
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "complete",
        "flow-learn": "in_progress",
    })
    path = write_state(state_dir, branch, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule",
                                     "--title", "Add rule: check imports",
                                     "--url", "https://github.com/test/test/issues/10",
                                     "--phase", "flow-learn"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["issue_count"] == 1

    on_disk = json.loads(path.read_text())
    assert len(on_disk["issues_filed"]) == 1


def test_cli_branch_flag(state_dir, git_repo, monkeypatch, capsys):
    """--branch flag finds the state file for a different branch."""
    mod = _import_module()
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "in_progress",
    })
    write_state(state_dir, "other-feature", state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Tech Debt",
                                     "--title", "Clean up handler",
                                     "--url", "https://github.com/test/test/issues/5",
                                     "--phase", "flow-code",
                                     "--branch", "other-feature"])
    mod.main()

    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert data["issue_count"] == 1


def test_corrupt_state_file_returns_error(state_dir, git_repo, branch, monkeypatch, capsys):
    """Corrupt state file returns a read error."""
    mod = _import_module()
    bad_file = state_dir / f"{branch}.json"
    bad_file.write_text("{bad json")

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule", "--title", "test",
                                     "--url", "https://example.com", "--phase", "flow-learn"])
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "Could not read" in data["message"]


def test_write_failure_returns_error(state_dir, git_repo, branch, monkeypatch, capsys):
    """Read-only state file returns a write error."""
    mod = _import_module()
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete",
        "flow-code": "complete", "flow-code-review": "complete",
        "flow-learn": "in_progress",
    })
    path = write_state(state_dir, branch, state)
    path.chmod(0o444)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule", "--title", "test",
                                     "--url", "https://example.com", "--phase", "flow-learn"])
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    path.chmod(0o644)
    assert exc_info.value.code == 1
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert "Failed to add" in data["message"]


def test_noop_ambiguous_multiple_state_files(state_dir, git_repo, monkeypatch, capsys):
    """Multiple state files with no exact match returns no_state (silent no-op)."""
    mod = _import_module()
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete",
            "flow-code": "in_progress",
        })
        write_state(state_dir, name, state)

    monkeypatch.chdir(git_repo)
    monkeypatch.setattr("sys.argv", ["add-issue", "--label", "Rule", "--title", "test",
                                     "--url", "https://example.com", "--phase", "flow-learn"])
    with pytest.raises(SystemExit) as exc_info:
        mod.main()
    assert exc_info.value.code == 0
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "no_state"
