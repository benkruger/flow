#!/usr/bin/env python3
"""Stop hook that forces continuation when _continue_pending is set.

When a phase skill sets _continue_pending=<skill_name> in the state file
before invoking a child skill, this hook fires when the model tries to
end its turn. If the flag is non-empty, the hook clears it and blocks
the stop, forcing Claude to continue generating and follow the parent
skill's remaining instructions.

Fail-open with error reporting: any error allows the stop (exit 0, no
block output), but writes a diagnostic to stderr and attempts to log
to .flow-states/<branch>.log for post-mortem visibility.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import (
    current_branch, detect_repo, mutate_state, now, project_root,
    write_tab_sequences,
)


_UNSET = object()


def _log_error(root, branch, tag, exc):
    """Write a fail-open diagnostic to stderr and (best-effort) the flow log.

    Always writes to stderr first. Then attempts to append to
    .flow-states/<branch>.log if root and branch are known. If logging
    itself fails, the original stderr diagnostic is preserved.
    """
    sys.stderr.write(f"[FLOW stop-continue] {tag} error: {exc}\n")
    try:
        if root and branch:
            log_path = root / ".flow-states" / f"{branch}.log"
            with open(log_path, "a") as log_file:
                log_file.write(
                    f"{now()} [stop-continue] {tag} error: {exc}\n"
                )
    except Exception:
        pass


def _resolve(root, branch):
    """Resolve root and branch defaults from environment.

    root=None → project_root(); branch=_UNSET → current_branch().
    Passing branch=None explicitly (e.g. in tests) skips auto-detect.
    """
    if root is None:
        root = project_root()
    if branch is _UNSET:
        branch = current_branch()
    return root, branch


def capture_session_id(hook_input, root=None, branch=_UNSET):
    """Update session_id and transcript_path in active state file."""
    session_id = hook_input.get("session_id")
    if not session_id:
        return

    try:
        root, branch = _resolve(root, branch)
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        def transform(state):
            if state.get("session_id") == session_id:
                return
            state["session_id"] = session_id
            transcript_path = hook_input.get("transcript_path")
            if transcript_path:
                state["transcript_path"] = transcript_path

        mutate_state(state_path, transform)
    except Exception as exc:
        _log_error(root, branch, "capture_session_id", exc)


def check_continue(hook_input=None, root=None, branch=_UNSET):
    """Check if _continue_pending flag is set in the active state file.

    Returns (should_block: bool, skill_name: str|None, context: str|None).
    If should_block is True, both _continue_pending and _continue_context
    have been cleared in the state file.

    Session isolation: if the state file's session_id differs from the
    hook input's session_id, the flag is stale (set by a previous session).
    Clear it and allow stop. Backward compatible: if either session_id is
    missing, skip the check and fire the flag as before.

    Fail-open with diagnostics: on any exception, writes a one-line error
    to stderr and attempts to log to .flow-states/<branch>.log if the
    branch is known.
    """
    try:
        root, branch = _resolve(root, branch)

        if not branch:
            return (False, None, None)

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return (False, None, None)

        result = {"should_block": False, "skill": None, "context": None}

        def transform(state):
            pending = state.get("_continue_pending", "")
            if not pending:
                return

            state_sid = state.get("session_id")
            hook_sid = (hook_input or {}).get("session_id")
            if state_sid and hook_sid and state_sid != hook_sid:
                state["_continue_pending"] = ""
                state["_continue_context"] = ""
                return

            result["context"] = state.get("_continue_context", "") or None
            state["_continue_pending"] = ""
            state["_continue_context"] = ""
            result["should_block"] = True
            result["skill"] = pending

        mutate_state(state_path, transform)
        return (result["should_block"], result["skill"], result["context"])
    except Exception as exc:
        _log_error(root, branch, "check_continue", exc)
        return (False, None, None)


def set_tab_title(root=None, branch=_UNSET):
    """Write the current FLOW phase and repo color to the terminal tab via /dev/tty.

    Delegates to write_tab_sequences() for the actual escape sequence
    building and tty writing. This wrapper handles root/branch resolution
    and fail-open error logging.
    """
    try:
        root, branch = _resolve(root, branch)
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if state_path.exists():
            state = json.loads(state_path.read_text())
            write_tab_sequences(state, root=root)
        else:
            write_tab_sequences(repo=detect_repo(), root=root)
    except Exception as exc:
        _log_error(root, branch, "set_tab_title", exc)


def main():
    hook_input = {}
    try:
        hook_input = json.load(sys.stdin)
    except Exception:
        pass

    try:
        root = project_root()
        branch = current_branch()
    except Exception:
        return

    if not branch:
        return

    should_block, skill_name, context = check_continue(
        hook_input, root=root, branch=branch
    )

    capture_session_id(hook_input, root=root, branch=branch)

    set_tab_title(root=root, branch=branch)

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
