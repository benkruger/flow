"""Tests for SKILL.md content contracts.

The SKILL.md files are markdown, but they contain highly structured content:
phase gates, state field references, JSON schemas, cross-skill invocations,
and back navigation rules. All parseable with regex.
"""

import json
import re

from conftest import DOCS_DIR, REPO_ROOT, SKILLS_DIR


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def _phase_skills():
    """Return {phase_number: skill_name} for phases 1-8."""
    data = _load_phases()
    result = {}
    for num, phase in data["phases"].items():
        # /flow:start -> start, /flow:research -> research, etc.
        skill_name = phase["command"].split(":")[1]
        result[int(num)] = skill_name
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


def test_phase_skills_2_through_7_have_hard_gate_checking_previous_phase():
    """Phases 2-7 must have a HARD-GATE that checks phases.<N-1>.status."""
    phase_skills = _phase_skills()
    for phase_num in range(2, 8):
        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)
        prev = phase_num - 1

        assert "<HARD-GATE>" in content, (
            f"Phase {phase_num} ({skill_name}) has no <HARD-GATE>"
        )
        pattern = rf"phases\.{prev}\.status"
        assert re.search(pattern, content), (
            f"Phase {phase_num} ({skill_name}) HARD-GATE doesn't check "
            f"phases.{prev}.status"
        )


def test_utility_skills_have_no_phase_gate():
    """Utility skills should not have phase entry gates."""
    for name in _utility_skills():
        content = _read_skill(name)
        # They should not have the structured phase entry HARD-GATE
        # (checking phases.N.status)
        assert not re.search(r"phases\.\d+\.status", content), (
            f"Utility skill '{name}' has a phase status check — "
            f"utility skills should not gate on phase status"
        )


def test_phase_1_has_no_previous_phase_gate():
    """Phase 1 (Start) should not check a previous phase's status."""
    content = _read_skill("start")
    # Start has HARD-GATE but for feature name, not for previous phase
    assert not re.search(r"phases\.\d+\.status", content), (
        "Phase 1 (start) should not gate on any phase status"
    )


# --- State field schema ---


def test_embedded_json_blocks_are_valid():
    """Every fenced JSON block in any SKILL.md must be valid JSON."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        content = (d / "SKILL.md").read_text()
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
                    f"Invalid JSON in skills/{d.name}/SKILL.md block {i}: {e}"
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


def test_initial_state_template_has_all_8_phases():
    """start/SKILL.md initial state template must have all 8 phases."""
    content = _read_skill("start")
    # Find the state file JSON block (the big one with "phases")
    blocks = re.findall(r"```json\s*\n(.*?)```", content, re.DOTALL)

    state_block = None
    for block in blocks:
        if '"phases"' in block:
            cleaned = _clean_template_json(block)
            try:
                parsed = json.loads(cleaned)
                if "phases" in parsed:
                    state_block = parsed
                    break
            except json.JSONDecodeError:
                continue

    assert state_block is not None, "Could not find state template in start/SKILL.md"

    phases = state_block["phases"]
    assert len(phases) == 8, f"Expected 8 phases, got {len(phases)}"

    required_fields = [
        "name", "status", "started_at", "completed_at",
        "session_started_at", "cumulative_seconds", "visit_count",
    ]
    for i in range(1, 9):
        key = str(i)
        assert key in phases, f"Phase {i} missing from initial state template"
        for field in required_fields:
            assert field in phases[key], (
                f"Phase {i} missing field '{field}' in initial state template"
            )


def test_phase_names_in_state_match_flow_phases():
    """Phase names in start/SKILL.md initial state must match flow-phases.json."""
    data = _load_phases()
    content = _read_skill("start")
    blocks = re.findall(r"```json\s*\n(.*?)```", content, re.DOTALL)

    for block in blocks:
        if '"phases"' not in block:
            continue
        cleaned = _clean_template_json(block)
        try:
            parsed = json.loads(cleaned)
        except json.JSONDecodeError:
            continue
        if "phases" not in parsed:
            continue

        for num, phase in data["phases"].items():
            assert parsed["phases"][num]["name"] == phase["name"], (
                f"Phase {num}: state template has "
                f"'{parsed['phases'][num]['name']}' but flow-phases.json "
                f"has '{phase['name']}'"
            )
        return

    raise AssertionError("Could not find state template to validate phase names")


# --- Cross-skill invocations ---


def test_flow_references_point_to_existing_skills():
    """Every /flow:<name> reference in any SKILL.md must have a matching skills/<name>/."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        content = (d / "SKILL.md").read_text()
        refs = re.findall(r"/flow:(\w+)", content)
        for ref in refs:
            assert (SKILLS_DIR / ref).is_dir(), (
                f"skills/{d.name}/SKILL.md references /flow:{ref} "
                f"but skills/{ref}/ does not exist"
            )


