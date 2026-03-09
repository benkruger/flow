"""Tests for SKILL.md content contracts.

The SKILL.md files are markdown, but they contain highly structured content:
phase gates, state field references, JSON schemas, cross-skill invocations,
and back navigation rules. All parseable with regex.
"""

import json
import re

from conftest import DOCS_DIR, LIB_DIR, REPO_ROOT, SKILLS_DIR, PHASE_ORDER
from flow_utils import PHASE_NUMBER


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def _plugin_version():
    """Return the version string from plugin.json (e.g. '0.7.1')."""
    plugin = json.loads(
        (REPO_ROOT / ".claude-plugin" / "plugin.json").read_text()
    )
    return plugin["version"]


def _phase_skills():
    """Return {phase_key: skill_name} for all phases."""
    data = _load_phases()
    result = {}
    for key in data["order"]:
        phase = data["phases"][key]
        # /flow:flow-start -> flow-start, /flow:flow-plan -> flow-plan, etc.
        skill_name = phase["command"].split(":")[1]
        result[key] = skill_name
    return result


def _read_skill(name):
    return (SKILLS_DIR / name / "SKILL.md").read_text()


def _utility_skills():
    """Return skill names that are NOT phase skills."""
    phase_names = set(_phase_skills().values())
    return [
        d.name for d in sorted(SKILLS_DIR.iterdir())
        if d.is_dir() and d.name not in phase_names
    ]


# --- Phase gate consistency ---


def test_phase_skills_2_through_5_have_hard_gate_checking_previous_phase():
    """Phases 2-5 must have a HARD-GATE that checks phases.<prev>.status."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        prev_idx = PHASE_ORDER.index(key) - 1
        prev_key = PHASE_ORDER[prev_idx]

        assert "<HARD-GATE>" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no <HARD-GATE>"
        )
        pattern = rf"phases\.{prev_key}\.status"
        assert re.search(pattern, content), (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) HARD-GATE doesn't check "
            f"phases.{prev_key}.status"
        )


def test_utility_skills_have_no_phase_gate():
    """Utility skills should not have phase entry gates."""
    for name in _utility_skills():
        content = _read_skill(name)
        # They should not have the structured phase entry HARD-GATE
        # (checking phases.<key>.status)
        assert not re.search(r"phases\.[\w-]+\.status", content), (
            f"Utility skill '{name}' has a phase status check — "
            f"utility skills should not gate on phase status"
        )


def test_phase_1_has_no_previous_phase_gate():
    """Phase 1 (Start) should not check a previous phase's status."""
    content = _read_skill("flow-start")
    # Start has HARD-GATE but for feature name, not for previous phase
    assert not re.search(r"phases\.[\w-]+\.status", content), (
        "Phase 1 (start) should not gate on any phase status"
    )


# --- State field schema ---


def test_embedded_json_blocks_are_valid():
    """Every fenced JSON block in any skill .md file must be valid JSON."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        for md_file in sorted(d.glob("*.md")):
            content = md_file.read_text()
            rel = md_file.relative_to(REPO_ROOT)
            # Match ```json ... ``` blocks
            blocks = re.findall(r"```json\s*\n(.*?)```", content, re.DOTALL)
            for i, block in enumerate(blocks):
                stripped = block.strip()
                # Skip blocks with angle-bracket placeholders
                if re.search(r"<[^>]+>", block):
                    continue
                # Skip fragments that aren't top-level JSON
                if not stripped.startswith(("{", "[")):
                    continue
                # Skip example blocks with [...] or ... shorthand
                if "[...]" in block or "..." in block:
                    continue
                try:
                    json.loads(block)
                except json.JSONDecodeError as e:
                    raise AssertionError(
                        f"Invalid JSON in {rel} block {i}: {e}"
                    )


def _clean_template_json(block):
    """Replace angle-bracket placeholders so the block parses as JSON.

    Handles both bare placeholders (``<pr_number>``) and placeholders
    embedded inside quoted strings (``".worktrees/<feature-name>"``).
    """
    # First: replace entire quoted strings that contain a placeholder
    # Use [^"\n] to avoid matching across line boundaries
    cleaned = re.sub(r'"[^"\n]*<[^>]+>[^"\n]*"', '"placeholder"', block)
    # Then: replace any remaining bare placeholders (e.g. <pr_number>)
    cleaned = re.sub(r"<[^>]+>", "1", cleaned)
    return cleaned


def test_initial_state_template_has_all_6_phases():
    """start-setup.py state template must have all 6 phases."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "start_setup", LIB_DIR / "start-setup.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    # Call _create_state_file's phase construction logic via a temp dir
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        from pathlib import Path
        root = Path(tmp)
        mod._create_state_file(root, "test", "Test", "http://x/pull/1", 1)
        state = json.loads((root / ".flow-states" / "test.json").read_text())

    phases = state["phases"]
    assert len(phases) == 6, f"Expected 6 phases, got {len(phases)}"

    required_fields = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for key in PHASE_ORDER:
        assert key in phases, (
            f"Phase {PHASE_NUMBER[key]} ({key}) missing from initial state template"
        )
        for field in required_fields:
            assert field in phases[key], (
                f"Phase {PHASE_NUMBER[key]} ({key}) missing field '{field}' "
                f"in initial state template"
            )


