"""Tests for Bash permission coverage.

Every Bash command in every skill must have a matching permission entry in
init/SKILL.md (for plugin skills) or .claude/settings.json (for maintainer
skills). Prohibition tests enforce that bash blocks never contain patterns
that break permission matching or shell behavior.
"""

import json
import re

from conftest import LIB_DIR, REPO_ROOT, SKILLS_DIR

MAINTAINER_SKILLS_DIR = REPO_ROOT / ".claude" / "skills"
SETTINGS_JSON = REPO_ROOT / ".claude" / "settings.json"


def _read_skill(name):
    return (SKILLS_DIR / name / "SKILL.md").read_text()


def _all_skill_names():
    return [d.name for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()]


def _all_plugin_skill_files():
    """Return (relative_path, content) for all .md files in all skill dirs.

    Includes SKILL.md and any fragment files (rails.md, python.md, etc.).
    """
    result = []
    for skill_dir in sorted(SKILLS_DIR.iterdir()):
        if not skill_dir.is_dir():
            continue
        for md_file in sorted(skill_dir.glob("*.md")):
            rel = md_file.relative_to(REPO_ROOT)
            result.append((str(rel), md_file.read_text()))
    return result


def _all_docs_files():
    """Return (relative_path, content) for all .md files in docs/, recursively."""
    result = []
    for path in sorted(REPO_ROOT.joinpath("docs").rglob("*.md")):
        rel = path.relative_to(REPO_ROOT)
        result.append((str(rel), path.read_text()))
    return result


def _logging_skills():
    """Return skill names that actively log (have ## Logging with content).

    Skills that keep ## Logging but explicitly opt out (e.g., cleanup says
    'No logging for this phase') are excluded from detailed logging checks.
    """
    result = []
    for name in _all_skill_names():
        content = _read_skill(name)
        if "## Logging" not in content:
            continue
        match = re.search(
            r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
        )
        if match and "No logging" in match.group(1):
            continue
        result.append(name)
    return result


def _extract_init_permissions_block():
    """Extract the full permissions dict from init/SKILL.md reference JSON."""
    content = _read_skill("init")
    blocks = re.findall(r"```json\s*\n(.*?)```", content, re.DOTALL)
    for block in blocks:
        if '"permissions"' in block and '"allow"' in block:
            cleaned = re.sub(r'<[^>]+>', 'placeholder', block)
            try:
                parsed = json.loads(cleaned)
                return parsed["permissions"]
            except (json.JSONDecodeError, KeyError):
                continue
    raise AssertionError("Could not find permissions JSON in init/SKILL.md")


def _extract_init_permissions():
    """Extract the allow list from init/SKILL.md reference JSON."""
    return _extract_init_permissions_block()["allow"]


def _extract_init_deny():
    """Extract the deny list from init/SKILL.md reference JSON."""
    return _extract_init_permissions_block()["deny"]


def _concrete_example(perm):
    """Generate a representative command from a Bash(pattern) permission.

    Replaces * with 'test-value' to create a concrete command that should
    match the pattern. Returns None for non-Bash entries.
    """
    match = re.match(r"Bash\((.+)\)", perm)
    if not match:
        return None
    return match.group(1).replace("*", "test-value")


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
AUTO_ALLOWED = {"cd", "git status", "git diff", "git log", "git branch",
                "git show", "git blame", "git worktree list",
                "git rev-parse"}

# Commands that touch ~/.claude/ paths. Claude Code's built-in sensitive-path
# protection always prompts for these regardless of settings.json entries.
# Excluded from permission coverage checks because no allow entry can help.
SENSITIVE_PATH_COMMANDS = {
    "rm -rf ~/.claude/plugins/cache/flow-marketplace",
}


PLACEHOLDER_SUBS = {
    "<feature-name>": "test-feature",
    "<branch>": "test-branch",
    "<project_root>": "/tmp/test",
    "<framework>": "rails",
    "<worktree_path>": ".worktrees/test-branch",
    "<pr_number>": "123",
    "<note_text>": "test note",
    "<current-branch>": "test-branch",
    "<test/path/to/file_test.rb>": "test/models/user_test.rb",
    "<tests/path/to/test_file.py>": "tests/test_foo.py",
    "<N>": "2",
    "<path>": "design.approved_at",
    "<plan_file_path>": ".claude/plans/test-plan.md",
    "<issue_title>": "Test issue title",
    "<issue_body>": "Test issue body",
}


