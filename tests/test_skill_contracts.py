"""Tests for SKILL.md content contracts.

The SKILL.md files are markdown, but they contain highly structured content:
phase gates, state field references, JSON schemas, cross-skill invocations,
and back navigation rules. All parseable with regex.
"""

import json
import re

from conftest import DOCS_DIR, LIB_DIR, REPO_ROOT, SKILLS_DIR, PHASE_ORDER
from flow_utils import PHASE_NAMES, PHASE_NUMBER


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


def test_phase_skills_1_through_5_have_done_section_hard_gate():
    """Phases 1-5 must have a HARD-GATE enforcing continue-mode branching."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        # Extract all HARD-GATE blocks
        hard_gates = re.findall(
            r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
        )

        # At least one HARD-GATE must enforce continue-mode branching
        has_continue_gate = any(
            "continue=manual" in gate and "continue=auto" in gate
            for gate in hard_gates
        )
        assert has_continue_gate, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no HARD-GATE "
            f"enforcing continue-mode branching (must contain both "
            f"'continue=auto' and 'continue=manual')"
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
    assert '"flow:ci-fixer"' in content, (
        "skills/flow-start/SKILL.md must reference flow:ci-fixer sub-agent"
    )
    assert '"general-purpose"' not in content, (
        "skills/flow-start/SKILL.md must not reference general-purpose "
        "sub-agent — use flow:ci-fixer instead"
    )


def test_complete_uses_ci_fixer_subagent():
    """Complete skill must reference the ci-fixer sub-agent for CI failures."""
    content = _read_skill("flow-complete")
    assert '"flow:ci-fixer"' in content, (
        "skills/flow-complete/SKILL.md must reference flow:ci-fixer sub-agent"
    )
    assert '"general-purpose"' not in content, (
        "skills/flow-complete/SKILL.md must not reference general-purpose "
        "sub-agent — use flow:ci-fixer instead"
    )


def test_complete_merge_command_no_delete_branch():
    """Complete skill merge command must not include --delete-branch."""
    content = _read_skill("flow-complete")
    # Find the gh pr merge bash block
    in_bash = False
    for line in content.splitlines():
        if line.strip() == "```bash":
            in_bash = True
            continue
        if line.strip() == "```" and in_bash:
            in_bash = False
            continue
        if in_bash and "gh pr merge" in line:
            assert "--delete-branch" not in line, (
                "Complete skill merge command must not use --delete-branch "
                "— cleanup.py handles branch deletion"
            )


def test_complete_does_not_contain_admin_flag():
    """Complete skill must never mention the --admin flag."""
    content = _read_skill("flow-complete")
    assert "--admin" not in content, (
        "skills/flow-complete/SKILL.md must not contain --admin"
    )


def test_complete_navigates_to_project_root():
    """Complete skill must cd to project root before cleanup (Step 10)."""
    content = _read_skill("flow-complete")
    assert "cd <project_root>" in content, (
        "Complete skill must include cd <project_root> before cleanup — "
        "worktree removal cannot run from inside the worktree"
    )


def test_ci_fixer_agent_exists():
    """agents/ci-fixer.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "ci-fixer.md"
    assert agent_file.exists(), "agents/ci-fixer.md does not exist"
    content = agent_file.read_text()
    assert "name: ci-fixer" in content, (
        "agents/ci-fixer.md missing 'name: ci-fixer' in frontmatter"
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
    """Phases 2-6 must use bin/flow phase-transition for state entry.
    Phase 1 uses start-setup.py which creates the state file directly."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:]:
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
    """Phases 1-6 must use bin/flow phase-transition for state completion."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER:
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


def test_start_logging_uses_safe_pattern():
    """Start SKILL.md logging section must use a safe logging pattern.

    Either Read+Write (tool layer timestamps) or bin/flow log (Python
    subprocess) is acceptable. The >> (Bash append) pattern requires
    $(date ...) which triggers Claude Code's security prompt."""
    content = _read_skill("flow-start")
    logging_match = re.search(
        r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
    )
    assert logging_match, "flow-start/SKILL.md has no ## Logging section"
    logging_section = logging_match.group(1)

    uses_read_write = "Read" in logging_section and "Write" in logging_section
    uses_flow_log = "bin/flow log" in logging_section
    assert uses_read_write or uses_flow_log, (
        "flow-start/SKILL.md ## Logging section must use Read+Write or "
        "bin/flow log pattern — Bash >> with $(date) triggers permission prompts"
    )
    assert ">>" not in logging_section, (
        "flow-start/SKILL.md ## Logging section must NOT use >> (Bash append) — "
        "it requires $(date) which triggers Claude Code's security prompt"
    )


def test_logged_phases_use_bin_flow_log():
    """Phases 2-4 logging sections must use bin/flow log, not Read+Write.

    The Read+Write pattern (read log file, append line, write back) is
    unreliable — Claude frequently skips the multi-step process. bin/flow log
    is a single command that always works. Phase 1 already uses it.
    Phases 5-6 intentionally have no logging commands."""
    logged_phases = ["flow-plan", "flow-code", "flow-code-review"]
    for skill_name in logged_phases:
        content = _read_skill(skill_name)
        logging_match = re.search(
            r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL
        )
        assert logging_match, f"{skill_name}/SKILL.md has no ## Logging section"
        logging_section = logging_match.group(1)

        assert "bin/flow log" in logging_section, (
            f"{skill_name}/SKILL.md ## Logging section must use bin/flow log"
        )
        has_read_write = "Read" in logging_section and "Write" in logging_section
        assert not has_read_write, (
            f"{skill_name}/SKILL.md ## Logging section must NOT use Read+Write "
            "pattern — it is unreliable. Use bin/flow log instead"
        )


def test_plan_dag_capture_is_explicit():
    """Plan SKILL.md Step 2 must have explicit DAG capture instructions.

    The vague phrase 'DAG content from the conversation' led to inconsistent
    DAG files — sometimes XML only, sometimes synthesis only. The instructions
    must specify capturing the complete decompose output."""
    content = _read_skill("flow-plan")
    # Extract Step 2 section
    step2_match = re.search(
        r"## Step 2.*?\n(.*?)(?=\n## Step 3|\Z)", content, re.DOTALL
    )
    assert step2_match, "flow-plan/SKILL.md has no Step 2 section"
    step2 = step2_match.group(1)

    assert "DAG content from the conversation" not in step2, (
        "flow-plan/SKILL.md Step 2 must NOT use the vague phrase "
        "'DAG content from the conversation' — it leads to inconsistent captures"
    )
    assert "complete decompose output" in step2.lower() or "complete output" in step2.lower(), (
        "flow-plan/SKILL.md Step 2 must instruct capturing the complete "
        "decompose output (XML plan + node executions + synthesis)"
    )
    step2_lower = step2.lower()
    assert (
        "do not summarize" in step2_lower
        or "do not condense" in step2_lower
        or "never rewrite" in step2_lower
    ), (
        "flow-plan/SKILL.md Step 2 must explicitly prohibit summarizing, "
        "condensing, or rewriting the decompose output"
    )


def test_learn_step3_requires_output_for_mistakes():
    """Learn SKILL.md Step 3 must require concrete output for every mistake.

    When Learn identifies Claude mistakes in Step 2, Step 3 must not allow
    'existing rules cover it' as an escape hatch. Every mistake must produce
    at least one artifact (CLAUDE.md edit, Rule issue, or Flow issue)."""
    content = _read_skill("flow-learn")
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?=\n## Step 4|\Z)", content, re.DOTALL
    )
    assert step3_match, "flow-learn/SKILL.md has no Step 3 section"
    step3_lower = step3_match.group(1).lower()

    assert (
        "every mistake must produce" in step3_lower
        or "must produce at least one" in step3_lower
    ), (
        "flow-learn/SKILL.md Step 3 must require every identified mistake "
        "to produce at least one concrete artifact"
    )
    assert "failed to prevent" in step3_lower, (
        "flow-learn/SKILL.md Step 3 must state that a rule which failed "
        "to prevent a mistake is not sufficient coverage"
    )