def test_phase_transitions_follow_sequence():
    """Phase N's 'ready to begin' question should reference phase N+1."""
    phase_skills = _phase_skills()
    data = _load_phases()

    for phase_num in range(1, 8):  # 1-7 transition to next
        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)
        next_num = phase_num + 1
        next_name = data["phases"][str(next_num)]["name"]

        # Look for "Phase N+1: Name" in a transition question
        pattern = rf"Phase {next_num}:\s*{re.escape(next_name)}"
        assert re.search(pattern, content), (
            f"Phase {phase_num} ({skill_name}) does not reference "
            f"Phase {next_num}: {next_name} in its transition"
        )


def test_back_navigation_matches_can_return_to():
    """Back navigation options in each skill should only reference phases
    listed in that phase's can_return_to from flow-phases.json."""
    data = _load_phases()
    phase_skills = _phase_skills()

    for num_str, phase in data["phases"].items():
        phase_num = int(num_str)
        if not phase["can_return_to"]:
            continue

        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)

        # Find "Return to Phase N" or "Go back to Phase N" patterns
        back_refs = re.findall(
            r"(?:Return|Go back|Back) to (?:Phase )?(\d+)", content, re.IGNORECASE
        )

        for ref in back_refs:
            assert ref in phase["can_return_to"], (
                f"Phase {phase_num} ({skill_name}) has back navigation to "
                f"Phase {ref} but can_return_to only allows "
                f"{phase['can_return_to']}"
            )


# --- Sub-agent contracts ---


def test_subagent_prompts_include_tool_restriction():
    """Research, Design, Plan, Review sub-agent prompts must include the
    tool restriction rule."""
    subagent_skills = ["research", "design", "plan", "review"]
    for name in subagent_skills:
        content = _read_skill(name)
        assert "Glob" in content and "Read" in content, (
            f"skills/{name}/SKILL.md sub-agent prompt missing "
            f"Glob/Read tool restriction"
        )


def test_subagent_types_match_requirements():
    """Research/Design/Plan/Review use Explore; Start uses general-purpose."""
    explore_skills = ["research", "design", "plan", "review"]
    for name in explore_skills:
        content = _read_skill(name)
        assert '"Explore"' in content, (
            f"skills/{name}/SKILL.md should use Explore subagent_type"
        )

    start_content = _read_skill("start")
    assert '"general-purpose"' in start_content, (
        "skills/start/SKILL.md should use general-purpose subagent_type"
    )


# --- Structural format ---


def test_phase_skills_have_announce_banner():
    """Every phase skill (1-8) must have an announce banner with correct
    phase number and name."""
    phase_skills = _phase_skills()
    data = _load_phases()

    for phase_num, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        name = data["phases"][str(phase_num)]["name"]

        pattern = rf"Phase {phase_num}:\s*{re.escape(name)}\s*—\s*STARTING"
        assert re.search(pattern, content), (
            f"Phase {phase_num} ({skill_name}) missing announce banner "
            f"'Phase {phase_num}: {name} — STARTING'"
        )


def test_phase_skills_have_update_state_section():
    """Phases 1-7 should have state update instructions.
    Phase 8 (cleanup) deletes the state file instead of updating it."""
    phase_skills = _phase_skills()

    for phase_num, skill_name in phase_skills.items():
        if phase_num == 8:
            continue  # Cleanup deletes state, doesn't update it
        content = _read_skill(skill_name)

        has_update = (
            "Update State" in content
            or "Update state" in content
            or "update state" in content
        )
        assert has_update, (
            f"Phase {phase_num} ({skill_name}) has no 'Update State' section"
        )


