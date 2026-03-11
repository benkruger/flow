"""Tests that documentation stays in sync with skills and flow-phases.json.

Skills are hand-authored for different audiences (Claude vs. public users)
so auto-generation isn't appropriate. These tests catch structural drift —
missing files, wrong names, stale references.
"""

import json
import re

from conftest import DOCS_DIR, REPO_ROOT, SKILLS_DIR

from flow_utils import PHASE_NUMBER


REQUIRED_FEATURES = {
    "Autonomy config": ["autonomy"],
    "Learning system": ["learning system"],
    "Native Claude features": ["plan mode"],
    "Model selection": ["opus"],
    "Zero dependencies": ["zero dependencies"],
    "Minimal repo artifacts": [".flow-states"],
    "Multi-language": ["rails"],
    "Issue auto-close": ["close issues"],
}


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
        doc = DOCS_DIR / "skills" / f"{name}.md"
        assert doc.exists(), (
            f"skills/{name}/ exists but docs/skills/{name}.md is missing"
        )


def test_every_docs_skill_page_has_a_skill_dir():
    """Every docs/skills/flow-<name>.md must have a skills/<name>/."""
    for path in sorted((DOCS_DIR / "skills").iterdir()):
        if path.name == "index.md" or not path.suffix == ".md":
            continue
        # flow-commit.md -> flow-commit
        skill_name = path.stem
        assert (SKILLS_DIR / skill_name).is_dir(), (
            f"docs/skills/{path.name} exists but skills/{skill_name}/ is missing"
        )


# --- Phase docs match flow-phases.json ---


def test_every_phase_has_a_docs_page():
    """Every phase in flow-phases.json must have a docs/phases/phase-<N>-<name>.md."""
    data = _load_phases()
    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        name = phase["name"].lower().replace(" ", "-")
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name}.md"
        assert doc.exists(), (
            f"Phase {num} ({phase['name']}) has no docs/phases/phase-{num}-{name}.md"
        )


def test_phase_docs_contain_correct_command():
    """Each phase doc must contain the command from flow-phases.json."""
    data = _load_phases()
    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        name = phase["name"].lower().replace(" ", "-")
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name}.md"
        content = doc.read_text()
        # Docs use /flow-start, not /flow:flow-start
        user_command = phase["command"].replace("/flow:", "/")
        assert user_command in content, (
            f"docs/phases/phase-{num}-{name}.md does not mention "
            f"command '{user_command}'"
        )


def test_phase_docs_have_correct_title():
    """Each phase doc title must contain 'Phase N: Name'."""
    data = _load_phases()
    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        name_lower = phase["name"].lower().replace(" ", "-")
        doc = DOCS_DIR / "phases" / f"phase-{num}-{name_lower}.md"
        content = doc.read_text()
        pattern = rf"Phase {num}:\s*{re.escape(phase['name'])}"
        assert re.search(pattern, content), (
            f"docs/phases/phase-{num}-{name_lower}.md missing "
            f"'Phase {num}: {phase['name']}' in title"
        )


# --- Index completeness ---


def test_index_mentions_every_skill_command():
    """docs/skills/index.md must mention every /<name> command."""
    index = (DOCS_DIR / "skills" / "index.md").read_text()
    for name in _skill_names():
        command = f"/{name}"
        assert command in index, (
            f"docs/skills/index.md does not mention {command}"
        )


def test_index_phase_table_shows_all_phases():
    """docs/skills/index.md phase table must show 'N — Name' for all 6 phases."""
    data = _load_phases()
    index = (DOCS_DIR / "skills" / "index.md").read_text()
    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        pattern = rf"{num}\s*—\s*{re.escape(phase['name'])}"
        assert re.search(pattern, index), (
            f"docs/skills/index.md missing '{num} — {phase['name']}' "
            f"in phase table"
        )


# --- README completeness ---


def test_readme_mentions_all_phase_commands():
    """README.md must mention all 6 phase commands and 'N: Name' strings."""
    readme = (REPO_ROOT / "README.md").read_text()
    data = _load_phases()
    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        # README uses /flow-start, not /flow:flow-start
        user_command = phase["command"].replace("/flow:", "/")
        assert user_command in readme, (
            f"README.md does not mention phase command '{user_command}'"
        )
        pattern = rf"{num}:\s*{re.escape(phase['name'])}"
        assert re.search(pattern, readme), (
            f"README.md does not mention '{num}: {phase['name']}'"
        )


def test_readme_mentions_all_maintainer_commands():
    """README.md must mention all maintainer skill commands as /<name>."""
    readme = (REPO_ROOT / "README.md").read_text()
    maintainer_dir = REPO_ROOT / ".claude" / "skills"
    for d in sorted(maintainer_dir.iterdir()):
        if d.is_dir() and (d / "SKILL.md").exists():
            command = f"/{d.name}"
            assert command in readme, (
                f"README.md does not mention maintainer command '{command}'"
            )


def test_readme_mentions_all_utility_commands():
    """README.md must mention all utility skill commands."""
    readme = (REPO_ROOT / "README.md").read_text()
    for name in _utility_skill_names():
        command = f"/{name}"
        assert command in readme, (
            f"README.md does not mention utility command '{command}'"
        )


# --- Landing page completeness ---


def test_landing_page_mentions_all_phase_names():
    """docs/index.html must mention all 6 phase names."""
    html = (DOCS_DIR / "index.html").read_text()
    data = _load_phases()
    for key, phase in data["phases"].items():
        assert phase["name"] in html, (
            f"docs/index.html does not mention phase name '{phase['name']}'"
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
        "started_at", "current_phase", "prompt", "notes",
    ]
    for field in top_level_fields:
        pattern = rf"`{re.escape(field)}`"
        assert re.search(pattern, schema), (
            f"docs/reference/flow-state-schema.md does not document "
            f"top-level field '{field}'"
        )


# --- Key feature coverage ---


def _assert_covers_key_features(content, source_label):
    """Assert content mentions every feature in REQUIRED_FEATURES."""
    for feature, keywords in REQUIRED_FEATURES.items():
        found = any(kw.lower() in content for kw in keywords)
        assert found, (
            f"{source_label} does not mention feature '{feature}' "
            f"(looked for: {keywords})"
        )


def test_readme_covers_key_features():
    """README.md must mention all key features by keyword."""
    _assert_covers_key_features((REPO_ROOT / "README.md").read_text().lower(), "README.md")


def test_landing_page_covers_key_features():
    """docs/index.html must mention all key features by keyword."""
    _assert_covers_key_features((DOCS_DIR / "index.html").read_text().lower(), "docs/index.html")