def test_anti_patterns_has_inline_output_rule():
    """Project .claude/rules/anti-patterns.md must have inline output rule.

    When a phase produces output the user needs to review, Claude must render
    it inline — never redirect to a file path."""
    anti_patterns = (REPO_ROOT / ".claude" / "rules" / "anti-patterns.md").read_text()
    lower = anti_patterns.lower()
    assert "inline" in lower and ("file path" in lower or "render" in lower), (
        ".claude/rules/anti-patterns.md must contain an inline output rule "
        "that prohibits redirecting users to file paths"
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
    utility_with_banners = ["flow-commit", "flow-abort", "flow-status", "flow-issues"]

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


# --- Banner style: tiered weight ---


def test_no_skills_use_equals_banners():
    """No SKILL.md should contain the old ============ banner pattern."""
    skill_dirs = [
        d for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()
    ]
    maintainer_dir = REPO_ROOT / ".claude" / "skills"
    if maintainer_dir.is_dir():
        skill_dirs.extend(
            d for d in sorted(maintainer_dir.iterdir()) if d.is_dir()
        )

    for skill_dir in skill_dirs:
        skill_file = skill_dir / "SKILL.md"
        if not skill_file.exists():
            continue
        content = skill_file.read_text()
        name = skill_dir.name
        assert "============" not in content, (
            f"{name}/SKILL.md still uses old ============ banner pattern — "
            f"use tiered Unicode borders instead (━ heavy, ─ light, ═ double)"
        )


def test_starting_banners_use_light_horizontal():
    """Every phase STARTING banner must use ── (light horizontal) borders."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        num = PHASE_NUMBER[key]
        # STARTING banner must be preceded/followed by light ── lines
        pattern = rf"──{{10,}}.*?STARTING.*?──{{10,}}"
        assert re.search(pattern, content, re.DOTALL), (
            f"Phase {num} ({skill_name}) STARTING banner must use "
            f"── (light horizontal) borders, not ━ or ═ or ="
        )


def test_complete_banners_use_heavy_horizontal():
    """Every phase COMPLETE banner must use ━━ (heavy horizontal) borders."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        num = PHASE_NUMBER[key]
        # COMPLETE banner must be preceded/followed by heavy ━━ lines
        pattern = rf"━━{{10,}}.*?COMPLETE.*?━━{{10,}}"
        assert re.search(pattern, content, re.DOTALL), (
            f"Phase {num} ({skill_name}) COMPLETE banner must use "
            f"━━ (heavy horizontal) borders, not ─ or ═ or ="
        )


def test_paused_banners_use_double_horizontal():
    """Every PAUSED banner must use ══ (double horizontal) borders."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        if "Paused" not in content:
            continue
        # PAUSED banner must use double ══ lines
        pattern = r"══{10,}.*?Paused.*?══{10,}"
        assert re.search(pattern, content, re.DOTALL), (
            f"{skill_name} PAUSED banner must use "
            f"══ (double horizontal) borders, not ━ or ─ or ="
        )


def test_complete_banners_have_check_mark():
    """Phase COMPLETE banner title lines must include ✓ marker."""
    phase_skills = _phase_skills()
    version = _plugin_version()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        num = PHASE_NUMBER[key]
        pattern = rf"✓\s+FLOW v{re.escape(version)}\s*—\s*Phase {num}:.*COMPLETE"
        assert re.search(pattern, content), (
            f"Phase {num} ({skill_name}) COMPLETE banner title must "
            f"include ✓ marker before FLOW version"
        )


def test_paused_banners_have_diamond():
    """PAUSED banner title lines must include ◆ marker."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        if "Paused" not in content:
            continue
        pattern = r"◆\s+FLOW\s*—\s*Paused"
        assert re.search(pattern, content), (
            f"{skill_name} PAUSED banner title must "
            f"include ◆ marker before FLOW"
        )


def test_format_status_no_equals_banners():
    """format-status.py must not use old ============ banner pattern."""
    content = (LIB_DIR / "format-status.py").read_text()
    assert "============" not in content, (
        "lib/format-status.py still uses old ============ banner pattern — "
        "use tiered Unicode borders (─ for status, ━ for all-complete)"
    )


def test_docs_no_equals_banners():
    """Docs reference files must not use old ============ banner pattern."""
    doc_files = [
        DOCS_DIR / "reference" / "skill-pattern.md",
        DOCS_DIR / "skills" / "flow-status.md",
    ]
    for doc_file in doc_files:
        if not doc_file.exists():
            continue
        content = doc_file.read_text()
        assert "============" not in content, (
            f"{doc_file.name} still uses old ============ banner pattern — "
            f"update to tiered Unicode borders"
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


def test_commit_has_commit_format_support():
    """Commit SKILL.md must support both commit_format options."""
    content = _read_skill("flow-commit")
    assert "commit_format" in content, (
        "skills/flow-commit/SKILL.md must reference 'commit_format' config key"
    )
    assert "title-only" in content, (
        "skills/flow-commit/SKILL.md must document 'title-only' format"
    )
    assert "full" in content.lower(), (
        "skills/flow-commit/SKILL.md must document 'full' format"
    )


def test_no_skill_invokes_commit_with_auto():
    """Skills that use /flow:flow-commit --auto must be in the allow list.

    Start uses --auto for CI baseline fixes and dependency commits on main.
    Learn uses --auto because the phase is fully autonomous. Code and
    Code Review conditionally use --auto based on the commit axis setting."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir() or d.name in (
            "flow-commit", "flow-start", "flow-learn", "flow-code", "flow-code-review",
        ):
            continue
        content = (d / "SKILL.md").read_text()
        assert "/flow:flow-commit --auto" not in content, (
            f"skills/{d.name}/SKILL.md references '/flow:flow-commit --auto' — "
            f"--auto is user-invoked only, skills must not invoke it programmatically"
        )


# --- Release flags ---


def test_release_manual_requires_approval():
    """Release SKILL.md --manual flag must pause for approval; default proceeds."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-release" / "SKILL.md").read_text()
    assert "AskUserQuestion" in content, (
        "Release SKILL.md must use AskUserQuestion for --manual approval"
    )
    assert "If `--manual` was explicitly passed" in content, (
        "Release SKILL.md must only prompt when --manual is explicitly passed"
    )
    assert "Unless `--manual` was explicitly passed" in content, (
        "Release SKILL.md default must proceed directly to Step 5"
    )


def test_prime_supports_reprime_flag():
    """Prime SKILL.md must support --reprime for fast upgrades."""
    content = _read_skill("flow-prime")
    assert "--reprime" in content, (
        "flow-prime/SKILL.md must support --reprime flag"
    )
    assert ".flow.json" in content and "Skip" in content, (
        "flow-prime --reprime must read .flow.json and skip questions"
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


def test_learning_repo_destinations_use_worktree_path():
    """Learn skill must use <worktree_path> for repo-destination edits.

    In Phase 5 mode, repo destinations (2 and 4) must be edited in the
    worktree, not at the project root. The skill must reference
    <worktree_path> to build paths for CLAUDE.md and .claude/rules/."""
    content = _read_skill("flow-learn")
    assert "<worktree_path>" in content, (
        "Learn skill must reference <worktree_path> for repo-destination "
        "edits so files are edited in the worktree, not the project root"
    )


def test_learning_has_no_private_destination_paths():
    """Learn skill must not route to paths outside the repo.

    All learnings go to repo-local destinations only (project CLAUDE.md
    and project rules). No writes to user-private paths."""
    content = _read_skill("flow-learn")
    private_paths = [
        "~/.claude/CLAUDE.md",
        "~/.claude/rules/",
        "~/.claude/projects/",
    ]
    found = [p for p in private_paths if p in content]
    assert not found, (
        f"skills/flow-learn/SKILL.md still references private paths: {found} — "
        f"all destinations must be repo-local"
    )
    assert "5 destinations" not in content.lower(), (
        "skills/flow-learn/SKILL.md still references '5 destinations' — "
        "should be 2 repo-local destinations"
    )


def test_learning_destinations_are_repo_only():
    """Learn skill must define repo-local destinations with correct routing.

    CLAUDE.md edits are direct (on disk). Rules edits are filed as issues."""
    content = _read_skill("flow-learn")
    assert "Destinations and routing" in content, (
        "Learn skill must have a 'Destinations and routing' section"
    )
    assert "Project CLAUDE.md" in content, (
        "Learn skill must include 'Project CLAUDE.md' as a destination"
    )
    assert "Edit on disk" in content, (
        "Learn skill must route CLAUDE.md edits to disk"
    )
    assert "Rule" in content, (
        "Learn skill must route .claude/rules/ changes as Rule issues"
    )


def test_learning_detects_dangling_async_operations():
    """Learn Source B must check for background agents launched but never awaited.

    Issue #177: Learn synthesis missed dangling background agents. Source B
    must include a proactive signal for async operations, and Step 2 must
    explain how to classify them."""
    content = _read_skill("flow-learn")
    # Source B section
    source_b_match = re.search(
        r"### Source B.*?\n(.*?)(?:\n### Source C|\n---)", content, re.DOTALL
    )
    assert source_b_match, "Learn skill has no Source B section"
    source_b_text = source_b_match.group(1)
    assert "background" in source_b_text.lower(), (
        "Learn Source B must mention background agents as a conversation signal"
    )
    # Step 2 section (reuse existing helper)
    step2_text = _learn_step_text(2)
    assert "dangling" in step2_text.lower() or "async" in step2_text.lower(), (
        "Learn Step 2 must include guidance on classifying dangling async findings"
    )


def test_learning_files_rule_issues():
    """Learn skill must file Rule issues for .claude/rules/ learnings."""
    content = _read_skill("flow-learn")
    # Step 3 must instruct filing issues with --label containing "Rule"
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?:\n## Step 4|\n---)", content, re.DOTALL
    )
    assert step3_match, "Learn skill has no Step 3 section"
    step3_text = step3_match.group(1)
    assert '--label' in step3_text and 'Rule' in step3_text, (
        "Learn Step 3 must instruct filing issues with label 'Rule'"
    )
    assert "bin/flow issue" in step3_text, (
        "Learn Step 3 must use 'bin/flow issue' to file Rule issues"
    )


def test_learning_files_flow_issues_not_learning():
    """Learn Step 6 must use label 'Flow', not 'learning'."""
    content = _read_skill("flow-learn")
    # Step 6 section
    step6_match = re.search(
        r"## Step 6.*?\n(.*?)(?:\n## Step 7|\n---)", content, re.DOTALL
    )
    assert step6_match, "Learn skill has no Step 6 section"
    step6_text = step6_match.group(1)
    assert "--label" in step6_text, (
        "Learn Step 6 must specify a --label for issue filing"
    )
    assert "Flow" in step6_text, (
        "Learn Step 6 must use label 'Flow' for process gap issues"
    )
    assert 'learning' not in step6_text.split("--label")[1].split("\n")[0].lower(), (
        "Learn Step 6 must not use label 'learning' — use 'Flow' instead"
    )


def test_code_files_flaky_test_issues():
    """Code skill CI Gate must file Flaky Test issues for intermittent failures."""
    content = _read_skill("flow-code")
    # CI Gate section must mention flaky test detection
    ci_gate_match = re.search(
        r"### bin/flow ci Gate.*?\n(.*?)(?:\n### Commit|\n---)",
        content,
        re.DOTALL,
    )
    assert ci_gate_match, "Code skill has no 'bin/flow ci Gate' section"
    ci_gate_text = ci_gate_match.group(1)
    assert "Flaky Test" in ci_gate_text, (
        "Code CI Gate must detect and file 'Flaky Test' issues"
    )
    assert "bin/flow issue" in ci_gate_text, (
        "Code CI Gate must use 'bin/flow issue' to file flaky test issues"
    )


def test_code_review_files_tech_debt_issues():
    """Code Review skill must file Tech Debt issues for out-of-scope findings."""
    content = _read_skill("flow-code-review")
    assert "Tech Debt" in content, (
        "Code Review skill must mention 'Tech Debt' for out-of-scope findings"
    )
    assert "bin/flow issue" in content, (
        "Code Review skill must use 'bin/flow issue' to file issues"
    )


def test_code_review_step1_files_tech_debt_issues():
    """Code Review Step 1 (Simplify) must file Tech Debt issues for out-of-scope findings."""
    content = _read_skill("flow-code-review")
    step1_start = content.index("## Step 1")
    step2_start = content.index("## Step 2")
    step1_content = content[step1_start:step2_start]
    assert "Tech Debt" in step1_content, (
        "Code Review Step 1 must mention 'Tech Debt' for out-of-scope findings"
    )
    assert "bin/flow issue" in step1_content, (
        "Code Review Step 1 must use 'bin/flow issue' to file issues"
    )
    assert "bin/flow add-issue" in step1_content, (
        "Code Review Step 1 must use 'bin/flow add-issue' to record filed issues"
    )


def test_code_review_files_doc_drift_issues():
    """Code Review skill must file Documentation Drift issues for stale docs."""
    content = _read_skill("flow-code-review")
    assert "Documentation Drift" in content, (
        "Code Review skill must mention 'Documentation Drift' for stale docs"
    )


def test_skills_record_issues_via_add_issue():
    """Every skill that calls bin/flow issue must also call bin/flow add-issue."""
    skills_with_issues = []
    for skill_path in sorted(SKILLS_DIR.glob("*/SKILL.md")):
        content = skill_path.read_text()
        if "bin/flow issue" in content:
            skills_with_issues.append(skill_path)
    assert skills_with_issues, "No skills call bin/flow issue — test is misconfigured"
    for skill_path in skills_with_issues:
        content = skill_path.read_text()
        assert "bin/flow add-issue" in content, (
            f"{skill_path.parent.name}/SKILL.md calls bin/flow issue but "
            f"never calls bin/flow add-issue to record it"
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
    "flow-start", "flow-plan", "flow-code", "flow-code-review",
    "flow-learn", "flow-abort", "flow-complete",
]


def test_configurable_skills_support_both_flags():
    """All 7 configurable skills must mention --auto and --manual in Usage."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "--auto" in content, (
            f"skills/{name}/SKILL.md missing '--auto' flag in Usage"
        )
        assert "--manual" in content, (
            f"skills/{name}/SKILL.md missing '--manual' flag in Usage"
        )


def test_configurable_skills_have_mode_resolution():
    """All 7 configurable skills must contain a Mode Resolution section."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "## Mode Resolution" in content, (
            f"skills/{name}/SKILL.md missing '## Mode Resolution' section"
        )


