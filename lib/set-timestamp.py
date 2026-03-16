"""Set timestamp and value fields in the FLOW state file.

Handles mid-phase timestamp fields and Code phase task updates.
Dot-notation paths navigate nested objects. Numeric path segments
index arrays. NOW is a magic value replaced with current Pacific Time timestamp.

Usage:
  bin/flow set-timestamp --set <path>=<value> [--set <path>=<value> ...]

Examples:
  bin/flow set-timestamp --set design.approved_at=NOW
  bin/flow set-timestamp --set plan.tasks.3.status=in_progress --set plan.tasks.3.started_at=NOW

Output (JSON to stdout):
  Success: {"status": "ok", "updates": [{"path": "...", "value": "..."}]}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import now, project_root, resolve_branch


def _set_nested(obj, path_parts, value):
    """Navigate a nested dict/list by path parts and set the final value.

    Numeric path segments are treated as array indexes (0-based).
    """
    current = obj
    for part in path_parts[:-1]:
        if isinstance(current, list):
            try:
                index = int(part)
            except ValueError:
                raise KeyError(f"Expected numeric index for list, got '{part}'")
            if index < 0 or index >= len(current):
                raise IndexError(f"Index {index} out of range (list has {len(current)} items)")
            current = current[index]
        elif isinstance(current, dict):
            if part in current:
                current = current[part]
            else:
                raise KeyError(f"Key '{part}' not found")
        else:
            raise KeyError(f"Cannot navigate into {type(current).__name__} with key '{part}'")

    final_key = path_parts[-1]
    if isinstance(current, list):
        try:
            index = int(final_key)
        except ValueError:
            raise KeyError(f"Expected numeric index for list, got '{final_key}'")
        if index < 0 or index >= len(current):
            raise IndexError(f"Index {index} out of range (list has {len(current)} items)")
        current[index] = value
    elif isinstance(current, dict):
        current[final_key] = value
    else:
        raise KeyError(f"Cannot set key '{final_key}' on {type(current).__name__}")


def _validate_code_task(state, new_value):
    """Validate code_task can only increment by 1 or reset to 0.

    Prevents batching multiple plan tasks into a single commit by
    ensuring code_task advances one task at a time.
    """
    if not isinstance(new_value, int):
        raise ValueError(f"code_task must be an integer, got '{new_value}'")
    if new_value == 0:
        return
    current = state.get("code_task", 0)
    if new_value != current + 1:
        raise ValueError(
            f"code_task can only increment by 1. "
            f"Current: {current}, attempted: {new_value}. "
            f"Commit each task individually before advancing."
        )


def apply_updates(state, set_args):
    """Apply a list of path=value updates to the state dict.

    Returns (state, updates_list) where updates_list records what was set.
    """
    updates = []
    for assignment in set_args:
        if "=" not in assignment:
            raise ValueError(f"Invalid format '{assignment}' — expected path=value")

        path, value = assignment.split("=", 1)
        path_parts = path.split(".")

        if value == "NOW":
            value = now()
        elif value.isdigit():
            value = int(value)

        if path_parts == ["code_task"]:
            _validate_code_task(state, value)

        _set_nested(state, path_parts, value)
        updates.append({"path": path, "value": value})

    return state, updates


def main():
    parser = argparse.ArgumentParser(description="Set state file fields")
    parser.add_argument("--set", dest="set_args", action="append", required=True,
                        help="path=value (use NOW for current timestamp)")
    parser.add_argument("--branch", type=str, default=None,
                        help="Override branch for state file lookup")
    args = parser.parse_args()

    root = project_root()
    branch, candidates = resolve_branch(args.branch)

    if branch is None:
        if candidates:
            print(json.dumps({
                "status": "error",
                "message": "Multiple active features. Pass --branch.",
                "candidates": candidates,
            }))
        else:
            print(json.dumps({
                "status": "error",
                "message": "Could not determine current branch",
            }))
        sys.exit(1)

    state_path = root / ".flow-states" / f"{branch}.json"

    if not state_path.exists():
        print(json.dumps({
            "status": "error",
            "message": f"No state file found: {state_path}",
        }))
        sys.exit(1)

    try:
        state = json.loads(state_path.read_text())
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)

    try:
        state, updates = apply_updates(state, args.set_args)
    except (KeyError, IndexError, ValueError) as e:
        print(json.dumps({
            "status": "error",
            "message": str(e),
        }))
        sys.exit(1)

    state_path.write_text(json.dumps(state, indent=2))
    print(json.dumps({"status": "ok", "updates": updates}))


if __name__ == "__main__":
    main()
