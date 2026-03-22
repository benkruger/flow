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
import sys
from pathlib import Path


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


def main():
    try:
        json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    # Lazy import to avoid flow_utils module-level flow-phases.json load
    # on every hook invocation — this hook only needs branch/root helpers.
    from flow_utils import current_branch, project_root

    branch = current_branch()
    if not branch:
        sys.exit(0)

    state_path = project_root() / ".flow-states" / f"{branch}.json"
    allowed, message = validate(str(state_path))
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