TWO_AXIS_SKILLS = ["flow-code", "flow-code-review", "flow-learn"]
CONTINUE_ONLY_SKILLS = ["flow-start", "flow-plan"]
UTILITY_SKILLS = ["flow-abort", "flow-complete"]


def test_mode_resolution_references_config_source():
    """All 7 configurable skills Mode Resolution must reference config source."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        resolution_match = re.search(
            r"## Mode Resolution\n(.*?)(?:\n## |\Z)", content, re.DOTALL
        )
        assert resolution_match, (
            f"skills/{name}/SKILL.md has no Mode Resolution section"
        )
        resolution_text = resolution_match.group(1)
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


def test_prime_presets_cover_all_configurable_skills():
    """Every skill in CONFIGURABLE_SKILLS must appear in all 3 prime presets."""
    content = _read_skill("flow-prime")
    # Extract the 3 preset JSON blocks (autonomous, manual, recommended)
    # They are the first 3 ```json blocks in the file
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3, (
        f"Expected at least 3 JSON blocks in flow-prime SKILL.md, "
        f"found {len(json_blocks)}"
    )
    preset_names = ["fully autonomous", "fully manual", "recommended"]
    for i, preset_name in enumerate(preset_names):
        parsed = json.loads(json_blocks[i])
        for skill in CONFIGURABLE_SKILLS:
            assert skill in parsed, (
                f"'{skill}' missing from {preset_name} preset in "
                f"flow-prime SKILL.md"
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


def test_phase_1_hard_gate_requires_rerun_with_arguments():
    """Phase 1 first HARD-GATE must tell user to re-run with arguments."""
    content = _read_skill("flow-start")
    gate_match = re.search(
        r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
    )
    assert gate_match, "Could not extract first HARD-GATE from flow-start"
    gate_text = gate_match.group(1)
    assert "feature name required" in gate_text.lower(), (
        "flow-start first HARD-GATE must tell the user that a feature "
        "name is required"
    )
    assert "/flow:flow-start" in gate_text, (
        "flow-start first HARD-GATE must show the usage pattern"
    )


def test_start_step_2_has_ci_fix_subagent():
    """Step 2 must launch the ci-fixer sub-agent for CI failures on main."""
    content = _read_skill("flow-start")
    step2_match = re.search(
        r"### Step 2.*?\n(.*?)(?=\n### Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-start/SKILL.md"
    step2_text = step2_match.group(1)
    assert "ci-fixer" in step2_text, (
        "flow-start Step 2 must reference the ci-fixer sub-agent "
        "for automatic CI fix"
    )
    assert "sub-agent" in step2_text.lower() or "Agent" in step2_text, (
        "flow-start Step 2 must reference launching a sub-agent"
    )


def test_start_ci_fixes_committed_via_flow_commit():
    """CI fixes on main must be committed via /flow:flow-commit in Step 2."""
    content = _read_skill("flow-start")
    step2_match = re.search(
        r"### Step 2.*?\n(.*?)(?=\n### Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-start/SKILL.md"
    step2_text = step2_match.group(1)
    assert "/flow:flow-commit" in step2_text, (
        "flow-start Step 2 must commit CI fixes via /flow:flow-commit"
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

    # Step 3 must continue to Step 4
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?=\n## Step 4)", content,
        re.DOTALL,
    )
    assert step3_match, "Could not find Step 3 in flow-code-review/SKILL.md"
    assert "continue to Step 4" in step3_match.group(1), (
        "flow-code-review Step 3 must contain 'continue to Step 4' directive"
    )

    # Step 4 must continue to Done
    step4_match = re.search(
        r"## Step 4.*?\n(.*?)(?=\n## Back Navigation|\n## Done)", content,
        re.DOTALL,
    )
    assert step4_match, "Could not find Step 4 in flow-code-review/SKILL.md"
    assert "continue to Done" in step4_match.group(1), (
        "flow-code-review Step 4 must contain 'continue to Done' directive"
    )


def test_code_review_hard_rules_require_step_continuation():
    """Hard Rules must require immediate continuation between all 4 steps."""
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
    for step_name in ["Simplify", "Review", "Security", "Code Review Plugin"]:
        assert step_name in hard_rules, (
            f"flow-code-review Hard Rules must mention '{step_name}' step"
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


def test_code_review_step_3_handles_no_findings():
    """Step 3 must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?=\n## Step 4)", content, re.DOTALL
    )
    assert step3_match, "Could not find Step 3 in flow-code-review/SKILL.md"
    assert "no findings" in step3_match.group(1).lower(), (
        "flow-code-review Step 3 must handle the no-findings path"
    )


