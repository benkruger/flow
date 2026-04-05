"""Structural invariant tests for FLOW plugin configuration files."""

import configparser
import json
import os
import re

from conftest import (
    BIN_DIR,
    FRAMEWORKS_DIR,
    HOOKS_DIR,
    LIB_DIR,
    REPO_ROOT,
    SKILLS_DIR,
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
        assert re.match(r"^/flow:[\w-]+$", cmd), f"Phase '{key}' command '{cmd}' doesn't match /flow:<name> pattern"


def test_can_return_to_references_valid_lower_phases():
    data = _load_phases()
    order = data["order"]
    for key, phase in data["phases"].items():
        key_index = order.index(key)
        for target in phase["can_return_to"]:
            assert target in data["phases"], f"Phase '{key}' can_return_to references non-existent phase '{target}'"
            target_index = order.index(target)
            assert target_index < key_index, f"Phase '{key}' can_return_to references same or higher phase '{target}'"


def test_version_matches_across_files():
    plugin = json.loads((REPO_ROOT / ".claude-plugin" / "plugin.json").read_text())
    marketplace = json.loads((REPO_ROOT / ".claude-plugin" / "marketplace.json").read_text())
    v_plugin = plugin["version"]
    v_meta = marketplace["metadata"]["version"]
    v_entry = marketplace["plugins"][0]["version"]
    assert v_plugin == v_meta, f"plugin.json ({v_plugin}) != marketplace metadata ({v_meta})"
    assert v_plugin == v_entry, f"plugin.json ({v_plugin}) != marketplace plugins[0] ({v_entry})"


def test_every_skill_dir_has_skill_md():
    for d in sorted(SKILLS_DIR.iterdir()):
        if d.is_dir():
            skill_md = d / "SKILL.md"
            assert skill_md.exists(), f"skills/{d.name}/ has no SKILL.md"


def test_every_skill_dir_starts_with_flow_prefix():
    for d in sorted(SKILLS_DIR.iterdir()):
        if d.is_dir():
            assert d.name.startswith("flow-"), f"skills/{d.name}/ does not start with 'flow-' prefix"


def test_phase_names_in_flow_utils_match_flow_phases():
    """PHASE_NAMES in flow_utils.py must match flow-phases.json."""
    from flow_utils import PHASE_NAMES

    data = _load_phases()
    for key, phase in data["phases"].items():
        assert key in PHASE_NAMES, f"Phase '{key}' not found in flow_utils.py PHASE_NAMES"
        assert PHASE_NAMES[key] == phase["name"], (
            f"Phase '{key}': flow_utils.py has '{PHASE_NAMES[key]}' but flow-phases.json has '{phase['name']}'"
        )


def test_check_phase_commands_match_flow_phases():
    """COMMANDS in flow_utils.py must match flow-phases.json."""
    from flow_utils import COMMANDS

    data = _load_phases()
    for key, phase in data["phases"].items():
        assert key in COMMANDS, f"Phase '{key}' not found in flow_utils.py COMMANDS"
        assert COMMANDS[key] == phase["command"], (
            f"Phase '{key}': flow_utils.py has '{COMMANDS[key]}' but flow-phases.json has '{phase['command']}'"
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
                assert (
                    REPO_ROOT.joinpath(script_path.replace(str(REPO_ROOT) + "/", "")).exists()
                    or __import__("pathlib").Path(script_path).exists()
                ), f"Hook command references non-existent file: {cmd}"


def test_hook_scripts_are_executable():
    """Every script referenced in hooks.json must have execute permission."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    non_executable = []
    for matchers in hooks["hooks"].values():
        for matcher in matchers:
            for hook in matcher["hooks"]:
                cmd = hook["command"]
                resolved = cmd.replace("${CLAUDE_PLUGIN_ROOT}", str(REPO_ROOT))
                script_path = resolved.split()[0]
                path = REPO_ROOT / script_path.replace(str(REPO_ROOT) + "/", "")
                if path.exists() and not os.access(path, os.X_OK):
                    non_executable.append(str(path.relative_to(REPO_ROOT)))
    assert not non_executable, f"Hook scripts missing execute permission: {', '.join(non_executable)}"


def test_hooks_json_has_pretooluse_bash_validator():
    """hooks.json must register the pretool validator as a global PreToolUse hook."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    assert "PreToolUse" in hooks["hooks"], (
        "hooks.json missing PreToolUse key — the global Bash validator must be registered"
    )
    matchers = hooks["hooks"]["PreToolUse"]
    bash_matchers = [m for m in matchers if "Bash" in m["matcher"]]
    assert len(bash_matchers) == 1, f"Expected exactly 1 Bash-matching matcher in PreToolUse, got {len(bash_matchers)}"
    assert "Agent" in bash_matchers[0]["matcher"], (
        "PreToolUse Bash validator must also match Agent tool (matcher should be 'Bash|Agent')"
    )
    commands = [h["command"] for h in bash_matchers[0]["hooks"]]
    assert any("bin/flow hook validate-pretool" in cmd for cmd in commands), (
        "PreToolUse Bash hook must reference bin/flow hook validate-pretool"
    )


def test_no_validate_ci_bash_filename():
    """Tombstone: renamed in PR #693. Must not return."""
    lib_files = [f.name for f in LIB_DIR.iterdir() if f.is_file()]
    assert "validate-ci-bash.py" not in lib_files, (
        "validate-ci-bash.py must not exist in lib/ — replaced by Rust hook bin/flow hook validate-pretool"
    )
    hooks_content = (HOOKS_DIR / "hooks.json").read_text()
    assert "validate-ci-bash" not in hooks_content, "validate-ci-bash must not appear in hooks.json"
    test_files = [f.name for f in (REPO_ROOT / "tests").iterdir() if f.is_file()]
    assert "test_validate_ci_bash.py" not in test_files, "test_validate_ci_bash.py must not exist in tests/"


def test_no_python_validator_scripts():
    """Tombstone: validate-*.py scripts were ported to Rust in PR #856. Must not return."""
    lib_files = {f.name for f in LIB_DIR.iterdir() if f.is_file()}
    ported = [
        "validate-pretool.py",
        "validate-claude-paths.py",
        "validate-worktree-paths.py",
        "validate-ask-user.py",
    ]
    for name in ported:
        assert name not in lib_files, (
            f"{name} must not exist in lib/ — ported to Rust. "
            f"Use bin/flow hook {name.replace('.py', '').replace('validate-', 'validate-')} instead."
        )


def test_no_python_validator_tests():
    """Tombstone: test_validate_*.py tests were ported to Rust in PR #856. Must not return."""
    test_files = {f.name for f in (REPO_ROOT / "tests").iterdir() if f.is_file()}
    ported_tests = [
        "test_validate_pretool.py",
        "test_validate_claude_paths.py",
        "test_validate_worktree_paths.py",
        "test_validate_ask_user.py",
    ]
    for name in ported_tests:
        assert name not in test_files, (
            f"{name} must not exist in tests/ — ported to Rust. "
            f"See src/hooks/ for the Rust implementations and inline tests."
        )


def test_hooks_json_uses_bin_flow_hook_for_pretool_validators():
    """All PreToolUse hook commands must use bin/flow hook instead of Python scripts."""
    hooks_content = (HOOKS_DIR / "hooks.json").read_text()
    for name in ("validate-pretool", "validate-claude-paths", "validate-worktree-paths", "validate-ask-user"):
        assert f"lib/{name}.py" not in hooks_content, (
            f"hooks.json must not reference lib/{name}.py — use bin/flow hook {name} instead"
        )


def test_bin_flow_fails_closed_for_hook_subcommand():
    """bin/flow must exit 2 (block) not 1 (error) when the hook subcommand has no handler.

    PR #856 added this guard so that if the Rust binary is absent and cargo is
    unavailable to build it, PreToolUse hooks fail closed instead of silently
    allowing all tool calls through. Claude Code treats exit 2 as a block with
    stderr feedback; any other non-zero exit is treated as a non-blocking hook
    error which would bypass every safety layer.
    """
    bin_flow = (BIN_DIR / "flow").read_text()
    # The hook-specific fail-closed branch must exist and use exit 2
    assert 'if [ "$subcmd" = "hook" ]; then' in bin_flow, (
        "bin/flow must have a hook-specific fail-closed branch in the Python fallback"
    )
    # Find the hook branch and verify it uses exit 2
    hook_branch_start = bin_flow.index('if [ "$subcmd" = "hook" ]; then')
    hook_branch_end = bin_flow.index("fi", hook_branch_start)
    hook_branch = bin_flow[hook_branch_start:hook_branch_end]
    assert "exit 2" in hook_branch, "Hook fail-closed branch must use exit 2 (block), not exit 1 (error)"


def test_session_start_no_embedded_python():
    """Tombstone: session-start.sh embedded Python heredoc ported to Rust in PR #853. Must not return.

    The session-start hook previously embedded ~430 lines of Python in a bash
    heredoc. That logic now lives in `bin/flow session-context` (Rust). The
    bash shim must keep only the fast-exit guards and a single exec delegation.
    """
    content = (HOOKS_DIR / "session-start.sh").read_text()
    assert "python3 -" not in content, (
        "session-start.sh must not embed Python — ported to `bin/flow session-context` in PR #853"
    )
    assert "<< 'PYTHON'" not in content, (
        "session-start.sh must not contain the Python heredoc marker — ported to Rust in PR #853"
    )
    assert "session-context" in content, (
        "session-start.sh must delegate to `bin/flow session-context` (Rust subcommand)"
    )


def test_hooks_json_read_glob_grep_consolidated():
    """Read, Glob, Grep must share a single matcher entry in hooks.json."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    matchers = hooks["hooks"]["PreToolUse"]
    read_matchers = [m for m in matchers if m["matcher"] == "Read"]
    glob_matchers = [m for m in matchers if m["matcher"] == "Glob"]
    grep_matchers = [m for m in matchers if m["matcher"] == "Grep"]
    assert len(read_matchers) == 0, "Read should not have a separate matcher entry"
    assert len(glob_matchers) == 0, "Glob should not have a separate matcher entry"
    assert len(grep_matchers) == 0, "Grep should not have a separate matcher entry"
    combined = [m for m in matchers if "Read" in m["matcher"] and "Glob" in m["matcher"] and "Grep" in m["matcher"]]
    assert len(combined) == 1, f"Expected exactly 1 combined Read|Glob|Grep matcher, got {len(combined)}"


def test_hooks_json_has_no_exit_plan_validator():
    """hooks.json must NOT register an ExitPlanMode hook — plan mode removed."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    assert "PreToolUse" in hooks["hooks"]
    matchers = hooks["hooks"]["PreToolUse"]
    exit_plan_matchers = [m for m in matchers if m["matcher"] == "ExitPlanMode"]
    assert len(exit_plan_matchers) == 0, (
        f"ExitPlanMode hook should not exist — plan mode was removed. Found {len(exit_plan_matchers)} matcher(s)"
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
            f"Phase '{key}': conftest.make_state() uses '{fixture_name}' but flow-phases.json uses '{canonical_name}'"
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
    missing = [str(script) for script, test in scripts.items() if not test.exists()]
    assert not missing, f"Scripts without test files: {', '.join(missing)}"


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

    Learnings belong in rules files (.claude/rules/), not in CLAUDE.md.
    CLAUDE.md is for architecture, conventions, and project description."""
    content = (REPO_ROOT / "CLAUDE.md").read_text()
    assert "## Lessons Learned" not in content, (
        "CLAUDE.md still has a '## Lessons Learned' section — learnings belong in rules files, not CLAUDE.md"
    )


# --- Framework definition directory ---

FRAMEWORK_REQUIRED_FILES = ["detect.json", "permissions.json", "dependencies", "priming.md"]


def _load_frameworks():
    """Return list of (name, path) for every framework directory."""
    assert FRAMEWORKS_DIR.is_dir(), f"frameworks/ directory does not exist at {FRAMEWORKS_DIR}"
    frameworks = [(d.name, d) for d in sorted(FRAMEWORKS_DIR.iterdir()) if d.is_dir()]
    assert len(frameworks) >= 1, "frameworks/ directory has no framework subdirectories"
    return frameworks


def test_frameworks_directory_has_required_files():
    """Every frameworks/<name>/ must have detect.json, permissions.json,
    dependencies, and priming.md."""
    for name, path in _load_frameworks():
        for filename in FRAMEWORK_REQUIRED_FILES:
            assert (path / filename).exists(), f"frameworks/{name}/ missing required file: {filename}"


def test_framework_detect_json_schema():
    """Each detect.json must have name, display_name, and detect_globs fields."""
    for name, path in _load_frameworks():
        data = json.loads((path / "detect.json").read_text())
        assert "name" in data, f"frameworks/{name}/detect.json missing 'name'"
        assert "display_name" in data, f"frameworks/{name}/detect.json missing 'display_name'"
        assert "detect_globs" in data, f"frameworks/{name}/detect.json missing 'detect_globs'"
        assert isinstance(data["detect_globs"], list), f"frameworks/{name}/detect.json 'detect_globs' must be a list"
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
        assert "allow" in data, f"frameworks/{name}/permissions.json missing 'allow'"
        assert isinstance(data["allow"], list), f"frameworks/{name}/permissions.json 'allow' must be a list"
        for entry in data["allow"]:
            assert isinstance(entry, str), f"frameworks/{name}/permissions.json 'allow' entries must be strings"
            assert entry.startswith("Bash("), (
                f"frameworks/{name}/permissions.json entry '{entry}' must start with 'Bash('"
            )


def test_framework_dependencies_is_executable_script():
    """Each dependencies file must start with a shebang line."""
    for name, path in _load_frameworks():
        content = (path / "dependencies").read_text()
        assert content.startswith("#!/"), f"frameworks/{name}/dependencies must start with a shebang (#!/...)"


def test_plugin_json_has_no_config_hash():
    """plugin.json must not contain config_hash — it breaks the validator.

    The hash is computed dynamically by prime-setup.py and prime-check.py.
    """
    plugin = json.loads((REPO_ROOT / ".claude-plugin" / "plugin.json").read_text())
    assert "config_hash" not in plugin, (
        "plugin.json must not contain config_hash — Claude Code's plugin validator rejects unrecognized keys"
    )


def test_hooks_json_has_post_compact_hook():
    """hooks.json must register the post-compact hook via bin/flow hook post-compact."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    assert "PostCompact" in hooks["hooks"], (
        "hooks.json missing PostCompact key — the compaction data capture hook must be registered"
    )
    matchers = hooks["hooks"]["PostCompact"]
    assert len(matchers) >= 1, "PostCompact hook must have at least one entry"
    commands = [h["command"] for entry in matchers for h in entry["hooks"]]
    assert any("hook post-compact" in cmd for cmd in commands), (
        "PostCompact hook must reference bin/flow hook post-compact"
    )


def test_hooks_json_has_stop_continue_hook():
    """hooks.json must register the stop-continue hook via bin/flow hook stop-continue."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    assert "Stop" in hooks["hooks"], "hooks.json missing Stop key — the continuation hook must be registered"
    matchers = hooks["hooks"]["Stop"]
    assert len(matchers) >= 1, "Stop hook must have at least one entry"
    commands = [h["command"] for entry in matchers for h in entry["hooks"]]
    assert any("hook stop-continue" in cmd for cmd in commands), "Stop hook must reference bin/flow hook stop-continue"


def test_hooks_json_has_stop_failure_hook():
    """hooks.json must register the stop-failure hook via bin/flow hook stop-failure."""
    hooks = json.loads((HOOKS_DIR / "hooks.json").read_text())
    assert "StopFailure" in hooks["hooks"], (
        "hooks.json missing StopFailure key — the API error capture hook must be registered"
    )
    matchers = hooks["hooks"]["StopFailure"]
    assert len(matchers) >= 1, "StopFailure hook must have at least one entry"
    commands = [h["command"] for entry in matchers for h in entry["hooks"]]
    assert any("hook stop-failure" in cmd for cmd in commands), (
        "StopFailure hook must reference bin/flow hook stop-failure"
    )


def test_hooks_python_files_removed():
    """Tombstone: removed in PR #857. Python hook scripts must not return."""
    lib_dir = REPO_ROOT / "lib"
    assert not (lib_dir / "stop-continue.py").exists(), (
        "lib/stop-continue.py was ported to Rust (src/hooks/stop_continue.rs) and must not return"
    )
    assert not (lib_dir / "stop-failure.py").exists(), (
        "lib/stop-failure.py was ported to Rust (src/hooks/stop_failure.rs) and must not return"
    )
    assert not (lib_dir / "post-compact.py").exists(), (
        "lib/post-compact.py was ported to Rust (src/hooks/post_compact.rs) and must not return"
    )


def test_hooks_json_no_python_script_references():
    """Tombstone: removed in PR #857. hooks.json must not reference Python hook scripts."""
    hooks_content = (HOOKS_DIR / "hooks.json").read_text()
    assert "stop-continue.py" not in hooks_content, (
        "hooks.json must not reference stop-continue.py — ported to Rust in PR #857"
    )
    assert "stop-failure.py" not in hooks_content, (
        "hooks.json must not reference stop-failure.py — ported to Rust in PR #857"
    )
    assert "post-compact.py" not in hooks_content, (
        "hooks.json must not reference post-compact.py — ported to Rust in PR #857"
    )


AGENTS_DIR = REPO_ROOT / "agents"

SUPPORTED_AGENT_FRONTMATTER_KEYS = {
    "name",
    "description",
    "model",
    "effort",
    "maxTurns",
    "tools",
    "disallowedTools",
    "skills",
    "memory",
    "background",
    "isolation",
}


def test_agent_frontmatter_only_supported_keys():
    """Agent frontmatter must only use keys supported by Claude Code's plugin agent system.

    Tombstone: hooks removed from agent frontmatter in PR #656. Must not return.
    Unsupported keys (e.g. hooks, mcpServers, permissionMode) may cause
    agent loading failures in Claude Code versions that validate frontmatter
    strictly. The global PreToolUse hook in hooks.json provides Bash enforcement.
    """
    import yaml

    for agent_file in sorted(AGENTS_DIR.glob("*.md")):
        content = agent_file.read_text()
        parts = content.split("---", 2)
        assert len(parts) >= 3, f"{agent_file.name} missing YAML frontmatter delimiters"
        frontmatter = yaml.safe_load(parts[1])
        assert isinstance(frontmatter, dict), f"{agent_file.name} frontmatter is not a dict"
        unsupported = set(frontmatter.keys()) - SUPPORTED_AGENT_FRONTMATTER_KEYS
        assert not unsupported, (
            f"{agent_file.name} has unsupported frontmatter keys: {unsupported}. "
            f"Supported keys: {sorted(SUPPORTED_AGENT_FRONTMATTER_KEYS)}"
        )


def test_all_agents_specify_model_sonnet():
    """All sub-agents must specify model: sonnet to avoid inheriting Opus from the parent session."""
    import yaml

    for agent_file in sorted(AGENTS_DIR.glob("*.md")):
        content = agent_file.read_text()
        parts = content.split("---", 2)
        assert len(parts) >= 3, f"{agent_file.name} missing YAML frontmatter delimiters"
        frontmatter = yaml.safe_load(parts[1])
        assert isinstance(frontmatter, dict), f"{agent_file.name} frontmatter is not a dict"
        assert "model" in frontmatter, (
            f"{agent_file.name} missing 'model' key in frontmatter — "
            f"agents without an explicit model inherit Opus from the parent session"
        )
        assert frontmatter["model"] == "sonnet", (
            f"{agent_file.name} has model={frontmatter['model']!r}, expected 'sonnet' — "
            f"all sub-agents should run on Sonnet to reduce credit consumption"
        )


# --- Tombstone: Python phase lifecycle files ported to Rust in PR #805 ---


def test_no_python_check_phase():
    """Tombstone: ported to Rust in PR #805. Must not return."""
    assert not (LIB_DIR / "check-phase.py").exists(), "check-phase.py was ported to Rust — use flow-rs check-phase"


def test_no_python_phase_transition():
    """Tombstone: ported to Rust in PR #805. Must not return."""
    assert not (LIB_DIR / "phase-transition.py").exists(), (
        "phase-transition.py was ported to Rust — use flow-rs phase-transition"
    )


def test_no_python_test_check_phase():
    """Tombstone: ported to Rust in PR #805. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_check_phase.py").exists(), (
        "test_check_phase.py was ported to Rust — tests are in tests/check_phase.rs"
    )


def test_no_python_test_phase_transition():
    """Tombstone: ported to Rust in PR #805. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_phase_transition.py").exists(), (
        "test_phase_transition.py was ported to Rust — tests are in tests/phase_transition.rs"
    )


