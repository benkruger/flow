"""Tests for lib/set-timestamp.py — mid-phase timestamp and value updates."""

import importlib.util
import json
import re
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "set-timestamp.py")

ISO_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]")

_spec = importlib.util.spec_from_file_location(
    "set_timestamp", LIB_DIR / "set-timestamp.py"
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)


def _run(git_repo, *set_args, branch=None):
    """Run set-timestamp.py with --set arguments."""
    cmd = [sys.executable, SCRIPT]
    for arg in set_args:
        cmd += ["--set", arg]
    if branch is not None:
        cmd += ["--branch", branch]
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


# --- Simple paths (in-process) ---


def test_simple_path_with_now():
    """design.approved_at=NOW writes a timestamp."""
    state = make_state(current_phase="flow-code", phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"})
    state["design"] = {"approved_at": None}

    updated, updates = _mod.apply_updates(state, ["design.approved_at=NOW"])

    assert len(updates) == 1
    assert updates[0]["path"] == "design.approved_at"
    assert ISO_PATTERN.match(updates[0]["value"])
    assert ISO_PATTERN.match(updated["design"]["approved_at"])


def test_simple_path_with_string_value():
    """Non-NOW values are written as plain strings."""
    state = make_state(current_phase="flow-code", phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"})
    state["design"] = {"status": "pending"}

    updated, updates = _mod.apply_updates(state, ["design.status=approved"])

    assert updated["design"]["status"] == "approved"


# --- Nested paths with array indexing (in-process) ---


def test_nested_path_with_array_index():
    """plan.tasks.0.started_at=NOW navigates into an array."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
            {"id": 2, "status": "pending", "started_at": None},
        ],
    }

    updated, updates = _mod.apply_updates(state, ["plan.tasks.0.started_at=NOW"])

    assert ISO_PATTERN.match(updated["plan"]["tasks"][0]["started_at"])
    assert updated["plan"]["tasks"][1]["started_at"] is None


def test_task_status_update():
    """plan.tasks.0.status=in_progress sets a string value on a task."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
        ],
    }

    updated, updates = _mod.apply_updates(state, ["plan.tasks.0.status=in_progress"])

    assert updated["plan"]["tasks"][0]["status"] == "in_progress"


# --- Multiple --set args (in-process) ---


def test_multiple_set_args():
    """Two --set args are applied atomically in one write."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
        ],
    }

    updated, updates = _mod.apply_updates(state, [
        "plan.tasks.0.status=in_progress",
        "plan.tasks.0.started_at=NOW",
    ])

    assert len(updates) == 2
    assert updated["plan"]["tasks"][0]["status"] == "in_progress"
    assert ISO_PATTERN.match(updated["plan"]["tasks"][0]["started_at"])


# --- Security scanned_at (in-process) ---


def test_security_scanned_at():
    """security.scanned_at=NOW sets the scan timestamp."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "in_progress",
    })
    state["security"] = {"findings": [], "clean_checks": [], "scanned_at": None}

    updated, updates = _mod.apply_updates(state, ["security.scanned_at=NOW"])

    assert ISO_PATTERN.match(updated["security"]["scanned_at"])


# --- CLI integration (subprocess) ---


def test_cli_happy_path(git_repo, state_dir, branch):
    """CLI happy path: write value and confirm state file updated via subprocess."""
    state = make_state(current_phase="flow-code", phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"})
    state["design"] = {"status": "pending"}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "design.status=approved")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["updates"][0]["value"] == "approved"


# --- Error cases ---


def test_error_no_state_file(git_repo):
    """No state file returns error."""
    result = _run(git_repo, "design.approved_at=NOW")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "No state file" in output["message"]


def test_error_invalid_path(git_repo, state_dir, branch):
    """Nonexistent path key returns error."""
    state = make_state(current_phase="flow-code")
    write_state(state_dir, branch, state)

    result = _run(git_repo, "nonexistent.field=NOW")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "not found" in output["message"]


def test_error_array_index_out_of_range(git_repo, state_dir, branch):
    """Array index out of range returns error."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete", "flow-code-review": "in_progress",
    })
    state["plan"] = {"tasks": [{"id": 1, "status": "pending"}]}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "plan.tasks.5.status=in_progress")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "out of range" in output["message"]


def test_error_invalid_format(git_repo, state_dir, branch):
    """Missing = in --set arg returns error."""
    state = make_state(current_phase="flow-code")
    write_state(state_dir, branch, state)

    result = _run(git_repo, "design.approved_at")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Invalid format" in output["message"]


def test_error_corrupt_json(git_repo, state_dir, branch):
    """Corrupt JSON state file returns error."""
    state_dir.mkdir(parents=True, exist_ok=True)
    (state_dir / f"{branch}.json").write_text("{bad json")

    result = _run(git_repo, "design.approved_at=NOW")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Could not read" in output["message"]


def test_detached_head_auto_resolves_single_state_file(git_repo, state_dir, branch):
    """Detached HEAD with a single state file auto-resolves to that branch."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["design"] = {"status": "pending"}
    write_state(state_dir, branch, state)

    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    result = _run(git_repo, "design.status=approved")
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert output["status"] == "ok"


def test_error_detached_head_no_state_files(git_repo):
    """Detached HEAD with no state files returns error."""
    subprocess.run(
        ["git", "checkout", "--detach", "HEAD"],
        cwd=str(git_repo), capture_output=True, check=True,
    )

    result = _run(git_repo, "design.approved_at=NOW")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "branch" in output["message"]


# --- Unit tests for _set_nested edge cases ---


def _load_module():
    """Import set-timestamp.py as a module for unit testing."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "set_timestamp", LIB_DIR / "set-timestamp.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_set_nested_list_non_numeric_intermediate():
    """Non-numeric key on a list intermediate raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"items": [{"a": 1}]}
    with pytest.raises(KeyError, match="Expected numeric index"):
        mod._set_nested(obj, ["items", "abc", "a"], "val")


def test_set_nested_non_traversable_intermediate():
    """Navigating into a string (non-dict, non-list) raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"outer": {"name": "hello"}}
    with pytest.raises(KeyError, match="Cannot navigate into"):
        mod._set_nested(obj, ["outer", "name", "deep", "sub"], "val")


def test_set_nested_list_final_non_numeric():
    """Non-numeric final key on a list raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"items": [1, 2, 3]}
    with pytest.raises(KeyError, match="Expected numeric index"):
        mod._set_nested(obj, ["items", "abc"], "val")


def test_set_nested_list_final_out_of_range():
    """Out-of-range final index on a list raises IndexError."""
    import pytest
    mod = _load_module()
    obj = {"items": [1, 2, 3]}
    with pytest.raises(IndexError, match="out of range"):
        mod._set_nested(obj, ["items", "99"], "val")


def test_set_nested_non_settable_final():
    """Setting a key on a non-dict, non-list final target raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"x": 42}
    with pytest.raises(KeyError, match="Cannot set key"):
        # x is an int, so navigating to x then trying to set "y" fails
        # We need a path where the second-to-last lookup gives an int
        obj2 = {"items": [1, 2]}
        # items[0] is int 1, then try to set "sub" on it
        mod._set_nested(obj2, ["items", "0", "sub"], "val")


def test_set_nested_list_intermediate_out_of_range():
    """Out-of-range intermediate index on a list raises IndexError."""
    import pytest
    mod = _load_module()
    obj = {"items": [{"a": 1}]}
    with pytest.raises(IndexError, match="out of range"):
        mod._set_nested(obj, ["items", "99", "a"], "val")


def test_set_nested_list_final_sets_value():
    """Setting a value by numeric index on a list works."""
    mod = _load_module()
    obj = {"items": [10, 20, 30]}
    mod._set_nested(obj, ["items", "1"], 99)
    assert obj["items"][1] == 99


def test_integer_coercion_for_digit_values():
    """Pure-digit values are stored as int, not str."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "in_progress",
    })
    state["code_review_step"] = 0

    updated, updates = _mod.apply_updates(state, ["code_review_step=1"])

    assert updated["code_review_step"] == 1
    assert isinstance(updated["code_review_step"], int)


