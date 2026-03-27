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
    current_branch,
    detect_repo,
    mutate_state,
    now,
    project_root,
    write_tab_sequences,
)

_UNSET = object()


def _log(root, branch, message):
    """Write a diagnostic to stderr and (best-effort) the flow log.

    Always writes to stderr first. Then attempts to append to
    .flow-states/<branch>.log if root and branch are known. If logging
    itself fails, the original stderr diagnostic is preserved.
    """
    sys.stderr.write(f"[FLOW stop-continue] {message}\n")
    try:
        if root and branch:
            log_path = root / ".flow-states" / f"{branch}.log"
            with open(log_path, "a") as log_file:
                log_file.write(f"{now()} [stop-continue] {message}\n")
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
        _log(root, branch, f"capture_session_id error: {exc}")


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

        result = {
            "should_block": False,
            "skill": None,
            "context": None,
            "decision": None,
        }

        def transform(state):
            pending = state.get("_continue_pending", "")
            if not pending:
                return

            state_sid = state.get("session_id")
            hook_sid = (hook_input or {}).get("session_id")
            if state_sid and hook_sid and state_sid != hook_sid:
                state["_continue_pending"] = ""
                state["_continue_context"] = ""
                result["decision"] = f"session mismatch (state={state_sid} hook={hook_sid}), cleared pending={pending}"
                return

            result["context"] = state.get("_continue_context", "") or None
            state["_continue_pending"] = ""
            state["_continue_context"] = ""
            result["should_block"] = True
            result["skill"] = pending
            result["decision"] = f"blocking: pending={pending}"

        mutate_state(state_path, transform)

        if result["decision"]:
            _log(root, branch, result["decision"])

        return (result["should_block"], result["skill"], result["context"])
    except Exception as exc:
        _log(root, branch, f"check_continue error: {exc}")
        return (False, None, None)


def clear_blocked(root=None, branch=_UNSET):
    """Clear _blocked flag from the active state file.

    Defense-in-depth counterpart to clear-blocked.py (PostToolUse hook).
    The PostToolUse hook clears _blocked on the normal path (user responds).
    This Stop hook clears it as a safety net for crashed sessions or
    session endings where PostToolUse did not fire.
    """
    try:
        root, branch = _resolve(root, branch)
        if not branch:
            return

        state_path = root / ".flow-states" / f"{branch}.json"
        if not state_path.exists():
            return

        def transform(state):
            state.pop("_blocked", None)

        mutate_state(state_path, transform)
    except Exception as exc:
        _log(root, branch, f"clear_blocked error: {exc}")


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
        _log(root, branch, f"set_tab_title error: {exc}")


def check_qa_pending(root=None):
    """Check for a QA continuation breadcrumb at .flow-states/qa-pending.json.

    The /flow-qa skill writes this file before invoking flow-start --auto.
    After all 6 phases complete, the branch state file is deleted by cleanup.
    This breadcrumb survives cleanup and forces the stop hook to block,
    returning control to the QA skill's remaining steps.

    Returns (should_block: bool, context: str|None).
    Does NOT delete the file — the QA skill handles cleanup.
    Fail-open: any error allows the stop.
    """
    try:
        if root is None:
            root = project_root()
        qa_path = root / ".flow-states" / "qa-pending.json"
        if not qa_path.exists():
            return (False, None)

        data = json.loads(qa_path.read_text())
        context = data.get("_continue_context", "")
        if not context:
            return (False, None)

        return (True, context)
    except Exception:
        return (False, None)


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

    should_block, skill_name, context = check_continue(hook_input, root=root, branch=branch)

    capture_session_id(hook_input, root=root, branch=branch)

    clear_blocked(root=root, branch=branch)

    set_tab_title(root=root, branch=branch)

    # Fallback: check for QA continuation breadcrumb when no branch
    # state file blocked the stop.
    if not should_block:
        qa_block, qa_context = check_qa_pending(root=root)
        if qa_block:
            should_block = True
            skill_name = "flow-complete"
            context = qa_context

    if should_block:
        reason = f"Continue parent phase — child skill '{skill_name}' has returned."
        if context:
            reason += f"\n\nNext steps:\n{context}"
        else:
            reason += " Resume the parent skill instructions."
        print(json.dumps({"decision": "block", "reason": reason}))


if __name__ == "__main__":
    main()
