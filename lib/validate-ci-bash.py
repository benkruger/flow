#!/usr/bin/env python3
"""
PreToolUse hook validator for the ci-fixer sub-agent.

Reads the Claude Code hook input JSON from stdin, checks the Bash
command against blocked patterns, and exits with the appropriate code.

Exit 0 — allow (command passes through to normal permission system)
Exit 2 — block (error message on stderr is fed back to the sub-agent)

Blocked patterns:
1. Compound commands (&&, ;) — "Use separate Bash calls instead"
2. File-read commands (cat, head, tail, grep, rg, find, ls) —
   "Use Read/Glob/Grep tools instead"
"""

import json
import re
import sys

# Commands that have dedicated tool alternatives
FILE_READ_COMMANDS = {"cat", "head", "tail", "grep", "rg", "find", "ls"}


def validate(command):
    """Validate a Bash command string.

    Returns (allowed: bool, message: str).
    message is empty if allowed, otherwise explains why blocked.
    """
    # Block compound commands (&&, ;)
    if "&&" in command or re.search(r"(?<!\\);", command):
        return (False,
                "BLOCKED: Compound commands (&&, ;) are not allowed. "
                "Use separate Bash calls for each command.")

    # Block file-read commands
    first_word = command.strip().split()[0] if command.strip() else ""
    if first_word in FILE_READ_COMMANDS:
        return (False,
                f"BLOCKED: '{first_word}' is not allowed. "
                f"Use the dedicated tool instead "
                f"(Read for cat/head/tail, Grep for grep/rg, "
                f"Glob for find/ls).")

    return (True, "")


def main():
    try:
        hook_input = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        # Can't parse input — allow through, let normal permissions handle it
        sys.exit(0)

    command = hook_input.get("tool_input", {}).get("command", "")
    if not command:
        sys.exit(0)

    allowed, message = validate(command)
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