def test_code_review_delegates_to_code_review_plugin():
    """Code Review must invoke the code-review:code-review plugin."""
    content = _read_skill("flow-code-review")
    assert "code-review:code-review" in content, (
        "flow-code-review must reference code-review:code-review plugin"
    )


def test_code_review_does_not_use_comment_flag():
    """Code Review must not use --comment flag with the plugin."""
    content = _read_skill("flow-code-review")
    assert "--comment" not in content, (
        "flow-code-review must not use --comment flag with code-review plugin"
    )


def test_code_review_step_4_handles_no_findings():
    """Step 4 must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step4_match = re.search(
        r"## Step 4.*?\n(.*?)(?=\n## Back Navigation|\n## Done)", content,
        re.DOTALL,
    )
    assert step4_match, "Could not find Step 4 in flow-code-review/SKILL.md"
    assert "no findings" in step4_match.group(1).lower(), (
        "flow-code-review Step 4 must handle the no-findings path"
    )


def test_code_review_has_resume_check():
    """Code Review SKILL.md must have a Resume Check section that reads code_review_step."""
    content = _read_skill("flow-code-review")
    resume_match = re.search(
        r"## Resume Check\n(.*?)(?=\n## Step 1)", content, re.DOTALL
    )
    assert resume_match, (
        "flow-code-review must have a Resume Check section before Step 1"
    )
    resume_text = resume_match.group(1)
    assert "code_review_step" in resume_text, (
        "Resume Check must reference code_review_step field"
    )


def _code_review_steps():
    """Yield (step_num, step_text) for each Code Review step section."""
    content = _read_skill("flow-code-review")
    for step_num in range(1, 5):
        if step_num < 4:
            next_header = f"## Step {step_num + 1}"
        else:
            next_header = "## Back Navigation|## Done"
        step_match = re.search(
            rf"## Step {step_num}.*?\n(.*?)(?=\n(?:{next_header}))",
            content, re.DOTALL,
        )
        assert step_match, f"Could not find Step {step_num} in flow-code-review/SKILL.md"
        yield step_num, step_match.group(1)


def test_code_review_steps_record_completion():
    """Each Code Review step must record completion via set-timestamp --set code_review_step=N."""
    for step_num, step_text in _code_review_steps():
        assert f"code_review_step={step_num}" in step_text, (
            f"Step {step_num} must contain 'code_review_step={step_num}' marker"
        )


def test_code_review_steps_self_invoke():
    """Each Code Review step must self-invoke flow:flow-code-review --continue-step."""
    for step_num, step_text in _code_review_steps():
        assert "flow:flow-code-review --continue-step" in step_text, (
            f"Step {step_num} must self-invoke via 'flow:flow-code-review --continue-step'"
        )


def test_code_review_steps_await_background_agents():
    """Steps 2-4 must instruct waiting for background agents (Step 1 uses foreground agents)."""
    for step_num, step_text in _code_review_steps():
        if step_num == 1:
            continue
        assert "background agent" in step_text.lower(), (
            f"Step {step_num} must contain background agent wait instructions"
        )


def test_code_review_has_self_invocation_check():
    """Code Review must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-code-review")
    assert "## Self-Invocation Check" in content, (
        "flow-code-review must have a '## Self-Invocation Check' section"
    )
    si_match = re.search(
        r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL
    )
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), (
        "Self-Invocation Check must reference --continue-step flag"
    )