# --- Recommended models ---


def test_model_recommendations_are_valid():
    """Every skill with a 'Recommended model' line must specify Haiku, Sonnet, or Opus."""
    valid_models = {"Haiku", "Sonnet", "Opus"}
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        content = (d / "SKILL.md").read_text()
        match = re.search(r"Recommended model:\s*(\w+)", content)
        if match:
            model = match.group(1)
            assert model in valid_models, (
                f"skills/{d.name}/SKILL.md recommends '{model}' — "
                f"must be one of {valid_models}"
            )


def test_model_recommendations_match_documented_table():
    """Model recommendations must match: Opus for Design/Code, Sonnet for
    Research/Plan/Review/Reflect/Commit, Haiku for Start/Cleanup."""
    expected = {
        "start": "Haiku",
        "research": "Sonnet",
        "design": "Opus",
        "plan": "Sonnet",
        "code": "Opus",
        "review": "Sonnet",
        "reflect": "Sonnet",
        "cleanup": "Haiku",
        "commit": "Sonnet",
    }
    for skill_name, expected_model in expected.items():
        content = _read_skill(skill_name)
        match = re.search(r"Recommended model:\s*(\w+)", content)
        assert match, (
            f"skills/{skill_name}/SKILL.md has no 'Recommended model' line"
        )
        assert match.group(1) == expected_model, (
            f"skills/{skill_name}/SKILL.md recommends '{match.group(1)}' "
            f"but expected '{expected_model}'"
        )


# --- Cross-file consistency ---


def test_cleanup_and_abort_mention_log_when_docs_delete_log():
    """If cleanup-process.md deletes .log files, abort and cleanup user-facing
    text must mention 'state file and log' (not just 'state file')."""
    cleanup_doc = (DOCS_DIR / "cleanup-process.md").read_text()
    if ".log" not in cleanup_doc:
        return  # Conditional contract — docs don't mention .log yet

    for skill_name in ("abort", "cleanup"):
        content = _read_skill(skill_name)
        # Extract user-facing text: blockquote lines and fenced code blocks
        user_facing = []
        for line in content.splitlines():
            if line.startswith("> "):
                user_facing.append(line)
        for block in re.findall(r"```\n(.*?)```", content, re.DOTALL):
            user_facing.extend(block.splitlines())
        combined = "\n".join(user_facing)

        assert "state file and log" in combined, (
            f"skills/{skill_name}/SKILL.md user-facing text mentions 'state file' "
            f"but not 'state file and log' — cleanup-process.md deletes both "
            f".json and .log files"
        )


def test_phase_transition_names_current_phase():
    """Phase N's transition question should include 'Phase N: Name is complete'."""
    phase_skills = _phase_skills()
    data = _load_phases()

    for phase_num in range(1, 8):  # 1-7 have transitions
        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)
        name = data["phases"][str(phase_num)]["name"]

        pattern = rf"Phase\s+{phase_num}:\s*{re.escape(name)}\s+is complete"
        assert re.search(pattern, content), (
            f"Phase {phase_num} ({skill_name}) does not contain "
            f"'Phase {phase_num}: {name} is complete' in its transition"
        )


def test_phase_8_has_soft_gate_not_hard_gate():
    """Phase 8 (cleanup) should have a SOFT-GATE, not a HARD-GATE.
    Cleanup warns but never blocks — it's the final escape hatch."""
    content = _read_skill("cleanup")
    assert "<SOFT-GATE>" in content, (
        "Phase 8 (cleanup) should have <SOFT-GATE> — cleanup warns but never blocks"
    )
    assert "<HARD-GATE>" not in content, (
        "Phase 8 (cleanup) should NOT have <HARD-GATE> — cleanup must never block"
    )


def test_phase_transitions_have_note_capture_option():
    """Phases 1-7 transition questions must offer a note-capture option.
    This is the third AskUserQuestion option at every phase boundary."""
    phase_skills = _phase_skills()
    for phase_num in range(1, 8):
        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)
        assert "correction or learning to capture" in content, (
            f"Phase {phase_num} ({skill_name}) transition question missing "
            f"'correction or learning to capture' option"
        )