def test_no_python_append_note():
    """Tombstone: ported to Rust in PR #804. Must not return."""
    assert not (LIB_DIR / "append-note.py").exists(), "append-note.py was ported to Rust and must not be re-added"


def test_no_python_add_issue():
    """Tombstone: ported to Rust in PR #804. Must not return."""
    assert not (LIB_DIR / "add-issue.py").exists(), "add-issue.py was ported to Rust and must not be re-added"


def test_no_python_add_notification():
    """Tombstone: ported to Rust in PR #804. Must not return."""
    assert not (LIB_DIR / "add-notification.py").exists(), (
        "add-notification.py was ported to Rust and must not be re-added"
    )


def test_no_python_generate_id():
    """Tombstone: ported to Rust in PR #802. Must not return."""
    assert not (LIB_DIR / "generate-id.py").exists(), (
        "lib/generate-id.py must not exist — ported to Rust src/commands/generate_id.rs"
    )


def test_no_python_test_generate_id():
    """Tombstone: ported to Rust in PR #802. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_generate_id.py").exists(), (
        "tests/test_generate_id.py must not exist — tests ported to Rust"
    )


def test_no_python_test_log():
    """Tombstone: ported to Rust in PR #802. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_log.py").exists(), "tests/test_log.py must not exist — tests ported to Rust"


