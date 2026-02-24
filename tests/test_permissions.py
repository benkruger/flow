"""Tests for Bash permission coverage and logging patterns.

Every SKILL.md uses a logging pattern that wraps Bash commands. These tests
ensure the logging pattern doesn't break permission matching, and that every
Bash command in every skill has a matching permission entry.
"""

import re

from conftest import REPO_ROOT, SKILLS_DIR


def _read_skill(name):
    return (SKILLS_DIR / name / "SKILL.md").read_text()


def _all_skill_names():
    return [d.name for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()]


def _logging_skills():
    """Return skill names that have a ## Logging section."""
    return [
        name for name in _all_skill_names()
        if "## Logging" in _read_skill(name)
    ]


def _extract_step6_permissions():
    """Extract permissions from Start Step 6 JSON block."""
    content = _read_skill("start")
    # Find the JSON block inside Step 6 that has "permissions"
    blocks = re.findall(r"```json\s*\n(.*?)```", content, re.DOTALL)
    for block in blocks:
        if '"permissions"' in block and '"allow"' in block:
            # Clean placeholders for parsing
            cleaned = re.sub(r'<[^>]+>', 'placeholder', block)
            import json
            try:
                parsed = json.loads(cleaned)
                return parsed["permissions"]["allow"]
            except (json.JSONDecodeError, KeyError):
                continue
    raise AssertionError("Could not find permissions JSON in start/SKILL.md Step 6")


def _permission_to_regex(perm):
    """Convert a Bash(pattern) permission to a regex.

    Bash(git push) -> ^git push$
    Bash(git push *) -> ^git push .*$
    Bash(bin/ci;*) -> ^bin/ci;.*$
    """
    # Extract the pattern inside Bash(...)
    match = re.match(r"Bash\((.+)\)", perm)
    if not match:
        return None
    pattern = match.group(1)
    # Escape regex-special chars except *, then replace * with .*
    escaped = re.escape(pattern).replace(r"\*", ".*")
    return re.compile("^" + escaped + "$")


# Auto-allowed commands that Claude Code never prompts for (read-only)
AUTO_ALLOWED = {"cd", "cat", "git status", "git diff", "git log", "git branch",
                "git show", "git blame", "git worktree list", "git pull"}


def _extract_primary_command(bash_block):
    """Extract the primary command from a bash code block.

    Strips:
    - cd <path> && prefix
    - COMMAND placeholder
    - Trailing ; EC=$?; ... logging suffix
    """
    line = bash_block.strip()

    # Strip leading blockquote markers (> ) from sub-agent prompts
    lines = line.split("\n")
    lines = [re.sub(r'^>\s*', '', l) for l in lines]
    line = "\n".join(lines).strip()

    # Skip template placeholders
    if "COMMAND" in line:
        return None

    # Strip cd prefix: cd <path> && REST -> REST
    line = re.sub(r'^cd\s+\S+\s*&&\s*', '', line)

    # Take only the first command in a chain (before ;)
    # But handle git commit -F /tmp/... && rm ... as one unit
    if "; EC=$?" in line:
        line = line.split("; EC=$?")[0]
    elif ";" in line and "&&" not in line:
        line = line.split(";")[0]

    # Strip trailing continuation
    line = line.strip()

    # Collapse multi-line (backslash continuation)
    line = re.sub(r'\s*\\\n\s*', ' ', line)

    return line if line else None


# --- Tests ---


def test_logging_uses_project_local_path():
    """Every skill's ## Logging section must reference .claude/flow-states/,
    not /tmp/."""
    for name in _logging_skills():
        content = _read_skill(name)
        # Find the ## Logging section
        logging_match = re.search(
            r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
        )
        assert logging_match, f"skills/{name}/SKILL.md has ## Logging header but no content"
        logging_section = logging_match.group(1)

        assert "/tmp/" not in logging_section, (
            f"skills/{name}/SKILL.md ## Logging section references /tmp/ — "
            f"must use .claude/flow-states/<branch>.log instead"
        )
        assert ".claude/flow-states/" in logging_section, (
            f"skills/{name}/SKILL.md ## Logging section does not reference "
            f".claude/flow-states/ for the log path"
        )


def test_logging_template_is_command_first():
    """The ```bash``` block inside ## Logging must start with COMMAND,
    not with date."""
    for name in _logging_skills():
        content = _read_skill(name)
        logging_match = re.search(
            r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
        )
        assert logging_match, f"skills/{name}/SKILL.md has ## Logging header but no content"
        logging_section = logging_match.group(1)

        # Find the bash block inside the logging section
        bash_match = re.search(r"```bash\s*\n(.+?)```", logging_section, re.DOTALL)
        assert bash_match, f"skills/{name}/SKILL.md ## Logging has no ```bash``` block"
        bash_content = bash_match.group(1).strip()

        assert bash_content.startswith("COMMAND"), (
            f"skills/{name}/SKILL.md ## Logging bash template must start "
            f"with COMMAND, not '{bash_content[:30]}...'"
        )


def test_exact_permissions_have_logged_variants():
    """Every exact-match permission Bash(foo) (no trailing *) in Start Step 6
    must have a corresponding Bash(foo;*) entry — unless a wildcard sibling
    already covers the logged form (e.g. Bash(rubocop *) covers rubocop -A;...)."""
    permissions = _extract_step6_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    # Find exact-match permissions (no trailing *)
    exact = [p for p in permissions if not p.endswith("*)")]

    for perm in exact:
        # Extract the command pattern
        match = re.match(r"Bash\((.+)\)", perm)
        if not match:
            continue
        cmd = match.group(1)
        variant = f"Bash({cmd};*)"

        # Check if variant exists directly
        if variant in permissions:
            continue

        # Check if a wildcard sibling already covers the logged form
        # e.g. "rubocop -A; EC=$?" matches "Bash(rubocop *)"
        test_logged = f"{cmd}; EC=$?"
        covered = any(r.match(test_logged) for r in regexes)
        assert covered, (
            f"Exact-match permission '{perm}' in Start Step 6 has no "
            f"logged variant '{variant}' and no wildcard permission "
            f"covers '{test_logged}'. Add a variant to support the "
            f"command-first logging pattern."
        )


def test_all_bash_commands_have_permission_coverage():
    """Every ```bash``` block in all SKILL.md and docs/*.md files must match
    at least one permission from Start Step 6 or be in the auto-allowed set."""
    permissions = _extract_step6_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    errors = []

    # Collect all files to check
    files_to_check = []
    for name in _all_skill_names():
        files_to_check.append(
            (f"skills/{name}/SKILL.md", _read_skill(name))
        )
    for doc in sorted((REPO_ROOT / "docs").iterdir()):
        if doc.suffix == ".md":
            files_to_check.append(
                (f"docs/{doc.name}", doc.read_text())
            )

    for filepath, content in files_to_check:
        # Find all ```bash blocks
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            cmd = _extract_primary_command(block)
            if cmd is None:
                continue

            # Check if auto-allowed
            is_auto = False
            for allowed in AUTO_ALLOWED:
                if cmd == allowed or cmd.startswith(allowed + " "):
                    is_auto = True
                    break
            if is_auto:
                continue

            # Check against permission regexes
            matched = False
            for regex in regexes:
                if regex.match(cmd):
                    matched = True
                    break

            if not matched:
                errors.append(
                    f"{filepath}: command '{cmd}' has no matching permission "
                    f"in Start Step 6. Add a Bash({cmd} *) or Bash({cmd}) "
                    f"entry to the permissions block."
                )

    assert not errors, (
        f"Found {len(errors)} Bash command(s) without permission coverage:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )