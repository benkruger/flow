"""Tests for lib/flow_utils.py — shared utilities."""

import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

from conftest import LIB_DIR, make_state

# Import flow_utils for in-process unit tests
_spec = importlib.util.spec_from_file_location(
    "flow_utils", LIB_DIR / "flow_utils.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


# --- format_time ---


def test_format_time_under_60_seconds():
    assert _mod.format_time(0) == "<1m"
    assert _mod.format_time(30) == "<1m"
    assert _mod.format_time(59) == "<1m"


def test_format_time_exactly_60_seconds():
    assert _mod.format_time(60) == "1m"


def test_format_time_minutes_only():
    assert _mod.format_time(120) == "2m"
    assert _mod.format_time(3599) == "59m"


def test_format_time_hours_and_minutes():
    assert _mod.format_time(3600) == "1h 0m"
    assert _mod.format_time(3660) == "1h 1m"
    assert _mod.format_time(7200) == "2h 0m"
    assert _mod.format_time(7380) == "2h 3m"


def test_format_time_large_values():
    assert _mod.format_time(36000) == "10h 0m"


def test_format_time_string_input():
    assert _mod.format_time("120") == "2m"
    assert _mod.format_time("3661") == "1h 1m"
    assert _mod.format_time("30") == "<1m"


def test_format_time_non_numeric_string():
    assert _mod.format_time("<1m") == "?"
    assert _mod.format_time("fast") == "?"


def test_format_time_none_input():
    assert _mod.format_time(None) == "?"


# --- project_root ---


def test_project_root_returns_path_in_git_repo(git_repo):
    result = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        capture_output=True, text=True, cwd=str(git_repo),
    )
    assert result.returncode == 0
    # project_root relies on cwd for subprocess — test the function directly
    # by running it in the git_repo context would require monkeypatching cwd


def test_project_root_falls_back_on_git_failure(monkeypatch):
    def _raise(*args, **kwargs):
        raise OSError("git not found")
    monkeypatch.setattr(subprocess, "run", _raise)
    assert _mod.project_root() == Path(".")


# --- current_branch ---


def test_current_branch_returns_none_on_git_failure(monkeypatch):
    def _raise(*args, **kwargs):
        raise OSError("git not found")
    monkeypatch.setattr(subprocess, "run", _raise)
    assert _mod.current_branch() is None


def test_current_branch_returns_none_for_empty_string(monkeypatch):
    class FakeResult:
        stdout = ""
        returncode = 0
    monkeypatch.setattr(
        subprocess, "run",
        lambda *args, **kwargs: FakeResult(),
    )
    assert _mod.current_branch() is None


# --- find_state_files ---