def test_phase_names_in_state_match_flow_phases():
    """Phase names in start-setup.py state must match flow-phases.json."""
    import importlib.util
    import tempfile
    from pathlib import Path

    data = _load_phases()

    spec = importlib.util.spec_from_file_location(
        "start_setup", LIB_DIR / "start-setup.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        mod._create_state_file(root, "test", "Test", "http://x/pull/1", 1)
        state = json.loads((root / ".flow-states" / "test.json").read_text())

    for key, phase in data["phases"].items():
        assert state["phases"][key]["name"] == phase["name"], (
            f"Phase {PHASE_NUMBER[key]} ({key}): start-setup.py has "
            f"'{state['phases'][key]['name']}' but flow-phases.json "
            f"has '{phase['name']}'"
        )


# --- Cross-skill invocations ---


def test_flow_references_point_to_existing_skills():
    """Every /flow:<name> reference in any skill .md file must have a matching skills/<name>/."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        for md_file in sorted(d.glob("*.md")):
            content = md_file.read_text()
            rel = md_file.relative_to(REPO_ROOT)
            refs = re.findall(r"/flow:([\w-]+)", content)
            for ref in refs:
                if ref.endswith("-"):
                    continue  # placeholder like /flow:flow-<skill>
                assert (SKILLS_DIR / ref).is_dir(), (
                    f"{rel} references /flow:{ref} "
                    f"but skills/{ref}/ does not exist"
                )


def test_phase_transitions_follow_sequence():
    """Phase N's 'ready to begin' question should reference phase N+1."""
    phase_skills = _phase_skills()
    data = _load_phases()

    for idx, key in enumerate(PHASE_ORDER[:-1]):  # all but last transition to next
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        next_key = PHASE_ORDER[idx + 1]
        next_num = PHASE_NUMBER[next_key]
        next_name = data["phases"][next_key]["name"]

        # Look for "Phase N+1: Name" in a transition question
        pattern = rf"Phase {next_num}:\s*{re.escape(next_name)}"
        assert re.search(pattern, content), (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) does not reference "
            f"Phase {next_num}: {next_name} in its transition"
        )


# --- Sub-agent contracts ---


def test_start_uses_ci_fixer_subagent():
    """Start skill must reference the ci-fixer sub-agent for CI failures."""
    content = _read_skill("flow-start")
    assert '"ci-fixer"' in content, (
        "skills/flow-start/SKILL.md must reference ci-fixer sub-agent"
    )
    assert '"general-purpose"' not in content, (
        "skills/flow-start/SKILL.md must not reference general-purpose "
        "sub-agent — use ci-fixer instead"
    )


def test_complete_uses_ci_fixer_subagent():
    """Complete skill must reference the ci-fixer sub-agent for CI failures."""
    content = _read_skill("flow-complete")
    assert '"ci-fixer"' in content, (
        "skills/flow-complete/SKILL.md must reference ci-fixer sub-agent"
    )
    assert '"general-purpose"' not in content, (
        "skills/flow-complete/SKILL.md must not reference general-purpose "
        "sub-agent — use ci-fixer instead"
    )


def test_ci_fixer_agent_exists():
    """agents/ci-fixer.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "ci-fixer.md"
    assert agent_file.exists(), "agents/ci-fixer.md does not exist"
    content = agent_file.read_text()
    assert "name: ci-fixer" in content, (
        "agents/ci-fixer.md missing 'name: ci-fixer' in frontmatter"
    )
    assert "model: sonnet" in content, (
        "agents/ci-fixer.md missing 'model: sonnet' in frontmatter"
    )
    assert "PreToolUse" in content, (
        "agents/ci-fixer.md missing PreToolUse hook"
    )
    assert "validate-ci-bash" in content, (
        "agents/ci-fixer.md missing reference to validate-ci-bash"
    )


def test_code_review_delegates_to_builtin_review():
    """Code Review skill must delegate to Claude's built-in /review command."""
    content = _read_skill("flow-code-review")
    assert "/review" in content, (
        "skills/flow-code-review/SKILL.md must delegate to /review"
    )


def test_code_review_delegates_to_builtin_security_review():
    """Code Review skill must delegate to Claude's built-in /security-review."""
    content = _read_skill("flow-code-review")
    assert "/security-review" in content, (
        "skills/flow-code-review/SKILL.md must delegate to /security-review"
    )


def test_phase_skills_have_tool_restriction_in_hard_rules():
    """Every phase skill must have tool restriction language in its
    Hard Rules section.

    Rules in .claude/rules/ are passive context that Claude ignores under
    task pressure. Putting tool restrictions in the skill's Hard Rules
    makes them co-located with the active instructions Claude follows."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        hard_rules_match = re.search(
            r"## (?:Hard )?Rules\n(.*?)(?:\n## |\Z)", content, re.DOTALL
        )
        assert hard_rules_match, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no Hard Rules section"
        )
        rules_text = hard_rules_match.group(1)
        assert "Bash" in rules_text and ("Glob" in rules_text or "Read" in rules_text), (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Hard Rules missing tool "
            f"restriction — must restrict Bash and reference Glob/Read"
        )


# --- Structural format ---


def test_phase_skills_have_announce_banner():
    """Every phase skill (1-9) must have an announce banner with correct
    phase number, name, and version."""
    phase_skills = _phase_skills()
    data = _load_phases()
    version = _plugin_version()

    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        name = data["phases"][key]["name"]
        num = PHASE_NUMBER[key]

        pattern = (
            rf"FLOW v{re.escape(version)}\s*—\s*"
            rf"Phase {num}:\s*{re.escape(name)}\s*—\s*STARTING"
        )
        assert re.search(pattern, content), (
            f"Phase {num} ({skill_name}) missing announce banner "
            f"'FLOW v{version} — Phase {num}: {name} — STARTING'"
        )


def test_phase_skills_have_update_state_section():
    """Phases 1-5 should have state update instructions.
    Phase 6 (cleanup) deletes the state file instead of updating it."""
    phase_skills = _phase_skills()

    for key, skill_name in phase_skills.items():
        if key == "flow-complete":
            continue  # Complete deletes state, doesn't update it
        content = _read_skill(skill_name)

        has_update = (
            "Update State" in content
            or "Update state" in content
            or "update state" in content
        )
        assert has_update, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no 'Update State' section"
        )


# --- Phase transition commands ---


def test_phase_skills_use_phase_transition_for_entry():
    """Phases 2-5 must use bin/flow phase-transition for state entry.
    Phase 1 uses start-setup.py which creates the state file directly.
    Phase 6 (cleanup) uses bin/flow cleanup instead."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "phase-transition" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing "
            f"'phase-transition' command for entry"
        )
        assert "--action enter" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing "
            f"'--action enter' for phase entry"
        )


