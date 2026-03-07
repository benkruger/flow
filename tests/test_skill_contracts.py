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
        # /flow:start -> start, /flow:research -> research, etc.
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


def test_phase_skills_2_through_7_have_hard_gate_checking_previous_phase():
    """Phases 2-7 must have a HARD-GATE that checks phases.<prev>.status."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:7]:
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
        assert not re.search(r"phases\.\w+\.status", content), (
            f"Utility skill '{name}' has a phase status check — "
            f"utility skills should not gate on phase status"
        )


def test_phase_1_has_no_previous_phase_gate():
    """Phase 1 (Start) should not check a previous phase's status."""
    content = _read_skill("start")
    # Start has HARD-GATE but for feature name, not for previous phase
    assert not re.search(r"phases\.\w+\.status", content), (
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


def test_initial_state_template_has_all_8_phases():
    """start-setup.py state template must have all 8 phases."""
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
    assert len(phases) == 8, f"Expected 8 phases, got {len(phases)}"

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
            refs = re.findall(r"/flow:(\w+)", content)
            for ref in refs:
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


def test_subagent_prompts_include_tool_restriction():
    """Review, Security sub-agent prompts must include
    the tool restriction rule in SKILL.md."""
    subagent_skills = ["review", "security"]
    for name in subagent_skills:
        skill_dir = SKILLS_DIR / name
        combined = ""
        for md_file in sorted(skill_dir.glob("*.md")):
            combined += md_file.read_text()
        assert "Glob" in combined and "Read" in combined, (
            f"skills/{name}/ sub-agent prompt missing "
            f"Glob/Read tool restriction"
        )


def test_subagent_prompts_allow_git_show():
    """Review/Security sub-agent prompts must allow git show.

    Sub-agents need git show to compare files against origin/main.
    Without it, sub-agents improvise with git show piped through sed,
    which triggers permission prompts."""
    subagent_skills = ["review", "security"]
    for name in subagent_skills:
        content = _read_skill(name)
        assert "git show" in content, (
            f"skills/{name}/SKILL.md sub-agent prompt missing 'git show' "
            f"in allowed git commands — sub-agents need it to compare "
            f"against origin/main"
        )


def test_subagent_prompts_ban_piping():
    """Review/Security sub-agent prompts must ban piping git output
    through sed, grep, or awk.

    Sub-agents pipe git show through sed to extract line ranges, which
    triggers permission prompts. The prompt must explicitly ban this."""
    subagent_skills = ["review", "security"]
    for name in subagent_skills:
        content = _read_skill(name)
        assert re.search(r"[Nn]ever pipe", content), (
            f"skills/{name}/SKILL.md sub-agent prompt missing pipe "
            f"restriction — sub-agents pipe git output through sed/grep "
            f"which triggers permission prompts"
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


def test_subagent_types_match_requirements():
    """Review/Security and Start use general-purpose."""
    subagent_skills = ["review", "security"]
    for name in subagent_skills:
        content = _read_skill(name)
        assert '"general-purpose"' in content, (
            f"skills/{name}/SKILL.md should use general-purpose subagent_type"
        )

    # Start's general-purpose sub-agent is in SKILL.md (framework sections merged)
    start_content = _read_skill("start")
    assert '"general-purpose"' in start_content, (
        "skills/start/SKILL.md should use general-purpose subagent_type"
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
    """Phases 1-7 should have state update instructions.
    Phase 8 (cleanup) deletes the state file instead of updating it."""
    phase_skills = _phase_skills()

    for key, skill_name in phase_skills.items():
        if key == "cleanup":
            continue  # Cleanup deletes state, doesn't update it
        content = _read_skill(skill_name)

        has_update = (
            "Update State" in content
            or "Update state" in content
            or "update state" in content
        )
        assert has_update, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no 'Update State' section"
        )


def test_phase_skills_with_content_writes_have_state_write_instruction():
    """Phase skills that write complex content objects (security) must
    instruct Claude to use Read+Write (not Edit) for state file updates.
    Without this, Claude uses the Edit tool, which fails on non-unique
    field names across phases.

    Plan uses Claude Code's native plan mode (plan file, not state).
    Skills that only use bin/flow commands for state mutations (start, code,
    review, learning) do not need this instruction."""
    phase_skills = _phase_skills()
    content_write_phases = {"security"}
    for key in content_write_phases:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "Never use the Edit tool for state file" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing "
            f"'Never use the Edit tool for state file' instruction"
        )


# --- Phase transition commands ---


def test_phase_skills_use_phase_transition_for_entry():
    """Phases 2-7 must use bin/flow phase-transition for state entry.
    Phase 1 uses start-setup.py which creates the state file directly.
    Phase 8 (cleanup) uses bin/flow cleanup instead."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:7]:
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
    """Model frontmatter must match: opus for Plan/Code/Security, sonnet for
    Review/Learning/Commit, haiku for Start/Cleanup."""
    expected = {
        "start": "haiku",
        "plan": "opus",
        "code": "opus",
        "review": "sonnet",
        "security": "opus",
        "learning": "sonnet",
        "cleanup": "haiku",
        "commit": "sonnet",
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
    for skill_name in ("abort", "cleanup"):
        content = _read_skill(skill_name)
        if ".log" not in content:
            continue  # Conditional contract — skill doesn't mention .log yet

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
            f"but not 'state file and log' — skill deletes both "
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
    """All 7 phase skills must have a ## Logging section."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "## Logging" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no '## Logging' section"
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
        "Phase 7 (cleanup) should have delete/remove instructions for state file"
    )
    # Should NOT have "Update State" section like other phases
    has_update_state = bool(re.search(r"##.*Update State", content, re.IGNORECASE))
    assert not has_update_state, (
        "Phase 7 (cleanup) should NOT have an 'Update State' section — "
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
    state = make_state(current_phase="start", phase_statuses={"start": "in_progress"})
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
        current_phase="plan",
        phase_statuses={"start": "complete", "plan": "in_progress"},
    )
    state["phases"]["start"]["cumulative_seconds"] = 300
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
    content = _read_skill("start")
    logging_match = re.search(
        r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
    )
    assert logging_match, "start/SKILL.md has no ## Logging section"
    logging_section = logging_match.group(1)

    assert "Read" in logging_section and "Write" in logging_section, (
        "start/SKILL.md ## Logging section must use Read+Write pattern — "
        "Bash >> with $(date) triggers permission prompts"
    )
    assert ">>" not in logging_section, (
        "start/SKILL.md ## Logging section must NOT use >> (Bash append) — "
        "it requires $(date) which triggers Claude Code's security prompt"
    )