def _substitute_placeholders(line):
    """Replace angle-bracket placeholders with concrete test values.

    Returns the substituted line, or None if unrecognized placeholders remain.
    """
    for placeholder, value in PLACEHOLDER_SUBS.items():
        line = line.replace(placeholder, value)
    # If any angle-bracket placeholders remain, skip (safety net)
    if re.search(r"<[a-z_/-]+>", line, re.IGNORECASE):
        return None
    return line


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

    # Substitute angle-bracket placeholders with concrete test values
    line = _substitute_placeholders(line)
    if line is None:
        return None

    # Strip cd prefix: cd <path> && REST -> REST
    line = re.sub(r'^cd\s+\S+\s*&&\s*', '', line)

    # Take only the first command in a chain (before ;)
    # But handle git commit -F /tmp/... && rm ... as one unit
    if ";" in line and "&&" not in line:
        line = line.split(";")[0]

    # Strip trailing continuation
    line = line.strip()

    # Collapse multi-line (backslash continuation)
    line = re.sub(r'\s*\\\n\s*', ' ', line)

    # Take only the first line (multi-line blocks list alternatives)
    if "\n" in line:
        line = line.split("\n")[0].strip()

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

    # Substitute angle-bracket placeholders with concrete test values
    line = _substitute_placeholders(line)
    if line is None:
        return None

    # NOTE: cd prefix is NOT stripped — preserves the full command

    if ";" in line and "&&" not in line:
        line = line.split(";")[0]

    line = line.strip()
    line = re.sub(r'\s*\\\n\s*', ' ', line)

    # Take only the first line (multi-line blocks list alternatives)
    if "\n" in line:
        line = line.split("\n")[0].strip()

    return line if line else None


# --- Tests ---


def test_no_bash_commands_reference_tmp():
    """No ```bash``` block in any SKILL.md or docs/*.md should reference /tmp/.

    Paths outside the project directory trigger permission prompts that
    project-level settings.json cannot suppress. All file operations must
    stay inside the project root."""
    errors = []

    files_to_check = _all_plugin_skill_files()
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


