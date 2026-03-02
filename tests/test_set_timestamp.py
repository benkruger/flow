"""Tests for lib/set-timestamp.py — mid-phase timestamp and value updates."""

import json
import re
import subprocess
import sys

from conftest import LIB_DIR, make_state, write_state

SCRIPT = str(LIB_DIR / "set-timestamp.py")

ISO_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[Z+-]")


def _run(git_repo, *set_args):
    """Run set-timestamp.py with --set arguments."""
    cmd = [sys.executable, SCRIPT]
    for arg in set_args:
        cmd += ["--set", arg]
    result = subprocess.run(
        cmd, capture_output=True, text=True, cwd=str(git_repo),
    )
    return result


def _read_state(state_dir, branch):
    """Read and parse the state file."""
    return json.loads((state_dir / f"{branch}.json").read_text())


# --- Simple paths ---


def test_simple_path_with_now(git_repo, state_dir, branch):
    """design.approved_at=NOW writes a timestamp."""
    state = make_state(current_phase=3, phase_statuses={1: "complete", 2: "complete", 3: "in_progress"})
    state["design"] = {"approved_at": None}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "design.approved_at=NOW")
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert output["status"] == "ok"
    assert len(output["updates"]) == 1
    assert output["updates"][0]["path"] == "design.approved_at"
    assert ISO_PATTERN.match(output["updates"][0]["value"])

    updated = _read_state(state_dir, branch)
    assert ISO_PATTERN.match(updated["design"]["approved_at"])


def test_simple_path_with_string_value(git_repo, state_dir, branch):
    """Non-NOW values are written as plain strings."""
    state = make_state(current_phase=3, phase_statuses={1: "complete", 2: "complete", 3: "in_progress"})
    state["design"] = {"status": "pending"}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "design.status=approved")
    assert result.returncode == 0

    updated = _read_state(state_dir, branch)
    assert updated["design"]["status"] == "approved"


# --- Nested paths with array indexing ---


def test_nested_path_with_array_index(git_repo, state_dir, branch):
    """plan.tasks.0.started_at=NOW navigates into an array."""
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete", 5: "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
            {"id": 2, "status": "pending", "started_at": None},
        ],
    }
    write_state(state_dir, branch, state)

    result = _run(git_repo, "plan.tasks.0.started_at=NOW")
    assert result.returncode == 0

    updated = _read_state(state_dir, branch)
    assert ISO_PATTERN.match(updated["plan"]["tasks"][0]["started_at"])
    assert updated["plan"]["tasks"][1]["started_at"] is None


def test_task_status_update(git_repo, state_dir, branch):
    """plan.tasks.0.status=in_progress sets a string value on a task."""
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete", 5: "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
        ],
    }
    write_state(state_dir, branch, state)

    result = _run(git_repo, "plan.tasks.0.status=in_progress")
    assert result.returncode == 0

    updated = _read_state(state_dir, branch)
    assert updated["plan"]["tasks"][0]["status"] == "in_progress"


# --- Multiple --set args ---


def test_multiple_set_args(git_repo, state_dir, branch):
    """Two --set args are applied atomically in one write."""
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete", 5: "in_progress",
    })
    state["plan"] = {
        "tasks": [
            {"id": 1, "status": "pending", "started_at": None},
        ],
    }
    write_state(state_dir, branch, state)

    result = _run(
        git_repo,
        "plan.tasks.0.status=in_progress",
        "plan.tasks.0.started_at=NOW",
    )
    assert result.returncode == 0

    output = json.loads(result.stdout)
    assert len(output["updates"]) == 2

    updated = _read_state(state_dir, branch)
    assert updated["plan"]["tasks"][0]["status"] == "in_progress"
    assert ISO_PATTERN.match(updated["plan"]["tasks"][0]["started_at"])


# --- Security scanned_at ---


def test_security_scanned_at(git_repo, state_dir, branch):
    """security.scanned_at=NOW sets the scan timestamp."""
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete",
        5: "in_progress",
    })
    state["security"] = {"findings": [], "clean_checks": [], "scanned_at": None}
    write_state(state_dir, branch, state)

    result = _run(git_repo, "security.scanned_at=NOW")
    assert result.returncode == 0

    updated = _read_state(state_dir, branch)
    assert ISO_PATTERN.match(updated["security"]["scanned_at"])


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
    state = make_state(current_phase=3)
    write_state(state_dir, branch, state)

    result = _run(git_repo, "nonexistent.field=NOW")
    assert result.returncode == 1

    output = json.loads(result.stdout)
    assert output["status"] == "error"
    assert "not found" in output["message"]


def test_error_array_index_out_of_range(git_repo, state_dir, branch):
    """Array index out of range returns error."""
    state = make_state(current_phase=5, phase_statuses={
        1: "complete", 2: "complete", 3: "complete", 4: "complete", 5: "in_progress",
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
    state = make_state(current_phase=3)
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


def test_error_detached_head(git_repo, state_dir, branch):
    """Detached HEAD returns error."""
    state = make_state(current_phase=3)
    write_state(state_dir, branch, state)

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


def test_set_nested_dict_key_not_found_intermediate():
    """Missing key in intermediate dict raises KeyError."""
    import pytest
    mod = _load_module()
    obj = {"a": {"b": 1}}
    with pytest.raises(KeyError, match="not found"):
        mod._set_nested(obj, ["a", "missing", "x"], "val")
