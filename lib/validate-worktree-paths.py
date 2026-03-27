#!/usr/bin/env python3
"""
PreToolUse hook that blocks file tool calls targeting the main repo
when the working directory is inside a FLOW worktree.

Fires on Edit, Write, Read, Glob, and Grep tool calls.

Exit 0 — allow (path is fine or not in a worktree)
Exit 2 — block (path targets main repo instead of worktree)
"""

import json
import os
import sys

WORKTREE_MARKER = ".worktrees/"


def get_file_path(tool_input):
    """Extract the file path from tool input.

    Edit/Write/Read use 'file_path'. Glob/Grep use 'path'.
    """
    return tool_input.get("file_path") or tool_input.get("path") or ""


def validate(file_path, cwd):
    """Validate that file_path targets the worktree, not the main repo.

    Returns (allowed: bool, message: str).
    """
    if not file_path:
        return (True, "")

    marker_pos = cwd.find(WORKTREE_MARKER)
    if marker_pos == -1:
        return (True, "")

    project_root = cwd[:marker_pos].rstrip("/")

    if not file_path.startswith(project_root + "/"):
        return (True, "")

    if file_path.startswith(cwd + "/") or file_path == cwd:
        return (True, "")

    if file_path.startswith(project_root + "/.flow-states/"):
        return (True, "")

    relative = file_path[len(project_root) + 1 :]
    corrected = cwd + "/" + relative

    return (False, f"BLOCKED: You are in worktree {cwd}. Use {corrected} instead of {file_path}")


def main():
    try:
        hook_input = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        sys.exit(0)

    tool_input = hook_input.get("tool_input", {})
    file_path = get_file_path(tool_input)
    if not file_path:
        sys.exit(0)

    cwd = os.getcwd()

    allowed, message = validate(file_path, cwd)
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
