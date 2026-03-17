#!/usr/bin/env python3
"""Stop hook that forces continuation when _continue_pending is set.

When a phase skill sets _continue_pending=<skill_name> in the state file
before invoking a child skill, this hook fires when the model tries to
end its turn. If the flag is non-empty, the hook clears it and blocks
the stop, forcing Claude to continue generating and follow the parent
skill's remaining instructions.

Fail-open: any error silently allows the stop (exit 0, no output).
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import project_root, resolve_branch


def capture_session_id(hook_input):
    """Update session_id and transcript_path in active state file."""
    session_id = hook_input.get("session_id")
    if not session_id:
        return

    try:
        root = project_root()
        branch, _ = resolve_branch()
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        state = json.loads(state_path.read_text())
        if state.get("session_id") == session_id:
            return  # Already set, skip write

        state["session_id"] = session_id
        transcript_path = hook_input.get("transcript_path")
        if transcript_path:
            state["transcript_path"] = transcript_path

        state_path.write_text(json.dumps(state, indent=2))
    except Exception:
        pass  # Fail-open, same as check_continue


def check_continue():
    """Check if _continue_pending flag is set in the active state file.

    Returns (should_block: bool, skill_name: str|None, context: str|None).
    If should_block is True, both _continue_pending and _continue_context
    have been cleared in the state file.
    """
    try:
        root = project_root()
        branch, _ = resolve_branch()

        if not branch:
            return (False, None, None)

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return (False, None, None)

        state = json.loads(state_path.read_text())
        pending = state.get("_continue_pending", "")

        if not pending:
            return (False, None, None)

        context = state.get("_continue_context", "") or None

        state["_continue_pending"] = ""
        state["_continue_context"] = ""
        state_path.write_text(json.dumps(state, indent=2))

        return (True, pending, context)
    except Exception:
        return (False, None, None)


def main():
    hook_input = {}
    try:
        hook_input = json.load(sys.stdin)
    except Exception:
        pass

    capture_session_id(hook_input)

    should_block, skill_name, context = check_continue()

    if should_block:
        reason = (
            f"Continue parent phase — child skill '{skill_name}' "
            f"has returned."
        )
        if context:
            reason += f"\n\nNext steps:\n{context}"
        else:
            reason += " Resume the parent skill instructions."
        print(json.dumps({"decision": "block", "reason": reason}))


if __name__ == "__main__":
    main()
