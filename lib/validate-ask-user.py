#!/usr/bin/env python3
"""
PreToolUse hook for AskUserQuestion — enforces auto-continue.

Blocks AskUserQuestion when `_auto_continue` is set in the state file.
This prevents the model from prompting the user when autonomous phase
transitions are configured.

Exit 0 — allow
Exit 2 — block (error message on stderr)
"""

import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, now


def write_blocked(state_path):
    """Write _blocked timestamp to the state file.

    Best-effort: any error is silently ignored so the hook
    never interferes with AskUserQuestion delivery.
    """
    try:
        if state_path is None or not Path(state_path).exists():
            return

        def transform(state):
            state["_blocked"] = now()

        mutate_state(Path(state_path), transform)
    except Exception:
        pass


def validate(state_path):
    """Validate that auto-continue is not active.

    Returns (allowed: bool, message: str).
    """
    if state_path is None or not Path(state_path).exists():
        return (True, "")

    try:
        state = json.loads(Path(state_path).read_text())
    except (json.JSONDecodeError, ValueError):
        return (True, "")

    auto_cmd = state.get("_auto_continue")
    if not auto_cmd:
        return (True, "")

    return (False,
            f"BLOCKED: Auto-continue is active. Invoke {auto_cmd} now.")


def _current_branch():
    """Get current branch name via git."""
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        capture_output=True, text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def _project_root():
    """Get project root via git worktree list."""
    result = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return None
    for line in result.stdout.splitlines():
        if line.startswith("worktree "):
            return line.split(" ", 1)[1]
    return None


def main():
    try:
        json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    branch = _current_branch()
    root = _project_root()
    if not branch or not root:
        sys.exit(0)

    state_path = Path(root) / ".flow-states" / f"{branch}.json"
    allowed, message = validate(str(state_path))
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    write_blocked(str(state_path))
    sys.exit(0)


if __name__ == "__main__":
    main()