def test_start_step_2_acquires_lock():
    """Step 2 must acquire the start lock before CI and dependency work."""
    content = _read_skill("flow-start")
    step2_match = re.search(
        r"### Step 2.*?\n(.*?)(?=\n### Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-start/SKILL.md"
    step2_text = step2_match.group(1)
    assert "start-lock" in step2_text, (
        "flow-start Step 2 must reference start-lock for serialization"
    )


def test_start_step_2_has_two_ci_gates():
    """Step 2 must have two bin/flow ci calls (baseline + post-deps)."""
    content = _read_skill("flow-start")
    step2_match = re.search(
        r"### Step 2.*?\n(.*?)(?=\n### Step 3)", content, re.DOTALL
    )
    assert step2_match, "Could not find Step 2 in flow-start/SKILL.md"
    step2_text = step2_match.group(1)
    ci_count = step2_text.count("bin/flow ci")
    assert ci_count >= 2, (
        f"flow-start Step 2 must have at least 2 bin/flow ci calls "
        f"(baseline + post-deps), found {ci_count}"
    )


def test_start_truncation_proceeds_without_confirmation():
    """Truncation instruction must tell Claude to proceed without confirming."""
    content = _read_skill("flow-start")
    assert "without" in content and "confirm" in content, (
        "flow-start SKILL.md must instruct Claude to proceed without "
        "asking for confirmation after branch name truncation"
    )


def test_start_derives_branch_name_from_prompt():
    """flow-start must derive a concise branch name, not pass all words verbatim."""
    content = _read_skill("flow-start")
    # Old verbatim instruction must be gone
    assert "ALL remaining words are the feature name" not in content, (
        "flow-start SKILL.md must not tell Claude to use all words as the "
        "feature name — Claude should derive a concise branch name"
    )
    # Must instruct Claude to derive a branch name
    assert "derive" in content.lower(), (
        "flow-start SKILL.md must instruct Claude to derive a branch name "
        "from the prompt"
    )
    # A HARD-GATE must prohibit treating the prompt as conversation
    gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)
    conversation_gate = any("conversation" in g.lower() for g in gates)
    assert conversation_gate, (
        "flow-start SKILL.md must have a HARD-GATE that prohibits treating "
        "the prompt as a conversation"
    )


def test_prime_commit_step_enforces_flow_commit_exclusively():
    """flow-prime commit step must use /flow:flow-commit and not raw git commands."""
    content = _read_skill("flow-prime")
    step_match = re.search(
        r"### Step 6.*?\n(.*?)(?=\n### Done)", content, re.DOTALL
    )
    assert step_match, "Could not find Step 6 (commit) in flow-prime/SKILL.md"
    step_text = step_match.group(1)
    assert "/flow:flow-commit" in step_text, (
        "flow-prime Step 6 must reference /flow:flow-commit"
    )
    for line in step_text.splitlines():
        if "git commit" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-prime Step 6 mentions 'git commit' outside a "
                f"prohibition: {line.strip()}"
            )
        if "git add" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-prime Step 6 mentions 'git add' outside a "
                f"prohibition: {line.strip()}"
            )


def test_prime_has_plugin_installation_step():
    """flow-prime must have a step installing the code-review plugin."""
    content = _read_skill("flow-prime")
    assert "claude plugin list" in content, (
        "flow-prime must include 'claude plugin list' command"
    )
    assert "claude plugin marketplace add" in content, (
        "flow-prime must include 'claude plugin marketplace add' command"
    )
    assert "claude plugin install" in content, (
        "flow-prime must include 'claude plugin install' command"
    )


def test_prime_has_commit_format_prompt():
    """flow-prime must prompt the user to choose commit message format."""
    content = _read_skill("flow-prime")
    assert "commit_format" in content, (
        "flow-prime must reference 'commit_format' config key"
    )
    assert "title-only" in content, (
        "flow-prime must offer 'title-only' format option"
    )
    assert "full" in content.lower(), (
        "flow-prime must offer 'full' format option"
    )


def test_code_review_sets_continue_pending_before_child_skills():
    """Each Code Review step must set _continue_pending before child skill."""
    content = _read_skill("flow-code-review")
    child_skills = [
        ("review", "/review"),
        ("security-review", "/security-review"),
        ("code-review:code-review", "code-review:code-review"),
    ]
    for flag_value, skill_ref in child_skills:
        flag_pattern = f"_continue_pending={flag_value}"
        assert flag_pattern in content, (
            f"Code Review must set _continue_pending={flag_value} "
            f"before invoking {skill_ref}"
        )
        flag_pos = content.index(flag_pattern)
        skill_pos = content.index(skill_ref, flag_pos)
        assert flag_pos < skill_pos, (
            f"_continue_pending={flag_value} must appear before "
            f"{skill_ref} invocation"
        )


def test_code_skill_sets_continue_pending_before_commit():
    """Code phase must set _continue_pending before /flow:flow-commit."""
    content = _read_skill("flow-code")
    assert "_continue_pending=commit" in content, (
        "Code phase must set _continue_pending=commit before commit"
    )
    flag_pos = content.index("_continue_pending=commit")
    commit_pos = content.index("/flow:flow-commit", flag_pos)
    assert flag_pos < commit_pos, (
        "_continue_pending=commit must appear before /flow:flow-commit"
    )


def test_plan_step_1_fetches_referenced_issues():
    """Plan Step 1 must instruct fetching referenced GitHub issues."""
    content = _read_skill("flow-plan")
    assert "gh issue view" in content


# --- Code phase self-invocation contracts ---


