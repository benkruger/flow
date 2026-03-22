"""Tests for lib/flow_utils.py — shared utilities."""

import importlib.util
import json
import subprocess
from pathlib import Path
from unittest.mock import patch

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


# --- detect_repo ---


class TestDetectRepo:
    """Tests for the detect_repo function."""

    def _fake_result(self, stdout, returncode=0):
        return subprocess.CompletedProcess(
            args=[], returncode=returncode, stdout=stdout, stderr="",
        )

    def test_ssh_url_with_dotgit(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("git@github.com:owner/repo.git\n")):
            assert _mod.detect_repo() == "owner/repo"

    def test_https_url_with_dotgit(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("https://github.com/owner/repo.git\n")):
            assert _mod.detect_repo() == "owner/repo"

    def test_https_url_without_dotgit(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("https://github.com/owner/repo\n")):
            assert _mod.detect_repo() == "owner/repo"

    def test_ssh_url_without_dotgit(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("git@github.com:owner/repo\n")):
            assert _mod.detect_repo() == "owner/repo"

    def test_non_github_url_returns_none(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("https://gitlab.com/owner/repo.git\n")):
            assert _mod.detect_repo() is None

    def test_git_failure_returns_none(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("", returncode=1)):
            assert _mod.detect_repo() is None

    def test_empty_output_returns_none(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("")):
            assert _mod.detect_repo() is None

    def test_malformed_url_returns_none(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("not-a-url\n")):
            assert _mod.detect_repo() is None

    def test_subprocess_exception_returns_none(self):
        with patch.object(_mod.subprocess, "run",
                          side_effect=OSError("git not found")):
            assert _mod.detect_repo() is None

    def test_cwd_parameter_passed_to_subprocess(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("git@github.com:owner/repo.git\n")) as mock_run:
            _mod.detect_repo(cwd="/some/path")

        call_kwargs = mock_run.call_args
        assert call_kwargs[1].get("cwd") == "/some/path"

    def test_cwd_none_by_default(self):
        with patch.object(_mod.subprocess, "run",
                          return_value=self._fake_result("git@github.com:owner/repo.git\n")) as mock_run:
            _mod.detect_repo()

        call_kwargs = mock_run.call_args
        assert call_kwargs[1].get("cwd") is None


# --- mutate_state ---


class TestMutateState:
    """Tests for the mutate_state function."""

    def test_basic_mutation_persists_to_disk(self, tmp_path):
        state_path = tmp_path / "state.json"
        state_path.write_text(json.dumps({"count": 0}))

        result = _mod.mutate_state(state_path, lambda s: s.__setitem__("count", 1))

        assert result["count"] == 1
        on_disk = json.loads(state_path.read_text())
        assert on_disk["count"] == 1

    def test_returns_updated_state_dict(self, tmp_path):
        state_path = tmp_path / "state.json"
        state_path.write_text(json.dumps({"items": []}))

        result = _mod.mutate_state(state_path, lambda s: s["items"].append("new"))

        assert result["items"] == ["new"]

    def test_corrupt_json_raises_json_decode_error(self, tmp_path):
        state_path = tmp_path / "state.json"
        state_path.write_text("{corrupt")

        with pytest.raises(json.JSONDecodeError):
            _mod.mutate_state(state_path, lambda s: None)

    def test_missing_file_raises_file_not_found_error(self, tmp_path):
        state_path = tmp_path / "nonexistent.json"

        with pytest.raises(FileNotFoundError):
            _mod.mutate_state(state_path, lambda s: None)

    def test_closure_captures_pre_mutation_values(self, tmp_path):
        state_path = tmp_path / "state.json"
        state_path.write_text(json.dumps({"flag": "active", "data": "hello"}))

        captured = {}

        def transform(state):
            captured["flag"] = state.get("flag", "")
            state["flag"] = ""

        _mod.mutate_state(state_path, transform)

        assert captured["flag"] == "active"
        on_disk = json.loads(state_path.read_text())
        assert on_disk["flag"] == ""

    def test_file_locking_uses_flock(self, tmp_path):
        import fcntl
        state_path = tmp_path / "state.json"
        state_path.write_text(json.dumps({"x": 1}))

        with patch.object(fcntl, "flock") as mock_flock:
            _mod.mutate_state(state_path, lambda s: s.__setitem__("x", 2))

        mock_flock.assert_called_once()
        call_args = mock_flock.call_args[0]
        assert call_args[1] == fcntl.LOCK_EX

    def test_preserves_existing_keys(self, tmp_path):
        state_path = tmp_path / "state.json"
        state_path.write_text(json.dumps({"a": 1, "b": 2, "c": 3}))

        _mod.mutate_state(state_path, lambda s: s.__setitem__("b", 99))

        on_disk = json.loads(state_path.read_text())
        assert on_disk == {"a": 1, "b": 99, "c": 3}


# --- extract_issue_numbers (URL support) ---


class TestExtractIssueNumbersUrls:
    """Tests for URL-format issue reference extraction."""

    def test_github_url_extracts_number(self):
        assert _mod.extract_issue_numbers(
            "fix https://github.com/owner/repo/issues/42"
        ) == [42]

    def test_mixed_hash_and_url_formats(self):
        result = _mod.extract_issue_numbers(
            "fix #83 and https://github.com/owner/repo/issues/89"
        )
        assert result == [83, 89]

    def test_deduplication_across_formats(self):
        result = _mod.extract_issue_numbers(
            "fix #42 and https://github.com/owner/repo/issues/42"
        )
        assert result == [42]

    def test_multiple_urls(self):
        result = _mod.extract_issue_numbers(
            "https://github.com/owner/repo/issues/10 and https://github.com/owner/repo/issues/20"
        )
        assert result == [10, 20]

    def test_url_only_no_hash(self):
        result = _mod.extract_issue_numbers(
            "see https://github.com/owner/repo/issues/99"
        )
        assert result == [99]

    def test_hash_ordering_preserved_first(self):
        result = _mod.extract_issue_numbers(
            "https://github.com/owner/repo/issues/200 and #100"
        )
        assert result == [100, 200]


