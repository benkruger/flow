#!/usr/bin/env python3
"""
PreToolUse hook that blocks Edit/Write on .claude/rules/ and CLAUDE.md
during active FLOW phases, redirecting to bin/flow write-rule.

Fires on Edit and Write tool calls.

Exit 0 — allow (path is not protected, or no FLOW phase active)
Exit 2 — block (path is protected and FLOW phase is active)

Protected paths:
- .claude/rules/ (and subdirectories)
- CLAUDE.md (at any level)

Not protected:
- .claude/settings.json (managed by prime-setup and promote-permissions)
- .claude/settings.local.json (managed by Claude Code itself)
"""

import json
import subprocess
import sys
from pathlib import Path

WORKTREE_MARKER = ".worktrees/"


def _detect_branch_from_cwd():
    """Detect the current branch name from the working directory.

    In a worktree (.worktrees/<branch>/), extracts the branch from
    the path with no subprocess cost. Otherwise falls back to
    ``git branch --show-current`` (one subprocess).
    """
    cwd = str(Path.cwd())
    marker_pos = cwd.find(WORKTREE_MARKER)
    if marker_pos != -1:
        after_marker = cwd[marker_pos + len(WORKTREE_MARKER) :]
        branch = after_marker.split("/")[0]
        return branch if branch else None
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip() or None
    except Exception:
        return None


def _find_project_root():
    """Walk up from CWD looking for .flow-states/ directory.

    Returns the directory containing .flow-states/, or None.
    """
    current = Path.cwd().resolve()
    for directory in [current, *current.parents]:
        if (directory / ".flow-states").is_dir():
            return directory
    return None


def _is_flow_active(branch, project_root):
    """Check if a FLOW feature is active for the given branch."""
    if not branch or project_root is None:
        return False
    state_file = project_root / ".flow-states" / f"{branch}.json"
    return state_file.is_file()


def _is_protected_path(file_path):
    """Check if a file path targets a protected .claude/ location.

    Protected: .claude/rules/ (any depth), CLAUDE.md (any level).
    Not protected: .claude/settings.json, .claude/settings.local.json.
    """
    if not file_path:
        return False

    parts = Path(file_path).parts

    # Check for .claude/rules/ at any depth
    for i, part in enumerate(parts):
        if part == ".claude" and i + 1 < len(parts) and parts[i + 1] == "rules":
            return True

    # Check for CLAUDE.md at any level
    if parts and parts[-1] == "CLAUDE.md":
        return True

    return False


def validate(file_path, flow_active=False):
    """Validate that an Edit/Write on this path is allowed.

    Returns (allowed: bool, message: str).
    """
    if not file_path:
        return (True, "")

    if not flow_active:
        return (True, "")

    if not _is_protected_path(file_path):
        return (True, "")

    return (
        False,
        "BLOCKED: .claude/ paths are protected during FLOW phases. "
        "Use `bin/flow write-rule --path <target> --content-file <temp>` instead. "
        "Write the full file content to a temp file in .flow-states/, "
        "then run the write-rule command.",
    )


def main():
    try:
        hook_input = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    tool_input = hook_input.get("tool_input", {})
    file_path = tool_input.get("file_path") or ""
    if not file_path:
        sys.exit(0)

    project_root = _find_project_root()
    branch = _detect_branch_from_cwd() if project_root is not None else None
    flow_active = _is_flow_active(branch, project_root)

    allowed, message = validate(file_path, flow_active=flow_active)
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