def test_code_has_resume_check():
    """Code SKILL.md must have a Resume Check section that reads code_task."""
    content = _read_skill("flow-code")
    resume_match = re.search(
        r"## Resume Check\n(.*?)(?=\n## Execute Next Task)", content, re.DOTALL
    )
    assert resume_match, (
        "flow-code must have a Resume Check section before Execute Next Task"
    )
    resume_text = resume_match.group(1)
    assert "code_task" in resume_text, (
        "Resume Check must reference code_task field"
    )


def test_code_has_self_invocation_check():
    """Code must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-code")
    assert "## Self-Invocation Check" in content, (
        "flow-code must have a '## Self-Invocation Check' section"
    )
    si_match = re.search(
        r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL
    )
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), (
        "Self-Invocation Check must reference --continue-step flag"
    )


def test_code_commit_self_invokes():
    """Code Commit section must self-invoke flow:flow-code --continue-step."""
    content = _read_skill("flow-code")
    commit_match = re.search(
        r"### Commit\n(.*?)(?=\n## |\n---\n\n## )", content, re.DOTALL
    )
    assert commit_match, "Could not find Commit section in flow-code/SKILL.md"
    assert "flow:flow-code --continue-step" in commit_match.group(1), (
        "Commit section must self-invoke via 'flow:flow-code --continue-step'"
    )


def test_code_commit_records_task():
    """Code Commit section must record code_task via set-timestamp."""
    content = _read_skill("flow-code")
    commit_match = re.search(
        r"### Commit\n(.*?)(?=\n## |\n---\n\n## )", content, re.DOTALL
    )
    assert commit_match, "Could not find Commit section in flow-code/SKILL.md"
    assert "code_task=" in commit_match.group(1), (
        "Commit section must contain 'code_task=' marker"
    )


# --- Code phase single-task framing contracts ---


def test_code_skill_uses_single_task_framing():
    """Code skill must use single-task framing, not loop-iteration language."""
    content = _read_skill("flow-code")
    assert "## Execute Next Task" in content, (
        "flow-code must have '## Execute Next Task' section (not '## Task Loop')"
    )
    assert "## Task Loop" not in content, (
        "flow-code must not contain '## Task Loop' — use single-task framing"
    )
    assert "Work through each task" not in content, (
        "flow-code must not contain loop-iteration language"
    )
    assert "For each task" not in content, (
        "flow-code must not contain 'For each task' loop language"
    )


# --- Learn phase self-invocation contracts ---


def test_learn_has_resume_check():
    """Learn SKILL.md must have a Resume Check section that reads learn_step."""
    content = _read_skill("flow-learn")
    resume_match = re.search(
        r"## Resume Check\n(.*?)(?=\n## Step 1)", content, re.DOTALL
    )
    assert resume_match, (
        "flow-learn must have a Resume Check section before Step 1"
    )
    resume_text = resume_match.group(1)
    assert "learn_step" in resume_text, (
        "Resume Check must reference learn_step field"
    )


def test_learn_has_self_invocation_check():
    """Learn must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-learn")
    assert "## Self-Invocation Check" in content, (
        "flow-learn must have a '## Self-Invocation Check' section"
    )
    si_match = re.search(
        r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL
    )
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), (
        "Self-Invocation Check must reference --continue-step flag"
    )


def _learn_step_text(step_num):
    """Extract Learn step section text by number."""
    content = _read_skill("flow-learn")
    if step_num < 7:
        next_header = f"## Step {step_num + 1}"
    else:
        next_header = "## Done"
    step_match = re.search(
        rf"## Step {step_num}.*?\n(.*?)(?=\n(?:{next_header}))",
        content, re.DOTALL,
    )
    assert step_match, (
        f"Could not find Step {step_num} in flow-learn/SKILL.md"
    )
    return step_match.group(1)


def test_learn_step_4_self_invokes():
    """Learn Step 4 must self-invoke flow:flow-learn --continue-step."""
    step_text = _learn_step_text(4)
    assert "flow:flow-learn --continue-step" in step_text, (
        "Step 4 must self-invoke via 'flow:flow-learn --continue-step'"
    )


def test_learn_step_5_self_invokes():
    """Learn Step 5 must self-invoke flow:flow-learn --continue-step."""
    step_text = _learn_step_text(5)
    assert "flow:flow-learn --continue-step" in step_text, (
        "Step 5 must self-invoke via 'flow:flow-learn --continue-step'"
    )


def test_learn_sets_continue_pending_before_child_skills():
    """Learn must set _continue_pending before each child skill invocation."""
    content = _read_skill("flow-learn")
    child_skills = [
        ("local-permission", "/flow:flow-local-permission"),
        ("commit", "/flow:flow-commit"),
    ]
    for flag_value, skill_ref in child_skills:
        flag_pattern = f"_continue_pending={flag_value}"
        assert flag_pattern in content, (
            f"Learn must set _continue_pending={flag_value} "
            f"before invoking {skill_ref}"
        )
        flag_pos = content.index(flag_pattern)
        skill_pos = content.index(skill_ref, flag_pos)
        assert flag_pos < skill_pos, (
            f"_continue_pending={flag_value} must appear before "
            f"{skill_ref} invocation"
        )


def test_learn_steps_record_completion():
    """Learn Steps 4 and 5 must record completion via set-timestamp."""
    for step_num in [4, 5]:
        step_text = _learn_step_text(step_num)
        assert f"learn_step={step_num}" in step_text, (
            f"Step {step_num} must contain 'learn_step={step_num}' marker"
        )


def test_plan_skill_does_not_reference_transcript_path():
    """Plan must not contain transcript_path — session log artifact lives in Complete."""
    content = _read_skill("flow-plan")
    assert "transcript_path" not in content, (
        "flow-plan/SKILL.md must not reference transcript_path. "
        "The session log artifact belongs in flow-complete Step 6."
    )


def test_complete_skill_uses_render_pr_body():
    """Complete Step 6 must use render-pr-body for PR archival."""
    content = _read_skill("flow-complete")
    assert "render-pr-body" in content, (
        "flow-complete/SKILL.md must use render-pr-body for PR body rendering"
    )


def test_plan_skill_uses_render_pr_body():
    """Plan Step 4 must use render-pr-body for PR body rendering."""
    content = _read_skill("flow-plan")
    assert "render-pr-body" in content, (
        "flow-plan/SKILL.md must use render-pr-body for PR body rendering"
    )


def test_complete_done_banner_includes_pr_url():
    """Complete Done banner must include the PR URL for quick access."""
    content = _read_skill("flow-complete")
    # Verify PR: <pr_url> appears after the Done banner heading
    in_done = False
    found_pr_in_banner = False
    for line in content.splitlines():
        if "Done" in line and "Print banner" in line:
            in_done = True
        if in_done and "<pr_url>" in line and "PR:" in line:
            found_pr_in_banner = True
            break
    assert found_pr_in_banner, (
        "flow-complete/SKILL.md Done banner must include 'PR: <pr_url>' line"
    )


def test_complete_done_banner_includes_phase_timings():
    """Complete Done banner must include per-phase timing summary."""
    content = _read_skill("flow-complete")
    # Check that all phase names appear as timing entries in the banner
    for name in PHASE_NAMES.values():
        label = f"{name}:"
        assert label in content, (
            f"flow-complete/SKILL.md Done banner must include '{label}' "
            f"for per-phase timing summary"
        )