def test_phase_skills_use_phase_transition_for_completion():
    """Phases 1-7 must use bin/flow phase-transition for state completion."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "--action complete" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing "
            f"'--action complete' for phase completion"
        )


def test_phase_skills_no_inline_time_computation():
    """No phase skill may contain inline time computation instructions.
    All timing goes through bin/flow phase-transition. The hallmark
    pattern 'current_time - session_started_at' causes Claude to
    improvise python3 heredocs that trigger permission prompts."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "current_time - session_started_at" not in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) contains inline time "
            f"computation 'current_time - session_started_at' — "
            f"use bin/flow phase-transition instead"
        )


# --- Recommended models ---


def test_model_frontmatter_is_valid():
    """Every skill with a 'model' frontmatter field must specify haiku, sonnet, or opus."""
    valid_models = {"haiku", "sonnet", "opus"}
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        content = (d / "SKILL.md").read_text()
        match = re.search(r"^model:\s*(\w+)", content, re.MULTILINE)
        if match:
            model = match.group(1)
            assert model in valid_models, (
                f"skills/{d.name}/SKILL.md has model '{model}' — "
                f"must be one of {valid_models}"
            )


def test_model_frontmatter_matches_documented_table():
    """Model frontmatter must match: opus for Plan/Code/Code Review, sonnet for
    Learn/Commit, haiku for Start/Complete."""
    expected = {
        "flow-start": "haiku",
        "flow-plan": "opus",
        "flow-code": "opus",
        "flow-code-review": "opus",
        "flow-learn": "sonnet",
        "flow-complete": "haiku",
        "flow-commit": "sonnet",
    }
    for skill_name, expected_model in expected.items():
        content = _read_skill(skill_name)
        match = re.search(r"^model:\s*(\w+)", content, re.MULTILINE)
        assert match, (
            f"skills/{skill_name}/SKILL.md has no 'model' in frontmatter"
        )
        assert match.group(1) == expected_model, (
            f"skills/{skill_name}/SKILL.md has model '{match.group(1)}' "
            f"but expected '{expected_model}'"
        )


# --- Cross-file consistency ---


def test_cleanup_and_abort_mention_log_in_user_facing_text():
    """If cleanup/abort skills delete .log files, their user-facing
    text must mention 'state file and log' (not just 'state file')."""
    for skill_name in ("flow-abort", "flow-complete"):
        content = _read_skill(skill_name)
        if ".log" not in content:
            continue  # Conditional contract — skill doesn't mention .log yet

        # Check full content — blockquotes, banners (nested fenced blocks),
        # and prose are all user-facing in a skill file
        assert "state file and log" in content, (
            f"skills/{skill_name}/SKILL.md mentions '.log' files "
            f"but nowhere says 'state file and log' — skill deletes both "
            f".json and .log files"
        )


def test_phase_transition_names_current_phase():
    """Phase N's transition question should include 'Phase N: Name is complete'."""
    phase_skills = _phase_skills()
    data = _load_phases()

    for key in PHASE_ORDER[:-1]:  # all but last have transitions
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        name = data["phases"][key]["name"]
        num = PHASE_NUMBER[key]

        pattern = rf"Phase\s+{num}:\s*{re.escape(name)}\s+is complete"
        assert re.search(pattern, content), (
            f"Phase {num} ({skill_name}) does not contain "
            f"'Phase {num}: {name} is complete' in its transition"
        )


def test_phase_6_has_soft_gate_not_hard_gate():
    """Phase 6 (complete) should have a SOFT-GATE, not a HARD-GATE.
    Complete warns but never blocks — it's the final escape hatch."""
    content = _read_skill("flow-complete")
    assert "<SOFT-GATE>" in content, (
        "Phase 6 (complete) should have <SOFT-GATE> — complete warns but never blocks"
    )
    assert "<HARD-GATE>" not in content, (
        "Phase 6 (complete) should NOT have <HARD-GATE> — complete must never block"
    )


def test_phase_transitions_have_note_capture_option():
    """Phases 1-5 transition questions must offer a note-capture option.
    This is the third AskUserQuestion option at every phase boundary."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "correction or learning to capture" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) transition question missing "
            f"'correction or learning to capture' option"
        )


def test_phase_1_hard_gate_checks_feature_name():
    """Phase 1 (start) should have a HARD-GATE that checks for feature name,
    not for a previous phase status."""
    content = _read_skill("flow-start")
    assert "<HARD-GATE>" in content, "flow-start/SKILL.md has no <HARD-GATE>"
    # Gate should mention feature name requirement
    gate_match = re.search(
        r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
    )
    assert gate_match, "Could not extract HARD-GATE content from flow-start/SKILL.md"
    gate_text = gate_match.group(1)
    assert "feature" in gate_text.lower(), (
        "flow-start/SKILL.md HARD-GATE should check for feature name"
    )


def test_flow_start_surfaces_auto_upgrade():
    """flow-start Step 1 must handle auto_upgraded from prime-check output."""
    content = _read_skill("flow-start")
    assert "auto_upgraded" in content, (
        "flow-start/SKILL.md must mention auto_upgraded to surface "
        "auto-upgrade notices from prime-check"
    )


def test_phase_skills_have_logging_section():
    """All phase skills must have a ## Logging section."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "## Logging" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no '## Logging' section"
        )