def test_checksum_version_invariant():
    """Validate checksum functions exist and the upgrade mechanism is documented.

    This test verifies:
    1. compute_setup_hash() exists and returns a valid 12-char hex digest
    2. compute_config_hash() exists and returns a valid 12-char hex digest
    3. The checksum → version section is documented in CLAUDE.md

    Hashes are used by prime-check.py for auto-upgrade detection at runtime.
    When a version mismatch exists, matching hashes allow auto-upgrade while
    mismatching hashes force a re-prime. Hash changes during development do
    not require version bumps — version bumps are a release decision.
    """
    import importlib.util

    prime_setup_path = LIB_DIR / "prime-setup.py"
    spec = importlib.util.spec_from_file_location("prime_setup", prime_setup_path)
    prime_setup = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prime_setup)

    setup_hash = prime_setup.compute_setup_hash()
    assert len(setup_hash) == 12
    assert all(c in "0123456789abcdef" for c in setup_hash)

    config_hash = prime_setup.compute_config_hash("python")
    assert len(config_hash) == 12
    assert all(c in "0123456789abcdef" for c in config_hash)

    claude_md = (REPO_ROOT / "CLAUDE.md").read_text()
    assert "Checksum → Version Invariant" in claude_md, "CLAUDE.md must document the checksum → version invariant"


