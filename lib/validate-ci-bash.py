#!/usr/bin/env python3
"""
Global PreToolUse hook validator for all Bash commands.

Reads the Claude Code hook input JSON from stdin, checks the Bash
command against blocked patterns, and exits with the appropriate code.

Exit 0 — allow (command passes through to normal permission system)
Exit 2 — block (error message on stderr is fed back to the sub-agent)

Validation layers (in order):
1. Compound commands (&&, ;, |) — "Use separate Bash calls instead"
2. Shell redirection (>, >>, 2>, etc.) — "Use Read/Write tools instead"
3. Blanket restore (git restore .) — "Restore files individually"
4. Deny list — command matches a deny pattern in settings.json
5. File-read commands (cat, head, tail, grep, rg, find, ls) —
   "Use Read/Glob/Grep tools instead"
6. Whitelist — command must match a Bash(...) allow pattern in
   .claude/settings.json. If settings.json is missing or unparseable,
   fall through (don't break non-FLOW projects).
"""

import json
import re
import sys
from pathlib import Path

from flow_utils import permission_to_regex

# Commands that have dedicated tool alternatives
FILE_READ_COMMANDS = {"cat", "head", "tail", "grep", "rg", "find", "ls"}


def _find_settings_and_root():
    """Walk up from CWD looking for .claude/settings.json.

    Returns (settings_dict, project_root) where project_root is the
    directory containing .claude/. Returns (None, None) if not found
    or unparseable.
    """
    current = Path.cwd().resolve()
    for directory in [current, *current.parents]:
        settings_path = directory / ".claude" / "settings.json"
        if settings_path.is_file():
            try:
                return json.loads(settings_path.read_text()), directory
            except (json.JSONDecodeError, ValueError, OSError):
                return None, None
    return None, None


def _build_permission_regexes(settings, list_key):
    """Extract Bash(...) patterns from settings and compile to regexes.

    Args:
        settings: The parsed .claude/settings.json dict.
        list_key: Either "allow" or "deny".
    """
    entries = settings.get("permissions", {}).get(list_key, [])
    regexes = []
    for entry in entries:
        regex = permission_to_regex(entry)
        if regex is not None:
            regexes.append(regex)
    return regexes


def validate(command, settings=None):
    """Validate a Bash command string.

    Returns (allowed: bool, message: str).
    message is empty if allowed, otherwise explains why blocked.

    If settings is provided, also checks command against the allow-list
    whitelist. If settings is None, the whitelist check is skipped.
    """
    # Block compound commands (&&, ;, |)
    if "&&" in command or re.search(r"(?<!\\);", command) or "|" in command:
        return (False,
                "BLOCKED: Compound commands (&&, ;, |) are not allowed. "
                "Use separate Bash calls for each command.")

    # Block shell redirection operators (>, >>, 2>, etc.)
    if re.search(r"(?<![=\-])>{1,2}", command):
        return (False,
                "BLOCKED: Shell redirection (>, >>) is not allowed. "
                "Use the Read tool to view file contents and the "
                "Write tool to create files.")

    # Block blanket restore (git restore . wipes all changes without review)
    stripped = command.strip()
    if stripped == "git restore .":
        return (False,
                "BLOCKED: 'git restore .' discards ALL changes without review. "
                "Use 'git restore <file>' for each file individually. "
                "Before restoring, run 'git diff' to capture what will be lost.")

    # Deny-list check — deny always wins over allow
    if settings is not None:
        deny_regexes = _build_permission_regexes(settings, "deny")
        if deny_regexes:
            for regex in deny_regexes:
                if regex.match(stripped):
                    return (False,
                            f"BLOCKED: Command matches deny list: '{command}'. "
                            f"This operation is explicitly forbidden.")

    # Block file-read commands
    first_word = stripped.split()[0] if stripped else ""
    if first_word in FILE_READ_COMMANDS:
        return (False,
                f"BLOCKED: '{first_word}' is not allowed. "
                f"Use the dedicated tool instead "
                f"(Read for cat/head/tail, Grep for grep/rg, "
                f"Glob for find/ls).")

    # Whitelist check — only if settings are available
    if settings is not None:
        regexes = _build_permission_regexes(settings, "allow")
        if regexes:
            matched = any(r.match(command) for r in regexes)
            if not matched:
                return (False,
                        f"BLOCKED: Command not in allow list: '{command}'. "
                        f"Check .claude/settings.json allow patterns.")

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

    settings, _project_root = _find_settings_and_root()
    allowed, message = validate(command, settings=settings)
    if not allowed:
        print(message, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