def test_no_command_substitution_in_bash_blocks():
    """No ```bash``` block in any SKILL.md or docs/*.md should contain $().

    Command substitution like $(date ...) triggers Claude Code's security
    prompt, which settings.json cannot suppress. Use the Read+Write pattern
    instead: run the command, capture the exit code, then use Claude's
    Read/Write tools to append the log line with the timestamp."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if "$(" in block:
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block contains $(): '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) containing $(). Command "
        f"substitution triggers Claude Code's security prompt, which "
        f"settings.json cannot suppress. Use the Read+Write logging "
        f"pattern instead.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_bash_redirects_to_dot_claude():
    """No ```bash``` block should use >> to redirect into .claude/ paths.

    Claude Code applies built-in protection for .claude/ paths that
    settings.json cannot suppress. The mkdir fix (commit 6aaa8e4) switched
    to the Write tool; logging must do the same. Use Read + Write instead
    of Bash >> for any .claude/ path."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if ">>" in block and ".claude/" in block:
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block redirects to .claude/: '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) using >> to redirect into .claude/ "
        f"paths. Claude Code's built-in .claude/ directory protection triggers "
        f"permission prompts that settings.json cannot suppress. Use Read + "
        f"Write tools instead of Bash >>.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_logging_uses_project_local_path():
    """Every skill's ## Logging section must reference .flow-states/,
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
            f"must use .flow-states/<branch>.log instead"
        )
        assert ".flow-states/" in logging_section, (
            f"skills/{name}/SKILL.md ## Logging section does not reference "
            f".flow-states/ for the log path"
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


def test_no_exit_in_bash_blocks():
    """No ```bash``` block in any SKILL.md or docs/*.md should contain exit.

    Appending '; EC=$?; exit $EC' (or any exit variant) to bash commands
    turns them into compound commands that bypass permission matching and
    kill the shell, breaking working directory persistence. The Bash tool
    returns the exit code natively — the wrapper adds nothing."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if "; exit" in block or "exit $" in block:
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block contains exit: '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) containing exit commands. "
        f"Chained exit commands ('; EC=$?; exit $EC') break permission "
        f"matching (compound commands don't match simple patterns) and "
        f"kill the shell (breaking working directory persistence). The "
        f"Bash tool returns exit codes natively — just run COMMAND alone.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_heredoc_in_bash_blocks():
    """No ```bash``` block in any SKILL.md or docs/*.md should contain heredoc syntax.

    Heredoc (<<) triggers Claude Code's "Shell expansion syntax in paths
    requires manual approval" heuristic, causing permission prompts that
    settings.json cannot suppress. Output banners as text in the response,
    not via Bash."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if "<<" in block:
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block contains heredoc (<<): '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) containing heredoc syntax (<<). "
        f"Heredoc triggers Claude Code's shell expansion heuristic, causing "
        f"permission prompts that settings.json cannot suppress. Output "
        f"banners as text in the response, not via Bash.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_cd_compound_in_bash_blocks():
    """No ```bash``` block in any SKILL.md or docs/*.md should contain cd && compound commands.

    Claude Code's "bare repository attacks" heuristic fires on any `cd <path> && git`
    compound command, causing permission prompts that settings.json cannot suppress.
    Use `git -C <path>` instead, or run commands from the current directory."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            if re.search(r'\bcd\s+\S+\s*&&', block):
                cmd = block.strip().split("\n")[0]
                errors.append(
                    f"{filepath}: bash block contains cd && compound: '{cmd}'"
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) containing cd && compound commands. "
        f"Claude Code's 'bare repository attacks' heuristic fires on cd <path> && "
        f"git commands, causing permission prompts that settings.json cannot "
        f"suppress. Use git -C <path> instead.\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_all_bash_commands_have_permission_coverage():
    """Every ```bash``` block in all SKILL.md and docs/*.md files must match
    at least one permission from init/SKILL.md or be in the auto-allowed set."""
    permissions = _extract_init_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    errors = []

    # Collect all files to check
    files_to_check = _all_plugin_skill_files()
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
                    f"in init/SKILL.md. Add a Bash({cmd} *) or Bash({cmd}) "
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
    permissions = _extract_init_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    cd_pattern = re.compile(r'^cd\s+\S+\s*&&\s*')
    errors = []

    files_to_check = _all_plugin_skill_files()
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
                    f"'Bash(git -C *)' to cover worktree commands."
                )

    assert not errors, (
        f"Found {len(errors)} cd-prefixed command(s) without permission coverage:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_worktree_cd_persists_no_repeated_cd():
    """Start skill must cd into the worktree once (bare cd) and never repeat
    cd .worktrees/ in compound commands. The Bash tool persists working
    directory, so repeated cd .worktrees/ breaks — the second cd looks for
    a nested .worktrees/ inside the worktree that doesn't exist."""
    content = _read_skill("start")

    # Find all bash blocks across all steps
    bare_cd_count = 0
    compound_cd_blocks = []

    for match in re.finditer(
        r'### Step (\d+) — .*?\n(.*?)(?=### Step \d+|### Done|\Z)',
        content, re.DOTALL
    ):
        step_num = int(match.group(1))
        step_content = match.group(2)
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", step_content, re.DOTALL)
        for block in bash_blocks:
            stripped = block.strip()
            if 'cd .worktrees/' not in stripped:
                continue
            # Bare cd (just changes directory, no compound command)
            if stripped.startswith('cd .worktrees/') and '&&' not in stripped:
                bare_cd_count += 1
            else:
                compound_cd_blocks.append(
                    f"Step {step_num}: '{stripped.split(chr(10))[0]}'"
                )

    assert bare_cd_count == 1, (
        f"Expected exactly 1 bare 'cd .worktrees/' block (to set persistent "
        f"working directory), found {bare_cd_count}"
    )

    assert not compound_cd_blocks, (
        f"Found {len(compound_cd_blocks)} compound 'cd .worktrees/ && ...' "
        f"block(s). The Bash tool persists working directory, so after the "
        f"initial cd, all commands run inside the worktree automatically. "
        f"Repeating cd .worktrees/ breaks because it looks for a nested "
        f".worktrees/ inside the worktree.\n"
        + "\n".join(f"  - {b}" for b in compound_cd_blocks)
    )


def _load_settings_permissions():
    """Load allowed permission patterns from .claude/settings.json."""
    data = json.loads(SETTINGS_JSON.read_text())
    return data["permissions"]["allow"]


def _maintainer_files():
    """Collect maintainer skill files.

    These run in this repo (not the target Rails project), so their bash
    commands must be covered by .claude/settings.json, not init/SKILL.md.
    """
    files = []
    for d in sorted(MAINTAINER_SKILLS_DIR.iterdir()):
        if d.is_dir():
            skill_md = d / "SKILL.md"
            if skill_md.exists():
                files.append((f".claude/skills/{d.name}/SKILL.md", skill_md.read_text()))
    return files


def test_maintainer_bash_commands_have_settings_coverage():
    """Every ```bash``` block in maintainer skills must match a permission
    in .claude/settings.json or be auto-allowed.

    Maintainer skills (.claude/skills/) run in this repo, not the target
    Rails project, so they need coverage in settings.json — not init/SKILL.md."""
    permissions = _load_settings_permissions()
    regexes = [_permission_to_regex(p) for p in permissions]
    regexes = [r for r in regexes if r is not None]

    errors = []

    for filepath, content in _maintainer_files():
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

            # Skip sensitive-path commands (always prompt, can't suppress)
            if cmd in SENSITIVE_PATH_COMMANDS:
                continue

            # Check against settings.json permission regexes
            matched = any(r.match(cmd) for r in regexes)
            if not matched:
                errors.append(
                    f"{filepath}: command '{cmd}' has no matching permission "
                    f"in .claude/settings.json. Add a Bash({cmd} *) or "
                    f"Bash({cmd}) entry."
                )

    assert not errors, (
        f"Found {len(errors)} maintainer Bash command(s) without "
        f".claude/settings.json coverage:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


REQUIRED_DENY_ENTRIES = [
    "Bash(git rebase *)",
    "Bash(git push --force *)",
    "Bash(git push -f *)",
    "Bash(git reset --hard *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git clean *)",
]

# Commands that have dedicated tool alternatives (Read, Grep, Glob).
# Must be denied in both global (~/.claude/settings.json) and project settings.
REQUIRED_TOOL_ALTERNATIVE_DENIES = [
    "Bash(cat *)",
    "Bash(head *)",
    "Bash(tail *)",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(find *)",
    "Bash(ls *)",
    "Bash(* && *)",
]


def test_plugin_permissions_deny_destructive_git():
    """Plugin permissions in init/SKILL.md must deny destructive git operations.

    The maintainer settings.json denies these, and the plugin permissions
    written to the target project must do the same."""
    permissions = _extract_init_permissions_block()
    assert "deny" in permissions, (
        "init/SKILL.md permissions JSON has no 'deny' list. "
        "Add deny entries for destructive git operations."
    )
    deny = permissions["deny"]
    for entry in REQUIRED_DENY_ENTRIES:
        assert entry in deny, f"Missing deny entry: {entry}"


def test_maintainer_permissions_deny_destructive_git():
    """Maintainer settings.json must deny destructive git operations."""
    data = json.loads(SETTINGS_JSON.read_text())
    assert "deny" in data["permissions"], (
        ".claude/settings.json has no 'deny' list."
    )
    deny = data["permissions"]["deny"]
    for entry in REQUIRED_DENY_ENTRIES:
        assert entry in deny, f"Missing deny entry in settings.json: {entry}"


def test_no_unrecognized_placeholders_in_bash_blocks():
    """Every angle-bracket placeholder in bash blocks must be in PLACEHOLDER_SUBS.

    Unrecognized placeholders cause _substitute_placeholders to return None,
    which silently skips the command — meaning it has zero permission coverage.
    This test makes the silent skip a loud failure."""
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            line = block.strip()

            # Strip leading blockquote markers from sub-agent prompts
            lines = line.split("\n")
            lines = [re.sub(r'^>\s*', '', l) for l in lines]
            line = "\n".join(lines).strip()

            # Skip COMMAND templates (logging templates)
            if "COMMAND" in line:
                continue

            # Apply all known substitutions
            for placeholder, value in PLACEHOLDER_SUBS.items():
                line = line.replace(placeholder, value)

            # Check for remaining angle-bracket placeholders
            remaining = re.findall(r"<[a-z_/-]+>", line, re.IGNORECASE)
            if remaining:
                unique = sorted(set(remaining))
                errors.append(
                    f"{filepath}: unrecognized placeholder(s) {unique} in "
                    f"bash block. Add entries to PLACEHOLDER_SUBS in "
                    f"test_permissions.py."
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) with unrecognized placeholders. "
        f"These commands are silently skipped by permission tests, meaning "
        f"they have NO coverage. Add the missing placeholders to "
        f"PLACEHOLDER_SUBS:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_permission_to_regex_known_conversions():
    """Pin known input/output pairs for _permission_to_regex."""
    # Exact match — no trailing wildcard
    r = _permission_to_regex("Bash(git push)")
    assert r is not None
    assert r.match("git push")
    assert not r.match("git push origin")

    # Trailing glob
    r = _permission_to_regex("Bash(git push *)")
    assert r is not None
    assert r.match("git push origin main")
    assert not r.match("git pull")

    # Exact binary — no glob
    r = _permission_to_regex("Bash(bin/ci)")
    assert r is not None
    assert r.match("bin/ci")
    assert not r.match("bin/ci --if-dirty")
    assert not r.match("bin/test")

    # git -C prefix with glob
    r = _permission_to_regex("Bash(git -C *)")
    assert r is not None
    assert r.match("git -C .worktrees/my-branch status")

    # Leading glob
    r = _permission_to_regex("Bash(*bin/flow *)")
    assert r is not None
    assert r.match("/usr/local/bin/flow run")
    assert r.match("bin/flow run")

    # rm with glob suffix
    r = _permission_to_regex("Bash(rm .flow-commit-*)")
    assert r is not None
    assert r.match("rm .flow-commit-abc123")

    # Non-Bash entries return None
    assert _permission_to_regex("Write(*)") is None
    assert _permission_to_regex("plain string") is None


def test_no_skill_command_matches_deny():
    """No bash command in any plugin skill or docs file should match a deny pattern.

    Deny always wins over allow. A command matching both is always blocked."""
    deny = _extract_init_deny()
    deny_regexes = [_permission_to_regex(d) for d in deny]
    deny_regexes = [r for r in deny_regexes if r is not None]

    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            cmd = _extract_primary_command(block)
            if cmd is None:
                continue

            for i, regex in enumerate(deny_regexes):
                if regex.match(cmd):
                    errors.append(
                        f"{filepath}: command '{cmd}' matches deny entry "
                        f"'{deny[i]}'. This command will always be blocked."
                    )
                    break

    assert not errors, (
        f"Found {len(errors)} command(s) matching deny patterns. "
        f"Deny always wins — these commands will trigger permission prompts "
        f"regardless of allow entries:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_maintainer_command_matches_deny():
    """No bash command in maintainer skills should match a settings.json deny pattern."""
    data = json.loads(SETTINGS_JSON.read_text())
    deny = data["permissions"]["deny"]
    deny_regexes = [_permission_to_regex(d) for d in deny]
    deny_regexes = [r for r in deny_regexes if r is not None]

    errors = []

    for filepath, content in _maintainer_files():
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            cmd = _extract_primary_command(block)
            if cmd is None:
                continue

            for i, regex in enumerate(deny_regexes):
                if regex.match(cmd):
                    errors.append(
                        f"{filepath}: command '{cmd}' matches deny entry "
                        f"'{deny[i]}'. This command will always be blocked."
                    )
                    break

    assert not errors, (
        f"Found {len(errors)} maintainer command(s) matching deny patterns:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_allow_deny_overlap_in_plugin_permissions():
    """No allow entry in init/SKILL.md should produce a command that matches deny.

    If an allow pattern generates commands that also match a deny pattern,
    the allow entry is dead — deny always wins."""
    perms = _extract_init_permissions_block()
    allow = perms["allow"]
    deny = perms["deny"]
    deny_regexes = [_permission_to_regex(d) for d in deny]
    deny_regexes = [r for r in deny_regexes if r is not None]

    errors = []

    for entry in allow:
        example = _concrete_example(entry)
        if example is None:
            continue
        for i, regex in enumerate(deny_regexes):
            if regex.match(example):
                errors.append(
                    f"allow '{entry}' (example: '{example}') matches deny "
                    f"'{deny[i]}'. This allow entry is dead — deny always wins."
                )
                break

    assert not errors, (
        f"Found {len(errors)} allow/deny overlap(s) in init/SKILL.md "
        f"permissions:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_no_allow_deny_overlap_in_maintainer_settings():
    """No allow entry in settings.json should produce a command that matches deny."""
    data = json.loads(SETTINGS_JSON.read_text())
    allow = data["permissions"]["allow"]
    deny = data["permissions"]["deny"]
    deny_regexes = [_permission_to_regex(d) for d in deny]
    deny_regexes = [r for r in deny_regexes if r is not None]

    errors = []

    for entry in allow:
        example = _concrete_example(entry)
        if example is None:
            continue
        for i, regex in enumerate(deny_regexes):
            if regex.match(example):
                errors.append(
                    f"allow '{entry}' (example: '{example}') matches deny "
                    f"'{deny[i]}'. This allow entry is dead — deny always wins."
                )
                break

    assert not errors, (
        f"Found {len(errors)} allow/deny overlap(s) in .claude/settings.json:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_tool_alternative_denies_in_project_settings():
    """Project settings.json must deny commands that have dedicated tool alternatives.

    cat/head/tail -> Read tool, grep/rg -> Grep tool, find/ls -> Glob tool,
    && compounds -> separate tool calls. Denying these forces Claude to use
    the dedicated tools, which never trigger permission prompts."""
    data = json.loads(SETTINGS_JSON.read_text())
    deny = data["permissions"]["deny"]
    for entry in REQUIRED_TOOL_ALTERNATIVE_DENIES:
        assert entry in deny, (
            f"Missing deny entry in .claude/settings.json: {entry}. "
            f"This command has a dedicated tool alternative and must be denied."
        )


def test_no_dedicated_tool_commands_in_bash_blocks():
    """No ```bash``` block in any skill or docs file should use commands that
    have dedicated tool alternatives.

    cat/head/tail -> Read tool, grep/rg -> Grep tool, find/ls -> Glob tool.
    These commands are denied in settings and must not appear in skills."""
    denied_prefixes = {"cat", "head", "tail", "grep", "rg", "find", "ls"}
    errors = []

    files_to_check = _all_plugin_skill_files()
    for rel, content in _all_docs_files():
        files_to_check.append((rel, content))
    for filepath, content in _maintainer_files():
        files_to_check.append((filepath, content))

    for filepath, content in files_to_check:
        bash_blocks = re.findall(r"```bash\s*\n(.*?)```", content, re.DOTALL)
        for block in bash_blocks:
            cmd = _extract_primary_command(block)
            if cmd is None:
                continue
            first_word = cmd.split()[0] if cmd.split() else ""
            if first_word in denied_prefixes:
                errors.append(
                    f"{filepath}: bash block starts with '{first_word}': "
                    f"'{cmd}'. Use the dedicated tool instead "
                    f"(Read for cat/head/tail, Grep for grep/rg, "
                    f"Glob for find/ls)."
                )

    assert not errors, (
        f"Found {len(errors)} bash block(s) using commands that have "
        f"dedicated tool alternatives:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_init_setup_lists_match_skill_md_reference():
    """Permission lists in lib/init-setup.py must match init/SKILL.md JSON.

    init-setup.py is the code that writes permissions at runtime.
    init/SKILL.md contains the reference JSON for documentation.
    If they drift, the docs lie about what gets installed."""
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "init_setup", LIB_DIR / "init-setup.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    # Build complete sets from init-setup.py
    code_allow = set(mod.UNIVERSAL_ALLOW + mod.RAILS_ALLOW + mod.PYTHON_ALLOW)
    code_deny = set(mod.FLOW_DENY)

    # Get reference JSON from init/SKILL.md
    perms = _extract_init_permissions_block()
    skill_allow = set(perms["allow"])
    skill_deny = set(perms["deny"])

    errors = []

    only_in_code_allow = code_allow - skill_allow
    if only_in_code_allow:
        errors.append(
            f"In init-setup.py allow but not init/SKILL.md: "
            f"{sorted(only_in_code_allow)}"
        )

    only_in_skill_allow = skill_allow - code_allow
    if only_in_skill_allow:
        errors.append(
            f"In init/SKILL.md allow but not init-setup.py: "
            f"{sorted(only_in_skill_allow)}"
        )

    only_in_code_deny = code_deny - skill_deny
    if only_in_code_deny:
        errors.append(
            f"In init-setup.py deny but not init/SKILL.md: "
            f"{sorted(only_in_code_deny)}"
        )

    only_in_skill_deny = skill_deny - code_deny
    if only_in_skill_deny:
        errors.append(
            f"In init/SKILL.md deny but not init-setup.py: "
            f"{sorted(only_in_skill_deny)}"
        )

    assert not errors, (
        f"Permission lists out of sync between lib/init-setup.py (runtime "
        f"source) and skills/init/SKILL.md (reference docs):\n"
        + "\n".join(f"  - {e}" for e in errors)
    )