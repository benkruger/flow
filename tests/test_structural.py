"""Structural invariant tests for FLOW plugin configuration files."""

import json
import os
import re

from conftest import BIN_DIR, HOOKS_DIR, LIB_DIR, REPO_ROOT, SKILLS_DIR, make_state


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def test_phases_has_1_through_9():
    data = _load_phases()
    phases = data["phases"]
    for i in range(1, 10):
        assert str(i) in phases, f"Phase {i} missing from flow-phases.json"
    assert len(phases) == 9


def test_commands_match_flow_pattern():
    data = _load_phases()
    for num, phase in data["phases"].items():
        cmd = phase["command"]
        assert re.match(r"^/flow:\w+$", cmd), (
            f"Phase {num} command '{cmd}' doesn't match /flow:<name> pattern"
        )


def test_can_return_to_references_valid_lower_phases():
    data = _load_phases()
    for num, phase in data["phases"].items():
        for target in phase["can_return_to"]:
            assert target in data["phases"], (
                f"Phase {num} can_return_to references non-existent phase '{target}'"
            )
            assert int(target) < int(num), (
                f"Phase {num} can_return_to references same or higher phase '{target}'"
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
    data = _load_phases()
    script = (LIB_DIR / "flow_utils.py").read_text()

    # Extract PHASE_NAMES dict from script
    phases_match = re.search(
        r"^PHASE_NAMES\s*=\s*\{(.+?)\}", script, re.DOTALL | re.MULTILINE
    )
    assert phases_match, "Could not find PHASE_NAMES dict in flow_utils.py"

    for num, phase in data["phases"].items():
        pattern = rf'{num}:\s*"{re.escape(phase["name"])}"'
        assert re.search(pattern, phases_match.group(0)), (
            f"Phase {num} name '{phase['name']}' not found in flow_utils.py PHASE_NAMES"
        )


def test_check_phase_commands_match_flow_phases():
    """COMMANDS in check-phase.py must match flow-phases.json."""
    data = _load_phases()
    script = (LIB_DIR / "check-phase.py").read_text()

    commands_match = re.search(
        r"^COMMANDS\s*=\s*\{(.+?)\}", script, re.DOTALL | re.MULTILINE
    )
    assert commands_match, "Could not find COMMANDS dict in check-phase.py"

    for num, phase in data["phases"].items():
        pattern = rf'"{num}":\s*"{re.escape(phase["command"])}"'
        assert re.search(pattern, commands_match.group(0)), (
            f"Phase {num} command '{phase['command']}' not found in check-phase.py COMMANDS"
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
    for num_str, phase in data["phases"].items():
        fixture_name = state["phases"][num_str]["name"]
        canonical_name = phase["name"]
        assert fixture_name == canonical_name, (
            f"Phase {num_str}: conftest.make_state() uses '{fixture_name}' "
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
    import configparser
    config = configparser.ConfigParser()
    config.read(REPO_ROOT / "pytest.ini")
    addopts = config.get("pytest", "addopts")
    assert "-n auto" in addopts, "-n auto not found in pytest.ini addopts"