def test_non_digit_values_remain_strings():
    """Non-digit values stay as strings."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "in_progress",
    })
    state["some_field"] = "old"

    updated, updates = _mod.apply_updates(state, ["some_field=in_progress"])

    assert updated["some_field"] == "in_progress"
    assert isinstance(updated["some_field"], str)


def test_now_values_remain_timestamp_strings():
    """NOW values are still stored as timestamp strings, not coerced."""
    state = make_state(current_phase="flow-code-review", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "complete",
        "flow-code-review": "in_progress",
    })
    state["design"] = {"approved_at": None}

    updated, updates = _mod.apply_updates(state, ["design.approved_at=NOW"])

    assert isinstance(updated["design"]["approved_at"], str)
    assert ISO_PATTERN.match(updated["design"]["approved_at"])


def test_set_nested_dict_key_not_found_intermediate():
    """Missing key in intermediate dict raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"a": {"b": 1}}
    with pytest.raises(KeyError, match="not found"):
        mod._set_nested(obj, ["a", "missing", "x"], "val")


# --- code_task increment validation (in-process) ---


def test_code_task_increment_by_one_allowed():
    """code_task can increment by 1: 0→1 and 1→2."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 0

    updated, updates = _mod.apply_updates(state, ["code_task=1"])
    assert updated["code_task"] == 1

    updated, updates = _mod.apply_updates(updated, ["code_task=2"])
    assert updated["code_task"] == 2


def test_code_task_initial_set_to_one_allowed():
    """code_task absent → 1 is allowed (first task)."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })

    updated, updates = _mod.apply_updates(state, ["code_task=1"])
    assert updated["code_task"] == 1


def test_code_task_jump_blocked():
    """code_task 0→5 is blocked (batching attempt)."""
    import pytest
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 0

    with pytest.raises(ValueError, match="increment by 1"):
        _mod.apply_updates(state, ["code_task=5"])


def test_code_task_skip_blocked():
    """code_task 3→5 is blocked (skipped a task)."""
    import pytest
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 3

    with pytest.raises(ValueError, match="increment by 1"):
        _mod.apply_updates(state, ["code_task=5"])


def test_code_task_reset_to_zero_allowed():
    """code_task 3→0 is allowed (phase re-entry reset)."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 3

    updated, updates = _mod.apply_updates(state, ["code_task=0"])
    assert updated["code_task"] == 0


def test_code_task_initial_jump_blocked():
    """code_task absent → 3 is blocked (must start at 1)."""
    import pytest
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })

    with pytest.raises(ValueError, match="increment by 1"):
        _mod.apply_updates(state, ["code_task=3"])


def test_code_task_non_integer_blocked():
    """code_task with a non-integer value is blocked."""
    import pytest
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 0

    with pytest.raises(ValueError, match="must be an integer"):
        _mod.apply_updates(state, ["code_task=abc"])


def test_code_task_cli_increment_blocked(git_repo, state_dir, branch):
    """CLI blocks code_task jump with error exit code."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["code_task"] = 0
    write_state(state_dir, branch, state)

    result = _run(git_repo, "code_task=5")
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "increment by 1" in output["message"]


# --- --branch flag (subprocess) ---


def test_cli_branch_flag_uses_specified_state_file(git_repo, state_dir):
    """--branch flag finds the state file for a different branch."""
    state = make_state(current_phase="flow-code", phase_statuses={
        "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
    })
    state["design"] = {"status": "pending"}
    write_state(state_dir, "other-feature", state)

    result = _run(git_repo, "design.status=approved", branch="other-feature")
    assert result.returncode == 0
    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert output["updates"][0]["value"] == "approved"


def test_error_ambiguous_multiple_state_files(git_repo, state_dir):
    """Multiple state files with no exact match returns ambiguity error."""
    for name in ["feat-a", "feat-b"]:
        state = make_state(current_phase="flow-code", phase_statuses={
            "flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress",
        })
        write_state(state_dir, name, state)

    result = _run(git_repo, "current_phase=flow-code")
    assert result.returncode == 1
    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "Multiple" in output["message"]
    assert sorted(output["candidates"]) == ["feat-a", "feat-b"]