def test_start_references_setup_script():
    """Start SKILL.md must reference start-setup.py for consolidated setup."""
    content = _read_skill("start")
    assert "start-setup" in content, (
        "start/SKILL.md must reference start-setup — "
        "Steps 2-7 are consolidated into a single Python script"
    )


# --- Release skill (maintainer) ---


def test_release_complete_banner_confirms_marketplace_update():
    """Release COMPLETE banner must say 'Local plugin upgraded:' to confirm
    the marketplace update ran, not ask the user to run it manually."""
    content = (REPO_ROOT / ".claude" / "skills" / "release" / "SKILL.md").read_text()
    assert "Local plugin upgraded:" in content, (
        "Release COMPLETE banner must confirm the marketplace update ran — "
        "use 'Local plugin upgraded:' not 'Run manually'"
    )


# --- Banner consistency ---


def test_utility_skill_banners_include_version():
    """Utility skill STARTING and COMPLETE banners must include the version."""
    version = _plugin_version()
    utility_with_banners = ["commit", "abort", "status"]

    for name in utility_with_banners:
        content = _read_skill(name)
        starting_pattern = rf"FLOW v{re.escape(version)}\s*—\s*flow:{name}|FLOW v{re.escape(version)}\s*—\s*{name.capitalize()}"
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
    content = (SKILLS_DIR / "commit" / "SKILL.md").read_text()

    restriction = "`--auto` is user-invoked only"
    assert restriction in content, (
        "skills/commit/SKILL.md missing '--auto is user-invoked only' restriction"
    )


def test_commit_tri_modal_detection():
    """Commit SKILL.md must have tri-modal detection (FLOW/Maintainer/Standalone)."""
    content = (SKILLS_DIR / "commit" / "SKILL.md").read_text()

    assert "flow-phases.json" in content, (
        "skills/commit/SKILL.md missing 'flow-phases.json' for mode detection"
    )
    assert "Maintainer" in content, (
        "skills/commit/SKILL.md missing 'Maintainer' mode reference"
    )
    assert "Standalone" in content, (
        "skills/commit/SKILL.md missing 'Standalone' mode reference"
    )
    assert ".flow-states" in content, (
        "skills/commit/SKILL.md missing '.flow-states' for FLOW mode detection"
    )


# --- Reset skill (maintainer) ---


def test_reset_guard_requires_main_branch():
    """Reset SKILL.md must guard against running outside main branch."""
    content = (REPO_ROOT / ".claude" / "skills" / "reset" / "SKILL.md").read_text()
    assert "main" in content, (
        "Reset SKILL.md must reference the main branch"
    )
    assert "git branch --show-current" in content, (
        "Reset SKILL.md must check current branch with git branch --show-current"
    )