# --- Tombstone tests: Python scripts ported to Rust (PR #803) ---


def test_no_python_set_timestamp():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (LIB_DIR / "set-timestamp.py").exists(), "set-timestamp.py ported to Rust (flow-rs set-timestamp)"


def test_no_python_set_blocked():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (LIB_DIR / "set-blocked.py").exists(), "set-blocked.py was ported to Rust — use flow-rs set-blocked"


def test_no_python_clear_blocked():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (LIB_DIR / "clear-blocked.py").exists(), "clear-blocked.py ported to Rust (flow-rs clear-blocked)"


def test_no_python_test_set_timestamp():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_set_timestamp.py").exists(), (
        "test_set_timestamp.py was ported to Rust — tests are in tests/set_timestamp.rs"
    )


def test_no_python_test_set_blocked():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_set_blocked.py").exists(), (
        "test_set_blocked.py was ported to Rust — tests are in tests/set_blocked.rs"
    )


def test_no_python_test_clear_blocked():
    """Tombstone: ported to Rust in PR #803. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_clear_blocked.py").exists(), (
        "test_clear_blocked.py was ported to Rust — tests are in tests/clear_blocked.rs"
    )


# --- Tombstone tests: Python scripts ported to Rust (PR #831) ---


def test_no_python_close_issue():
    """Tombstone: ported to Rust in PR #831. Must not return."""
    assert not (LIB_DIR / "close-issue.py").exists(), "close-issue.py was ported to Rust (flow-rs close-issue)"