def test_phase_6_has_delete_state_instructions():
    """Phase 6 (complete) should have instructions to delete the state file,
    not update it."""
    content = _read_skill("flow-complete")
    has_delete = (
        "delete" in content.lower()
        or "remove" in content.lower()
        or "rm " in content
    )
    assert has_delete, (
        "Phase 6 (complete) should have delete/remove instructions for state file"
    )
    # Should NOT have "Update State" section like other phases
    has_update_state = bool(re.search(r"##.*Update State", content, re.IGNORECASE))
    assert not has_update_state, (
        "Phase 6 (cleanup) should NOT have an 'Update State' section — "
        "it deletes the state file instead"
    )


def test_back_navigation_names_match_can_return_to():
    """Back navigation options in each skill (using phase names like
    'Go back to Code') must only reference phases listed in can_return_to."""
    data = _load_phases()
    phase_skills = _phase_skills()

    # Build name -> phase key mapping
    name_to_key = {}
    for key, phase in data["phases"].items():
        name_to_key[phase["name"]] = key

    for key, phase in data["phases"].items():
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        # Match "Go back to <Name>" patterns (names, not numbers)
        back_refs = re.findall(
            r"Go back to (\w+)", content, re.IGNORECASE
        )

        for ref_name in back_refs:
            ref_key = name_to_key.get(ref_name)
            if ref_key is None:
                continue  # Not a phase name (e.g., "Go back to an approved section")
            assert ref_key in phase["can_return_to"], (
                f"Phase {PHASE_NUMBER[key]} ({skill_name}) has 'Go back to {ref_name}' "
                f"({ref_key}) but can_return_to only allows "
                f"{phase['can_return_to']}"
            )


def test_can_return_to_targets_are_reachable():
    """Every can_return_to target must appear as a back navigation option
    in the skill text."""
    data = _load_phases()
    phase_skills = _phase_skills()

    for key, phase in data["phases"].items():
        if not phase["can_return_to"]:
            continue

        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        for target in phase["can_return_to"]:
            target_name = data["phases"][target]["name"]
            pattern = rf"(?:Go back|Return|Back) to {re.escape(target_name)}"
            assert re.search(pattern, content, re.IGNORECASE), (
                f"Phase {PHASE_NUMBER[key]} ({skill_name}) has can_return_to "
                f"target {target} ({target_name}) but no matching "
                f"back navigation text found"
            )


def test_status_formatter_phase_names_match_flow_phases():
    """format-status.py panel must include all 7 phases with correct names from
    flow-phases.json."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "format_status", LIB_DIR / "format-status.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    from conftest import make_state
    data = _load_phases()
    state = make_state(current_phase="flow-start", phase_statuses={"flow-start": "in_progress"})
    panel = mod.format_panel(state, _plugin_version())

    for key, phase in data["phases"].items():
        num = PHASE_NUMBER[key]
        pattern = rf"Phase\s+{num}:\s+{re.escape(phase['name'])}"
        assert re.search(pattern, panel), (
            f"format-status.py panel does not contain "
            f"'Phase {num}: {phase['name']}' — "
            f"phase name may be out of sync with flow-phases.json"
        )


def test_phase_skills_complete_banner_includes_timing():
    """Every phase skill (1-7) COMPLETE banner must include version and
    formatted_time in parentheses after COMPLETE."""
    phase_skills = _phase_skills()
    data = _load_phases()
    version = _plugin_version()

    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        name = data["phases"][key]["name"]
        num = PHASE_NUMBER[key]

        pattern = (
            rf"FLOW v{re.escape(version)}\s*—\s*"
            rf"Phase {num}:\s*{re.escape(name)}\s*—\s*"
            rf"COMPLETE\s*\(<formatted_time>\)"
        )
        assert re.search(pattern, content), (
            f"Phase {num} ({skill_name}) COMPLETE banner missing "
            f"version or formatted_time — expected "
            f"'FLOW v{version} — Phase {num}: {name} — "
            f"COMPLETE (<formatted_time>)'"
        )


def test_status_formatter_shows_timing_for_completed_phases():
    """format-status.py panel must show timing for completed phases
    ([x] lines)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "format_status", LIB_DIR / "format-status.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    from conftest import make_state
    state = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state["phases"]["flow-start"]["cumulative_seconds"] = 300
    panel = mod.format_panel(state, _plugin_version())
    match = re.search(r"\[x\].*Phase.*\(", panel)
    assert match, (
        "format-status.py panel missing timing on completed "
        "phase lines — [x] lines should include (Xh Ym)"
    )


# --- Start phase setup script ---