def test_complete_step6_archives_all_pr_sections():
    """Complete Step 6 must reference all required PR body section headings."""
    content = _read_skill("flow-complete")
    required_headings = [
        "Phase Timings",
        "State File",
        "Session Log",
        "Issues Filed",
    ]
    for heading in required_headings:
        assert heading in content, (
            f"flow-complete/SKILL.md must reference '{heading}' "
            f"section heading in Step 6 archive"
        )


def test_complete_merged_path_includes_archive():
    """Complete Step 2 MERGED path must route through Step 6 (archive)."""
    content = _read_skill("flow-complete")
    assert "Step 6" in content, (
        "flow-complete/SKILL.md must reference Step 6"
    )
    # The MERGED path instruction must mention Step 6
    # to ensure archive runs before cleanup
    merged_idx = content.find("MERGED")
    assert merged_idx != -1, (
        "flow-complete/SKILL.md must contain MERGED path handling"
    )
    # Use the next status check as boundary instead of a magic number
    open_idx = content.find("**If `OPEN`**", merged_idx)
    assert open_idx > merged_idx, (
        "flow-complete/SKILL.md must have OPEN path after MERGED path"
    )
    merged_section = content[merged_idx:open_idx]
    assert "Step 6" in merged_section, (
        "flow-complete/SKILL.md MERGED path must route through Step 6 "
        "(archive artifacts) before proceeding to cleanup"
    )


# --- Complete phase self-invocation contracts ---


def test_complete_has_self_invocation_check():
    """Complete must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-complete")
    assert "## Self-Invocation Check" in content, (
        "flow-complete must have a '## Self-Invocation Check' section"
    )
    si_match = re.search(
        r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL
    )
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), (
        "Self-Invocation Check must reference --continue-step flag"
    )


def test_complete_done_uses_format_complete_summary():
    """Complete Done section must call format-complete-summary script."""
    content = _read_skill("flow-complete")
    in_done = False
    found_script_call = False
    for line in content.splitlines():
        if "Done" in line and "Print banner" in line:
            in_done = True
        if in_done and "format-complete-summary" in line:
            found_script_call = True
            break
    assert found_script_call, (
        "flow-complete/SKILL.md Done section must call "
        "format-complete-summary to generate the summary banner"
    )


def test_complete_has_resume_check():
    """Complete must have a Resume Check section that reads complete_step."""
    content = _read_skill("flow-complete")
    resume_match = re.search(
        r"## Resume Check\n(.*?)(?=\n## Steps)", content, re.DOTALL
    )
    assert resume_match, (
        "flow-complete must have a Resume Check section before Steps"
    )
    resume_text = resume_match.group(1)
    assert "complete_step" in resume_text, (
        "Resume Check must reference complete_step field"
    )


def test_complete_sets_continue_pending_before_commit():
    """Complete must set _continue_pending=commit before every /flow:flow-commit."""
    content = _read_skill("flow-complete")
    # Find all _continue_pending=commit occurrences
    flag_positions = []
    start = 0
    while True:
        pos = content.find("_continue_pending=commit", start)
        if pos == -1:
            break
        flag_positions.append(pos)
        start = pos + 1
    assert len(flag_positions) >= 2, (
        "Complete must set _continue_pending=commit at least twice "
        f"(Step 3 and Step 4), found {len(flag_positions)}"
    )
    # Each flag must precede a /flow:flow-commit
    for i, flag_pos in enumerate(flag_positions):
        commit_pos = content.find("/flow:flow-commit", flag_pos)
        assert commit_pos > flag_pos, (
            f"_continue_pending=commit occurrence {i + 1} must appear "
            f"before a /flow:flow-commit invocation"
        )


def test_complete_commit_points_self_invoke():
    """Complete Steps 3 and 4 must self-invoke via --continue-step."""
    content = _read_skill("flow-complete")
    # Step 3 section (merge conflicts)
    step3_match = re.search(
        r"### Step 3.*?\n(.*?)(?=\n### Step 4)", content, re.DOTALL
    )
    assert step3_match, "Could not find Step 3 section"
    assert "flow:flow-complete --continue-step" in step3_match.group(1), (
        "Step 3 must self-invoke via 'flow:flow-complete --continue-step'"
    )
    # Step 4 section (CI check)
    step4_match = re.search(
        r"### Step 4.*?\n(.*?)(?=\n### Step 5)", content, re.DOTALL
    )
    assert step4_match, "Could not find Step 4 section"
    assert "flow:flow-complete --continue-step" in step4_match.group(1), (
        "Step 4 must self-invoke via 'flow:flow-complete --continue-step'"
    )


def test_complete_commit_points_record_step():
    """Complete Steps 3 and 4 must record complete_step via set-timestamp."""
    content = _read_skill("flow-complete")
    assert "complete_step=4" in content, (
        "Complete must record complete_step=4 after Step 3 commit"
    )


# --- DAG decomposition contracts ---


def test_plan_skill_has_dag_step():
    """Plan SKILL.md must reference the decompose plugin for DAG analysis."""
    content = _read_skill("flow-plan")
    assert "decompose:decompose" in content, (
        "flow-plan/SKILL.md must reference decompose:decompose plugin"
    )


def test_plan_skill_has_dag_resume_check():
    """Plan SKILL.md must check dag_file and plan_file for resume."""
    content = _read_skill("flow-plan")
    assert "dag_file" in content, (
        "flow-plan/SKILL.md must reference dag_file for resume"
    )


def test_plan_skill_has_approval_gate():
    """Plan SKILL.md must have an approval gate using AskUserQuestion."""
    content = _read_skill("flow-plan")
    assert "AskUserQuestion" in content, (
        "flow-plan/SKILL.md must use AskUserQuestion for approval gate"
    )


def test_plan_skill_does_not_use_plan_mode():
    """Plan SKILL.md must not use EnterPlanMode or ExitPlanMode."""
    content = _read_skill("flow-plan")
    assert "EnterPlanMode" not in content, (
        "flow-plan/SKILL.md must not reference EnterPlanMode — "
        "plan mode was replaced by direct decompose plugin invocation"
    )
    assert "ExitPlanMode" not in content, (
        "flow-plan/SKILL.md must not reference ExitPlanMode — "
        "plan mode was replaced by direct decompose plugin invocation"
    )


def test_plan_has_self_invocation_check():
    """Plan must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-plan")
    assert "## Self-Invocation Check" in content, (
        "flow-plan must have a '## Self-Invocation Check' section"
    )
    si_match = re.search(
        r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL
    )
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), (
        "Self-Invocation Check must reference --continue-step flag"
    )


def test_plan_has_continue_pending_for_decompose():
    """Plan must set _continue_pending before decompose invocation."""
    content = _read_skill("flow-plan")
    assert "_continue_pending" in content, (
        "flow-plan/SKILL.md must set _continue_pending before decompose"
    )
    assert "_continue_context" in content, (
        "flow-plan/SKILL.md must set _continue_context before decompose"
    )


def test_plan_detects_decomposed_label():
    """Plan SKILL.md must detect the 'decomposed' label on referenced issues."""
    content = _read_skill("flow-plan")
    assert "decomposed" in content, (
        "flow-plan/SKILL.md must reference 'decomposed' label for skip detection"
    )