def test_reset_has_inventory_step():
    """Reset SKILL.md must inventory artifacts before destroying them."""
    content = (REPO_ROOT / ".claude" / "skills" / "reset" / "SKILL.md").read_text()
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
    content = (REPO_ROOT / ".claude" / "skills" / "reset" / "SKILL.md").read_text()
    assert "AskUserQuestion" in content, (
        "Reset SKILL.md must use AskUserQuestion to confirm before destroying"
    )


def test_commit_mode_resolution():
    """Commit SKILL.md must default to auto and have Mode Resolution."""
    content = (SKILLS_DIR / "commit" / "SKILL.md").read_text()
    assert "the default is auto" in content, (
        "skills/commit/SKILL.md missing 'the default is auto' — "
        "commit mode must default to auto (no approval prompt)"
    )
    assert "Mode Resolution" in content, (
        "skills/commit/SKILL.md missing Mode Resolution section"
    )


def test_no_skill_invokes_commit_with_auto():
    """Skills that use /flow:commit --auto must be in the allow list.

    Learning uses --auto because the phase is fully autonomous. Simplify
    uses --auto because the user already approved changes in Step 2. Code,
    review, and security conditionally use --auto based on the commit axis
    setting."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir() or d.name in (
            "commit", "learning", "simplify", "code", "review", "security",
        ):
            continue
        content = (d / "SKILL.md").read_text()
        assert "/flow:commit --auto" not in content, (
            f"skills/{d.name}/SKILL.md references '/flow:commit --auto' — "
            f"--auto is user-invoked only, skills must not invoke it programmatically"
        )


# --- Release flags ---


def test_release_default_skips_approval():
    """Release SKILL.md default (no flags) must proceed without approval."""
    content = (REPO_ROOT / ".claude" / "skills" / "release" / "SKILL.md").read_text()
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
    """Learning skill must not contain worktree memory rescue logic.

    Since Claude Code 2.1.63, auto-memory is shared across git worktrees
    of the same repository. Worktree-specific memory paths no longer exist,
    so Source D rescue is obsolete."""
    content = _read_skill("learning")
    obsolete_terms = [
        "Source D",
        "worktree auto-memory",
        "Worth preserving",
        "worktree memory rescue",
    ]
    found = [term for term in obsolete_terms if term in content]
    assert not found, (
        f"skills/learning/SKILL.md still contains obsolete terms: {found} — "
        f"worktree memory rescue is obsolete since Claude Code 2.1.63"
    )


def test_multi_framework_skills_have_both_sections():
    """Skills that had framework fragments must have both Rails and Python
    content inline in SKILL.md."""
    multi_framework_skills = [
        "start", "plan", "code", "review", "security",
    ]
    for name in multi_framework_skills:
        content = _read_skill(name)
        assert re.search(r"(?i)rails", content), (
            f"skills/{name}/SKILL.md missing Rails framework section"
        )
        assert re.search(r"(?i)python", content), (
            f"skills/{name}/SKILL.md missing Python framework section"
        )


# --- Configurable auto/manual mode ---

CONFIGURABLE_SKILLS = [
    "start", "code", "simplify", "review", "security",
    "learning", "abort", "cleanup",
]


def test_configurable_skills_support_both_flags():
    """All 8 configurable skills must mention --auto and --manual in Usage."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "--auto" in content, (
            f"skills/{name}/SKILL.md missing '--auto' flag in Usage"
        )
        assert "--manual" in content, (
            f"skills/{name}/SKILL.md missing '--manual' flag in Usage"
        )


def test_configurable_skills_have_mode_resolution():
    """All 8 configurable skills must contain a Mode Resolution section."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "## Mode Resolution" in content, (
            f"skills/{name}/SKILL.md missing '## Mode Resolution' section"
        )


TWO_AXIS_SKILLS = ["code", "simplify", "review", "security", "learning"]
CONTINUE_ONLY_SKILLS = ["start"]
UTILITY_SKILLS = ["abort", "cleanup"]


def test_mode_resolution_references_flow_json():
    """All 8 configurable skills Mode Resolution must reference .flow.json."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        resolution_match = re.search(
            r"## Mode Resolution\n(.*?)(?:\n## |\Z)", content, re.DOTALL
        )
        assert resolution_match, (
            f"skills/{name}/SKILL.md has no Mode Resolution section"
        )
        resolution_text = resolution_match.group(1)
        assert ".flow.json" in resolution_text, (
            f"skills/{name}/SKILL.md Mode Resolution does not reference "
            f".flow.json for config lookup"
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