"""Tests that documentation stays in sync with skills and flow-phases.json.

Skills are hand-authored for different audiences (Claude vs. public users)
so auto-generation isn't appropriate. These tests catch structural drift —
missing files, wrong names, stale references.
"""

import json
import re

from conftest import DOCS_DIR, REPO_ROOT, SKILLS_DIR


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def _skill_names():
    """Return sorted list of skill directory names."""
    return sorted(d.name for d in SKILLS_DIR.iterdir() if d.is_dir())


def _phase_skill_names():
    """Return set of skill names that correspond to phases."""
    data = _load_phases()
    return {phase["command"].split(":")[1] for phase in data["phases"].values()}


def _utility_skill_names():
    """Return sorted list of skill names that are NOT phase skills."""
    phase_names = _phase_skill_names()
    return sorted(n for n in _skill_names() if n not in phase_names)


# --- Skill docs existence (bidirectional) ---


def test_every_skill_has_a_docs_page():
    """Every skills/<name>/ must have a docs/skills/flow-<name>.md."""
    for name in _skill_names():
        doc = DOCS_DIR / "skills" / f"flow-{name}.md"
        assert doc.exists(), (
            f"skills/{name}/ exists but docs/skills/flow-{name}.md is missing"
        )


def test_every_docs_skill_page_has_a_skill_dir():
    """Every docs/skills/flow-<name>.md must have a skills/<name>/."""
    for path in sorted((DOCS_DIR / "skills").iterdir()):
        if path.name == "index.md" or not path.suffix == ".md":
            continue
        # flow-commit.md -> commit
        match = re.match(r"^flow-(.+)\.md$", path.name)
        assert match, f"Unexpected filename pattern: {path.name}"
        skill_name = match.group(1)
        assert (SKILLS_DIR / skill_name).is_dir(), (
            f"docs/skills/{path.name} exists but skills/{skill_name}/ is missing"
        )


# --- Phase docs match flow-phases.json ---


def test_every_phase_has_a_docs_page():
    """Every phase in flow-phases.json must have a docs/phases/phase-<N>-<name>.md."""
    data = _load_phases()
    for num, phase in data["phases"].items():
        name = phase["name"].lower()
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name}.md"
        assert doc.exists(), (
            f"Phase {num} ({phase['name']}) has no docs/phases/phase-{num}-{name}.md"
        )


def test_phase_docs_contain_correct_command():
    """Each phase doc must contain the command from flow-phases.json."""
    data = _load_phases()
    for num, phase in data["phases"].items():
        name = phase["name"].lower()
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name}.md"
        content = doc.read_text()
        assert phase["command"] in content, (
            f"docs/phases/phase-{num}-{name}.md does not mention "
            f"command '{phase['command']}'"
        )


def test_phase_docs_have_correct_title():
    """Each phase doc title must contain 'Phase N: Name'."""
    data = _load_phases()
    for num, phase in data["phases"].items():
        name_lower = phase["name"].lower()
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name_lower}.md"
        content = doc.read_text()
        pattern = rf"Phase {num}:\s*{re.escape(phase['name'])}"
        assert re.search(pattern, content), (
            f"docs/phases/phase-{num}-{name_lower}.md missing "
            f"'Phase {num}: {phase['name']}' in title"
        )


# --- Index completeness ---


def test_index_mentions_every_skill_command():
    """docs/skills/index.md must mention every /flow:<name> command."""
    index = (DOCS_DIR / "skills" / "index.md").read_text()
    for name in _skill_names():
        command = f"/flow:{name}"
        assert command in index, (
            f"docs/skills/index.md does not mention {command}"
        )


def test_index_phase_table_shows_all_phases():
    """docs/skills/index.md phase table must show 'N — Name' for all 8 phases."""
    data = _load_phases()
    index = (DOCS_DIR / "skills" / "index.md").read_text()
    for num, phase in data["phases"].items():
        pattern = rf"{num}\s*—\s*{re.escape(phase['name'])}"
        assert re.search(pattern, index), (
            f"docs/skills/index.md missing '{num} — {phase['name']}' "
            f"in phase table"
        )


# --- README completeness ---


def test_readme_mentions_all_phase_commands():
    """README.md must mention all 8 phase commands and 'N: Name' strings."""
    readme = (REPO_ROOT / "README.md").read_text()
    data = _load_phases()
    for num, phase in data["phases"].items():
        assert phase["command"] in readme, (
            f"README.md does not mention phase command '{phase['command']}'"
        )
        pattern = rf"{num}:\s*{re.escape(phase['name'])}"
        assert re.search(pattern, readme), (
            f"README.md does not mention '{num}: {phase['name']}'"
        )


def test_readme_mentions_all_utility_commands():
    """README.md must mention all utility skill commands."""
    readme = (REPO_ROOT / "README.md").read_text()
    for name in _utility_skill_names():
        command = f"/flow:{name}"
        assert command in readme, (
            f"README.md does not mention utility command '{command}'"
        )


# --- Landing page completeness ---


def test_landing_page_mentions_all_phase_names():
    """docs/index.html must mention all 8 phase names."""
    html = (DOCS_DIR / "index.html").read_text()
    data = _load_phases()
    for num, phase in data["phases"].items():
        assert phase["name"] in html, (
            f"docs/index.html does not mention phase name '{phase['name']}'"
        )


def test_landing_page_mentions_all_utility_commands():
    """docs/index.html must mention all utility skill commands."""
    html = (DOCS_DIR / "index.html").read_text()
    for name in _utility_skill_names():
        command = f"/flow:{name}"
        assert command in html, (
            f"docs/index.html does not mention utility command '{command}'"
        )


# --- State schema coverage ---


def test_schema_doc_covers_phase_fields():
    """Schema doc must document all phase-level fields from conftest.make_state()."""
    schema = (DOCS_DIR / "reference" / "flow-state-schema.md").read_text()
    phase_fields = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for field in phase_fields:
        pattern = rf"`{re.escape(field)}`"
        assert re.search(pattern, schema), (
            f"docs/reference/flow-state-schema.md does not document "
            f"phase field '{field}'"
        )


def test_schema_doc_covers_top_level_fields():
    """Schema doc must document all top-level fields from conftest.make_state()."""
    schema = (DOCS_DIR / "reference" / "flow-state-schema.md").read_text()
    top_level_fields = [
        "feature", "branch", "worktree", "pr_number", "pr_url",
        "started_at", "current_phase", "notes",
    ]
    for field in top_level_fields:
        pattern = rf"`{re.escape(field)}`"
        assert re.search(pattern, schema), (
            f"docs/reference/flow-state-schema.md does not document "
            f"top-level field '{field}'"
        )