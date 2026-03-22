#!/usr/bin/env python3
"""PostToolUse hook for AskUserQuestion — clears _blocked flag.

After the user responds to an AskUserQuestion, this hook fires and
clears the _blocked timestamp from the state file so the TUI stops
showing the flow as blocked.

Fail-open: any error silently exits 0 with no output.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import mutate_state, project_root, resolve_branch


def clear_blocked(hook_input):
    """Clear _blocked from the active state file.

    Resolves the current branch and project root, then removes
    _blocked via mutate_state. If _blocked is not present, does nothing.
    """
    try:
        root = project_root()
        branch, _ = resolve_branch()
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        def transform(state):
            state.pop("_blocked", None)

        mutate_state(state_path, transform)
    except Exception:
        pass


def main():
    hook_input = {}
    try:
        hook_input = json.load(sys.stdin)
    except Exception:
        pass

    clear_blocked(hook_input)


if __name__ == "__main__":
    main()