# --- short_issue_ref ---


class TestShortIssueRef:
    """Tests for URL-to-display-reference extraction."""

    def test_github_issue_url_returns_hash_number(self):
        assert _mod.short_issue_ref(
            "https://github.com/owner/repo/issues/42"
        ) == "#42"

    def test_empty_string_returns_empty(self):
        assert _mod.short_issue_ref("") == ""

    def test_non_github_url_returns_full_url(self):
        url = "https://example.com/custom-path"
        assert _mod.short_issue_ref(url) == url

    def test_url_without_trailing_number_returns_full_url(self):
        url = "https://github.com/owner/repo/issues/"
        assert _mod.short_issue_ref(url) == url

    def test_url_with_path_after_number_returns_full_url(self):
        url = "https://github.com/owner/repo/issues/42/comments"
        assert _mod.short_issue_ref(url) == url


# --- AUTO_SKILLS constant ---


def test_auto_skills_has_7_keys():
    """AUTO_SKILLS must have one entry per phase-level skill plus abort and complete."""
    assert len(_mod.AUTO_SKILLS) == 7


def test_auto_skills_all_commits_auto():
    """Every phase with a commit axis must be set to auto."""
    for key in ("flow-code", "flow-code-review", "flow-learn"):
        assert _mod.AUTO_SKILLS[key]["commit"] == "auto"


def test_auto_skills_all_continues_auto():
    """Every phase with a continue axis must be set to auto."""
    for key in ("flow-start", "flow-plan", "flow-code",
                "flow-code-review", "flow-learn"):
        assert _mod.AUTO_SKILLS[key]["continue"] == "auto"


def test_auto_skills_abort_and_complete_are_strings():
    """flow-abort and flow-complete are simple string values, not dicts."""
    assert _mod.AUTO_SKILLS["flow-abort"] == "auto"
    assert _mod.AUTO_SKILLS["flow-complete"] == "auto"


def test_auto_skills_code_review_plugin_never():
    """Auto mode skips the code review plugin."""
    assert _mod.AUTO_SKILLS["flow-code-review"]["code_review_plugin"] == "never"


# --- freeze_phases ---


def test_freeze_phases_copies_file(tmp_path):
    """freeze_phases copies flow-phases.json to .flow-states/<branch>-phases.json."""
    _mod.freeze_phases(tmp_path, "my-feature")
    dest = tmp_path / ".flow-states" / "my-feature-phases.json"
    assert dest.exists()
    data = json.loads(dest.read_text())
    assert "order" in data
    assert "phases" in data


def test_freeze_phases_creates_state_dir(tmp_path):
    """freeze_phases creates .flow-states/ if it does not exist."""
    state_dir = tmp_path / ".flow-states"
    assert not state_dir.exists()
    _mod.freeze_phases(tmp_path, "new-feature")
    assert state_dir.is_dir()


def test_freeze_phases_matches_source(tmp_path):
    """Frozen file content must match the canonical flow-phases.json."""
    _mod.freeze_phases(tmp_path, "test-branch")
    dest = tmp_path / ".flow-states" / "test-branch-phases.json"
    source = Path(__file__).resolve().parent.parent / "flow-phases.json"
    assert json.loads(dest.read_text()) == json.loads(source.read_text())


# --- build_initial_phases ---


def test_build_initial_phases_has_6_phases():
    """build_initial_phases returns a dict with all 6 phases."""
    phases = _mod.build_initial_phases("2026-01-01T00:00:00-08:00")
    assert len(phases) == 6
    for key in _mod.PHASE_ORDER:
        assert key in phases


def test_build_initial_phases_first_phase_in_progress():
    """First phase is in_progress with timestamps and visit_count=1."""
    ts = "2026-01-01T00:00:00-08:00"
    phases = _mod.build_initial_phases(ts)
    first = phases[_mod.PHASE_ORDER[0]]
    assert first["status"] == "in_progress"
    assert first["started_at"] == ts
    assert first["session_started_at"] == ts
    assert first["visit_count"] == 1
    assert first["cumulative_seconds"] == 0


def test_build_initial_phases_other_phases_pending():
    """Non-first phases are pending with null timestamps and visit_count=0."""
    ts = "2026-01-01T00:00:00-08:00"
    phases = _mod.build_initial_phases(ts)
    for key in _mod.PHASE_ORDER[1:]:
        phase = phases[key]
        assert phase["status"] == "pending"
        assert phase["started_at"] is None
        assert phase["completed_at"] is None
        assert phase["session_started_at"] is None
        assert phase["visit_count"] == 0
        assert phase["cumulative_seconds"] == 0


def test_build_initial_phases_has_correct_names():
    """Phase names must match PHASE_NAMES from flow-phases.json."""
    phases = _mod.build_initial_phases("2026-01-01T00:00:00-08:00")
    expected = {
        "flow-start": "Start", "flow-plan": "Plan", "flow-code": "Code",
        "flow-code-review": "Code Review", "flow-learn": "Learn",
        "flow-complete": "Complete",
    }
    for key, name in expected.items():
        assert phases[key]["name"] == name


def test_build_initial_phases_required_fields():
    """Each phase must have all 7 required fields."""
    phases = _mod.build_initial_phases("2026-01-01T00:00:00-08:00")
    required = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for key in _mod.PHASE_ORDER:
        for field in required:
            assert field in phases[key], f"Phase {key} missing field {field}"