def test_start_logging_uses_read_write():
    """Start SKILL.md logging section must use Read+Write like every other skill.

    The >> (Bash append) pattern requires $(date ...) for timestamps, which
    triggers Claude Code's security prompt. settings.json cannot suppress $()
    prompts. The Read+Write pattern avoids this by generating the timestamp
    in Claude's tool layer."""
    content = _read_skill("flow-start")
    logging_match = re.search(
        r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
    )
    assert logging_match, "flow-start/SKILL.md has no ## Logging section"
    logging_section = logging_match.group(1)

    assert "Read" in logging_section and "Write" in logging_section, (
        "flow-start/SKILL.md ## Logging section must use Read+Write pattern — "
        "Bash >> with $(date) triggers permission prompts"
    )
    assert ">>" not in logging_section, (
        "flow-start/SKILL.md ## Logging section must NOT use >> (Bash append) — "
        "it requires $(date) which triggers Claude Code's security prompt"
    )


def test_start_references_setup_script():
    """Start SKILL.md must reference start-setup.py for consolidated setup."""
    content = _read_skill("flow-start")
    assert "start-setup" in content, (
        "start/SKILL.md must reference start-setup — "
        "Steps 2-7 are consolidated into a single Python script"
    )


# --- Release skill (maintainer) ---


def test_release_complete_banner_confirms_marketplace_update():
    """Release COMPLETE banner must say 'Local plugin upgraded:' to confirm
    the marketplace update ran, not ask the user to run it manually."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-release" / "SKILL.md").read_text()
    assert "Local plugin upgraded:" in content, (
        "Release COMPLETE banner must confirm the marketplace update ran — "
        "use 'Local plugin upgraded:' not 'Run manually'"
    )


# --- Banner consistency ---


def test_utility_skill_banners_include_version():
    """Utility skill STARTING and COMPLETE banners must include the version."""
    version = _plugin_version()
    utility_with_banners = ["flow-commit", "flow-abort", "flow-status"]

    for name in utility_with_banners:
        content = _read_skill(name)
        short_name = name.removeprefix("flow-").capitalize()
        starting_pattern = rf"FLOW v{re.escape(version)}\s*—\s*(?:flow:{name}|{short_name})"
        assert re.search(starting_pattern, content, re.IGNORECASE), (
            f"skills/{name}/SKILL.md STARTING banner missing version — "
            f"expected 'FLOW v{version}'"
        )


def test_phase_state_updates_suppress_output():
    """Phases 1-7 state update sections must tell Claude not to print the
    timing calculation. Without this, Claude shows work like
    'Phase 1 started at X, now Y = Z seconds.' before the banner."""
    phase_skills = _phase_skills()

    for key in PHASE_ORDER[:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        assert re.search(r"[Dd]o not print", content), (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) state update section missing "
            f"'Do not print' instruction — Claude will show timing "
            f"calculation as visible output"
        )


def test_phase_complete_banners_use_formatted_time():
    """Phase COMPLETE banners must use <formatted_time>, not raw
    <cumulative_seconds>."""
    phase_skills = _phase_skills()

    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "<cumulative_seconds>" not in content or "<formatted_time>" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) uses <cumulative_seconds> "
            f"in banner — use <formatted_time> instead"
        )


def test_phase_skills_have_time_format_instruction():
    """Phases 1-7 must include time formatting instructions near the
    completion banner so Claude formats the time correctly."""
    phase_skills = _phase_skills()

    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        has_format = (
            "Xh Ym" in content
            or "formatted_time" in content
        )
        assert has_format, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing time format "
            f"instruction — must specify format (Xh Ym / Xm / <1m)"
        )


# --- Commit --auto flag ---


def test_commit_auto_flag_restriction():
    """Commit SKILL.md must document that --auto is user-invoked only."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()

    restriction = "`--auto` is user-invoked only"
    assert restriction in content, (
        "skills/flow-commit/SKILL.md missing '--auto is user-invoked only' restriction"
    )


def test_commit_tri_modal_detection():
    """Commit SKILL.md must have tri-modal detection (FLOW/Maintainer/Standalone)."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()

    assert "flow-phases.json" in content, (
        "skills/flow-commit/SKILL.md missing 'flow-phases.json' for mode detection"
    )
    assert "Maintainer" in content, (
        "skills/flow-commit/SKILL.md missing 'Maintainer' mode reference"
    )
    assert "Standalone" in content, (
        "skills/flow-commit/SKILL.md missing 'Standalone' mode reference"
    )
    assert ".flow-states" in content, (
        "skills/flow-commit/SKILL.md missing '.flow-states' for FLOW mode detection"
    )


# --- Reset skill (maintainer) ---


def test_reset_guard_requires_main_branch():
    """Reset SKILL.md must guard against running outside main branch."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-reset" / "SKILL.md").read_text()
    assert "main" in content, (
        "Reset SKILL.md must reference the main branch"
    )
    assert "git branch --show-current" in content, (
        "Reset SKILL.md must check current branch with git branch --show-current"
    )


def test_reset_has_inventory_step():
    """Reset SKILL.md must inventory artifacts before destroying them."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-reset" / "SKILL.md").read_text()
    assert "git worktree list" in content, (
        "Reset must inventory worktrees"
    )
    assert "gh pr list" in content, (
        "Reset must inventory open PRs"
    )
    assert ".flow-states" in content, (
        "Reset must inventory state files"
    )


def test_reset_has_confirmation():
    """Reset SKILL.md must confirm before destroying artifacts."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-reset" / "SKILL.md").read_text()
    assert "AskUserQuestion" in content, (
        "Reset SKILL.md must use AskUserQuestion to confirm before destroying"
    )


# --- QA skill (maintainer) ---


def test_flow_qa_has_dev_mode_marker():
    """QA SKILL.md must reference the .dev-mode marker file."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert ".flow-states/.dev-mode" in content, (
        "flow-qa/SKILL.md must reference .flow-states/.dev-mode marker"
    )


def test_flow_qa_has_cache_nuke():
    """QA SKILL.md must nuke the plugin cache directory."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "rm -rf ~/.claude/plugins/cache/flow-marketplace" in content, (
        "flow-qa/SKILL.md must contain cache nuke command"
    )


