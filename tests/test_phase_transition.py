"""Tests for lib/phase-transition.py — phase entry and completion."""

import importlib.util
import json
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "phase-transition.py")

_spec = importlib.util.spec_from_file_location(
    "phase_transition", LIB_DIR / "phase-transition.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(git_repo, phase, action, next_phase=None, branch=None):
    """Run phase-transition.py with the given args."""
    cmd = [sys.executable, SCRIPT, "--phase", phase, "--action", action]
    if next_phase is not None:
        cmd += ["--next-phase", next_phase]
    if branch is not None:
        cmd += ["--branch", branch]
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


# --- Phase entry (in-process) ---


def test_enter_sets_all_fields():
    """Enter sets status, started_at, session_started_at, visit_count, current_phase."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert result["status"] == "ok"
    assert result["phase"] == "flow-plan"
    assert result["action"] == "enter"
    assert result["visit_count"] == 1
    assert result["first_visit"] is True

    assert updated["phases"]["flow-plan"]["status"] == "in_progress"
    assert updated["phases"]["flow-plan"]["started_at"] is not None
    assert updated["phases"]["flow-plan"]["session_started_at"] is not None
    assert updated["phases"]["flow-plan"]["visit_count"] == 1
    assert updated["current_phase"] == "flow-plan"


def test_enter_first_visit_sets_started_at():
    """First visit sets started_at when it is null."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    assert state["phases"]["flow-plan"]["started_at"] is None

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert updated["phases"]["flow-plan"]["started_at"] is not None


def test_enter_reentry_preserves_started_at():
    """Re-entry preserves started_at and increments visit_count."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "complete"})
    state["phases"]["flow-plan"]["started_at"] = "2026-01-15T10:00:00Z"
    state["phases"]["flow-plan"]["visit_count"] = 2

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert result["visit_count"] == 3
    assert result["first_visit"] is False
    assert updated["phases"]["flow-plan"]["started_at"] == "2026-01-15T10:00:00Z"
    assert updated["phases"]["flow-plan"]["visit_count"] == 3


# --- Phase completion (in-process) ---


def test_complete_sets_all_fields():
    """Complete sets cumulative_seconds, status, completed_at, session_started_at=null, current_phase."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["status"] == "ok"
    assert result["phase"] == "flow-plan"
    assert result["action"] == "complete"
    assert "cumulative_seconds" in result
    assert "formatted_time" in result
    assert result["next_phase"] == "flow-code"

    assert updated["phases"]["flow-plan"]["status"] == "complete"
    assert updated["phases"]["flow-plan"]["completed_at"] is not None
    assert updated["phases"]["flow-plan"]["session_started_at"] is None
    assert updated["current_phase"] == "flow-code"


def test_complete_adds_to_existing_cumulative():
    """Complete adds elapsed time to existing cumulative_seconds."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["cumulative_seconds"] = 600

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["cumulative_seconds"] >= 600


def test_complete_formatted_time_less_than_one_minute():
    """Formatted time shows <1m for short durations."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["cumulative_seconds"] = 0
    state["phases"]["flow-plan"]["session_started_at"] = None

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["formatted_time"] == "<1m"


def test_complete_next_phase_override():
    """next_phase parameter overrides the default phase+1."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})

    updated, result = _mod.phase_complete(state, "flow-plan", next_phase="flow-code-review")

    assert result["next_phase"] == "flow-code-review"
    assert updated["current_phase"] == "flow-code-review"


def test_complete_null_session_started_at():
    """Null session_started_at on complete results in elapsed=0."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = None
    state["phases"]["flow-plan"]["cumulative_seconds"] = 100

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["cumulative_seconds"] == 100


# --- Formatted time values (in-process) ---


def test_formatted_time_minutes():
    """Formatted time shows Xm for >= 60 seconds."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["cumulative_seconds"] = 300
    state["phases"]["flow-plan"]["session_started_at"] = None

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["formatted_time"] == "5m"


def test_formatted_time_hours():
    """Formatted time shows Xh Ym for >= 3600 seconds."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["cumulative_seconds"] = 3900
    state["phases"]["flow-plan"]["session_started_at"] = None

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert result["formatted_time"] == "1h 5m"


# --- CLI integration (subprocess) ---


def test_cli_enter_and_complete_happy_path(git_repo, state_dir, branch):
    """CLI happy path: enter then complete a phase via subprocess."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, branch, state)

    enter_result = _run(git_repo, "flow-plan", "enter")
    assert enter_result.returncode == 0
    assert json.loads(enter_result.stdout)["status"] == "ok"

    complete_result = _run(git_repo, "flow-plan", "complete")
    assert complete_result.returncode == 0
    assert json.loads(complete_result.stdout)["status"] == "ok"


# --- Error cases ---


def test_error_missing_state_file(git_repo):
    """Missing state file returns error."""
    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "No state file" in output["message"]


def test_error_invalid_phase(git_repo, state_dir, branch):
    """Invalid phase name returns error."""
    state = make_state(current_phase="flow-start")
    write_state(state_dir, branch, state)

    result = _run(git_repo, "invalid", "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Invalid phase" in output["message"]


def test_error_phase_not_in_state(git_repo, state_dir, branch):
    """Phase key missing from state phases dict returns error."""
    state = {"branch": branch, "current_phase": "flow-start", "phases": {}}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "not found" in output["message"]


def test_error_corrupt_json(git_repo, state_dir, branch):
    """Corrupt JSON state file returns error."""
    state_dir.mkdir(parents=True, exist_ok=True)
    (state_dir / f"{branch}.json").write_text("{bad json")

    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Could not read" in output["message"]


def test_detached_head_auto_resolves_single_state_file(git_repo, state_dir, branch):
    """Detached HEAD with a single state file auto-resolves to that branch."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, branch, state)

    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == "flow-plan"


def test_error_detached_head_no_state_files(git_repo):
    """Detached HEAD with no state files returns error."""
    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "branch" in output["message"]


# --- Unit test for edge case ---


def test_complete_uses_custom_phase_order():
    """phase_complete with a custom phase_order uses that order for next phase."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    custom_order = ["flow-start", "flow-plan", "flow-code-review"]

    updated, result = _mod.phase_complete(state, "flow-plan", phase_order=custom_order)

    assert result["next_phase"] == "flow-code-review"
    assert updated["current_phase"] == "flow-code-review"


def test_cli_uses_frozen_phases_file(git_repo, state_dir, branch):
    """CLI uses frozen phases file when it exists."""
    import shutil
    source = LIB_DIR.parent / "flow-phases.json"
    frozen = state_dir / f"{branch}-phases.json"
    state_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(source), str(frozen))

    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, branch, state)

    enter_result = _run(git_repo, "flow-plan", "enter")
    assert enter_result.returncode == 0

    complete_result = _run(git_repo, "flow-plan", "complete")
    assert complete_result.returncode == 0
    data = json.loads(complete_result.stdout)
    assert data["status"] == "ok"
    assert data["next_phase"] == "flow-code"


def test_cli_falls_back_without_frozen_phases(git_repo, state_dir, branch):
    """CLI works without frozen phases file (backward compat)."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, branch, state)

    # No frozen phases file — should still work using module-level constants
    enter_result = _run(git_repo, "flow-plan", "enter")
    assert enter_result.returncode == 0

    complete_result = _run(git_repo, "flow-plan", "complete")
    assert complete_result.returncode == 0
    data = json.loads(complete_result.stdout)
    assert data["next_phase"] == "flow-code"


def test_enter_flow_complete():
    """Enter flow-complete sets status, started_at, session_started_at, visit_count."""
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete", "flow-learn": "complete",
    })

    updated, result = _mod.phase_enter(state, "flow-complete")

    assert result["status"] == "ok"
    assert result["phase"] == "flow-complete"
    assert result["visit_count"] == 1
    assert result["first_visit"] is True
    assert updated["phases"]["flow-complete"]["status"] == "in_progress"
    assert updated["phases"]["flow-complete"]["started_at"] is not None
    assert updated["current_phase"] == "flow-complete"


