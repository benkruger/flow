#!/usr/bin/env python3
"""StopFailure hook that captures API error context in the state file.

When Claude Code encounters an API error (rate limit, auth failure,
network timeout), this hook fires and writes _last_failure to the
active state file. SessionStart reads and clears this field on the
next session resume, injecting it into awareness context.

Fields written:
  _last_failure — object with type, message, and timestamp (transient,
                  cleared by SessionStart)

Fail-open: any error silently exits 0 with no output.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, now, project_root, resolve_branch


def capture_failure_data(hook_input):
    """Write _last_failure to the active state file.

    Requires error_type key in hook_input to confirm this is a real
    StopFailure event.
    """
    if "error_type" not in hook_input:
        return

    error_type = hook_input.get("error_type")
    error_message = hook_input.get("error_message", "")

    try:
        root = project_root()
        branch, _ = resolve_branch()
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        timestamp = now()

        def transform(state):
            state["_last_failure"] = {
                "type": error_type,
                "message": error_message,
                "timestamp": timestamp,
            }

        mutate_state(state_path, transform)
    except Exception:
        pass


def main():
    hook_input = {}
    try:
        hook_input = json.load(sys.stdin)
    except Exception:
        pass

    capture_failure_data(hook_input)


if __name__ == "__main__":
    main()
