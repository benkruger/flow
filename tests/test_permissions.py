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


def _all_docs_files():
    """Return (relative_path, content) for all .md files in docs/, recursively."""
    result = []
    for path in sorted(REPO_ROOT.joinpath("docs").rglob("*.md")):
        rel = path.relative_to(REPO_ROOT)
        result.append((str(rel), path.read_text()))
    return result


def _logging_skills():
    """Return skill names that have a ## Logging section."""
    return [
        name for name in _all_skill_names()
        if "## Logging" in _read_skill(name)
    ]


def _extract_step6_permissions():
    """Extract permissions from Start Step 4 JSON block."""
    content = _read_skill("start")
    # Find the JSON block inside Step 4 that has "permissions"
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
    raise AssertionError("Could not find permissions JSON in start/SKILL.md Step 4")


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

    # Skip angle-bracket placeholders (e.g. <branch>, <feature-name>)
    # These are documentation templates, not executable commands
    if re.search(r"<[a-z_-]+>", line, re.IGNORECASE):
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


def _extract_full_command(bash_block):
    """Extract the full command from a bash code block, preserving cd prefix.

    Like _extract_primary_command but does NOT strip the cd prefix.
    Used to verify that cd-prefixed commands match a permission pattern as-is.
    """
    line = bash_block.strip()

    lines = line.split("\n")
    lines = [re.sub(r'^>\s*', '', l) for l in lines]
    line = "\n".join(lines).strip()

    if "COMMAND" in line:
        return None

    # Skip angle-bracket placeholders (e.g. <branch>, <feature-name>)
    if re.search(r"<[a-z_-]+>", line, re.IGNORECASE):
        return None

    # NOTE: cd prefix is NOT stripped — preserves the full command

    if "; EC=$?" in line:
        line = line.split("; EC=$?")[0]
    elif ";" in line and "&&" not in line:
        line = line.split(";")[0]

    line = line.strip()
    line = re.sub(r'\s*\\\n\s*', ' ', line)

    return line if line else None


# --- Tests ---


def test_no_bash_commands_reference_tmp():
    """No ```bash``` block in any SKILL.md or docs/*.md should reference /tmp/.

    Paths outside the project directory trigger permission prompts that
    project-level settings.json cannot suppress. All file operations must
    stay inside the project root."""
    errors = []

    files_to_check = []
    for name in _all_skill_names():
        files_to_check.append(
            (f"skills/{name}/SKILL.md", _read_skill(name))
        )
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if "/tmp/" in block:
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block references /tmp/: '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) referencing /tmp/. Paths outside "
        f"the project trigger permission prompts that settings.json cannot "
        f"suppress. Use project-local paths instead.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


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
    """Every exact-match permission Bash(foo) (no trailing *) in Start Step 4
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
            f"Exact-match permission '{perm}' in Start Step 4 has no "
            f"logged variant '{variant}' and no wildcard permission "
            f"covers '{test_logged}'. Add a variant to support the "
            f"command-first logging pattern."
        )


def test_all_bash_commands_have_permission_coverage():
    """Every ```bash``` block in all SKILL.md and docs/*.md files must match
    at least one permission from Start Step 4 or be in the auto-allowed set."""
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
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

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
                    f"in Start Step 4. Add a Bash({cmd} *) or Bash({cmd}) "
                    f"entry to the permissions block."
                )

    assert not errors, (
        f"Found {len(errors)} Bash command(s) without permission coverage:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_cd_prefixed_commands_have_full_permission_coverage():
    """Bash blocks with a cd prefix must match a permission pattern as-is,
    without stripping the cd. This prevents Claude from dropping the cd
    to match a simpler pattern and running from the wrong directory."""
    permissions = _extract_step6_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    cd_pattern = re.compile(r'^cd\s+\S+\s*&&\s*')
    errors = []

    files_to_check = []
    for name in _all_skill_names():
        files_to_check.append(
            (f"skills/{name}/SKILL.md", _read_skill(name))
        )
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            full_cmd = _extract_full_command(block)
            if full_cmd is None:
                continue

            if not cd_pattern.match(full_cmd):
                continue

            matched = any(r.match(full_cmd) for r in regexes)
            if not matched:
                errors.append(
                    f"{filepath}: cd-prefixed command '{full_cmd}' has no "
                    f"matching permission. Add a pattern like "
                    f"'Bash(cd .worktrees/* && *)' to cover worktree commands."
                )

    assert not errors, (
        f"Found {len(errors)} cd-prefixed command(s) without permission coverage:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_permissions_step_precedes_worktree_commands():
    """The permissions step in start/SKILL.md must come before any step
    that runs cd .worktrees/ commands. Otherwise the cd-prefixed commands
    execute before their permission patterns exist in .claude/settings.json."""
    content = _read_skill("start")

    perm_match = re.search(
        r'### Step (\d+) — .*[Pp]ermission',
        content
    )
    assert perm_match, "Could not find permissions step in start/SKILL.md"
    perm_step = int(perm_match.group(1))

    steps_with_cd = set()
    for match in re.finditer(
        r'### Step (\d+) — .*?\n(.*?)(?=### Step \d+|### Done|\Z)',
        content, re.DOTALL
    ):
        step_num = int(match.group(1))
        step_content = match.group(2)
        # Only check bash blocks — ignore cd .worktrees/ in JSON permission strings
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", step_content, re.DOTALL)
        if any('cd .worktrees/' in block for block in bash_blocks):
            steps_with_cd.add(step_num)

    assert steps_with_cd, "No steps with cd .worktrees/ found in start/SKILL.md"

    earliest_cd_step = min(steps_with_cd)
    assert perm_step < earliest_cd_step, (
        f"Permissions step (Step {perm_step}) must come before the first "
        f"worktree command step (Step {earliest_cd_step}). Otherwise "
        f"cd-prefixed commands won't match any permission pattern."
    )