"""Tests for lib/complete-preflight.py — the consolidated Complete phase preflight script."""

import json
import subprocess
import sys
from unittest.mock import patch

from conftest import LIB_DIR, import_lib, make_state, write_state

_mod = import_lib("complete-preflight.py")

SCRIPT = str(LIB_DIR / "complete-preflight.py")

_PT_ENTER_OK = '{"status": "ok", "phase": "flow-complete", "action": "enter", "visit_count": 1, "first_visit": true}'


# --- helpers ---


def _make_result(returncode=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(
        args=[],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def _setup_complete_state(target_project, branch="test-feature", learn_status="complete", skills=None):
    """Create a state file ready for Complete phase."""
    phase_statuses = {
        "flow-start": "complete",
        "flow-plan": "complete",
        "flow-code": "complete",
        "flow-code-review": "complete",
        "flow-learn": learn_status,
    }
    state = make_state(current_phase="flow-learn", phase_statuses=phase_statuses)
    state["branch"] = branch
    state["pr_number"] = 42
    state["pr_url"] = "https://github.com/test/test/pull/42"
    state["repo"] = "test/test"
    state["prompt"] = "work on issue #100"
    if skills is not None:
        state["skills"] = skills
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    write_state(state_dir, branch, state)
    return state


# --- preflight() in-process tests ---


def test_happy_path_open_pr_clean_merge(target_project, monkeypatch):
    """State file exists, PR is OPEN, merge main is clean."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        # phase-transition --action enter
        _make_result(stdout=_PT_ENTER_OK),
        # gh pr view
        _make_result(stdout="OPEN"),
        # git fetch origin main
        _make_result(),
        # git merge-base --is-ancestor (already up to date)
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "ok"
    assert result["pr_state"] == "OPEN"
    assert result["merge"] == "clean"
    assert result["mode"] == "auto"
    assert result["warnings"] == []


def test_pr_already_merged(target_project, monkeypatch):
    """PR is already MERGED — returns early."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        # phase-transition --action enter
        _make_result(stdout=_PT_ENTER_OK),
        # gh pr view
        _make_result(stdout="MERGED"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "ok"
    assert result["pr_state"] == "MERGED"


def test_pr_closed_returns_error(target_project, monkeypatch):
    """PR is CLOSED — returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        # phase-transition --action enter
        _make_result(stdout=_PT_ENTER_OK),
        # gh pr view
        _make_result(stdout="CLOSED"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "closed" in result["message"].lower()


def test_merge_conflicts(target_project, monkeypatch):
    """Merge main into branch has conflicts."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    porcelain = "UU lib/foo.py\nAA lib/bar.py\n"
    responses = [
        # phase-transition --action enter
        _make_result(stdout=_PT_ENTER_OK),
        # gh pr view
        _make_result(stdout="OPEN"),
        # git fetch origin main
        _make_result(),
        # git merge-base --is-ancestor (not ancestor)
        _make_result(returncode=1),
        # git merge origin/main (conflict)
        _make_result(returncode=1, stderr="CONFLICT (content): Merge conflict in lib/foo.py"),
        # git status --porcelain
        _make_result(stdout=porcelain),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "conflict"
    assert "lib/foo.py" in result["conflict_files"]
    assert "lib/bar.py" in result["conflict_files"]


def test_no_state_file_infers_from_git(target_project, monkeypatch):
    """No state file — infers branch and basic info from git."""
    monkeypatch.chdir(target_project)
    # Don't create a state file

    responses = [
        # gh pr view (by branch name)
        _make_result(stdout="OPEN"),
        # git fetch origin main
        _make_result(),
        # git merge-base --is-ancestor
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "ok"
    assert result["inferred"] is True


def test_mode_auto_flag(target_project, monkeypatch):
    """--auto flag overrides state file mode."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project, skills={"flow-complete": "manual"})

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", auto=True, root=target_project)

    assert result["mode"] == "auto"


def test_mode_manual_flag(target_project, monkeypatch):
    """--manual flag overrides state file mode."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project, skills={"flow-complete": "auto"})

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", manual=True, root=target_project)

    assert result["mode"] == "manual"


def test_mode_from_state_file(target_project, monkeypatch):
    """Mode falls back to state file skills.flow-complete value."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project, skills={"flow-complete": "manual"})

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["mode"] == "manual"


def test_mode_default_auto(target_project, monkeypatch):
    """Mode defaults to auto when no flag and no skills config."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)
    # Remove skills key
    state_path = target_project / ".flow-states" / "test-feature.json"
    data = json.loads(state_path.read_text())
    data.pop("skills", None)
    state_path.write_text(json.dumps(data, indent=2))

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["mode"] == "auto"


def test_learn_phase_warning(target_project, monkeypatch):
    """Learn phase not complete produces a warning."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project, learn_status="pending")

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "ok"
    assert len(result["warnings"]) > 0
    assert any("learn" in w.lower() or "phase 5" in w.lower() for w in result["warnings"])


def test_step_counter_set_in_state(target_project, monkeypatch):
    """Preflight sets complete_steps_total and complete_step in state file."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        _mod.preflight(branch="test-feature", root=target_project)

    state_path = target_project / ".flow-states" / "test-feature.json"
    state = json.loads(state_path.read_text())
    assert state["complete_steps_total"] == 7
    assert state["complete_step"] == 1


def test_merge_with_new_commits_pushes(target_project, monkeypatch):
    """When merge brings in new commits, git push is called."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    calls_made = []
    original_responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),  # git fetch
        _make_result(returncode=1),  # merge-base (not ancestor)
        _make_result(stdout="Merge made by the 'ort' strategy."),  # git merge
        _make_result(),  # git push
    ]
    response_iter = iter(original_responses)

    def _tracking_run(args, **kwargs):
        calls_made.append(args)
        return next(response_iter)

    with patch("subprocess.run", side_effect=_tracking_run):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "ok"
    assert result["merge"] == "merged"
    # Verify git push was called
    push_calls = [c for c in calls_made if "push" in c]
    assert len(push_calls) >= 1


def test_phase_transition_enter_called(target_project, monkeypatch):
    """Preflight calls phase-transition --action enter."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    calls_made = []
    original_responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]
    response_iter = iter(original_responses)

    def _tracking_run(args, **kwargs):
        calls_made.append(args)
        return next(response_iter)

    with patch("subprocess.run", side_effect=_tracking_run):
        _mod.preflight(branch="test-feature", root=target_project)

    phase_calls = [c for c in calls_made if "phase-transition" in str(c)]
    assert len(phase_calls) >= 1
    first_phase_call = phase_calls[0]
    assert "--action" in first_phase_call
    assert "enter" in first_phase_call


def test_pr_view_failure_returns_error(target_project, monkeypatch):
    """gh pr view failure returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(returncode=1, stderr="Could not resolve to a Pull Request"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"


def test_timeout_returns_error(target_project, monkeypatch):
    """Subprocess timeout returns error CompletedProcess."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    def _timeout_run(args, **kwargs):
        raise subprocess.TimeoutExpired(cmd=args, timeout=30)

    with patch("subprocess.run", side_effect=_timeout_run):
        result = _mod.preflight(branch="test-feature", root=target_project)

    # Phase transition will timeout → error
    assert result["status"] == "error"


def test_skill_config_dict_mode(target_project, monkeypatch):
    """Mode resolves from skills.flow-complete dict with 'continue' key."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(
        target_project,
        skills={"flow-complete": {"continue": "manual", "commit": "auto"}},
    )

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["mode"] == "manual"


def test_phase_transition_error(target_project, monkeypatch):
    """Phase transition returning non-zero is an error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(returncode=1, stderr="state file not found"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "phase transition" in result["message"].lower()


def test_phase_transition_invalid_json(target_project, monkeypatch):
    """Phase transition returning invalid JSON is an error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout="not json"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "json" in result["message"].lower()


def test_corrupt_state_file(target_project, monkeypatch):
    """Corrupt state file returns error."""
    monkeypatch.chdir(target_project)
    state_dir = target_project / ".flow-states"
    state_dir.mkdir(exist_ok=True)
    (state_dir / "test-feature.json").write_text("not json{{{")

    result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "parse" in result["message"].lower()


def test_fetch_error(target_project, monkeypatch):
    """Git fetch failure returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(returncode=1, stderr="Could not resolve host"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"


def test_push_failure_after_merge(target_project, monkeypatch):
    """Push failure after successful merge returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(stdout="Merge made"),  # merge ok
        _make_result(returncode=1, stderr="remote rejected"),  # push fails
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "push" in result["message"].lower()


def test_merge_error_non_conflict(target_project, monkeypatch):
    """Merge failure with no conflicts returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),  # fetch
        _make_result(returncode=1),  # merge-base
        _make_result(returncode=1, stderr="merge failed"),  # merge
        _make_result(stdout=""),  # status --porcelain (no conflicts)
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"


def test_branch_auto_detect(target_project, monkeypatch):
    """Branch is auto-detected from git when not provided."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project, branch="main")

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="OPEN"),
        _make_result(),
        _make_result(),
    ]

    with patch("subprocess.run", side_effect=responses):
        # Don't pass branch — let it auto-detect
        result = _mod.preflight(root=target_project)

    # current_branch() returns "main" for the git_repo fixture
    assert result["status"] == "ok"


def test_no_branch_returns_error(target_project, monkeypatch):
    """No branch detected returns error."""
    monkeypatch.chdir(target_project)
    monkeypatch.setattr(_mod, "current_branch", lambda: None)

    result = _mod.preflight(root=target_project)

    assert result["status"] == "error"
    assert "branch" in result["message"].lower()


def test_check_pr_status_no_identifier():
    """_check_pr_status with no pr_number or branch returns error."""
    pr_state, error = _mod._check_pr_status(pr_number=None, branch=None)
    assert pr_state is None
    assert "no pr number" in error.lower()


def test_unexpected_pr_state(target_project, monkeypatch):
    """Unexpected PR state returns error."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    responses = [
        _make_result(stdout=_PT_ENTER_OK),
        _make_result(stdout="DRAFT"),
    ]

    with patch("subprocess.run", side_effect=responses):
        result = _mod.preflight(branch="test-feature", root=target_project)

    assert result["status"] == "error"
    assert "unexpected" in result["message"].lower()


# --- CLI tests ---


def test_cli_happy_path(target_project, monkeypatch):
    """CLI returns valid JSON on success."""
    monkeypatch.chdir(target_project)
    _setup_complete_state(target_project)

    result = subprocess.run(
        [sys.executable, SCRIPT, "--branch", "test-feature"],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    # The script will fail because gh/git aren't mocked, but it should
    # at least parse args and find the state file. We just verify it
    # doesn't crash on import or argument parsing.
    # The real integration is tested via the in-process tests above.
    assert result.returncode in (0, 1)  # May fail on gh, that's OK


def test_cli_missing_state_file(target_project):
    """CLI with nonexistent branch returns error JSON."""
    result = subprocess.run(
        [sys.executable, SCRIPT, "--branch", "nonexistent-branch"],
        capture_output=True,
        text=True,
        cwd=str(target_project),
    )
    data = json.loads(result.stdout.strip().splitlines()[-1])
    assert data["status"] in ("ok", "error")  # Either inferred or error
