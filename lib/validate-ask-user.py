#!/usr/bin/env python3
"""
PreToolUse hook for AskUserQuestion — enforces auto-continue.

When `_auto_continue` is set in the state file, answers AskUserQuestion
automatically via `updatedInput` (JSON on stdout with exit 0). This
prevents the model from prompting the user when autonomous phase
transitions are configured.

Exit 0 — allow (optionally with JSON on stdout for updatedInput)
"""

import json
import sys
from pathlib import Path


def set_blocked(state_path):
    """Write _blocked timestamp to the state file.

    Best-effort: any error is silently ignored so the hook
    never interferes with AskUserQuestion delivery.
    """
    try:
        if state_path is None or not Path(state_path).exists():
            return

        # Lazy import to avoid flow_utils module-level flow-phases.json load
        # on every hook invocation — this hook only needs mutate_state/now.
        from flow_utils import mutate_state, now

        def transform(state):
            state["_blocked"] = now()

        mutate_state(Path(state_path), transform)
    except Exception:
        pass


def validate(state_path):
    """Check auto-continue state and return hook response if active.

    Returns (allowed: bool, message: str, hook_response: dict | None).
    When hook_response is not None, main() prints it as JSON to stdout
    so Claude Code receives it as an updatedInput answer.
    """
    if state_path is None or not Path(state_path).exists():
        return (True, "", None)

    try:
        state = json.loads(Path(state_path).read_text())
    except (json.JSONDecodeError, ValueError):
        return (True, "", None)

    auto_cmd = state.get("_auto_continue")
    if not auto_cmd:
        return (True, "", None)

    return (
        True,
        "",
        {
            "permissionDecision": "allow",
            "updatedInput": f"Yes, proceed. Invoke {auto_cmd} now.",
        },
    )


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
    allowed, message, hook_response = validate(str(state_path))
    if hook_response:
        print(json.dumps(hook_response))
        sys.exit(0)

    set_blocked(str(state_path))
    sys.exit(0)


if __name__ == "__main__":
    main()
