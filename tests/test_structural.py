"""Structural invariant tests for FLOW plugin configuration files."""

import configparser
import json
import os
import re

from conftest import (
    BIN_DIR, FRAMEWORKS_DIR, HOOKS_DIR, LIB_DIR, REPO_ROOT, SKILLS_DIR,
    make_state,
)


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def test_phases_has_1_through_6():
    data = _load_phases()
    phases = data["phases"]
    order = data["order"]
    assert len(order) == 6, f"Expected 6 phases in order, got {len(order)}"
    for key in order:
        assert key in phases, f"Phase '{key}' in order but missing from phases"
    assert len(phases) == 6


def test_commands_match_flow_pattern():
    data = _load_phases()
    for key, phase in data["phases"].items():
        cmd = phase["command"]
        assert re.match(r"^/flow:[\w-]+$", cmd), (
            f"Phase '{key}' command '{cmd}' doesn't match /flow:<name> pattern"
        )


def test_can_return_to_references_valid_lower_phases():
    data = _load_phases()
    order = data["order"]
    for key, phase in data["phases"].items():
        key_index = order.index(key)
        for target in phase["can_return_to"]:
            assert target in data["phases"], (
                f"Phase '{key}' can_return_to references non-existent phase '{target}'"
            )
            target_index = order.index(target)
            assert target_index < key_index, (
                f"Phase '{key}' can_return_to references same or higher phase '{target}'"
            )


def test_version_matches_across_files():
    plugin = json.loads(
        (REPO_ROOT / ".claude-plugin" / "plugin.json").read_text()
    )
    marketplace = json.loads(
        (REPO_ROOT / ".claude-plugin" / "marketplace.json").read_text()
    )
    v_plugin = plugin["version"]
    v_meta = marketplace["metadata"]["version"]
    v_entry = marketplace["plugins"][0]["version"]
    assert v_plugin == v_meta, (
        f"plugin.json ({v_plugin}) != marketplace metadata ({v_meta})"
    )
    assert v_plugin == v_entry, (
        f"plugin.json ({v_plugin}) != marketplace plugins[0] ({v_entry})"
    )


def test_every_skill_dir_has_skill_md():
    for d in sorted(SKILLS_DIR.iterdir()):
        if d.is_dir():
            skill_md = d / "SKILL.md"
            assert skill_md.exists(), f"skills/{d.name}/ has no SKILL.md"


def test_phase_names_in_flow_utils_match_flow_phases():
    """PHASE_NAMES in flow_utils.py must match flow-phases.json."""
    from flow_utils import PHASE_NAMES
    data = _load_phases()
    for key, phase in data["phases"].items():
        assert key in PHASE_NAMES, (
            f"Phase '{key}' not found in flow_utils.py PHASE_NAMES"
        )
        assert PHASE_NAMES[key] == phase["name"], (
            f"Phase '{key}': flow_utils.py has '{PHASE_NAMES[key]}' "
            f"but flow-phases.json has '{phase['name']}'"
        )


def test_check_phase_commands_match_flow_phases():
    """COMMANDS in flow_utils.py must match flow-phases.json."""
    from flow_utils import COMMANDS
    data = _load_phases()
    for key, phase in data["phases"].items():
        assert key in COMMANDS, (
            f"Phase '{key}' not found in flow_utils.py COMMANDS"
        )
        assert COMMANDS[key] == phase["command"], (
            f"Phase '{key}': flow_utils.py has '{COMMANDS[key]}' "
            f"but flow-phases.json has '{phase['command']}'"
        )


def test_hooks_json_references_existing_files():
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    for event, matchers in hooks["hooks"].items():
        for matcher in matchers:
            for hook in matcher["hooks"]:
                cmd = hook["command"]
                # Replace ${CLAUDE_PLUGIN_ROOT} with repo root
                resolved = cmd.replace("${CLAUDE_PLUGIN_ROOT}", str(REPO_ROOT))
                # Extract the script path (first space-separated token)
                script_path = resolved.split()[0]
                assert REPO_ROOT.joinpath(
                    script_path.replace(str(REPO_ROOT) + "/", "")
                ).exists() or __import__("pathlib").Path(script_path).exists(), (
                    f"Hook command references non-existent file: {cmd}"
                )


def test_commands_are_unique():
    """All phase commands must be unique — no two phases share a command."""
    data = _load_phases()
    commands = [phase["command"] for phase in data["phases"].values()]
    assert len(commands) == len(set(commands)), (
        f"Duplicate commands found: {[c for c in commands if commands.count(c) > 1]}"
    )


def test_conftest_phase_names_match_flow_phases():
    """conftest.make_state() phase names must match flow-phases.json.
    Catches drift between test fixtures and canonical phase definitions."""
    data = _load_phases()
    state = make_state()
    for key, phase in data["phases"].items():
        fixture_name = state["phases"][key]["name"]
        canonical_name = phase["name"]
        assert fixture_name == canonical_name, (
            f"Phase '{key}': conftest.make_state() uses '{fixture_name}' "
            f"but flow-phases.json uses '{canonical_name}'"
        )


