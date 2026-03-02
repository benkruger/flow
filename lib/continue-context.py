"""Build continue-context for /flow:continue Path B.

Usage: bin/flow continue-context

Combines branch detection, state file reading, and panel formatting
into a single call — replacing three separate tool calls in Path B.

Output (JSON to stdout):
  Success: {"status": "ok", "panel": "...", "worktree": "...",
            "current_phase": N, "phase_name": "...",
            "phase_command": "/flow:..."}
  No state: {"status": "no_state", "branch": "..."}
  Error:   {"status": "error", "message": "..."}
"""

import importlib.util
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import current_branch, project_root, PHASE_NAMES

# Import format_panel from format-status.py (same pattern as tests)
_spec = importlib.util.spec_from_file_location(
    "format_status", Path(__file__).resolve().parent / "format-status.py"
)
_fs_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_fs_mod)

format_panel = _fs_mod.format_panel

COMMANDS = {
    1: "/flow:start", 2: "/flow:plan", 3: "/flow:code",
    4: "/flow:review", 5: "/flow:security", 6: "/flow:reflect",
    7: "/flow:cleanup",
}


def main():
    root = project_root()
    branch = current_branch()

    if not branch:
        print(json.dumps({
            "status": "error",
            "message": "Could not determine current branch",
        }))
        sys.exit(1)

    state_path = root / ".flow-states" / f"{branch}.json"

    if not state_path.exists():
        print(json.dumps({"status": "no_state", "branch": branch}))
        sys.exit(0)

    try:
        state = json.loads(state_path.read_text())
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Could not read state file: {e}",
        }))
        sys.exit(1)

    version = _fs_mod._read_version()
    panel = format_panel(state, version)

    current_phase = state.get("current_phase", 1)
    print(json.dumps({
        "status": "ok",
        "panel": panel,
        "worktree": state.get("worktree", ""),
        "current_phase": current_phase,
        "phase_name": PHASE_NAMES.get(current_phase, f"Phase {current_phase}"),
        "phase_command": COMMANDS.get(current_phase, f"/flow:phase{current_phase}"),
    }))


if __name__ == "__main__":
    main()