def test_flow_qa_has_plugin_install_commands():
    """QA SKILL.md must contain both plugin uninstall and install commands."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "claude plugin uninstall flow@flow-marketplace" in content, (
        "flow-qa/SKILL.md must contain 'claude plugin uninstall flow@flow-marketplace'"
    )
    assert "claude plugin install flow@flow-marketplace" in content, (
        "flow-qa/SKILL.md must contain 'claude plugin install flow@flow-marketplace'"
    )


def test_flow_qa_has_reload_plugins():
    """flow-qa must reload plugins after install/uninstall changes."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "/reload-plugins" in content, (
        "flow-qa/SKILL.md must include /reload-plugins — "
        "skill list is stale until plugins are reloaded"
    )


def test_flow_qa_bare_is_status():
    """Bare /flow-qa must be a status check, not an alias for --start."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "Bare `/flow-qa` (no flags)" in content, (
        "flow-qa/SKILL.md must have a 'Bare /flow-qa (no flags)' section"
    )
    assert "DEV MODE (local)" in content, (
        "flow-qa/SKILL.md must show DEV MODE (local) status"
    )
    assert "MARKETPLACE (remote)" in content, (
        "flow-qa/SKILL.md must show MARKETPLACE (remote) status"
    )


def test_flow_qa_no_ask_user():
    """flow-qa must not prompt — all paths are automatic."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "AskUserQuestion" not in content, (
        "flow-qa/SKILL.md must not use AskUserQuestion — "
        "all paths should run without prompting"
    )


def test_commit_mode_resolution():
    """Commit SKILL.md must default to auto and have Mode Resolution."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()
    assert "the default is auto" in content, (
        "skills/flow-commit/SKILL.md missing 'the default is auto' — "
        "commit mode must default to auto (no approval prompt)"
    )
    assert "Mode Resolution" in content, (
        "skills/flow-commit/SKILL.md missing Mode Resolution section"
    )


def test_no_skill_invokes_commit_with_auto():
    """Skills that use /flow:flow-commit --auto must be in the allow list.

    Learn uses --auto because the phase is fully autonomous. Code and
    Code Review conditionally use --auto based on the commit axis setting."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir() or d.name in (
            "flow-commit", "flow-learn", "flow-code", "flow-code-review",
        ):
            continue
        content = (d / "SKILL.md").read_text()
        assert "/flow:flow-commit --auto" not in content, (
            f"skills/{d.name}/SKILL.md references '/flow:flow-commit --auto' — "
            f"--auto is user-invoked only, skills must not invoke it programmatically"
        )


# --- Release flags ---


def test_release_default_skips_approval():
    """Release SKILL.md default (no flags) must proceed without approval."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-release" / "SKILL.md").read_text()
    assert "proceed directly to Step 6" in content, (
        "Release SKILL.md must indicate that the default proceeds directly "
        "to Step 6 without approval"
    )


# --- Framework fragment contracts ---


def test_no_framework_fragment_files():
    """No skill directory should have rails.md or python.md fragment files.
    Framework instructions are merged into SKILL.md."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        assert not (d / "rails.md").exists(), (
            f"skills/{d.name}/rails.md still exists — "
            f"framework fragments should be merged into SKILL.md"
        )
        assert not (d / "python.md").exists(), (
            f"skills/{d.name}/python.md still exists — "
            f"framework fragments should be merged into SKILL.md"
        )


def test_learning_has_no_worktree_memory_rescue():
    """Learn skill must not contain worktree memory rescue logic.

    Since Claude Code 2.1.63, auto-memory is shared across git worktrees
    of the same repository. Worktree-specific memory paths no longer exist,
    so Source D rescue is obsolete."""
    content = _read_skill("flow-learn")
    obsolete_terms = [
        "Source D",
        "worktree auto-memory",
        "Worth preserving",
        "worktree memory rescue",
    ]
    found = [term for term in obsolete_terms if term in content]
    assert not found, (
        f"skills/flow-learn/SKILL.md still contains obsolete terms: {found} — "
        f"worktree memory rescue is obsolete since Claude Code 2.1.63"
    )


def test_generic_skills_have_no_framework_conditionals():
    """Skills that were made generic must not contain framework conditionals.

    Framework knowledge lives in frameworks/<name>/priming.md and the
    project CLAUDE.md — skills reference CLAUDE.md generically."""
    generic_skills = [
        "flow-plan", "flow-code", "flow-code-review",
    ]
    for name in generic_skills:
        content = _read_skill(name)
        assert "### If Rails" not in content, (
            f"skills/{name}/SKILL.md still has '### If Rails' conditional"
        )
        assert "### If Python" not in content, (
            f"skills/{name}/SKILL.md still has '### If Python' conditional"
        )
        assert "#### If Rails" not in content, (
            f"skills/{name}/SKILL.md still has '#### If Rails' conditional"
        )
        assert "#### If Python" not in content, (
            f"skills/{name}/SKILL.md still has '#### If Python' conditional"
        )


# --- Configurable auto/manual mode ---

CONFIGURABLE_SKILLS = [
    "flow-start", "flow-code", "flow-code-review",
    "flow-learn", "flow-abort", "flow-complete",
]