def test_no_python_close_issues():
    """Tombstone: ported to Rust in PR #831. Must not return."""
    assert not (LIB_DIR / "close-issues.py").exists(), "close-issues.py was ported to Rust (flow-rs close-issues)"


def test_no_python_hook_refs_in_hooks_json():
    """Tombstone: hooks routed through bin/flow in PR #803. Must not return."""
    hooks_content = (HOOKS_DIR / "hooks.json").read_text()
    assert "set-blocked.py" not in hooks_content, (
        "hooks.json must not reference set-blocked.py — use bin/flow set-blocked"
    )
    assert "clear-blocked.py" not in hooks_content, (
        "hooks.json must not reference clear-blocked.py — use bin/flow clear-blocked"
    )


def test_no_python_analyze_issues():
    """Tombstone: ported to Rust in PR #834. Must not return."""
    assert not (LIB_DIR / "analyze-issues.py").exists(), (
        "analyze-issues.py was ported to Rust — use flow-rs analyze-issues"
    )


def test_no_python_test_analyze_issues():
    """Tombstone: ported to Rust in PR #834. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_analyze_issues.py").exists(), (
        "test_analyze_issues.py was ported to Rust — tests are in src/analyze_issues.rs"
    )


# --- Tombstone: Python auto-close-parent ported to Rust (PR #832) ---


def test_no_python_auto_close_parent():
    """Tombstone: ported to Rust in PR #832. Must not return."""
    assert not (LIB_DIR / "auto-close-parent.py").exists(), (
        "auto-close-parent.py was ported to Rust — use flow-rs auto-close-parent"
    )