def test_done_hardgates_reread_state_file():
    """Phases 1-5 Done HARD-GATEs must re-read continue mode from state file."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        hard_gates = re.findall(
            r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL
        )

        continue_gates = [
            gate for gate in hard_gates
            if "continue=manual" in gate and "continue=auto" in gate
        ]
        assert continue_gates, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no continue-mode "
            f"HARD-GATE (prerequisite for re-read check)"
        )

        has_reread = any(
            "Re-read" in gate or "re-read" in gate
            for gate in continue_gates
        )
        assert has_reread, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Done HARD-GATE must "
            f"re-read continue mode from state file (contain 'Re-read')"
        )


def test_plan_skill_has_dag_mode_resolution():
    """Plan SKILL.md Mode Resolution must reference dag config."""
    content = _read_skill("flow-plan")
    assert "skills.flow-plan.dag" in content, (
        "flow-plan/SKILL.md Mode Resolution must reference "
        "'skills.flow-plan.dag' key"
    )


def test_prime_presets_include_dag_config():
    """All 3 prime presets must include 'dag' key in flow-plan config."""
    content = _read_skill("flow-prime")
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3, (
        f"Expected at least 3 JSON preset blocks, found {len(json_blocks)}"
    )
    preset_names = ["fully autonomous", "fully manual", "recommended"]
    for i, preset_name in enumerate(preset_names):
        parsed = json.loads(json_blocks[i])
        plan_config = parsed.get("flow-plan", {})
        assert "dag" in plan_config, (
            f"'dag' key missing from flow-plan config in "
            f"{preset_name} preset"
        )


def test_prime_installs_decompose_plugin():
    """flow-prime SKILL.md must install the decompose plugin."""
    content = _read_skill("flow-prime")
    assert "decompose-marketplace" in content, (
        "flow-prime/SKILL.md must reference decompose-marketplace"
    )
    assert "decompose@decompose-marketplace" in content, (
        "flow-prime/SKILL.md must contain install command for "
        "decompose@decompose-marketplace"
    )


# --- flow-issues batch detection and work order ---


def test_flow_issues_has_batch_detection_step():
    """flow-issues SKILL.md must have a Batch Detection step."""
    content = _read_skill("flow-issues")
    assert re.search(r"##\s+Step\s+\d+\s*—\s*Batch", content), (
        "flow-issues/SKILL.md must have a step with 'Batch' in the heading"
    )


def test_flow_issues_has_work_order_section():
    """flow-issues SKILL.md must have a Work Order section in its display step."""
    content = _read_skill("flow-issues")
    assert "Work Order" in content, (
        "flow-issues/SKILL.md must contain 'Work Order' section"
    )


# --- flow-issues WIP detection ---


def test_flow_issues_has_wip_detection():
    """flow-issues SKILL.md must reference 'Flow In-Progress' for WIP detection."""
    content = _read_skill("flow-issues")
    assert "Flow In-Progress" in content, (
        "flow-issues/SKILL.md must contain 'Flow In-Progress' for WIP detection"
    )


# --- flow-issues decomposed detection ---


def test_flow_issues_has_decomposed_detection():
    """flow-issues SKILL.md must reference 'decomposed' for decomposed label detection."""
    content = _read_skill("flow-issues")
    assert "decomposed" in content, (
        "flow-issues/SKILL.md must contain 'decomposed' for decomposed label detection"
    )


# --- flow-issues dependency detection ---


def test_flow_issues_has_dependency_detection():
    """flow-issues SKILL.md must have cross-reference dependency detection."""
    content = _read_skill("flow-issues")
    assert "dependency" in content.lower(), (
        "flow-issues/SKILL.md must contain dependency detection logic"
    )
    assert "#N" in content, (
        "flow-issues/SKILL.md must reference #N cross-reference patterns"
    )


# --- flow-issues file count column ---


def test_flow_issues_has_file_count_column():
    """flow-issues SKILL.md must include a Files column in category tables."""
    content = _read_skill("flow-issues")
    assert "`Files`" in content, (
        "flow-issues/SKILL.md must contain a 'Files' column in the table definition"
    )


# --- flow-issues stale detection ---


def test_flow_issues_has_stale_detection():
    """flow-issues SKILL.md must have stale issue detection."""
    content = _read_skill("flow-issues")
    assert "stale" in content.lower(), (
        "flow-issues/SKILL.md must contain stale detection logic"
    )
    assert "60" in content, (
        "flow-issues/SKILL.md must reference the 60-day threshold"
    )


# --- flow-issues start commands ---


def test_flow_issues_has_start_commands():
    """flow-issues SKILL.md must include flow-start commands in work order."""
    content = _read_skill("flow-issues")
    assert "flow-start" in content, (
        "flow-issues/SKILL.md must contain flow-start command in work order"
    )


# --- label-issues integration in Start, Complete, Abort ---


def test_flow_start_labels_issues():
    """flow-start SKILL.md must call bin/flow label-issues with --add."""
    content = _read_skill("flow-start")
    assert "label-issues" in content, (
        "flow-start/SKILL.md must reference label-issues"
    )
    assert "--add" in content, (
        "flow-start/SKILL.md must use --add flag for label-issues"
    )


def test_flow_complete_removes_labels():
    """flow-complete SKILL.md must call bin/flow label-issues with --remove."""
    content = _read_skill("flow-complete")
    assert "label-issues" in content, (
        "flow-complete/SKILL.md must reference label-issues"
    )
    assert "--remove" in content, (
        "flow-complete/SKILL.md must use --remove flag for label-issues"
    )


def test_flow_abort_removes_labels():
    """flow-abort SKILL.md must call bin/flow label-issues with --remove."""
    content = _read_skill("flow-abort")
    assert "label-issues" in content, (
        "flow-abort/SKILL.md must reference label-issues"
    )
    assert "--remove" in content, (
        "flow-abort/SKILL.md must use --remove flag for label-issues"
    )


# --- code_review_plugin config axis ---


def test_code_review_skill_has_plugin_mode_resolution():
    """Code Review SKILL.md must reference code_review_plugin config."""
    content = _read_skill("flow-code-review")
    assert "skills.flow-code-review.code_review_plugin" in content, (
        "flow-code-review/SKILL.md Mode Resolution must reference "
        "'skills.flow-code-review.code_review_plugin' key"
    )


def test_prime_has_independent_code_review_plugin_question():
    """flow-prime must ask about code_review_plugin as an independent step."""
    content = _read_skill("flow-prime")
    assert "Code Review Plugin mode" in content, (
        "flow-prime/SKILL.md must contain an independent AskUserQuestion "
        "about 'Code Review Plugin mode'"
    )
    for option in ["Always", "Auto", "Never"]:
        assert option in content, (
            f"flow-prime/SKILL.md code review plugin question "
            f"must include '{option}' option"
        )
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3, (
        f"Expected at least 3 JSON preset blocks, found {len(json_blocks)}"
    )
    for i, name in enumerate(
        ["fully autonomous", "fully manual", "recommended"]
    ):
        parsed = json.loads(json_blocks[i])
        cr_config = parsed.get("flow-code-review", {})
        assert "code_review_plugin" not in cr_config, (
            f"'code_review_plugin' must NOT be in {name} preset — "
            f"it is now an independent question"
        )
