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

from flow_utils import current_branch, detect_repo, find_state_files, mutate_state, project_root, write_tab_sequences


def clear_blocked(hook_input):
    """Clear _blocked from the active state file, then reassert tab title.

    Uses current_branch() for direct branch resolution — not
    resolve_branch() — to avoid the scan-all-state-files fallback
    that could clear the wrong flow's flag in multi-flow environments.

    Tab title reassertion uses find_state_files() fallback so the title
    is written even when the session runs on main (no main.json).
    """
    try:
        root = project_root()
        branch = current_branch()
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"

        # Clear _blocked only from the exact branch's state file
        if state_path.exists():

            def transform(state):
                state.pop("_blocked", None)

            mutate_state(state_path, transform)

        # Reassert tab title after every PostToolUse to reduce flicker
        try:
            if state_path.exists():
                tab_state = json.loads(state_path.read_text())
                write_tab_sequences(tab_state, root=root)
            else:
                results = find_state_files(root, branch)
                if results:
                    _, tab_state, _ = results[0]
                    write_tab_sequences(tab_state, root=root)
                else:
                    write_tab_sequences(repo=detect_repo(), root=root)
        except Exception:
            pass
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