def test_no_python_test_auto_close_parent():
    """Tombstone: ported to Rust in PR #832. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_auto_close_parent.py").exists(), (
        "test_auto_close_parent.py was ported to Rust — tests are in src/auto_close_parent.rs"
    )


# --- Tombstone: Python PR body scripts ported to Rust (PR #850) ---


def test_no_python_render_pr_body():
    """Tombstone: ported to Rust in PR #850. Must not return."""
    assert not (LIB_DIR / "render-pr-body.py").exists(), (
        "render-pr-body.py was ported to Rust — use flow-rs render-pr-body"
    )


def test_no_python_update_pr_body():
    """Tombstone: ported to Rust in PR #850. Must not return."""
    assert not (LIB_DIR / "update-pr-body.py").exists(), (
        "update-pr-body.py was ported to Rust — use flow-rs update-pr-body"
    )


def test_no_python_format_pr_timings():
    """Tombstone: ported to Rust in PR #850 and PR #851. Must not return."""
    assert not (LIB_DIR / "format-pr-timings.py").exists(), (
        "format-pr-timings.py was ported to Rust — use flow-rs format-pr-timings"
    )


def test_no_python_test_render_pr_body():
    """Tombstone: ported to Rust in PR #850. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_render_pr_body.py").exists(), (
        "test_render_pr_body.py was ported to Rust — tests are in src/render_pr_body.rs"
    )


def test_no_python_test_update_pr_body():
    """Tombstone: ported to Rust in PR #850. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_update_pr_body.py").exists(), (
        "test_update_pr_body.py was ported to Rust — tests are in src/update_pr_body.rs"
    )


def test_no_python_test_format_pr_timings():
    """Tombstone: ported to Rust in PR #850 and PR #851. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_format_pr_timings.py").exists(), (
        "test_format_pr_timings.py was ported to Rust — tests are in src/format_pr_timings.rs"
    )


# --- Tombstone: Python display formatters ported to Rust (PR #851) ---


def test_no_python_format_complete_summary():
    """Tombstone: ported to Rust in PR #851. Must not return."""
    assert not (LIB_DIR / "format-complete-summary.py").exists(), (
        "format-complete-summary.py was ported to Rust — use flow-rs format-complete-summary"
    )


def test_no_python_test_format_complete_summary():
    """Tombstone: ported to Rust in PR #851. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_format_complete_summary.py").exists(), (
        "test_format_complete_summary.py was ported to Rust — tests are in src/format_complete_summary.rs"
    )