def test_configurable_skills_support_both_flags():
    """All 6 configurable skills must mention --auto and --manual in Usage."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "--auto" in content, (
            f"skills/{name}/SKILL.md missing '--auto' flag in Usage"
        )
        assert "--manual" in content, (
            f"skills/{name}/SKILL.md missing '--manual' flag in Usage"
        )


def test_configurable_skills_have_mode_resolution():
    """All 6 configurable skills must contain a Mode Resolution section."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "## Mode Resolution" in content, (
            f"skills/{name}/SKILL.md missing '## Mode Resolution' section"
        )


TWO_AXIS_SKILLS = ["flow-code", "flow-code-review", "flow-learn"]
CONTINUE_ONLY_SKILLS = ["flow-start"]
UTILITY_SKILLS = ["flow-abort", "flow-complete"]


def test_mode_resolution_references_config_source():
    """All 6 configurable skills Mode Resolution must reference config source."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        resolution_match = re.search(
            r"## Mode Resolution\n(.*?)(?:\n## |\Z)", content, re.DOTALL
        )
        assert resolution_match, (
            f"skills/{name}/SKILL.md has no Mode Resolution section"
        )
        resolution_text = resolution_match.group(1)
        if name == "flow-start":
            assert ".flow.json" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference "
                f".flow.json for config lookup"
            )
        else:
            assert ".flow-states/" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference "
                f"state file for config lookup"
            )
        assert f"skills.{name}" in resolution_text, (
            f"skills/{name}/SKILL.md Mode Resolution does not reference "
            f"'skills.{name}' key"
        )
        if name in TWO_AXIS_SKILLS:
            assert f"skills.{name}.commit" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference "
                f"'skills.{name}.commit' key"
            )
            assert f"skills.{name}.continue" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference "
                f"'skills.{name}.continue' key"
            )
        elif name in CONTINUE_ONLY_SKILLS:
            assert f"skills.{name}.continue" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference "
                f"'skills.{name}.continue' key"
            )


# --- Local permission skill contracts ---


def test_local_permission_skill_has_delete_command():
    """flow-local-permission SKILL.md must contain the rm command
    for deleting .claude/settings.local.json."""
    content = _read_skill("flow-local-permission")
    assert "rm .claude/settings.local.json" in content, (
        "skills/flow-local-permission/SKILL.md missing "
        "'rm .claude/settings.local.json' delete command"
    )


def test_local_permission_skill_uses_read_and_edit_tools():
    """flow-local-permission SKILL.md must reference Read and Edit tools
    for the permission merge workflow."""
    content = _read_skill("flow-local-permission")
    assert "Read" in content, (
        "skills/flow-local-permission/SKILL.md missing 'Read' tool reference"
    )
    assert "Edit" in content, (
        "skills/flow-local-permission/SKILL.md missing 'Edit' tool reference"
    )


def test_quadruple_fenced_blocks_use_markdown_and_text():
    """All ````-fenced blocks in skills must use ````markdown as the outer
    fence and ```text as the inner fence.

    Pattern 1 (correct):  ````markdown + ```text
    Pattern 2 (wrong):    ````text + bare ```
    Pattern 3 (wrong):    ````text with no inner fences
    Pattern 4 (wrong):    bare ``` for banners (no quadruple wrapper)
    """
    # Collect all skill files: public (skills/) and maintainer (.claude/skills/)
    skill_dirs = [
        d for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()
    ]
    maintainer_dir = REPO_ROOT / ".claude" / "skills"
    if maintainer_dir.is_dir():
        skill_dirs.extend(
            d for d in sorted(maintainer_dir.iterdir()) if d.is_dir()
        )

    errors = []
    for skill_dir in skill_dirs:
        skill_file = skill_dir / "SKILL.md"
        if not skill_file.exists():
            continue
        content = skill_file.read_text()
        name = skill_dir.name

        # Find all ````-fenced blocks (4+ backticks)
        # Pattern: ````<lang>\n...\n```` (matching closing fence)
        quad_blocks = re.finditer(
            r"^(`{4,})(\w*)\n(.*?)\n\1\s*$", content, re.MULTILINE | re.DOTALL
        )
        for match in quad_blocks:
            lang = match.group(2)
            inner = match.group(3)
            line_num = content[:match.start()].count("\n") + 1

            # Outer fence must be ````markdown, not ````text
            if lang != "markdown":
                errors.append(
                    f"{name}/SKILL.md:{line_num} — outer fence is "
                    f"````{lang}, should be ````markdown"
                )

            # Inner fences come in pairs: opening (```text) + closing (```)
            # Only validate opening fences (even indices: 0, 2, 4, ...)
            inner_fences = re.findall(r"^```(\w*)$", inner, re.MULTILINE)
            for i in range(0, len(inner_fences), 2):
                inner_lang = inner_fences[i]
                if inner_lang not in ("text", "diff"):
                    tag_desc = f"```{inner_lang}" if inner_lang else "bare ```"
                    errors.append(
                        f"{name}/SKILL.md:{line_num} — inner fence is "
                        f"{tag_desc}, should be ```text"
                    )

    assert not errors, (
        f"Quadruple-fenced blocks with wrong pattern:\n"
        + "\n".join(f"  - {e}" for e in errors)
    )


def test_learning_step_4_invokes_local_permission():
    """Learn SKILL.md Step 4 must invoke /flow:flow-local-permission."""
    content = _read_skill("flow-learn")
    step4_match = re.search(
        r"## Step 4.*?\n(.*?)(?:\n## Step 5|\n---)", content, re.DOTALL
    )
    assert step4_match, (
        "skills/flow-learn/SKILL.md has no Step 4 section"
    )
    step4_text = step4_match.group(1)
    assert "/flow:flow-local-permission" in step4_text, (
        "skills/flow-learn/SKILL.md Step 4 does not invoke "
        "/flow:flow-local-permission"
    )


# --- flow-start bug fixes ---


def test_phase_1_hard_gate_uses_ask_user_question():
    """Phase 1 first HARD-GATE must use AskUserQuestion tool."""
    content = _read_skill("flow-start")
    gate_match = re.search(
        r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
    )
    assert gate_match, "Could not extract first HARD-GATE from flow-start"
    gate_text = gate_match.group(1)
    assert "AskUserQuestion" in gate_text, (
        "flow-start first HARD-GATE must explicitly name the "
        "AskUserQuestion tool to ensure consistent prompting"
    )


def test_start_step_3_has_ci_fix_subagent():
    """Step 3 must launch the ci-fixer sub-agent to fix CI failures on main."""
    content = _read_skill("flow-start")
    step3_match = re.search(
        r"### Step 3.*?\n(.*?)(?=\n### Step 4)", content, re.DOTALL
    )
    assert step3_match, "Could not find Step 3 in flow-start/SKILL.md"
    step3_text = step3_match.group(1)
    assert "ci-fixer" in step3_text, (
        "flow-start Step 3 must reference the ci-fixer sub-agent "
        "for automatic CI fix"
    )
    assert "sub-agent" in step3_text.lower() or "Agent" in step3_text, (
        "flow-start Step 3 must reference launching a sub-agent"
    )


def test_start_step_3_commits_via_flow_commit():
    """Step 3 CI fixes on main must be committed via /flow:flow-commit."""
    content = _read_skill("flow-start")
    step3_match = re.search(
        r"### Step 3.*?\n(.*?)(?=\n### Step 4)", content, re.DOTALL
    )
    assert step3_match, "Could not find Step 3 in flow-start/SKILL.md"
    step3_text = step3_match.group(1)
    assert "/flow:flow-commit" in step3_text, (
        "flow-start Step 3 must commit CI fixes via /flow:flow-commit"
    )


def test_code_review_steps_have_continuation_directives():
    """Each Code Review step must have a continuation directive to the next."""
    content = _read_skill("flow-code-review")

    # Step 1 must continue to Step 2
    step1_match = re.search(
        r"## Step 1.*?\n(.*?)(?=\n## Step 2)", content, re.DOTALL
    )
    assert step1_match, "Could not find Step 1 in flow-code-review/SKILL.md"
    assert "continue to Step 2" in step1_match.group(1), (
        "flow-code-review Step 1 must contain 'continue to Step 2' directive"
    )

    # Step 2 must continue to Step 3
    step2_match = re.search(
        r"## Step 2.*?\n(.*?)(?=\n## Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-code-review/SKILL.md"
    assert "continue to Step 3" in step2_match.group(1), (
        "flow-code-review Step 2 must contain 'continue to Step 3' directive"
    )

    # Step 3 must continue to Done
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?=\n## Back Navigation|\n## Done)", content,
        re.DOTALL,
    )
    assert step3_match, "Could not find Step 3 in flow-code-review/SKILL.md"
    assert "continue to Done" in step3_match.group(1), (
        "flow-code-review Step 3 must contain 'continue to Done' directive"
    )


def test_code_review_hard_rules_require_step_continuation():
    """Hard Rules must require immediate continuation between steps."""
    content = _read_skill("flow-code-review")
    hard_rules_match = re.search(
        r"## Hard Rules\n(.*)", content, re.DOTALL
    )
    assert hard_rules_match, (
        "Could not find Hard Rules in flow-code-review/SKILL.md"
    )
    hard_rules = hard_rules_match.group(1)
    assert re.search(r"never pause", hard_rules, re.IGNORECASE), (
        "flow-code-review Hard Rules must contain 'never pause' language"
    )


def test_code_review_step_2_handles_no_findings():
    """Step 2 must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step2_match = re.search(
        r"## Step 2.*?\n(.*?)(?=\n## Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-code-review/SKILL.md"
    assert "no findings" in step2_match.group(1).lower(), (
        "flow-code-review Step 2 must handle the no-findings path"
    )


def test_start_step_6_enforces_flow_commit_exclusively():
    """Step 6 must use /flow:flow-commit and not suggest git commit."""
    content = _read_skill("flow-start")
    step6_match = re.search(
        r"### Step 6.*?\n(.*?)(?=\n### Done)", content, re.DOTALL
    )
    assert step6_match, "Could not find Step 6 in flow-start/SKILL.md"
    step6_text = step6_match.group(1)
    assert "/flow:flow-commit" in step6_text, (
        "flow-start Step 6 must reference /flow:flow-commit"
    )
    # Step 6 may mention "git commit" only in a prohibition (e.g. "Never use")
    for line in step6_text.splitlines():
        if "git commit" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-start Step 6 mentions 'git commit' outside a "
                f"prohibition: {line.strip()}"
            )


def test_prime_step_7_enforces_flow_commit_exclusively():
    """flow-prime Step 7 must use /flow:flow-commit and not suggest git commit."""
    content = _read_skill("flow-prime")
    step7_match = re.search(
        r"### Step 7.*?\n(.*?)(?=\n### Done)", content, re.DOTALL
    )
    assert step7_match, "Could not find Step 7 in flow-prime/SKILL.md"
    step7_text = step7_match.group(1)
    assert "/flow:flow-commit" in step7_text, (
        "flow-prime Step 7 must reference /flow:flow-commit"
    )
    for line in step7_text.splitlines():
        if "git commit" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-prime Step 7 mentions 'git commit' outside a "
                f"prohibition: {line.strip()}"
            )