def test_complete_flow_complete_with_next_phase():
    """Complete flow-complete with explicit next_phase works."""
    state = make_state(current_phase="flow-complete", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete", "flow-learn": "complete",
        "flow-complete": "in_progress",
    })

    updated, result = _mod.phase_complete(state, "flow-complete", next_phase="flow-complete")

    assert result["status"] == "ok"
    assert result["phase"] == "flow-complete"
    assert result["next_phase"] == "flow-complete"
    assert updated["phases"]["flow-complete"]["status"] == "complete"
    assert updated["phases"]["flow-complete"]["completed_at"] is not None
    assert updated["current_phase"] == "flow-complete"


def test_complete_terminal_phase_auto_next():
    """Complete flow-complete without explicit next_phase handles terminal phase."""
    state = make_state(current_phase="flow-complete", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete", "flow-learn": "complete",
        "flow-complete": "in_progress",
    })

    updated, result = _mod.phase_complete(state, "flow-complete")

    assert result["status"] == "ok"
    assert result["next_phase"] == "flow-complete"
    assert updated["current_phase"] == "flow-complete"


def test_cli_flow_complete_enter(git_repo, state_dir, branch):
    """CLI accepts flow-complete as a valid phase for entry."""
    state = make_state(current_phase="flow-learn", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete", "flow-learn": "complete",
    })
    write_state(state_dir, branch, state)

    result = _run(git_repo, "flow-complete", "enter")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == "flow-complete"