def test_phase_1_hard_gate_checks_feature_name():
    """Phase 1 (start) should have a HARD-GATE that checks for feature name,
    not for a previous phase status."""
    content = _read_skill("start")
    assert "<HARD-GATE>" in content, "start/SKILL.md has no <HARD-GATE>"
    # Gate should mention feature name requirement
    gate_match = re.search(
        r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
    )
    assert gate_match, "Could not extract HARD-GATE content from start/SKILL.md"
    gate_text = gate_match.group(1)
    assert "feature" in gate_text.lower(), (
        "start/SKILL.md HARD-GATE should check for feature name"
    )


def test_phase_skills_have_logging_section():
    """All 8 phase skills must have a ## Logging section."""
    phase_skills = _phase_skills()
    for phase_num, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "## Logging" in content, (
            f"Phase {phase_num} ({skill_name}) has no '## Logging' section"
        )


def test_phase_8_has_delete_state_instructions():
    """Phase 8 (cleanup) should have instructions to delete the state file,
    not update it."""
    content = _read_skill("cleanup")
    has_delete = (
        "delete" in content.lower()
        or "remove" in content.lower()
        or "rm " in content
    )
    assert has_delete, (
        "Phase 8 (cleanup) should have delete/remove instructions for state file"
    )
    # Should NOT have "Update State" section like other phases
    has_update_state = bool(re.search(r"##.*Update State", content, re.IGNORECASE))
    assert not has_update_state, (
        "Phase 8 (cleanup) should NOT have an 'Update State' section — "
        "it deletes the state file instead"
    )


def test_back_navigation_names_match_can_return_to():
    """Back navigation options in each skill (using phase names like
    'Go back to Research') must only reference phases listed in can_return_to."""
    data = _load_phases()
    phase_skills = _phase_skills()

    # Build name -> phase number mapping
    name_to_num = {}
    for num_str, phase in data["phases"].items():
        name_to_num[phase["name"]] = num_str

    for num_str, phase in data["phases"].items():
        phase_num = int(num_str)
        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)

        # Match "Go back to <Name>" patterns (names, not numbers)
        back_refs = re.findall(
            r"Go back to (\w+)", content, re.IGNORECASE
        )

        for ref_name in back_refs:
            ref_num = name_to_num.get(ref_name)
            if ref_num is None:
                continue  # Not a phase name (e.g., "Go back to an approved section")
            assert ref_num in phase["can_return_to"], (
                f"Phase {phase_num} ({skill_name}) has 'Go back to {ref_name}' "
                f"(Phase {ref_num}) but can_return_to only allows "
                f"{phase['can_return_to']}"
            )


def test_can_return_to_targets_are_reachable():
    """Every can_return_to target must appear as a back navigation option
    in the skill text."""
    data = _load_phases()
    phase_skills = _phase_skills()

    for num_str, phase in data["phases"].items():
        phase_num = int(num_str)
        if not phase["can_return_to"]:
            continue

        skill_name = phase_skills[phase_num]
        content = _read_skill(skill_name)

        for target in phase["can_return_to"]:
            target_name = data["phases"][target]["name"]
            pattern = rf"(?:Go back|Return|Back) to {re.escape(target_name)}"
            assert re.search(pattern, content, re.IGNORECASE), (
                f"Phase {phase_num} ({skill_name}) has can_return_to "
                f"target Phase {target} ({target_name}) but no matching "
                f"back navigation text found"
            )


def test_status_skill_phase_names_match_flow_phases():
    """Status skill template must list all 8 phases with correct names from
    flow-phases.json."""
    data = _load_phases()
    content = _read_skill("status")

    for num_str, phase in data["phases"].items():
        pattern = rf"Phase\s+{num_str}:\s+{re.escape(phase['name'])}"
        assert re.search(pattern, content), (
            f"skills/status/SKILL.md does not contain "
            f"'Phase {num_str}: {phase['name']}' — "
            f"phase name may be out of sync with flow-phases.json"
        )