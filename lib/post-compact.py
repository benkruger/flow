#!/usr/bin/env python3
"""PostCompact hook that captures compaction data in the state file.

When Claude Code compacts context, this hook fires and receives the
compact_summary (conversation summary) and cwd. It writes these to the
active state file so the SessionStart hook can inject them as context
on the next session resume.

Fields written:
  compact_summary — conversation summary (transient, cleared by SessionStart)
  compact_cwd     — CWD at compaction time (transient, cleared by SessionStart)
  compact_count   — total compactions this feature (permanent, incremented)

Fail-open: any error silently exits 0 with no output.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import project_root, resolve_branch


def capture_compact_data(hook_input):
    """Write compact_summary and compact_cwd to the active state file.

    Requires compact_summary key in hook_input to confirm this is a real
    PostCompact event. Increments compact_count on every call.
    """
    if "compact_summary" not in hook_input:
        return

    compact_summary = hook_input.get("compact_summary")
    cwd = hook_input.get("cwd")

    try:
        root = project_root()
        branch, _ = resolve_branch()
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        state = json.loads(state_path.read_text())

        if compact_summary:
            state["compact_summary"] = compact_summary
        if cwd:
            state["compact_cwd"] = cwd
        state["compact_count"] = state.get("compact_count", 0) + 1

        state_path.write_text(json.dumps(state, indent=2))
    except Exception:
        pass


def main():
    hook_input = {}
    try:
        hook_input = json.load(sys.stdin)
    except Exception:
        pass

    capture_compact_data(hook_input)


if __name__ == "__main__":
    main()
