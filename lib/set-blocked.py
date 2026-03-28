#!/usr/bin/env python3
"""PermissionRequest hook — sets _blocked flag for TUI blocked detection.

When a tool call triggers a permission prompt (Bash, Edit, Write not in the
allow list), this hook fires and writes _blocked = now() to the state file.
The PostToolUse hook (clear-blocked.py) clears it after the user responds.

Fail-open: any error silently exits 0 with no output.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))


def set_blocked(state_path):
    """Write _blocked timestamp to the state file.

    Best-effort: any error is silently ignored so the hook
    never interferes with permission prompt delivery.
    """
    try:
        if state_path is None or not Path(state_path).exists():
            return

        from flow_utils import mutate_state, now

        def transform(state):
            state["_blocked"] = now()

        mutate_state(Path(state_path), transform)
    except Exception:
        pass


def main():
    try:
        json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    from flow_utils import current_branch, project_root

    try:
        branch = current_branch()
        if not branch:
            sys.exit(0)

        state_path = project_root() / ".flow-states" / f"{branch}.json"
        set_blocked(str(state_path))
    except Exception:
        pass

    sys.exit(0)


if __name__ == "__main__":
    main()