def test_cli_flow_complete_complete(git_repo, state_dir, branch):
    """CLI accepts flow-complete for completion with --next-phase."""
    state = make_state(current_phase="flow-complete", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete", "flow-learn": "complete",
        "flow-complete": "in_progress",
    })
    write_state(state_dir, branch, state)

    result = _run(git_repo, "flow-complete", "complete", next_phase="flow-complete")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == "flow-complete"


def test_enter_code_review_sets_code_review_step():
    """Entering flow-code-review sets code_review_step to 0 (integer)."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
    })

    updated, result = _mod.phase_enter(state, "flow-code-review")

    assert updated["code_review_step"] == 0
    assert isinstance(updated["code_review_step"], int)


def test_enter_non_code_review_does_not_set_code_review_step():
    """Entering flow-plan does NOT set code_review_step."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert "code_review_step" not in updated


def test_reenter_code_review_resets_code_review_step():
    """Re-entering flow-code-review resets code_review_step to 0."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "complete",
    })
    state["code_review_step"] = 3

    updated, result = _mod.phase_enter(state, "flow-code-review")

    assert updated["code_review_step"] == 0


# --- Auto-continue flag (in-process) ---


def test_complete_sets_auto_continue_when_skills_continue_auto():
    """phase_complete sets _auto_continue when skills.<phase>.continue is 'auto'."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["skills"] = {"flow-start": {"continue": "auto"}}

    updated, result = _mod.phase_complete(state, "flow-start")

    assert updated["_auto_continue"] == "/flow:flow-plan"
    assert result["next_phase"] == "flow-plan"


def test_complete_sets_auto_continue_with_flat_string_config():
    """phase_complete handles flat string skill config (e.g. 'auto')."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["skills"] = {"flow-start": "auto"}

    updated, result = _mod.phase_complete(state, "flow-start")

    assert updated["_auto_continue"] == "/flow:flow-plan"


def test_complete_no_auto_continue_when_manual():
    """phase_complete does NOT set _auto_continue when continue is 'manual'."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["skills"] = {"flow-start": {"continue": "manual"}}

    updated, result = _mod.phase_complete(state, "flow-start")

    assert "_auto_continue" not in updated


def test_complete_no_auto_continue_when_no_skills():
    """phase_complete does NOT set _auto_continue when state has no skills key."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})

    updated, result = _mod.phase_complete(state, "flow-start")

    assert "_auto_continue" not in updated


def test_complete_clears_auto_continue_when_switching_to_manual():
    """phase_complete removes _auto_continue if it was set but mode is now manual."""
    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })
    state["skills"] = {"flow-plan": {"continue": "manual"}}
    state["_auto_continue"] = "/flow:flow-plan"

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert "_auto_continue" not in updated


def test_complete_no_auto_continue_when_skill_config_unexpected_type():
    """phase_complete does NOT set _auto_continue when skill config is unexpected type."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    state["skills"] = {"flow-start": 42}

    updated, result = _mod.phase_complete(state, "flow-start")

    assert "_auto_continue" not in updated


def test_enter_clears_auto_continue():
    """phase_enter clears _auto_continue from the previous phase."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    state["_auto_continue"] = "/flow:flow-plan"

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert "_auto_continue" not in updated


def test_enter_no_error_when_auto_continue_absent():
    """phase_enter does not error when _auto_continue is not in state."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert "_auto_continue" not in updated


def test_complete_future_session_started_clamps_to_zero():
    """If session_started_at is in the future, elapsed clamps to 0."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"})
    state["phases"]["flow-plan"]["session_started_at"] = "2099-12-31T23:59:59Z"
    state["phases"]["flow-plan"]["cumulative_seconds"] = 50

    updated, result = _mod.phase_complete(state, "flow-plan")
    assert result["cumulative_seconds"] == 50


# --- --branch flag (subprocess) ---


def test_cli_branch_flag_uses_specified_state_file(git_repo, state_dir):
    """--branch flag finds the state file for a different branch."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    write_state(state_dir, "other-feature", state)

    result = _run(git_repo, "flow-plan", "enter", branch="other-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["phase"] == "flow-plan"


def test_error_ambiguous_multiple_state_files(git_repo, state_dir):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
        write_state(state_dir, name, state)

    result = _run(git_repo, "flow-plan", "enter")
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Multiple" in output["message"]
    assert sorted(output["candidates"]) == ["feat-a", "feat-b"]


# --- Phase transitions recording ---


def test_enter_records_phase_transition():
    """phase_enter appends a transition entry with from/to/timestamp."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    state["phase_transitions"] = []

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert len(updated["phase_transitions"]) == 1
    entry = updated["phase_transitions"][0]
    assert entry["from"] == "flow-start"
    assert entry["to"] == "flow-plan"
    assert "timestamp" in entry
    assert "reason" not in entry


def test_enter_appends_to_existing_transitions():
    """phase_enter appends to existing transitions array."""
    state = make_state(current_phase="flow-plan", phase_statuses={"flow-start": "complete", "flow-plan": "complete"})
    state["phase_transitions"] = [
        {"from": "flow-start", "to": "flow-plan", "timestamp": "2026-01-01T00:00:00-08:00"},
    ]

    updated, result = _mod.phase_enter(state, "flow-code")

    assert len(updated["phase_transitions"]) == 2
    assert updated["phase_transitions"][1]["from"] == "flow-plan"
    assert updated["phase_transitions"][1]["to"] == "flow-code"


def test_enter_transition_has_no_reason_by_default():
    """phase_enter without reason parameter does not include reason key."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    state["phase_transitions"] = []

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert "reason" not in updated["phase_transitions"][0]


def test_enter_transition_with_reason():
    """phase_enter with reason includes it in the transition entry."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
    })
    state["phase_transitions"] = []

    updated, result = _mod.phase_enter(state, "flow-plan", reason="approach was wrong")

    assert updated["phase_transitions"][0]["reason"] == "approach was wrong"


def test_enter_creates_transitions_array_if_missing():
    """phase_enter creates phase_transitions if the key is absent (old state files)."""
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "complete"})
    state.pop("phase_transitions", None)

    updated, result = _mod.phase_enter(state, "flow-plan")

    assert "phase_transitions" in updated
    assert len(updated["phase_transitions"]) == 1


# --- Diff stats ---


def test_complete_code_phase_captures_diff_stats(git_repo, state_dir):
    """Code phase completion captures diff_stats with files/insertions/deletions."""
    # Add a file on main so the feature branch can delete it (covers deletion parsing)
    (git_repo / "old_file.py").write_text("remove_me\n")
    subprocess.run(["git", "add", "-A"], cwd=str(git_repo), capture_output=True, check=True)
    subprocess.run(
        ["git", "commit", "-m", "add old file"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    # Create a feature branch: add a new file and delete the old one
    subprocess.run(
        ["git", "checkout", "-b", "my-feature"],
        cwd=str(git_repo), capture_output=True, check=True,
    )
    (git_repo / "new_file.py").write_text("print('hello')\n")
    (git_repo / "old_file.py").unlink()
    subprocess.run(["git", "add", "-A"], cwd=str(git_repo), capture_output=True, check=True)
    subprocess.run(
        ["git", "commit", "-m", "add file and delete old"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    write_state(state_dir, "my-feature", state)

    result = _run(git_repo, "flow-code", "complete", branch="my-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"

    updated = json.loads((state_dir / "my-feature.json").read_text())
    assert "diff_stats" in updated
    assert updated["diff_stats"]["files_changed"] >= 1
    assert updated["diff_stats"]["insertions"] >= 1
    assert updated["diff_stats"]["deletions"] >= 1
    assert "captured_at" in updated["diff_stats"]


def test_complete_non_code_phase_no_diff_stats():
    """Plan phase completion does not capture diff_stats."""
    state = make_state(current_phase="flow-plan", phase_statuses={
        "flow-start": "complete", "flow-plan": "in_progress",
    })

    updated, result = _mod.phase_complete(state, "flow-plan")

    assert "diff_stats" not in updated


def test_complete_code_phase_no_git_skips_diff_stats():
    """Code phase completion without git access skips diff_stats gracefully."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })

    # In-process call — git diff runs in cwd which is a repo, so it succeeds
    updated, result = _mod.phase_complete(state, "flow-code")

    # Should succeed with diff_stats (zeros since no branch diff)
    assert result["status"] == "ok"
    assert "diff_stats" in updated


def test_capture_diff_stats_git_failure(monkeypatch):
    """_capture_diff_stats returns zeros when git diff fails."""
    class FakeResult:
        returncode = 1
        stdout = ""
    monkeypatch.setattr(subprocess, "run", lambda *a, **kw: FakeResult())
    stats = _mod._capture_diff_stats()
    assert stats["files_changed"] == 0
    assert stats["insertions"] == 0
    assert stats["deletions"] == 0
    assert "captured_at" in stats


def test_capture_diff_stats_exception(monkeypatch):
    """_capture_diff_stats returns zeros when subprocess raises."""
    def _raise(*a, **kw):
        raise OSError("git not found")
    monkeypatch.setattr(subprocess, "run", _raise)
    stats = _mod._capture_diff_stats()
    assert stats["files_changed"] == 0


def test_capture_diff_stats_empty_output(monkeypatch):
    """_capture_diff_stats returns zeros when git output is empty (no diff)."""
    class FakeResult:
        returncode = 0
        stdout = ""
    monkeypatch.setattr(subprocess, "run", lambda *a, **kw: FakeResult())
    stats = _mod._capture_diff_stats()
    assert stats["files_changed"] == 0
    assert stats["insertions"] == 0
    assert stats["deletions"] == 0