def test_find_state_files_exact_match(tmp_path):
    """Exact branch match returns single-item list."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    (state_dir / "my-feature.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "my-feature")
    assert len(results) == 1
    path, data, branch_name = results[0]
    assert path == state_dir / "my-feature.json"
    assert data["branch"] == "test-feature"
    assert branch_name == "my-feature"


def test_find_state_files_no_state_dir(tmp_path):
    """No .flow-states directory returns empty list."""
    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


def test_find_state_files_empty_state_dir(tmp_path):
    """Empty .flow-states directory returns empty list."""
    (tmp_path / ".flow-states").mkdir()
    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


def test_find_state_files_fallback_single(tmp_path):
    """Single non-matching file found via fallback scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase="flow-code")
    (state_dir / "feature-xyz.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 1
    path, data, branch_name = results[0]
    assert branch_name == "feature-xyz"


def test_find_state_files_fallback_multiple(tmp_path):
    """Multiple non-matching files returned as multi-item list."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    for name in ["feature-a", "feature-b", "feature-c"]:
        state = make_state(current_phase="flow-plan")
        (state_dir / f"{name}.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 3
    branches = [r[2] for r in results]
    assert "feature-a" in branches
    assert "feature-b" in branches
    assert "feature-c" in branches


def test_find_state_files_corrupt_skipped_in_scan(tmp_path):
    """Corrupt files are skipped during fallback scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "bad.json").write_text("{corrupt")
    state = make_state(current_phase="flow-plan")
    (state_dir / "good.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 1
    assert results[0][2] == "good"


def test_find_state_files_corrupt_exact_match_no_fallthrough(tmp_path):
    """Corrupt exact match returns empty — does not fall through to scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "main.json").write_text("{corrupt")
    state = make_state(current_phase="flow-plan")
    (state_dir / "other-feature.json").write_text(json.dumps(state))

    results = _mod.find_state_files(tmp_path, "main")
    assert results == []


# --- load_phase_config ---


def test_load_phase_config_returns_four_values(tmp_path):
    """load_phase_config returns (order, names, numbers, commands) tuple."""
    source = Path(__file__).resolve().parent.parent / "flow-phases.json"
    result = _mod.load_phase_config(source)
    assert len(result) == 4
    order, names, numbers, commands = result
    assert isinstance(order, list)
    assert isinstance(names, dict)
    assert isinstance(numbers, dict)
    assert isinstance(commands, dict)


def test_load_phase_config_matches_module_constants():
    """load_phase_config from source must match module-level constants."""
    source = Path(__file__).resolve().parent.parent / "flow-phases.json"
    order, names, numbers, commands = _mod.load_phase_config(source)
    assert order == _mod.PHASE_ORDER
    assert names == _mod.PHASE_NAMES
    assert numbers == _mod.PHASE_NUMBER
    assert commands == _mod.COMMANDS


def test_load_phase_config_from_frozen_file(tmp_path):
    """load_phase_config works with a frozen copy of flow-phases.json."""
    source = Path(__file__).resolve().parent.parent / "flow-phases.json"
    frozen = tmp_path / "test-feature-phases.json"
    frozen.write_text(source.read_text())
    order, names, numbers, commands = _mod.load_phase_config(frozen)
    assert order == _mod.PHASE_ORDER
    assert names == _mod.PHASE_NAMES


def test_load_phase_config_custom_content(tmp_path):
    """load_phase_config correctly parses a minimal phases file."""
    custom = {
        "order": ["alpha", "beta"],
        "phases": {
            "alpha": {"name": "Alpha", "command": "/test:alpha", "can_return_to": []},
            "beta": {"name": "Beta", "command": "/test:beta", "can_return_to": ["alpha"]},
        },
    }
    path = tmp_path / "phases.json"
    path.write_text(json.dumps(custom))
    order, names, numbers, commands = _mod.load_phase_config(path)
    assert order == ["alpha", "beta"]
    assert names == {"alpha": "Alpha", "beta": "Beta"}
    assert numbers == {"alpha": 1, "beta": 2}
    assert commands == {"alpha": "/test:alpha", "beta": "/test:beta"}


def test_find_state_files_skips_frozen_phases_files(tmp_path):
    """Fallback scan should not include -phases.json files."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state = make_state(current_phase="flow-plan")
    (state_dir / "feature-x.json").write_text(json.dumps(state))
    (state_dir / "feature-x-phases.json").write_text(
        json.dumps({"order": [], "phases": {}})
    )

    results = _mod.find_state_files(tmp_path, "main")
    assert len(results) == 1
    assert results[0][2] == "feature-x"


def test_find_state_files_exact_match_priority(tmp_path):
    """Exact match takes priority — other files are not returned."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    state_exact = make_state(current_phase="flow-plan")
    (state_dir / "my-branch.json").write_text(json.dumps(state_exact))
    state_other = make_state(current_phase="flow-code")
    (state_dir / "other-branch.json").write_text(json.dumps(state_other))

    results = _mod.find_state_files(tmp_path, "my-branch")
    assert len(results) == 1
    assert results[0][1]["branch"] == "test-feature"
    assert results[0][2] == "my-branch"


# --- resolve_branch ---


def test_resolve_branch_override_wins(monkeypatch):
    """Override parameter is returned immediately regardless of git/state."""
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: Path("/nonexistent"))
    branch, candidates = _mod.resolve_branch("explicit-branch")
    assert branch == "explicit-branch"
    assert candidates == []


def test_resolve_branch_exact_match(monkeypatch, tmp_path):
    """Current branch matching a state file returns that branch."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "feat-x.json").write_text(json.dumps(make_state()))
    monkeypatch.setattr(_mod, "current_branch", lambda: "feat-x")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "feat-x"
    assert candidates == []


def test_resolve_branch_single_file_fallback(monkeypatch, tmp_path):
    """On main with one state file, auto-resolves to that feature branch."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "feat-x.json").write_text(json.dumps(make_state()))
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "feat-x"
    assert candidates == []


def test_resolve_branch_ambiguous_multiple_files(monkeypatch, tmp_path):
    """Multiple state files with no exact match returns None with candidates."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "feat-a.json").write_text(json.dumps(make_state()))
    (state_dir / "feat-b.json").write_text(json.dumps(make_state()))
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch is None
    assert sorted(candidates) == ["feat-a", "feat-b"]


def test_resolve_branch_no_state_dir(monkeypatch, tmp_path):
    """No .flow-states directory returns current_branch() result."""
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "main"
    assert candidates == []


def test_resolve_branch_skips_phases_json(monkeypatch, tmp_path):
    """Frozen phases files (*-phases.json) are ignored during scan."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "feat-x.json").write_text(json.dumps(make_state()))
    (state_dir / "feat-x-phases.json").write_text(
        json.dumps({"order": [], "phases": {}})
    )
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "feat-x"
    assert candidates == []


def test_resolve_branch_empty_state_dir(monkeypatch, tmp_path):
    """Empty .flow-states directory returns current_branch() result."""
    (tmp_path / ".flow-states").mkdir()
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "main"
    assert candidates == []


def test_resolve_branch_skips_corrupt_files(monkeypatch, tmp_path):
    """Corrupt JSON files are skipped, valid ones still found."""
    state_dir = tmp_path / ".flow-states"
    state_dir.mkdir()
    (state_dir / "bad.json").write_text("{corrupt")
    (state_dir / "good.json").write_text(json.dumps(make_state()))
    monkeypatch.setattr(_mod, "current_branch", lambda: "main")
    monkeypatch.setattr(_mod, "project_root", lambda: tmp_path)
    branch, candidates = _mod.resolve_branch()
    assert branch == "good"
    assert candidates == []


# --- derive_feature ---


def test_derive_feature_from_branch():
    """Hyphenated branch name produces title-cased feature name."""
    assert _mod.derive_feature("app-payment-webhooks") == "App Payment Webhooks"


def test_derive_feature_single_word():
    """Single-word branch name produces capitalized feature name."""
    assert _mod.derive_feature("bugfix") == "Bugfix"


def test_derive_feature_already_capitalized():
    """Already-capitalized words are handled correctly."""
    assert _mod.derive_feature("fix-login-timeout") == "Fix Login Timeout"


# --- derive_worktree ---


def test_derive_worktree_from_branch():
    """Branch name produces .worktrees/ prefixed path."""
    assert _mod.derive_worktree("app-payment-webhooks") == ".worktrees/app-payment-webhooks"