def test_every_script_has_a_test_file():
    """Every shell script in hooks/ and executable in bin/ must have a test file."""
    scripts = {}
    for sh in sorted(HOOKS_DIR.glob("*.sh")):
        stem = sh.stem.replace("-", "_")
        scripts[sh.relative_to(REPO_ROOT)] = REPO_ROOT / "tests" / f"test_{stem}.py"
    for f in sorted(BIN_DIR.iterdir()):
        if f.is_file() and os.access(f, os.X_OK):
            stem = f.stem.replace("-", "_")
            scripts[f.relative_to(REPO_ROOT)] = REPO_ROOT / "tests" / f"test_bin_{stem}.py"
    missing = [
        str(script) for script, test in scripts.items() if not test.exists()
    ]
    assert not missing, (
        f"Scripts without test files: {', '.join(missing)}"
    )


def test_pytest_xdist_in_requirements():
    requirements = (REPO_ROOT / "requirements.txt").read_text()
    assert "pytest-xdist" in requirements, "pytest-xdist missing from requirements.txt"


def test_n_auto_in_pytest_ini():
    config = configparser.ConfigParser()
    config.read(REPO_ROOT / "pytest.ini")
    addopts = config.get("pytest", "addopts")
    assert "-n auto" in addopts, "-n auto not found in pytest.ini addopts"


def test_claude_md_has_no_lessons_learned_section():
    """CLAUDE.md must not have a Lessons Learned section.

    Learnings belong in rules files (.claude/rules/ or ~/.claude/rules/),
    not in CLAUDE.md. CLAUDE.md is for architecture, conventions, and
    project description."""
    content = (REPO_ROOT / "CLAUDE.md").read_text()
    assert "## Lessons Learned" not in content, (
        "CLAUDE.md still has a '## Lessons Learned' section — "
        "learnings belong in rules files, not CLAUDE.md"
    )


# --- Framework definition directory ---

FRAMEWORK_REQUIRED_FILES = ["detect.json", "permissions.json", "dependencies", "priming.md"]


def _load_frameworks():
    """Return list of (name, path) for every framework directory."""
    assert FRAMEWORKS_DIR.is_dir(), (
        f"frameworks/ directory does not exist at {FRAMEWORKS_DIR}"
    )
    frameworks = [
        (d.name, d) for d in sorted(FRAMEWORKS_DIR.iterdir()) if d.is_dir()
    ]
    assert len(frameworks) >= 1, "frameworks/ directory has no framework subdirectories"
    return frameworks


def test_frameworks_directory_has_required_files():
    """Every frameworks/<name>/ must have detect.json, permissions.json,
    dependencies, and priming.md."""
    for name, path in _load_frameworks():
        for filename in FRAMEWORK_REQUIRED_FILES:
            assert (path / filename).exists(), (
                f"frameworks/{name}/ missing required file: {filename}"
            )


def test_framework_detect_json_schema():
    """Each detect.json must have name, display_name, and detect_globs fields."""
    for name, path in _load_frameworks():
        data = json.loads((path / "detect.json").read_text())
        assert "name" in data, f"frameworks/{name}/detect.json missing 'name'"
        assert "display_name" in data, (
            f"frameworks/{name}/detect.json missing 'display_name'"
        )
        assert "detect_globs" in data, (
            f"frameworks/{name}/detect.json missing 'detect_globs'"
        )
        assert isinstance(data["detect_globs"], list), (
            f"frameworks/{name}/detect.json 'detect_globs' must be a list"
        )
        assert len(data["detect_globs"]) >= 1, (
            f"frameworks/{name}/detect.json 'detect_globs' must have at least one entry"
        )
        assert data["name"] == name, (
            f"frameworks/{name}/detect.json 'name' is '{data['name']}' but directory is '{name}'"
        )


def test_framework_permissions_json_schema():
    """Each permissions.json must have an 'allow' array of strings."""
    for name, path in _load_frameworks():
        data = json.loads((path / "permissions.json").read_text())
        assert "allow" in data, (
            f"frameworks/{name}/permissions.json missing 'allow'"
        )
        assert isinstance(data["allow"], list), (
            f"frameworks/{name}/permissions.json 'allow' must be a list"
        )
        for entry in data["allow"]:
            assert isinstance(entry, str), (
                f"frameworks/{name}/permissions.json 'allow' entries must be strings"
            )
            assert entry.startswith("Bash("), (
                f"frameworks/{name}/permissions.json entry '{entry}' "
                f"must start with 'Bash('"
            )


def test_framework_dependencies_is_executable_script():
    """Each dependencies file must start with a shebang line."""
    for name, path in _load_frameworks():
        content = (path / "dependencies").read_text()
        assert content.startswith("#!/"), (
            f"frameworks/{name}/dependencies must start with a shebang (#!/...)"
        )


def test_plugin_json_has_no_config_hash():
    """plugin.json must not contain config_hash — it breaks the validator.

    The hash is computed dynamically by prime-setup.py and prime-check.py.
    """
    plugin = json.loads(
        (REPO_ROOT / ".claude-plugin" / "plugin.json").read_text()
    )
    assert "config_hash" not in plugin, (
        "plugin.json must not contain config_hash — "
        "Claude Code's plugin validator rejects unrecognized keys"
    )
