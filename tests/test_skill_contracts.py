"""Tests for SKILL.md content contracts.

The SKILL.md files are markdown, but they contain highly structured content:
phase gates, state field references, JSON schemas, cross-skill invocations,
and back navigation rules. All parseable with regex.
"""

import json
import re

from conftest import DOCS_DIR, LIB_DIR, PHASE_ORDER, REPO_ROOT, SKILLS_DIR
from flow_utils import PHASE_NAMES, PHASE_NUMBER


def _load_phases():
    return json.loads((REPO_ROOT / "flow-phases.json").read_text())


def _plugin_version():
    """Return the version string from plugin.json (e.g. '0.7.1')."""
    plugin = json.loads((REPO_ROOT / ".claude-plugin" / "plugin.json").read_text())
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
    return [d.name for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir() and d.name not in phase_names]


# --- Phase gate consistency ---


def test_phase_skills_2_through_5_have_hard_gate_checking_previous_phase():
    """Phases 2-5 must have a HARD-GATE that checks phases.<prev>.status."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:-1]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        prev_idx = PHASE_ORDER.index(key) - 1
        prev_key = PHASE_ORDER[prev_idx]

        assert "<HARD-GATE>" in content, f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no <HARD-GATE>"
        pattern = rf"phases\.{prev_key}\.status"
        assert re.search(pattern, content), (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) HARD-GATE doesn't check phases.{prev_key}.status"
        )


def test_utility_skills_have_no_phase_gate():
    """Utility skills should not have phase entry gates."""
    for name in _utility_skills():
        content = _read_skill(name)
        # They should not have the structured phase entry HARD-GATE
        # (checking phases.<key>.status)
        assert not re.search(r"phases\.[\w-]+\.status", content), (
            f"Utility skill '{name}' has a phase status check — utility skills should not gate on phase status"
        )


def test_phase_1_has_no_previous_phase_gate():
    """Phase 1 (Start) should not check a previous phase's status."""
    content = _read_skill("flow-start")
    # Start has HARD-GATE but for feature name, not for previous phase
    assert not re.search(r"phases\.[\w-]+\.status", content), "Phase 1 (start) should not gate on any phase status"


def test_phase_skills_1_through_5_have_done_section_hard_gate():
    """Phases 1-5 must have a HARD-GATE enforcing continue-mode branching."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        # Extract all HARD-GATE blocks
        hard_gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)

        # At least one HARD-GATE must enforce continue-mode branching
        has_continue_gate = any("continue=manual" in gate and "continue=auto" in gate for gate in hard_gates)
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
                    raise AssertionError(f"Invalid JSON in {rel} block {i}: {e}")


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

    spec = importlib.util.spec_from_file_location("start_setup", LIB_DIR / "start-setup.py")
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
        "name",
        "status",
        "started_at",
        "completed_at",
        "session_started_at",
        "cumulative_seconds",
        "visit_count",
    ]
    for key in PHASE_ORDER:
        assert key in phases, f"Phase {PHASE_NUMBER[key]} ({key}) missing from initial state template"
        for field in required_fields:
            assert field in phases[key], (
                f"Phase {PHASE_NUMBER[key]} ({key}) missing field '{field}' in initial state template"
            )


def test_phase_names_in_state_match_flow_phases():
    """Phase names in start-setup.py state must match flow-phases.json."""
    import importlib.util
    import tempfile
    from pathlib import Path

    data = _load_phases()

    spec = importlib.util.spec_from_file_location("start_setup", LIB_DIR / "start-setup.py")
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
                assert (SKILLS_DIR / ref).is_dir(), f"{rel} references /flow:{ref} but skills/{ref}/ does not exist"


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
    assert '"flow:ci-fixer"' in content, "skills/flow-start/SKILL.md must reference flow:ci-fixer sub-agent"
    assert '"general-purpose"' not in content, (
        "skills/flow-start/SKILL.md must not reference general-purpose sub-agent — use flow:ci-fixer instead"
    )


def test_complete_uses_ci_fixer_subagent():
    """Complete skill must reference the ci-fixer sub-agent for CI failures."""
    content = _read_skill("flow-complete")
    assert '"flow:ci-fixer"' in content, "skills/flow-complete/SKILL.md must reference flow:ci-fixer sub-agent"
    assert '"general-purpose"' not in content, (
        "skills/flow-complete/SKILL.md must not reference general-purpose sub-agent — use flow:ci-fixer instead"
    )


def test_code_review_step1_no_general_purpose_agents():
    """Code Review Step 1 must not use Agent tool — inline review passes only."""
    for step_num, step_text in _code_review_steps():
        if step_num != 1:
            continue
        assert "Agent tool" not in step_text, (
            "Code Review Step 1 must not reference 'Agent tool' — use inline review passes instead"
        )
        assert '"general-purpose"' not in step_text, "Code Review Step 1 must not reference general-purpose sub-agents"


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
                "Complete skill merge command must not use --delete-branch — cleanup.py handles branch deletion"
            )


def test_complete_does_not_contain_admin_flag():
    """Complete skill must never mention the --admin flag."""
    content = _read_skill("flow-complete")
    assert "--admin" not in content, "skills/flow-complete/SKILL.md must not contain --admin"


def test_complete_navigates_to_project_root():
    """Complete skill must cd to project root before cleanup (Step 12)."""
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
    assert "name: ci-fixer" in content, "agents/ci-fixer.md missing 'name: ci-fixer' in frontmatter"
    # CI re-run must use an explicit bash block with plugin root prefix
    assert "```bash" in content, "agents/ci-fixer.md missing explicit bash block for bin/flow ci"
    assert "${CLAUDE_PLUGIN_ROOT}/bin/flow ci" in content, (
        "agents/ci-fixer.md must use ${CLAUDE_PLUGIN_ROOT}/bin/flow ci "
        "in a bash block — bare bin/flow fails in target projects"
    )


def test_pre_mortem_agent_exists():
    """agents/pre-mortem.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "pre-mortem.md"
    assert agent_file.exists(), "agents/pre-mortem.md does not exist"
    content = agent_file.read_text()
    assert "name: pre-mortem" in content, "agents/pre-mortem.md missing 'name: pre-mortem' in frontmatter"
    # Pre-mortem agent must be read-only — no Edit or Write tools
    assert "Edit" not in content.split("---")[1], (
        "agents/pre-mortem.md must not include Edit tool — pre-mortem is read-only"
    )
    assert "Write" not in content.split("---")[1], (
        "agents/pre-mortem.md must not include Write tool — pre-mortem is read-only"
    )


def test_onboarding_agent_exists():
    """agents/onboarding.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "onboarding.md"
    assert agent_file.exists(), "agents/onboarding.md does not exist"
    content = agent_file.read_text()
    assert "name: onboarding" in content, "agents/onboarding.md missing 'name: onboarding' in frontmatter"
    # Onboarding agent must be read-only — no Edit or Write tools
    assert "Edit" not in content.split("---")[1], (
        "agents/onboarding.md must not include Edit tool — onboarding is read-only"
    )
    assert "Write" not in content.split("---")[1], (
        "agents/onboarding.md must not include Write tool — onboarding is read-only"
    )


def test_learn_analyst_agent_exists():
    """agents/learn-analyst.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "learn-analyst.md"
    assert agent_file.exists(), "agents/learn-analyst.md does not exist"
    content = agent_file.read_text()
    assert "name: learn-analyst" in content, "agents/learn-analyst.md missing 'name: learn-analyst' in frontmatter"
    # Learn-analyst agent must be read-only — no Edit or Write tools
    assert "Edit" not in content.split("---")[1], (
        "agents/learn-analyst.md must not include Edit tool — learn-analyst is read-only"
    )
    assert "Write" not in content.split("---")[1], (
        "agents/learn-analyst.md must not include Write tool — learn-analyst is read-only"
    )


def test_learn_analyst_agent_has_design_note():
    """agents/learn-analyst.md must have a Design Note explaining context-rich tier placement."""
    agent_file = REPO_ROOT / "agents" / "learn-analyst.md"
    content = agent_file.read_text()
    assert "## Design Note" in content, (
        "agents/learn-analyst.md must have a '## Design Note' section explaining "
        "why it belongs in the context-rich tier of the Two-Tier Context Model"
    )


def test_learn_uses_onboarding_subagent():
    """Learn skill must reference the onboarding sub-agent."""
    content = _read_skill("flow-learn")
    assert '"flow:onboarding"' in content, "skills/flow-learn/SKILL.md must reference flow:onboarding sub-agent"


def test_learn_uses_learn_analyst_subagent():
    """Learn skill must reference the learn-analyst sub-agent."""
    content = _read_skill("flow-learn")
    assert '"flow:learn-analyst"' in content, "skills/flow-learn/SKILL.md must reference flow:learn-analyst sub-agent"


def test_reviewer_agent_exists():
    """agents/reviewer.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "reviewer.md"
    assert agent_file.exists(), "agents/reviewer.md does not exist"
    content = agent_file.read_text()
    assert "name: reviewer" in content, "agents/reviewer.md missing 'name: reviewer' in frontmatter"
    # Reviewer agent must be read-only — no Edit or Write tools
    assert "Edit" not in content.split("---")[1], (
        "agents/reviewer.md must not include Edit tool — reviewer is read-only"
    )
    assert "Write" not in content.split("---")[1], (
        "agents/reviewer.md must not include Write tool — reviewer is read-only"
    )


def test_adversarial_agent_exists():
    """agents/adversarial.md must exist with required frontmatter fields."""
    agent_file = REPO_ROOT / "agents" / "adversarial.md"
    assert agent_file.exists(), "agents/adversarial.md does not exist"
    content = agent_file.read_text()
    assert "name: adversarial" in content, "agents/adversarial.md missing 'name: adversarial' in frontmatter"
    # Adversarial agent needs Write (for temp test files) but not Edit
    frontmatter = content.split("---")[1]
    assert "Write" in frontmatter, "agents/adversarial.md must include Write tool for temp test files"
    assert "Edit" not in frontmatter, (
        "agents/adversarial.md must not include Edit tool — adversarial only writes new files"
    )


def test_agents_have_reasoning_discipline():
    """Code-semantic agents must have a Reasoning Discipline section with Premise/Trace/Conclude."""
    agents = ["pre-mortem", "reviewer", "ci-fixer", "adversarial"]
    for agent_name in agents:
        agent_file = REPO_ROOT / "agents" / f"{agent_name}.md"
        content = agent_file.read_text()
        assert "## Reasoning Discipline" in content, (
            f"agents/{agent_name}.md must have a '## Reasoning Discipline' section"
        )
        for keyword in ("Premise", "Trace", "Conclude"):
            assert keyword in content, f"agents/{agent_name}.md Reasoning Discipline section must reference '{keyword}'"


def test_semi_formal_reasoning_rule_exists():
    """Rule file for semi-formal reasoning criteria must exist with required sections."""
    rule_file = REPO_ROOT / ".claude" / "rules" / "semi-formal-reasoning.md"
    assert rule_file.exists(), ".claude/rules/semi-formal-reasoning.md must exist"
    content = rule_file.read_text()
    assert "# Semi-Formal Reasoning" in content, (
        ".claude/rules/semi-formal-reasoning.md must have the top-level heading"
    )
    for section in ("When to Include", "When Not to Include"):
        assert section in content, f".claude/rules/semi-formal-reasoning.md must have a '{section}' section"


def test_cognitive_isolation_lists_all_context_rich_agents():
    """Guard: cognitive-isolation.md must list all context-rich agents in the Two-Tier Context Model."""
    rule_file = REPO_ROOT / ".claude" / "rules" / "cognitive-isolation.md"
    content = rule_file.read_text()
    for agent_name in ("reviewer", "learn-analyst"):
        assert agent_name in content, (
            f".claude/rules/cognitive-isolation.md must list '{agent_name}' in the Context-rich tier"
        )


def test_investigation_agents_no_inline_context():
    """Guard: pre-mortem and onboarding agents must NOT receive inline context.

    These agents intentionally receive only the diff and must investigate the
    codebase themselves. Pre-supplied context (plan, CLAUDE.md, rules) masks
    failure modes by priming the agent with the same assumptions the author had.
    The reviewer and learn-analyst agents receive inline context because they
    check against known standards — a fundamentally different task. See
    agents/pre-mortem.md Design Note for the full rationale.
    """
    for agent_name in ("pre-mortem", "onboarding", "adversarial"):
        agent_file = REPO_ROOT / "agents" / f"{agent_name}.md"
        content = agent_file.read_text()
        # Split on frontmatter delimiter to check body only
        body = content.split("---", 2)[2] if content.startswith("---") else content
        assert "provided inline" not in body.lower(), (
            f"agents/{agent_name}.md must NOT contain 'provided inline' — "
            f"this agent intentionally receives only the diff to force independent investigation"
        )


def test_reviewer_inline_context_format_convention():
    """Code Review Step 4 and reviewer agent must agree on labeled section format.

    The producer (SKILL.md Step 4) must specify the exact section labels to use
    when passing inline context to the reviewer agent. The consumer (reviewer.md)
    must document the matching expected sections. This prevents unpredictable
    prompt assembly across sessions. See issue #651.
    """
    # Producer side: Step 4 must contain the section labels
    skill_content = _read_skill("flow-code-review")
    step4_start = skill_content.index("## Step 4")
    # Step 4 ends at Back Navigation or Done (Steps 5-6 merged into Step 4 in PR #686)
    step4_end = len(skill_content)
    for boundary in ("## Back Navigation", "## Done"):
        idx = skill_content.find(boundary, step4_start)
        if idx != -1:
            step4_end = min(step4_end, idx)
    step4_text = skill_content[step4_start:step4_end]
    for label in ("DIFF:", "PLAN:", "CLAUDE.MD:", "RULES:"):
        assert label in step4_text, (
            f"flow-code-review Step 4 must contain '{label}' section label — "
            f"format convention required for consistent reviewer agent prompts"
        )

    # Consumer side: reviewer agent must document expected sections
    reviewer_content = (REPO_ROOT / "agents" / "reviewer.md").read_text()
    body = reviewer_content.split("---", 2)[2] if reviewer_content.startswith("---") else reviewer_content
    for label in ("DIFF", "PLAN", "CLAUDE.MD", "RULES"):
        assert label in body, (
            f"agents/reviewer.md must document '{label}' as an expected section — "
            f"consumer must match producer format convention"
        )


def test_code_review_has_inline_correctness_review():
    """Code Review skill must perform inline correctness review in Step 2."""
    content = _read_skill("flow-code-review")
    # Step 2 must contain inline correctness review passes
    step2_pos = content.index("## Step 2")
    step3_pos = content.index("## Step 3")
    step2_content = content[step2_pos:step3_pos]
    assert "Plan Alignment" in step2_content, "Step 2 must include Plan Alignment pass"
    assert "Logic Correctness" in step2_content, "Step 2 must include Logic Correctness pass"
    assert "Test Coverage" in step2_content, "Step 2 must include Test Coverage pass"
    assert "API Contracts" in step2_content, "Step 2 must include API Contracts pass"
    assert "Rule Compliance" in step2_content, "Step 2 must include Rule Compliance pass"
    assert "git diff origin/main...HEAD" in step2_content, "Step 2 must get the branch diff inline (three-dot)"


def test_code_review_step2_has_step_numbering_verification():
    """Code Review Step 2 must verify step numbering consistency when diff contains step headings."""
    content = _read_skill("flow-code-review")
    step2_pos = content.index("## Step 2")
    step3_pos = content.index("## Step 3")
    step2_content = content[step2_pos:step3_pos]
    assert "step heading" in step2_content.lower(), "Step 2 must mention step headings as a trigger condition"
    assert "sequential" in step2_content.lower(), "Step 2 must verify sequential step numbering"
    assert "cross-reference" in step2_content.lower(), "Step 2 must verify cross-reference consistency"
    assert "read the full" in step2_content.lower(), "Step 2 must instruct reading the full file (not just the diff)"


def test_code_review_has_inline_security_review():
    """Code Review skill must perform inline security review in Step 3."""
    content = _read_skill("flow-code-review")
    # Step 3 must contain inline security review passes
    step3_pos = content.index("## Step 3")
    step3_content = content[step3_pos:]
    assert "Input Validation" in step3_content, "Step 3 must include Input Validation pass"
    assert "Authentication" in step3_content, "Step 3 must include Authentication pass"
    assert "Data Exposure" in step3_content, "Step 3 must include Data Exposure pass"
    assert "git diff origin/main...HEAD" in step3_content, "Step 3 must get the branch diff inline (three-dot)"


def test_phase_skills_have_tool_restriction_in_hard_rules():
    """Every phase skill must have tool restriction language in its
    Hard Rules section.

    Rules in .claude/rules/ are passive context that Claude ignores under
    task pressure. Putting tool restrictions in the skill's Hard Rules
    makes them co-located with the active instructions Claude follows."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        hard_rules_match = re.search(r"## (?:Hard )?Rules\n(.*?)(?:\n## |\Z)", content, re.DOTALL)
        assert hard_rules_match, f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no Hard Rules section"
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
            f"Phase {num} ({skill_name}) missing announce banner 'FLOW v{version} — Phase {num}: {name} — STARTING'"
        )


def test_phase_skills_have_update_state_section():
    """Phases 1-5 should have state update instructions.
    Phase 6 (cleanup) deletes the state file instead of updating it."""
    phase_skills = _phase_skills()

    for key, skill_name in phase_skills.items():
        if key == "flow-complete":
            continue  # Complete deletes state, doesn't update it
        content = _read_skill(skill_name)

        has_update = "Update State" in content or "Update state" in content or "update state" in content
        assert has_update, f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no 'Update State' section"


# --- Phase transition commands ---


def test_phase_skills_use_phase_transition_for_entry():
    """Phases 2-6 must use bin/flow phase-transition for state entry.
    Phase 1 uses start-setup.py which creates the state file directly."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[1:]:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "phase-transition" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing 'phase-transition' command for entry"
        )
        assert "--action enter" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing '--action enter' for phase entry"
        )


def test_phase_skills_use_phase_transition_for_completion():
    """Phases 1-6 must use bin/flow phase-transition for state completion."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER:
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)
        assert "--action complete" in content, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing '--action complete' for phase completion"
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
            f"Phase {num} ({skill_name}) does not contain 'Phase {num}: {name} is complete' in its transition"
        )


def test_phase_6_has_soft_gate_not_hard_gate():
    """Phase 6 (complete) entry gate should be SOFT-GATE, not HARD-GATE.
    Complete warns but never blocks at entry — it's the final escape hatch.
    HARD-GATE is allowed for decision points (e.g., merge approval)."""
    content = _read_skill("flow-complete")
    assert "<SOFT-GATE>" in content, "Phase 6 (complete) should have <SOFT-GATE> — complete warns but never blocks"
    # Entry section is everything before ## Announce
    entry_section = content.split("## Announce")[0]
    assert "<HARD-GATE>" not in entry_section, (
        "Phase 6 (complete) entry gate should NOT use <HARD-GATE> — complete must never block at entry"
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
    gate_match = re.search(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)
    assert gate_match, "Could not extract HARD-GATE content from flow-start/SKILL.md"
    gate_text = gate_match.group(1)
    assert "feature" in gate_text.lower(), "flow-start/SKILL.md HARD-GATE should check for feature name"


def test_flow_start_surfaces_auto_upgrade():
    """flow-start Step 1 must handle auto_upgraded from prime-check output."""
    content = _read_skill("flow-start")
    assert "auto_upgraded" in content, (
        "flow-start/SKILL.md must mention auto_upgraded to surface auto-upgrade notices from prime-check"
    )


def test_phase_skills_have_logging_section():
    """All phase skills must have a ## Logging section."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        assert "## Logging" in content, f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no '## Logging' section"


def test_phase_6_has_delete_state_instructions():
    """Phase 6 (complete) should have instructions to delete the state file,
    not update it."""
    content = _read_skill("flow-complete")
    has_delete = "delete" in content.lower() or "remove" in content.lower() or "rm " in content
    assert has_delete, "Phase 6 (complete) should have delete/remove instructions for state file"


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
        back_refs = re.findall(r"Go back to (\w+)", content, re.IGNORECASE)

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

    spec = importlib.util.spec_from_file_location("format_status", LIB_DIR / "format-status.py")
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

    spec = importlib.util.spec_from_file_location("format_status", LIB_DIR / "format-status.py")
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
    assert match, "format-status.py panel missing timing on completed phase lines — [x] lines should include (Xh Ym)"


# --- Start phase setup script ---


def test_start_logging_uses_safe_pattern():
    """Start SKILL.md logging section must use a safe logging pattern.

    Either Read+Write (tool layer timestamps) or bin/flow log (Python
    subprocess) is acceptable. The >> (Bash append) pattern requires
    $(date ...) which triggers Claude Code's security prompt."""
    content = _read_skill("flow-start")
    logging_match = re.search(r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL)
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
        logging_match = re.search(r"## Logging\n(.*?)(?=\n## |\n---|\Z)", content, re.DOTALL)
        assert logging_match, f"{skill_name}/SKILL.md has no ## Logging section"
        logging_section = logging_match.group(1)

        assert "bin/flow log" in logging_section, f"{skill_name}/SKILL.md ## Logging section must use bin/flow log"
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
    step2_match = re.search(r"## Step 2.*?\n(.*?)(?=\n## Step 3|\Z)", content, re.DOTALL)
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
    assert "do not summarize" in step2_lower or "do not condense" in step2_lower or "never rewrite" in step2_lower, (
        "flow-plan/SKILL.md Step 2 must explicitly prohibit summarizing, condensing, or rewriting the decompose output"
    )


def test_learn_step3_requires_output_for_mistakes():
    """Learn SKILL.md Step 3 must require concrete output for every mistake.

    When Learn identifies Claude mistakes in Step 2, Step 3 must not allow
    'existing rules cover it' as an escape hatch. Every mistake must produce
    at least one artifact (CLAUDE.md edit, Rule issue, or Flow issue)."""
    step3_lower = _learn_step_text(3).lower()

    assert "every mistake must produce" in step3_lower or "must produce at least one" in step3_lower, (
        "flow-learn/SKILL.md Step 3 must require every identified mistake to produce at least one concrete artifact"
    )
    assert "failed to prevent" in step3_lower, (
        "flow-learn/SKILL.md Step 3 must state that a rule which failed to prevent a mistake is not sufficient coverage"
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
        "start/SKILL.md must reference start-setup — Steps 2-7 are consolidated into a single Python script"
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
    utility_with_banners = [
        "flow-commit",
        "flow-abort",
        "flow-status",
        "flow-issues",
        "flow-create-issue",
        "flow-doc-sync",
        "flow-orchestrate",
    ]

    for name in utility_with_banners:
        content = _read_skill(name)
        short_name = name.removeprefix("flow-").capitalize()
        starting_pattern = rf"FLOW v{re.escape(version)}\s*—\s*(?:flow:{name}|{short_name})"
        assert re.search(starting_pattern, content, re.IGNORECASE), (
            f"skills/{name}/SKILL.md STARTING banner missing version — expected 'FLOW v{version}'"
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
        has_format = "Xh Ym" in content or "formatted_time" in content
        assert has_format, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) missing time format "
            f"instruction — must specify format (Xh Ym / Xm / <1m)"
        )


# --- Banner style: tiered weight ---


def test_no_skills_use_equals_banners():
    """No SKILL.md should contain the old ============ banner pattern."""
    skill_dirs = [d for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()]
    maintainer_dir = REPO_ROOT / ".claude" / "skills"
    if maintainer_dir.is_dir():
        skill_dirs.extend(d for d in sorted(maintainer_dir.iterdir()) if d.is_dir())

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
        pattern = r"──{10,}.*?STARTING.*?──{10,}"
        assert re.search(pattern, content, re.DOTALL), (
            f"Phase {num} ({skill_name}) STARTING banner must use ── (light horizontal) borders, not ━ or ═ or ="
        )


def test_complete_banners_use_heavy_horizontal():
    """Every phase COMPLETE banner must use ━━ (heavy horizontal) borders."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        num = PHASE_NUMBER[key]
        # COMPLETE banner must be preceded/followed by heavy ━━ lines
        pattern = r"━━{10,}.*?COMPLETE.*?━━{10,}"
        assert re.search(pattern, content, re.DOTALL), (
            f"Phase {num} ({skill_name}) COMPLETE banner must use ━━ (heavy horizontal) borders, not ─ or ═ or ="
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
            f"{skill_name} PAUSED banner must use ══ (double horizontal) borders, not ━ or ─ or ="
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
            f"Phase {num} ({skill_name}) COMPLETE banner title must include ✓ marker before FLOW version"
        )


def test_paused_banners_have_diamond():
    """PAUSED banner title lines must include ◆ marker."""
    phase_skills = _phase_skills()
    for key, skill_name in phase_skills.items():
        content = _read_skill(skill_name)
        if "Paused" not in content:
            continue
        pattern = r"◆\s+FLOW\s*—\s*Paused"
        assert re.search(pattern, content), f"{skill_name} PAUSED banner title must include ◆ marker before FLOW"


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
            f"{doc_file.name} still uses old ============ banner pattern — update to tiered Unicode borders"
        )


# --- Commit --auto flag ---


def test_commit_auto_flag_restriction():
    """Commit SKILL.md must document that --auto is user-invoked only."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()

    restriction = "`--auto` is user-invoked only"
    assert restriction in content, "skills/flow-commit/SKILL.md missing '--auto is user-invoked only' restriction"


def test_commit_tri_modal_detection():
    """Commit SKILL.md must have tri-modal detection (FLOW/Maintainer/Standalone)."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()

    assert "flow-phases.json" in content, "skills/flow-commit/SKILL.md missing 'flow-phases.json' for mode detection"
    assert "Maintainer" in content, "skills/flow-commit/SKILL.md missing 'Maintainer' mode reference"
    assert "Standalone" in content, "skills/flow-commit/SKILL.md missing 'Standalone' mode reference"
    assert ".flow-states" in content, "skills/flow-commit/SKILL.md missing '.flow-states' for FLOW mode detection"


# --- Reset skill (plugin) ---


def test_reset_guard_requires_main_branch():
    """Reset SKILL.md must guard against running outside main branch."""
    content = (SKILLS_DIR / "flow-reset" / "SKILL.md").read_text()
    assert "main" in content, "Reset SKILL.md must reference the main branch"
    assert "git branch --show-current" in content, (
        "Reset SKILL.md must check current branch with git branch --show-current"
    )


def test_reset_has_inventory_step():
    """Reset SKILL.md must inventory artifacts before destroying them."""
    content = (SKILLS_DIR / "flow-reset" / "SKILL.md").read_text()
    assert "git worktree list" in content, "Reset must inventory worktrees"
    assert "gh pr list" in content, "Reset must inventory open PRs"
    assert ".flow-states" in content, "Reset must inventory state files"


def test_reset_has_confirmation():
    """Reset SKILL.md must confirm before destroying artifacts."""
    content = (SKILLS_DIR / "flow-reset" / "SKILL.md").read_text()
    assert "AskUserQuestion" in content, "Reset SKILL.md must use AskUserQuestion to confirm before destroying"


# --- QA skill (maintainer) ---


def test_flow_qa_has_setup_check():
    """QA SKILL.md must check .qa-repos/ for setup status."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert ".qa-repos" in content, "flow-qa/SKILL.md must reference .qa-repos for setup detection"


def test_flow_qa_has_setup_commands():
    """QA SKILL.md must reference prime-setup and gh repo clone for setup."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "bin/flow prime-setup" in content, (
        "flow-qa/SKILL.md must reference 'bin/flow prime-setup' for priming QA repos"
    )
    assert "gh repo clone" in content, "flow-qa/SKILL.md must reference 'gh repo clone' for cloning QA repos"


def test_flow_qa_asks_for_framework():
    """flow-qa must prompt for framework when none is given."""
    content = (REPO_ROOT / ".claude" / "skills" / "flow-qa" / "SKILL.md").read_text()
    assert "AskUserQuestion" in content, "flow-qa/SKILL.md must use AskUserQuestion when no framework is given"


def test_commit_mode_resolution():
    """Commit SKILL.md must default to auto and have Mode Resolution."""
    content = (SKILLS_DIR / "flow-commit" / "SKILL.md").read_text()
    assert "the default is auto" in content, (
        "skills/flow-commit/SKILL.md missing 'the default is auto' — "
        "commit mode must default to auto (no approval prompt)"
    )
    assert "Mode Resolution" in content, "skills/flow-commit/SKILL.md missing Mode Resolution section"


def test_commit_has_commit_format_support():
    """Commit SKILL.md must support both commit_format options."""
    content = _read_skill("flow-commit")
    assert "commit_format" in content, "skills/flow-commit/SKILL.md must reference 'commit_format' config key"
    assert "title-only" in content, "skills/flow-commit/SKILL.md must document 'title-only' format"
    assert "full" in content.lower(), "skills/flow-commit/SKILL.md must document 'full' format"


def test_no_skill_invokes_commit_with_auto():
    """Skills that use /flow:flow-commit --auto must be in the allow list.

    Start uses --auto for CI baseline fixes and dependency commits on main.
    Learn uses --auto because the phase is fully autonomous. Code and
    Code Review conditionally use --auto based on the commit axis setting."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir() or d.name in (
            "flow-commit",
            "flow-start",
            "flow-learn",
            "flow-code",
            "flow-code-review",
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
    assert "AskUserQuestion" in content, "Release SKILL.md must use AskUserQuestion for --manual approval"
    assert "If `--manual` was explicitly passed" in content, (
        "Release SKILL.md must only prompt when --manual is explicitly passed"
    )
    assert "Unless `--manual` was explicitly passed" in content, (
        "Release SKILL.md default must proceed directly to Step 5"
    )


def test_prime_supports_reprime_flag():
    """Prime SKILL.md must support --reprime for fast upgrades."""
    content = _read_skill("flow-prime")
    assert "--reprime" in content, "flow-prime/SKILL.md must support --reprime flag"
    assert ".flow.json" in content and "Skip" in content, "flow-prime --reprime must read .flow.json and skip questions"


# --- Framework fragment contracts ---


def test_no_framework_fragment_files():
    """No skill directory should have rails.md or python.md fragment files.
    Framework instructions are merged into SKILL.md."""
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        assert not (d / "rails.md").exists(), (
            f"skills/{d.name}/rails.md still exists — framework fragments should be merged into SKILL.md"
        )
        assert not (d / "python.md").exists(), (
            f"skills/{d.name}/python.md still exists — framework fragments should be merged into SKILL.md"
        )


def test_learning_has_no_worktree_memory_rescue():
    """Learn skill must not contain worktree memory rescue logic.

    Since Claude Code 2.1.63, auto-memory is shared across git worktrees
    of the same repository. Worktree-specific memory paths no longer exist,
    so the old rescue logic is obsolete. Note: "Source D" was reused for
    the onboarding agent and is no longer an obsolete term."""
    content = _read_skill("flow-learn")
    obsolete_terms = [
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
        f"skills/flow-learn/SKILL.md still references private paths: {found} — all destinations must be repo-local"
    )
    assert "5 destinations" not in content.lower(), (
        "skills/flow-learn/SKILL.md still references '5 destinations' — should be 2 repo-local destinations"
    )


def test_learning_destinations_are_repo_only():
    """Learn skill must define repo-local destinations with correct routing.

    Both destinations are direct (on disk). CLAUDE.md and .claude/rules/
    are both edited using dedicated tools and committed in Step 4."""
    content = _read_skill("flow-learn")
    assert "Destinations and routing" in content, "Learn skill must have a 'Destinations and routing' section"
    assert "Project CLAUDE.md" in content, "Learn skill must include 'Project CLAUDE.md' as a destination"
    routing_match = re.search(
        r"Destinations and routing.*?\n\n(.*?)(?:\n###|\n---)",
        content,
        re.DOTALL,
    )
    assert routing_match, "Could not extract routing table"
    routing_text = routing_match.group(1)
    edit_count = routing_text.count("Edit on disk")
    assert edit_count >= 2, f"Both destinations must use 'Edit on disk' method, found {edit_count}"


def test_learning_detects_dangling_async_operations():
    """Learn must detect dangling async operations via learn-analyst agent or Step 2.

    Issue #177: Learn synthesis missed dangling background agents. The
    learn-analyst agent now detects these from diff patterns, and Step 2
    must include guidance on classifying dangling async findings."""
    # Step 2 must reference dangling async detection
    step2_text = _learn_step_text(2)
    assert "dangling" in step2_text.lower() or "async" in step2_text.lower(), (
        "Learn Step 2 must include guidance on classifying dangling async findings"
    )
    # The learn-analyst agent definition must also cover dangling async
    agent_file = REPO_ROOT / "agents" / "learn-analyst.md"
    agent_content = agent_file.read_text().lower()
    assert "dangling" in agent_content or "background agent" in agent_content, (
        "agents/learn-analyst.md must detect dangling async or background agent patterns"
    )


def test_learning_edits_rules_directly():
    """Learn skill must edit .claude/rules/ directly using dedicated tools.

    Issue #381: rules were previously filed as GitHub issues, deferring
    them indefinitely. Now both destinations (CLAUDE.md and .claude/rules/)
    are edited on disk and committed in Step 4."""
    step3_text = _learn_step_text(3)
    assert "<worktree_path>" in step3_text, "Learn Step 3 must reference <worktree_path> for .claude/rules/ edits"
    assert ".claude/rules/" in step3_text, "Learn Step 3 must mention .claude/rules/ as an edit target"
    assert "bin/flow issue" not in step3_text, "Learn Step 3 must not file issues — rules are edited directly on disk"


def test_learning_files_flow_issues_not_learning():
    """Learn Step 6 must use label 'Flow', not 'learning'."""
    step6_text = _learn_step_text(6)
    assert "--label" in step6_text, "Learn Step 6 must specify a --label for issue filing"
    assert "Flow" in step6_text, "Learn Step 6 must use label 'Flow' for process gap issues"
    assert "learning" not in step6_text.split("--label")[1].split("\n")[0].lower(), (
        "Learn Step 6 must not use label 'learning' — use 'Flow' instead"
    )


def test_learn_step3_excludes_flow_process_gaps():
    """Learn Step 3 must direct FLOW process gaps to Step 5, not file them here.

    Issue #311: learnings about FLOW skill behavior were misrouted as Rule
    issues on the user's project repo. Step 3 must contain explicit routing
    guidance that FLOW process gaps belong in Step 5."""
    step3_text = _learn_step_text(3)
    step3_lower = step3_text.lower()
    assert "process gap" in step3_lower, "Learn Step 3 must mention 'process gap' to guide routing"
    assert "step 5" in step3_lower, "Learn Step 3 must reference Step 5 as the destination for process gaps"


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
    assert "Flaky Test" in ci_gate_text, "Code CI Gate must detect and file 'Flaky Test' issues"
    assert "bin/flow issue" in ci_gate_text, "Code CI Gate must use 'bin/flow issue' to file flaky test issues"


def test_code_review_files_tech_debt_issues():
    """Code Review skill must file Tech Debt issues for out-of-scope findings."""
    content = _read_skill("flow-code-review")
    assert "Tech Debt" in content, "Code Review skill must mention 'Tech Debt' for out-of-scope findings"
    assert "bin/flow issue" in content, "Code Review skill must use 'bin/flow issue' to file issues"


def test_code_review_step1_files_tech_debt_issues():
    """Code Review Step 1 (Simplify) must file Tech Debt issues for out-of-scope findings."""
    content = _read_skill("flow-code-review")
    step1_start = content.index("## Step 1")
    step2_start = content.index("## Step 2")
    step1_content = content[step1_start:step2_start]
    assert "Tech Debt" in step1_content, "Code Review Step 1 must mention 'Tech Debt' for out-of-scope findings"
    assert "bin/flow issue" in step1_content, "Code Review Step 1 must use 'bin/flow issue' to file issues"
    assert "bin/flow add-issue" in step1_content, (
        "Code Review Step 1 must use 'bin/flow add-issue' to record filed issues"
    )


def test_code_review_files_doc_drift_issues():
    """Code Review skill must file Documentation Drift issues for stale docs."""
    content = _read_skill("flow-code-review")
    assert "Documentation Drift" in content, "Code Review skill must mention 'Documentation Drift' for stale docs"


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
            f"{skill_path.parent.name}/SKILL.md calls bin/flow issue but never calls bin/flow add-issue to record it"
        )


def test_generic_skills_have_no_framework_conditionals():
    """Skills that were made generic must not contain framework conditionals.

    Framework knowledge lives in frameworks/<name>/priming.md and the
    project CLAUDE.md — skills reference CLAUDE.md generically."""
    generic_skills = [
        "flow-plan",
        "flow-code",
        "flow-code-review",
    ]
    for name in generic_skills:
        content = _read_skill(name)
        assert "### If Rails" not in content, f"skills/{name}/SKILL.md still has '### If Rails' conditional"
        assert "### If Python" not in content, f"skills/{name}/SKILL.md still has '### If Python' conditional"
        assert "#### If Rails" not in content, f"skills/{name}/SKILL.md still has '#### If Rails' conditional"
        assert "#### If Python" not in content, f"skills/{name}/SKILL.md still has '#### If Python' conditional"


# --- Configurable auto/manual mode ---

CONFIGURABLE_SKILLS = [
    "flow-start",
    "flow-plan",
    "flow-code",
    "flow-code-review",
    "flow-learn",
    "flow-complete",
    "flow-abort",
]


def test_configurable_skills_support_both_flags():
    """All 7 configurable skills must mention --auto and --manual in Usage."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "--auto" in content, f"skills/{name}/SKILL.md missing '--auto' flag in Usage"
        assert "--manual" in content, f"skills/{name}/SKILL.md missing '--manual' flag in Usage"


def test_configurable_skills_have_mode_resolution():
    """All 7 configurable skills must contain a Mode Resolution section."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        assert "## Mode Resolution" in content, f"skills/{name}/SKILL.md missing '## Mode Resolution' section"


TWO_AXIS_SKILLS = ["flow-code", "flow-code-review", "flow-learn"]
CONTINUE_ONLY_SKILLS = ["flow-start", "flow-plan"]
UTILITY_SKILLS = ["flow-abort", "flow-complete"]


def test_mode_resolution_references_config_source():
    """All 7 configurable skills Mode Resolution must reference config source."""
    for name in CONFIGURABLE_SKILLS:
        content = _read_skill(name)
        resolution_match = re.search(r"## Mode Resolution\n(.*?)(?:\n## |\Z)", content, re.DOTALL)
        assert resolution_match, f"skills/{name}/SKILL.md has no Mode Resolution section"
        resolution_text = resolution_match.group(1)
        assert ".flow-states/" in resolution_text, (
            f"skills/{name}/SKILL.md Mode Resolution does not reference state file for config lookup"
        )
        assert f"skills.{name}" in resolution_text, (
            f"skills/{name}/SKILL.md Mode Resolution does not reference 'skills.{name}' key"
        )
        if name in TWO_AXIS_SKILLS:
            assert f"skills.{name}.commit" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference 'skills.{name}.commit' key"
            )
            assert f"skills.{name}.continue" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference 'skills.{name}.continue' key"
            )
        elif name in CONTINUE_ONLY_SKILLS:
            assert f"skills.{name}.continue" in resolution_text, (
                f"skills/{name}/SKILL.md Mode Resolution does not reference 'skills.{name}.continue' key"
            )


def test_prime_presets_cover_all_configurable_skills():
    """Every skill in CONFIGURABLE_SKILLS must appear in all 3 prime presets."""
    content = _read_skill("flow-prime")
    # Extract the 3 preset JSON blocks (autonomous, manual, recommended)
    # They are the first 3 ```json blocks in the file
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3, f"Expected at least 3 JSON blocks in flow-prime SKILL.md, found {len(json_blocks)}"
    preset_names = ["fully autonomous", "fully manual", "recommended"]
    for i, preset_name in enumerate(preset_names):
        parsed = json.loads(json_blocks[i])
        for skill in CONFIGURABLE_SKILLS:
            assert skill in parsed, f"'{skill}' missing from {preset_name} preset in flow-prime SKILL.md"


# Expected order: phases in canonical order, then abort (utility) last
EXPECTED_SKILL_ORDER = PHASE_ORDER + ["flow-abort"]

# Display name → skill name mapping for flow-prime SKILL.md
_PRIME_DISPLAY_NAME_MAP = {
    "start": "flow-start",
    "plan": "flow-plan",
    "code": "flow-code",
    "code-review": "flow-code-review",
    "learning": "flow-learn",
    "learn": "flow-learn",
    "abort": "flow-abort",
    "complete": "flow-complete",
}


def test_configurable_skills_match_phase_order():
    """CONFIGURABLE_SKILLS list must follow canonical phase order with abort last."""
    assert CONFIGURABLE_SKILLS == EXPECTED_SKILL_ORDER, (
        f"CONFIGURABLE_SKILLS order {CONFIGURABLE_SKILLS} does not match expected phase order {EXPECTED_SKILL_ORDER}"
    )


def test_prime_presets_keys_match_phase_order():
    """Preset JSON keys must appear in canonical phase order with abort last."""
    content = _read_skill("flow-prime")
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3
    preset_names = ["fully autonomous", "fully manual", "recommended"]
    for i, preset_name in enumerate(preset_names):
        parsed = json.loads(json_blocks[i])
        keys = list(parsed.keys())
        assert keys == EXPECTED_SKILL_ORDER, (
            f"{preset_name} preset key order {keys} does not match expected phase order {EXPECTED_SKILL_ORDER}"
        )


def test_prime_customize_questions_match_phase_order():
    """Customize question blocks must appear in canonical phase order."""
    content = _read_skill("flow-prime")
    # Extract skill names from "For **<skill>**" patterns in the Customize section
    customize_match = re.search(r"\*\*Customize\*\*.*?Store the result", content, re.DOTALL)
    assert customize_match, "Could not find Customize section in flow-prime SKILL.md"
    customize_text = customize_match.group(0)
    # Match "For **skill-name**" and "and **skill-name**" patterns
    skill_mentions = re.findall(r"(?:For|and) \*\*(\w[\w-]*)\*\*", customize_text)
    seen = []
    for name in skill_mentions:
        mapped = _PRIME_DISPLAY_NAME_MAP.get(name)
        if mapped and mapped not in seen:
            seen.append(mapped)
    assert len(seen) == len(EXPECTED_SKILL_ORDER), (
        f"Customize section has {len(seen)} skills, expected {len(EXPECTED_SKILL_ORDER)}"
    )
    assert seen == EXPECTED_SKILL_ORDER, (
        f"Customize question order {seen} does not match expected phase order {EXPECTED_SKILL_ORDER}"
    )


def test_prime_done_table_matches_phase_order():
    """Done section skills table rows must follow canonical phase order."""
    content = _read_skill("flow-prime")
    # Find the table in the Done section — rows like "| start       | —      | manual   |"
    table_match = re.search(r"\| Skill\s+\| Commit \| Continue \|.*?\n\|[-| ]+\|\n((?:\|.*\|\n)+)", content)
    assert table_match, "Could not find skills table in flow-prime Done section"
    rows = table_match.group(1).strip().split("\n")
    table_order = []
    for row in rows:
        cells = [c.strip() for c in row.split("|")]
        skill_name = cells[1]  # first column after leading |
        mapped = _PRIME_DISPLAY_NAME_MAP.get(skill_name)
        if mapped:
            table_order.append(mapped)
    assert table_order == EXPECTED_SKILL_ORDER, (
        f"Done table order {table_order} does not match expected phase order {EXPECTED_SKILL_ORDER}"
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
    skill_dirs = [d for d in sorted(SKILLS_DIR.iterdir()) if d.is_dir()]
    maintainer_dir = REPO_ROOT / ".claude" / "skills"
    if maintainer_dir.is_dir():
        skill_dirs.extend(d for d in sorted(maintainer_dir.iterdir()) if d.is_dir())

    errors = []
    for skill_dir in skill_dirs:
        skill_file = skill_dir / "SKILL.md"
        if not skill_file.exists():
            continue
        content = skill_file.read_text()
        name = skill_dir.name

        # Find all ````-fenced blocks (4+ backticks)
        # Pattern: ````<lang>\n...\n```` (matching closing fence)
        quad_blocks = re.finditer(r"^(`{4,})(\w*)\n(.*?)\n\1\s*$", content, re.MULTILINE | re.DOTALL)
        for match in quad_blocks:
            lang = match.group(2)
            inner = match.group(3)
            line_num = content[: match.start()].count("\n") + 1

            # Outer fence must be ````markdown, not ````text
            if lang != "markdown":
                errors.append(f"{name}/SKILL.md:{line_num} — outer fence is ````{lang}, should be ````markdown")

            # Inner fences come in pairs: opening (```text) + closing (```)
            # Only validate opening fences (even indices: 0, 2, 4, ...)
            inner_fences = re.findall(r"^```(\w*)$", inner, re.MULTILINE)
            for i in range(0, len(inner_fences), 2):
                inner_lang = inner_fences[i]
                if inner_lang not in ("text", "diff"):
                    tag_desc = f"```{inner_lang}" if inner_lang else "bare ```"
                    errors.append(f"{name}/SKILL.md:{line_num} — inner fence is {tag_desc}, should be ```text")

    assert not errors, "Quadruple-fenced blocks with wrong pattern:\n" + "\n".join(f"  - {e}" for e in errors)


# --- flow-start bug fixes ---


def test_phase_1_hard_gate_requires_rerun_with_arguments():
    """Phase 1 first HARD-GATE must tell user to re-run with arguments."""
    content = _read_skill("flow-start")
    gate_match = re.search(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)
    assert gate_match, "Could not extract first HARD-GATE from flow-start"
    gate_text = gate_match.group(1)
    assert "feature name required" in gate_text.lower(), (
        "flow-start first HARD-GATE must tell the user that a feature name is required"
    )
    assert "/flow:flow-start" in gate_text, "flow-start first HARD-GATE must show the usage pattern"


def test_start_step_2_has_ci_fix_subagent():
    """Locked section (Steps 1–10) must launch ci-fixer sub-agent for CI failures."""
    content = _read_skill("flow-start")
    locked_match = re.search(r"### Step 1.*?\n(.*?)(?=\n### Step 11)", content, re.DOTALL)
    assert locked_match, "Could not find Steps 1–10 in flow-start/SKILL.md"
    locked_text = locked_match.group(1)
    assert "ci-fixer" in locked_text, (
        "flow-start locked section must reference the ci-fixer sub-agent for automatic CI fix"
    )
    assert "sub-agent" in locked_text.lower() or "Agent" in locked_text, (
        "flow-start locked section must reference launching a sub-agent"
    )


def test_start_ci_fixes_committed_via_flow_commit():
    """CI fixes on main must be committed via /flow:flow-commit (Steps 1–10)."""
    content = _read_skill("flow-start")
    locked_match = re.search(r"### Step 1.*?\n(.*?)(?=\n### Step 11)", content, re.DOTALL)
    assert locked_match, "Could not find Steps 1–10 in flow-start/SKILL.md"
    locked_text = locked_match.group(1)
    assert "/flow:flow-commit" in locked_text, "flow-start locked section must commit CI fixes via /flow:flow-commit"


def test_code_review_steps_have_continuation_directives():
    """Each Code Review step must have a continuation directive to the next."""
    content = _read_skill("flow-code-review")

    # Step 1 must continue to Step 2
    step1_match = re.search(r"## Step 1.*?\n(.*?)(?=\n## Step 2)", content, re.DOTALL)
    assert step1_match, "Could not find Step 1 in flow-code-review/SKILL.md"
    assert "continue to Step 2" in step1_match.group(1), (
        "flow-code-review Step 1 must contain 'continue to Step 2' directive"
    )

    # Step 2 must continue to Step 3
    step2_match = re.search(r"## Step 2.*?\n(.*?)(?=\n## Step 3)", content, re.DOTALL)
    assert step2_match, "Could not find Step 2 in flow-code-review/SKILL.md"
    assert "continue to Step 3" in step2_match.group(1), (
        "flow-code-review Step 2 must contain 'continue to Step 3' directive"
    )

    # Step 3 must continue to Step 4
    step3_match = re.search(
        r"## Step 3.*?\n(.*?)(?=\n## Step 4)",
        content,
        re.DOTALL,
    )
    assert step3_match, "Could not find Step 3 in flow-code-review/SKILL.md"
    assert "continue to Step 4" in step3_match.group(1), (
        "flow-code-review Step 3 must contain 'continue to Step 4' directive"
    )

    # Step 4 must continue to Done
    step4_match = re.search(
        r"## Step 4.*?\n(.*?)(?=\n## Back Navigation|\n## Done)",
        content,
        re.DOTALL,
    )
    assert step4_match, "Could not find Step 4 in flow-code-review/SKILL.md"
    assert "continue to Done" in step4_match.group(1), (
        "flow-code-review Step 4 must contain 'continue to Done' directive"
    )


def test_code_review_hard_rules_require_step_continuation():
    """Hard Rules must require immediate continuation between all 4 steps and reference all review lenses."""
    content = _read_skill("flow-code-review")
    hard_rules_match = re.search(r"## Hard Rules\n(.*)", content, re.DOTALL)
    assert hard_rules_match, "Could not find Hard Rules in flow-code-review/SKILL.md"
    hard_rules = hard_rules_match.group(1)
    assert re.search(r"never pause", hard_rules, re.IGNORECASE), (
        "flow-code-review Hard Rules must contain 'never pause' language"
    )
    for step_name in ["Simplify", "Review", "Security", "Context-Isolated Review", "Pre-Mortem", "Adversarial"]:
        assert step_name in hard_rules, f"flow-code-review Hard Rules must mention '{step_name}' step"


def test_code_review_step_2_handles_no_findings():
    """Step 2 must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step2_match = re.search(r"## Step 2.*?\n(.*?)(?=\n## Step 3)", content, re.DOTALL)
    assert step2_match, "Could not find Step 2 in flow-code-review/SKILL.md"
    assert "no findings" in step2_match.group(1).lower(), "flow-code-review Step 2 must handle the no-findings path"


def test_code_review_step_3_handles_no_findings():
    """Step 3 must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step3_match = re.search(r"## Step 3.*?\n(.*?)(?=\n## Step 4)", content, re.DOTALL)
    assert step3_match, "Could not find Step 3 in flow-code-review/SKILL.md"
    assert "no findings" in step3_match.group(1).lower(), "flow-code-review Step 3 must handle the no-findings path"


def test_code_review_step_1_has_convention_compliance_pass():
    """Step 1 must include a convention compliance review pass."""
    content = _read_skill("flow-code-review")
    step1_match = re.search(r"## Step 1.*?\n(.*?)(?=\n## Step 2)", content, re.DOTALL)
    assert step1_match, "Could not find Step 1 in flow-code-review/SKILL.md"
    assert "convention compliance" in step1_match.group(1).lower(), (
        "flow-code-review Step 1 must include a convention compliance review pass"
    )


def test_code_review_no_plugin_step():
    """Tombstone: code-review:code-review plugin was removed in PR #587. Must not return."""
    content = _read_skill("flow-code-review")
    assert "code-review:code-review" not in content, (
        "flow-code-review must NOT reference code-review:code-review plugin"
        " (removed in PR #587, resurrected via merge in PR #600)"
    )


def test_code_review_no_plugin_config_axis():
    """Tombstone: code_review_plugin config axis was removed in PR #587. Must not return."""
    content = _read_skill("flow-code-review")
    assert "code_review_plugin" not in content, (
        "flow-code-review must NOT reference code_review_plugin config axis"
        " (removed in PR #587, resurrected via merge in PR #600)"
    )


def test_code_review_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = _read_skill("flow-code-review")
    assert "origin/main..HEAD" not in content, (
        "flow-code-review must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660 to exclude "
        "merged-from-main content"
    )


def test_learn_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = _read_skill("flow-learn")
    assert "origin/main..HEAD" not in content, (
        "flow-learn must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660 to exclude "
        "merged-from-main content"
    )


def test_learn_no_doc_drift_filing():
    """Tombstone: doc drift filing removed in PR #688. Must not return."""
    content = _read_skill("flow-learn")
    assert "Documentation Drift" not in content, (
        "flow-learn must NOT reference 'Documentation Drift' — "
        "drift filing was removed in PR #688; a separate /flow-doc-sync "
        "skill replaces this capability"
    )


def test_reviewer_agent_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = (REPO_ROOT / "agents" / "reviewer.md").read_text()
    assert "origin/main..HEAD" not in content, (
        "agents/reviewer.md must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660"
    )


def test_pre_mortem_agent_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = (REPO_ROOT / "agents" / "pre-mortem.md").read_text()
    assert "origin/main..HEAD" not in content, (
        "agents/pre-mortem.md must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660"
    )


def test_adversarial_agent_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = (REPO_ROOT / "agents" / "adversarial.md").read_text()
    assert "origin/main..HEAD" not in content, (
        "agents/adversarial.md must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660"
    )


def test_onboarding_agent_no_two_dot_diff():
    """Tombstone: two-dot diff replaced with three-dot in PR #660. Must not return."""
    content = (REPO_ROOT / "agents" / "onboarding.md").read_text()
    assert "origin/main..HEAD" not in content, (
        "agents/onboarding.md must NOT use two-dot diff (origin/main..HEAD) — "
        "replaced with three-dot (origin/main...HEAD) in PR #660"
    )


def test_onboarding_agent_filters_doc_accuracy():
    """Onboarding agent must filter out doc accuracy issues (PR #688)."""
    content = (REPO_ROOT / "agents" / "onboarding.md").read_text().lower()
    has_filter = "not report documentation accuracy" in content
    assert has_filter, (
        "agents/onboarding.md must instruct the agent not to report documentation accuracy or staleness issues"
    )


def test_code_review_step_4_handles_no_findings():
    """Step 4 (Agent Reviews) must explicitly handle the no-findings path."""
    content = _read_skill("flow-code-review")
    step4_match = re.search(
        r"## Step 4.*?\n(.*?)(?=\n## Back Navigation|\n## Done)",
        content,
        re.DOTALL,
    )
    assert step4_match, "Could not find Step 4 in flow-code-review/SKILL.md"
    assert "no findings" in step4_match.group(1).lower(), "flow-code-review Step 4 must handle the no-findings path"


def test_code_review_no_step_5():
    """Tombstone: Step 5 (Pre-Mortem) merged into Step 4 in PR #686. Must not return."""
    content = _read_skill("flow-code-review")
    assert "## Step 5" not in content, (
        "flow-code-review must NOT have a separate Step 5 — "
        "pre-mortem was merged into Step 4 (parallel agent reviews) in PR #686"
    )


def test_code_review_no_step_6():
    """Tombstone: Step 6 (Adversarial Testing) merged into Step 4 in PR #686. Must not return."""
    content = _read_skill("flow-code-review")
    assert "## Step 6" not in content, (
        "flow-code-review must NOT have a separate Step 6 — "
        "adversarial testing was merged into Step 4 (parallel agent reviews) in PR #686"
    )


def test_code_review_has_resume_check():
    """Code Review SKILL.md must have a Resume Check section that reads code_review_step."""
    content = _read_skill("flow-code-review")
    resume_match = re.search(r"## Resume Check\n(.*?)(?=\n## Step 1)", content, re.DOTALL)
    assert resume_match, "flow-code-review must have a Resume Check section before Step 1"
    resume_text = resume_match.group(1)
    assert "code_review_step" in resume_text, "Resume Check must reference code_review_step field"


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
            content,
            re.DOTALL,
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
    """Step 4 must reference all three agents (Steps 1-3 use inline review passes)."""
    for step_num, step_text in _code_review_steps():
        if step_num in (1, 2, 3):
            continue
        if step_num == 4:
            assert "reviewer" in step_text.lower(), "Step 4 must reference the reviewer agent"
            assert "pre-mortem" in step_text.lower(), "Step 4 must reference the pre-mortem agent"
            assert "adversarial" in step_text.lower(), "Step 4 must reference the adversarial agent"


def test_code_review_has_self_invocation_check():
    """Code Review must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-code-review")
    assert "## Self-Invocation Check" in content, "flow-code-review must have a '## Self-Invocation Check' section"
    si_match = re.search(r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), "Self-Invocation Check must reference --continue-step flag"


def test_start_step_2_acquires_lock():
    """Locked section (Steps 1–10) must acquire start lock before CI work."""
    content = _read_skill("flow-start")
    locked_match = re.search(r"### Step 1.*?\n(.*?)(?=\n### Step 11)", content, re.DOTALL)
    assert locked_match, "Could not find Steps 1–10 in flow-start/SKILL.md"
    locked_text = locked_match.group(1)
    assert "start-lock" in locked_text, "flow-start locked section must reference start-lock for serialization"


def test_start_step_2_has_two_ci_gates():
    """Locked section (Steps 1–10) must have two bin/flow ci calls."""
    content = _read_skill("flow-start")
    locked_match = re.search(r"### Step 1.*?\n(.*?)(?=\n### Step 11)", content, re.DOTALL)
    assert locked_match, "Could not find Steps 1–10 in flow-start/SKILL.md"
    locked_text = locked_match.group(1)
    ci_count = locked_text.count("bin/flow ci")
    assert ci_count >= 2, (
        f"flow-start locked section must have at least 2 bin/flow ci calls (baseline + post-deps), found {ci_count}"
    )


def test_start_files_flaky_test_issues():
    """Locked section (Steps 1–10) must file Flaky Test issues for intermittent CI failures."""
    content = _read_skill("flow-start")
    locked_match = re.search(r"### Step 1.*?\n(.*?)(?=\n### Step 11)", content, re.DOTALL)
    assert locked_match, "Could not find Steps 1–10 in flow-start/SKILL.md"
    locked_text = locked_match.group(1)
    assert "Flaky Test" in locked_text, "flow-start locked section must detect and file 'Flaky Test' issues"
    assert "bin/flow issue" in locked_text, (
        "flow-start locked section must use 'bin/flow issue' to file flaky test issues"
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
        "flow-start SKILL.md must instruct Claude to derive a branch name from the prompt"
    )
    # A HARD-GATE must prohibit treating the prompt as conversation
    gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)
    conversation_gate = any("conversation" in g.lower() for g in gates)
    assert conversation_gate, (
        "flow-start SKILL.md must have a HARD-GATE that prohibits treating the prompt as a conversation"
    )


def test_flow_start_issue_aware_branch_naming():
    """flow-start must fetch issue titles for branch naming when prompt has #N refs."""
    content = _read_skill("flow-start")
    assert "gh issue view" in content, "flow-start/SKILL.md must reference gh issue view for issue-aware branch naming"
    assert "fall back" in content.lower() and "prompt words" in content.lower(), (
        "flow-start/SKILL.md must instruct fallback to prompt words when issue fetch fails"
    )


def test_prime_commit_step_enforces_flow_commit_exclusively():
    """flow-prime commit step must use /flow:flow-commit and not raw git commands."""
    content = _read_skill("flow-prime")
    step_match = re.search(r"### Step 6.*?\n(.*?)(?=\n### Done)", content, re.DOTALL)
    assert step_match, "Could not find Step 6 (commit) in flow-prime/SKILL.md"
    step_text = step_match.group(1)
    assert "/flow:flow-commit" in step_text, "flow-prime Step 6 must reference /flow:flow-commit"
    for line in step_text.splitlines():
        if "git commit" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-prime Step 6 mentions 'git commit' outside a prohibition: {line.strip()}"
            )
        if "git add" in line:
            assert re.search(r"[Nn]ever", line), (
                f"flow-prime Step 6 mentions 'git add' outside a prohibition: {line.strip()}"
            )


def test_prime_step_6_no_git_exclude_option():
    """Tombstone: removed in PR #696. Must not return."""
    content = _read_skill("flow-prime")
    assert "Git-exclude" not in content, (
        "Git-exclude option was removed from flow-prime — prime now commits unconditionally"
    )


def test_prime_step_6_commits_generated_files():
    """flow-prime Step 6 must unconditionally commit via /flow:flow-commit."""
    content = _read_skill("flow-prime")
    step_match = re.search(r"### Step 6.*?\n(.*?)(?=\n### Done)", content, re.DOTALL)
    assert step_match, "Could not find Step 6 in flow-prime/SKILL.md"
    step_text = step_match.group(1)
    assert "flow-commit" in step_text, "flow-prime Step 6 must invoke /flow:flow-commit"


def test_prime_has_commit_format_prompt():
    """flow-prime must prompt the user to choose commit message format."""
    content = _read_skill("flow-prime")
    assert "commit_format" in content, "flow-prime must reference 'commit_format' config key"
    assert "title-only" in content, "flow-prime must offer 'title-only' format option"
    assert "full" in content.lower(), "flow-prime must offer 'full' format option"


def test_code_skill_sets_continue_pending_before_commit():
    """Code phase must set _continue_pending before /flow:flow-commit."""
    content = _read_skill("flow-code")
    assert "_continue_pending=commit" in content, "Code phase must set _continue_pending=commit before commit"
    flag_pos = content.index("_continue_pending=commit")
    commit_pos = content.index("/flow:flow-commit", flag_pos)
    assert flag_pos < commit_pos, "_continue_pending=commit must appear before /flow:flow-commit"


def test_plan_step_1_fetches_referenced_issues():
    """Plan Step 1 must instruct fetching referenced GitHub issues."""
    content = _read_skill("flow-plan")
    assert "gh issue view" in content


# --- Code phase self-invocation contracts ---


def test_code_has_resume_check():
    """Code SKILL.md must have a Resume Check section that reads code_task."""
    content = _read_skill("flow-code")
    resume_match = re.search(r"## Resume Check\n(.*?)(?=\n## Execute Next Task)", content, re.DOTALL)
    assert resume_match, "flow-code must have a Resume Check section before Execute Next Task"
    resume_text = resume_match.group(1)
    assert "code_task" in resume_text, "Resume Check must reference code_task field"


def test_code_has_self_invocation_check():
    """Code must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-code")
    assert "## Self-Invocation Check" in content, "flow-code must have a '## Self-Invocation Check' section"
    si_match = re.search(r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), "Self-Invocation Check must reference --continue-step flag"


def test_code_commit_self_invokes():
    """Code Commit section must self-invoke flow:flow-code --continue-step."""
    content = _read_skill("flow-code")
    commit_match = re.search(r"### Commit\n(.*?)(?=\n## |\n---\n\n## )", content, re.DOTALL)
    assert commit_match, "Could not find Commit section in flow-code/SKILL.md"
    assert "flow:flow-code --continue-step" in commit_match.group(1), (
        "Commit section must self-invoke via 'flow:flow-code --continue-step'"
    )


def test_code_commit_records_task():
    """Code Commit section must record code_task via set-timestamp."""
    content = _read_skill("flow-code")
    commit_match = re.search(r"### Commit\n(.*?)(?=\n## |\n---\n\n## )", content, re.DOTALL)
    assert commit_match, "Could not find Commit section in flow-code/SKILL.md"
    assert "code_task=" in commit_match.group(1), "Commit section must contain 'code_task=' marker"


# --- Code phase single-task framing contracts ---


def test_code_skill_uses_single_task_framing():
    """Code skill must use single-task framing, not loop-iteration language."""
    content = _read_skill("flow-code")
    assert "## Execute Next Task" in content, "flow-code must have '## Execute Next Task' section (not '## Task Loop')"
    assert "## Task Loop" not in content, "flow-code must not contain '## Task Loop' — use single-task framing"
    assert "Work through each task" not in content, "flow-code must not contain loop-iteration language"
    assert "For each task" not in content, "flow-code must not contain 'For each task' loop language"


def test_code_skill_has_atomic_group_handling():
    """Code skill must have atomic task group handling for circular CI dependencies."""
    content = _read_skill("flow-code")
    assert "### Atomic Task Group" in content, "flow-code must have '### Atomic Task Group' subsection"


# --- Learn phase self-invocation contracts ---


def test_learn_has_resume_check():
    """Learn SKILL.md must have a Resume Check section that reads learn_step."""
    content = _read_skill("flow-learn")
    resume_match = re.search(r"## Resume Check\n(.*?)(?=\n## Step 1)", content, re.DOTALL)
    assert resume_match, "flow-learn must have a Resume Check section before Step 1"
    resume_text = resume_match.group(1)
    assert "learn_step" in resume_text, "Resume Check must reference learn_step field"


def test_learn_has_self_invocation_check():
    """Learn must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-learn")
    assert "## Self-Invocation Check" in content, "flow-learn must have a '## Self-Invocation Check' section"
    si_match = re.search(r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), "Self-Invocation Check must reference --continue-step flag"


def _learn_step_text(step_num):
    """Extract Learn step section text by number."""
    content = _read_skill("flow-learn")
    if step_num < 7:
        next_header = f"## Step {step_num + 1}"
    else:
        next_header = "## Done"
    step_match = re.search(
        rf"## Step {step_num}.*?\n(.*?)(?=\n(?:{next_header}))",
        content,
        re.DOTALL,
    )
    assert step_match, f"Could not find Step {step_num} in flow-learn/SKILL.md"
    return step_match.group(1)


def test_learn_step_4_promotes_permissions():
    """Learn Step 4 must call promote-permissions."""
    step_text = _learn_step_text(4)
    assert "promote-permissions" in step_text, "Step 4 must contain 'promote-permissions'"


def test_learn_step_5_self_invokes():
    """Learn Step 5 (commit) must self-invoke flow:flow-learn --continue-step."""
    step_text = _learn_step_text(5)
    assert "flow:flow-learn --continue-step" in step_text, (
        "Step 5 must self-invoke via 'flow:flow-learn --continue-step'"
    )


def test_learn_sets_continue_pending_before_child_skills():
    """Learn must set _continue_pending before each child skill invocation."""
    content = _read_skill("flow-learn")
    child_skills = [
        ("commit", "/flow:flow-commit"),
    ]
    for flag_value, skill_ref in child_skills:
        flag_pattern = f"_continue_pending={flag_value}"
        assert flag_pattern in content, f"Learn must set _continue_pending={flag_value} before invoking {skill_ref}"
        flag_pos = content.index(flag_pattern)
        skill_pos = content.index(skill_ref, flag_pos)
        assert flag_pos < skill_pos, f"_continue_pending={flag_value} must appear before {skill_ref} invocation"


def test_learn_steps_record_completion():
    """Each Learn step must record learn_step via set-timestamp for TUI display."""
    content = _read_skill("flow-learn")
    for step_val in range(7):
        assert f"learn_step={step_val}" in content, (
            f"flow-learn/SKILL.md must contain 'learn_step={step_val}' for TUI step {step_val + 1} display"
        )


def test_learn_skill_sets_steps_total():
    """Learn Update State must set learn_steps_total=7 for TUI progress display."""
    content = _read_skill("flow-learn")
    assert "learn_steps_total=7" in content, (
        "flow-learn/SKILL.md must contain 'learn_steps_total=7' in the Update State section"
    )


def test_plan_skill_does_not_reference_transcript_path():
    """Plan must not contain transcript_path — session log artifact lives in Complete."""
    content = _read_skill("flow-plan")
    assert "transcript_path" not in content, (
        "flow-plan/SKILL.md must not reference transcript_path. "
        "The session log artifact belongs in flow-complete Step 6."
    )


def test_complete_skill_uses_render_pr_body():
    """Complete Step 7 must use render-pr-body for PR archival."""
    content = _read_skill("flow-complete")
    assert "render-pr-body" in content, "flow-complete/SKILL.md must use render-pr-body for PR body rendering"


def test_plan_skill_uses_render_pr_body():
    """Plan Step 4 must use render-pr-body for PR body rendering."""
    content = _read_skill("flow-plan")
    assert "render-pr-body" in content, "flow-plan/SKILL.md must use render-pr-body for PR body rendering"


def test_plan_skill_renders_plan_inline():
    """Plan Done section must render plan content inline before the COMPLETE banner."""
    content = _read_skill("flow-plan")
    assert "### Render Plan" in content, (
        "flow-plan/SKILL.md must have a '### Render Plan' subsection in the Done section to render the plan inline"
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
    assert found_pr_in_banner, "flow-complete/SKILL.md Done banner must include 'PR: <pr_url>' line"


def test_complete_done_banner_includes_phase_timings():
    """Complete Done banner must include per-phase timing summary."""
    content = _read_skill("flow-complete")
    # Check that all phase names appear as timing entries in the banner
    for name in PHASE_NAMES.values():
        label = f"{name}:"
        assert label in content, (
            f"flow-complete/SKILL.md Done banner must include '{label}' for per-phase timing summary"
        )


def test_complete_done_banner_includes_session_summary():
    """Complete Done section must instruct Claude to write a session summary."""
    content = _read_skill("flow-complete")
    assert re.search(r"### Done.*session summary", content, re.DOTALL | re.IGNORECASE), (
        "flow-complete/SKILL.md Done section must instruct Claude to write a prose session summary after the banner"
    )


def test_complete_step7_archives_all_pr_sections():
    """Complete Step 7 must reference all required PR body section headings."""
    content = _read_skill("flow-complete")
    required_headings = [
        "Phase Timings",
        "State File",
        "Session Log",
        "Issues Filed",
    ]
    for heading in required_headings:
        assert heading in content, (
            f"flow-complete/SKILL.md must reference '{heading}' section heading in Step 7 archive"
        )


def test_complete_merged_path_includes_archive():
    """Complete Step 2 MERGED path must route through Step 7 (archive)."""
    content = _read_skill("flow-complete")
    assert "Step 7" in content, "flow-complete/SKILL.md must reference Step 7"
    # The MERGED path instruction must mention Step 7
    # to ensure archive runs before cleanup
    merged_idx = content.find("MERGED")
    assert merged_idx != -1, "flow-complete/SKILL.md must contain MERGED path handling"
    # Use the next status check as boundary instead of a magic number
    open_idx = content.find("**If `OPEN`**", merged_idx)
    assert open_idx > merged_idx, "flow-complete/SKILL.md must have OPEN path after MERGED path"
    merged_section = content[merged_idx:open_idx]
    assert "Step 7" in merged_section, (
        "flow-complete/SKILL.md MERGED path must route through Step 7 (archive artifacts) before proceeding to cleanup"
    )


# --- Complete phase self-invocation contracts ---


def test_complete_has_self_invocation_check():
    """Complete must have a Self-Invocation Check section for --continue-step."""
    content = _read_skill("flow-complete")
    assert "## Self-Invocation Check" in content, "flow-complete must have a '## Self-Invocation Check' section"
    si_match = re.search(r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), "Self-Invocation Check must reference --continue-step flag"


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
        "flow-complete/SKILL.md Done section must call format-complete-summary to generate the summary banner"
    )


def test_complete_has_resume_check():
    """Complete must have a Resume Check section that reads complete_step."""
    content = _read_skill("flow-complete")
    resume_match = re.search(r"## Resume Check\n(.*?)(?=\n## Steps)", content, re.DOTALL)
    assert resume_match, "flow-complete must have a Resume Check section before Steps"
    resume_text = resume_match.group(1)
    assert "complete_step" in resume_text, "Resume Check must reference complete_step field"


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
    assert len(flag_positions) >= 5, (
        "Complete must set _continue_pending=commit at least five times "
        f"(Steps 3, 4, 5, 6, and 8), found {len(flag_positions)}"
    )
    # Each flag must precede a /flow:flow-commit
    for i, flag_pos in enumerate(flag_positions):
        commit_pos = content.find("/flow:flow-commit", flag_pos)
        assert commit_pos > flag_pos, (
            f"_continue_pending=commit occurrence {i + 1} must appear before a /flow:flow-commit invocation"
        )


def test_complete_commit_points_self_invoke():
    """Complete Steps 3, 4, 5, and 6 must self-invoke via --continue-step."""
    content = _read_skill("flow-complete")
    # Step 3 section (merge conflicts)
    step3_match = re.search(r"### Step 3.*?\n(.*?)(?=\n### Step 4)", content, re.DOTALL)
    assert step3_match, "Could not find Step 3 section"
    assert "flow:flow-complete --continue-step" in step3_match.group(1), (
        "Step 3 must self-invoke via 'flow:flow-complete --continue-step'"
    )
    # Step 4 section (local CI gate)
    step4_match = re.search(r"### Step 4.*?\n(.*?)(?=\n### Step 5)", content, re.DOTALL)
    assert step4_match, "Could not find Step 4 section"
    assert "flow:flow-complete --continue-step" in step4_match.group(1), (
        "Step 4 must self-invoke via 'flow:flow-complete --continue-step'"
    )
    # Step 5 section (GitHub CI status)
    step5_match = re.search(r"### Step 5.*?\n(.*?)(?=\n### Step 6)", content, re.DOTALL)
    assert step5_match, "Could not find Step 5 section"
    assert "flow:flow-complete --continue-step" in step5_match.group(1), (
        "Step 5 must self-invoke via 'flow:flow-complete --continue-step'"
    )
    # Step 6 section (confirm with user / feedback loop)
    step6_match = re.search(r"### Step 6.*?\n(.*?)(?=\n### Step 7)", content, re.DOTALL)
    assert step6_match, "Could not find Step 6 section"
    assert "flow:flow-complete --continue-step" in step6_match.group(1), (
        "Step 6 must self-invoke via 'flow:flow-complete --continue-step'"
    )
    # Step 8 section (freshness check + merge)
    step8_match = re.search(r"### Step 8.*?\n(.*?)(?=\n### Step 9)", content, re.DOTALL)
    assert step8_match, "Could not find Step 8 section"
    assert "flow:flow-complete --continue-step" in step8_match.group(1), (
        "Step 8 must self-invoke via 'flow:flow-complete --continue-step'"
    )


def test_complete_commit_points_record_step():
    """Every Complete step must record complete_step via set-timestamp for TUI display."""
    content = _read_skill("flow-complete")
    for step_val in range(1, 13):
        assert f"complete_step={step_val}" in content, (
            f"flow-complete/SKILL.md must contain 'complete_step={step_val}' for TUI step {step_val} display"
        )


def test_complete_skill_sets_steps_total():
    """Complete Update State must set complete_steps_total=12 for TUI progress display."""
    content = _read_skill("flow-complete")
    assert "complete_steps_total=12" in content, (
        "flow-complete/SKILL.md must contain 'complete_steps_total=12' in the Update State section"
    )


def test_continue_context_includes_mode_flag():
    """Every _continue_context with --continue-step must include --auto or --manual."""
    skills_with_min = {
        "flow-code": 2,
        "flow-code-review": 4,
        "flow-complete": 8,
        "flow-learn": 2,
    }
    for skill_name, min_step_contexts in skills_with_min.items():
        content = _read_skill(skill_name)
        contexts = re.findall(r'"_continue_context=([^"]+)"', content)
        step_contexts = [c for c in contexts if "--continue-step" in c]
        assert len(step_contexts) >= min_step_contexts, (
            f"Expected at least {min_step_contexts} _continue_context values "
            f"with --continue-step in {skill_name}, "
            f"found {len(step_contexts)}"
        )
        for ctx in step_contexts:
            assert "--auto" in ctx or "--manual" in ctx, (
                f"_continue_context with --continue-step in {skill_name} must include --auto or --manual, got: {ctx}"
            )


# --- Flat sequential step numbering ---


def test_skills_no_substep_markers():
    """No SKILL.md may use sub-step labels (bold markers or heading labels)."""
    bold_pattern = re.compile(r"\*\*\d+[a-z]\.")
    heading_pattern = re.compile(r"^###\s+\d+[a-z]", re.MULTILINE)
    for d in sorted(SKILLS_DIR.iterdir()):
        if not d.is_dir():
            continue
        skill_path = d / "SKILL.md"
        if not skill_path.exists():
            continue
        content = skill_path.read_text()
        bold_matches = bold_pattern.findall(content)
        assert not bold_matches, (
            f"{d.name}/SKILL.md contains bold sub-step markers: "
            f"{bold_matches}. Use flat sequential ### Step N headings."
        )
        heading_matches = heading_pattern.findall(content)
        assert not heading_matches, (
            f"{d.name}/SKILL.md contains heading sub-step labels: "
            f"{heading_matches}. Use bold prose markers within the step."
        )


# --- DAG decomposition contracts ---


def test_plan_skill_has_dag_step():
    """Plan SKILL.md must reference the decompose plugin for DAG analysis."""
    content = _read_skill("flow-plan")
    assert "decompose:decompose" in content, "flow-plan/SKILL.md must reference decompose:decompose plugin"


def test_plan_skill_has_dag_resume_check():
    """Plan SKILL.md must check dag_file and plan_file for resume."""
    content = _read_skill("flow-plan")
    assert "dag_file" in content, "flow-plan/SKILL.md must reference dag_file for resume"


def test_plan_skill_has_approval_gate():
    """Plan SKILL.md must have an approval gate using AskUserQuestion."""
    content = _read_skill("flow-plan")
    assert "AskUserQuestion" in content, "flow-plan/SKILL.md must use AskUserQuestion for approval gate"


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
    assert "## Self-Invocation Check" in content, "flow-plan must have a '## Self-Invocation Check' section"
    si_match = re.search(r"## Self-Invocation Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert si_match, "Could not find Self-Invocation Check section content"
    assert "--continue-step" in si_match.group(1), "Self-Invocation Check must reference --continue-step flag"


def test_plan_has_continue_pending_for_decompose():
    """Plan must set _continue_pending before decompose invocation."""
    content = _read_skill("flow-plan")
    assert "_continue_pending" in content, "flow-plan/SKILL.md must set _continue_pending before decompose"
    assert "_continue_context" in content, "flow-plan/SKILL.md must set _continue_context before decompose"


def test_plan_detects_decomposed_label():
    """Plan SKILL.md must detect the 'decomposed' label on referenced issues."""
    content = _read_skill("flow-plan")
    assert "decomposed" in content, "flow-plan/SKILL.md must reference 'decomposed' label for skip detection"


def test_plan_step3_extracts_implementation_plan_for_decomposed():
    """Plan Step 3 must extract Implementation Plan from DAG file for decomposed issues."""
    content = _read_skill("flow-plan")
    step3_match = re.search(r"## Step 3.*?(?=\n## Step 4|\n## (?!#))", content, re.DOTALL)
    assert step3_match, "flow-plan must have a Step 3 section"
    step3_text = step3_match.group(0)
    assert "Implementation Plan" in step3_text, (
        "Step 3 must reference 'Implementation Plan' for extraction from decomposed issues"
    )
    assert "extract" in step3_text.lower(), "Step 3 must describe extracting the plan from the DAG file"
    assert "promote" in step3_text.lower() or "heading" in step3_text.lower(), (
        "Step 3 must describe heading promotion (### to ##) during extraction"
    )


def test_done_hardgates_read_continue_action():
    """Phases 1-5 Done HARD-GATEs must read continue_action from phase-transition output."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        hard_gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)

        continue_gates = [gate for gate in hard_gates if "continue=manual" in gate and "continue=auto" in gate]
        assert continue_gates, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no continue-mode HARD-GATE "
            f"(prerequisite for continue_action check)"
        )

        has_continue_action = any("continue_action" in gate for gate in continue_gates)
        assert has_continue_action, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Done HARD-GATE must "
            f"read continue_action from phase-transition output (contain 'continue_action')"
        )


def test_done_hardgates_no_reread_state_file():
    """Tombstone: removed in PR #711. HARD-GATEs must not read continue mode from state file directly."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        hard_gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)
        continue_gates = [gate for gate in hard_gates if "continue=manual" in gate and "continue=auto" in gate]

        has_reread = any("Re-read" in gate for gate in continue_gates)
        assert not has_reread, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Done HARD-GATE must not contain 'Re-read' — "
            f"continue mode is now read from phase-transition command output via continue_action"
        )


def test_done_hard_gates_auto_path_has_final_action_language():
    """Phases 1-5 Done HARD-GATEs auto path must have strengthened language."""
    phase_skills = _phase_skills()
    for key in PHASE_ORDER[:-1]:  # Exclude flow-complete (terminal)
        skill_name = phase_skills[key]
        content = _read_skill(skill_name)

        hard_gates = re.findall(r"<HARD-GATE>(.*?)</HARD-GATE>", content, re.DOTALL)

        continue_gates = [gate for gate in hard_gates if "continue=auto" in gate and "continue=manual" in gate]
        assert continue_gates, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) has no continue-mode HARD-GATE "
            f"(prerequisite for auto-path check)"
        )

        has_final = any("FINAL action" in gate for gate in continue_gates)
        assert has_final, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Done HARD-GATE auto path must "
            f"contain 'FINAL action' language to prevent model from ignoring auto-continue"
        )

        has_skill_tool = any("using the Skill tool" in gate for gate in continue_gates)
        assert has_skill_tool, (
            f"Phase {PHASE_NUMBER[key]} ({skill_name}) Done HARD-GATE auto path must "
            f"contain 'using the Skill tool' to be explicit about invocation method"
        )


def test_plan_skill_has_dag_mode_resolution():
    """Plan SKILL.md Mode Resolution must reference dag config."""
    content = _read_skill("flow-plan")
    assert "skills.flow-plan.dag" in content, (
        "flow-plan/SKILL.md Mode Resolution must reference 'skills.flow-plan.dag' key"
    )


def test_plan_validates_target_file_paths():
    """Plan SKILL.md must have a Target Path Validation subsection."""
    content = _read_skill("flow-plan")
    assert "### Target Path Validation" in content, (
        "flow-plan/SKILL.md must have a '### Target Path Validation' subsection in Step 3"
    )
    section_match = re.search(
        r"### Target Path Validation\n(.*?)(?=\n### |\n## )",
        content,
        re.DOTALL,
    )
    assert section_match, "Could not extract Target Path Validation section content"
    section = section_match.group(1)
    assert "working tree" in section, "Target Path Validation must reference the repo working tree"
    assert "Risks section" in section, "Target Path Validation must instruct flagging in the Risks section"


def test_plan_verifies_script_behavior_assertions():
    """Plan SKILL.md must have a Script Behavior Verification subsection."""
    content = _read_skill("flow-plan")
    assert "### Script Behavior Verification" in content, (
        "flow-plan/SKILL.md must have a '### Script Behavior Verification' subsection in Step 3"
    )
    section_match = re.search(
        r"### Script Behavior Verification\n(.*?)(?=\n### |\n## )",
        content,
        re.DOTALL,
    )
    assert section_match, "Could not extract Script Behavior Verification section content"
    section = section_match.group(1)
    assert "issue bod" in section, "Script Behavior Verification must reference issue bodies as the source of claims"
    assert "script" in section.lower(), "Script Behavior Verification must reference verifying against script source"


def test_prime_presets_include_dag_config():
    """All 3 prime presets must include 'dag' key in flow-plan config."""
    content = _read_skill("flow-prime")
    json_blocks = re.findall(r"```json\n(\{.*?\})\n```", content, re.DOTALL)
    assert len(json_blocks) >= 3, f"Expected at least 3 JSON preset blocks, found {len(json_blocks)}"
    preset_names = ["fully autonomous", "fully manual", "recommended"]
    for i, preset_name in enumerate(preset_names):
        parsed = json.loads(json_blocks[i])
        plan_config = parsed.get("flow-plan", {})
        assert "dag" in plan_config, f"'dag' key missing from flow-plan config in {preset_name} preset"


def test_prime_installs_decompose_plugin():
    """flow-prime SKILL.md must install the decompose plugin."""
    content = _read_skill("flow-prime")
    assert "decompose-marketplace" in content, "flow-prime/SKILL.md must reference decompose-marketplace"
    assert "decompose@decompose-marketplace" in content, (
        "flow-prime/SKILL.md must contain install command for decompose@decompose-marketplace"
    )


# --- flow-issues work order ---


def test_flow_issues_has_work_order_section():
    """flow-issues SKILL.md must have a Work Order section in its display step."""
    content = _read_skill("flow-issues")
    assert "Work Order" in content, "flow-issues/SKILL.md must contain 'Work Order' section"


# --- flow-issues WIP detection ---


def test_flow_issues_has_wip_detection():
    """flow-issues SKILL.md must reference 'Flow In-Progress' for WIP detection."""
    content = _read_skill("flow-issues")
    assert "Flow In-Progress" in content, "flow-issues/SKILL.md must contain 'Flow In-Progress' for WIP detection"


# --- flow-issues decomposed detection ---


def test_flow_issues_has_decomposed_detection():
    """flow-issues SKILL.md must reference 'decomposed' for decomposed label detection."""
    content = _read_skill("flow-issues")
    assert "decomposed" in content, "flow-issues/SKILL.md must contain 'decomposed' for decomposed label detection"


# --- flow-issues blocked label detection ---


def test_flow_issues_no_dependency_detection():
    """Tombstone: removed in PR #661. Must not return."""
    content = _read_skill("flow-issues")
    assert "dependencies" not in content.lower(), "flow-issues/SKILL.md must not reference dependencies (PR #661)"


def test_flow_issues_has_blocked_label_detection():
    """flow-issues SKILL.md must reference Blocked label for blocked status detection."""
    content = _read_skill("flow-issues")
    assert "Blocked" in content, "flow-issues/SKILL.md must contain Blocked label detection"


# --- flow-issues stale detection ---


def test_flow_issues_has_stale_detection():
    """flow-issues SKILL.md must have stale issue detection."""
    content = _read_skill("flow-issues")
    assert "stale" in content.lower(), "flow-issues/SKILL.md must contain stale detection logic"
    assert "60" in content, "flow-issues/SKILL.md must reference the 60-day threshold"


# --- flow-issues start commands ---


def test_flow_issues_has_start_commands():
    """flow-issues SKILL.md must include flow-start commands in work order."""
    content = _read_skill("flow-issues")
    assert "flow-start" in content, "flow-issues/SKILL.md must contain flow-start command in work order"


def test_flow_issues_start_commands_include_title():
    """flow-issues SKILL.md must instruct Claude to add issue title comments below start commands."""
    content = _read_skill("flow-issues")
    assert "issue title" in content.lower(), "flow-issues/SKILL.md must reference issue title in Start Commands section"


# --- flow-issues impact analysis ---


def test_flow_issues_has_impact_ranking():
    """flow-issues SKILL.md must have impact ranking via LLM judgment."""
    content = _read_skill("flow-issues")
    assert "impact" in content.lower(), "flow-issues/SKILL.md must contain impact ranking logic"


# --- flow-issues blocked status ---


def test_flow_issues_has_status_column():
    """flow-issues SKILL.md must have a Status column in the work order table."""
    content = _read_skill("flow-issues")
    assert "Status" in content, "flow-issues/SKILL.md must contain 'Status' column in work order"


def test_flow_issues_has_ready_and_blocked_values():
    """flow-issues SKILL.md must define Ready and Blocked status values."""
    content = _read_skill("flow-issues")
    assert "`Ready`" in content, "flow-issues/SKILL.md must define Ready status value"
    assert "`Blocked`" in content, "flow-issues/SKILL.md must define Blocked status value"


def test_flow_issues_start_commands_exclude_blocked():
    """flow-issues SKILL.md must exclude blocked issues from Start Commands."""
    content = _read_skill("flow-issues")
    # The Start Commands section should mention only including ready issues
    assert "ready issues" in content.lower(), (
        "flow-issues/SKILL.md must reference 'ready issues' to exclude blocked from Start Commands"
    )


# --- label-issues integration in Start, Complete, Abort ---


def test_flow_start_labels_issues():
    """flow-start SKILL.md must call bin/flow label-issues with --add."""
    content = _read_skill("flow-start")
    assert "label-issues" in content, "flow-start/SKILL.md must reference label-issues"
    assert "--add" in content, "flow-start/SKILL.md must use --add flag for label-issues"


def test_flow_complete_removes_labels():
    """flow-complete SKILL.md must call bin/flow label-issues with --remove."""
    content = _read_skill("flow-complete")
    assert "label-issues" in content, "flow-complete/SKILL.md must reference label-issues"
    assert "--remove" in content, "flow-complete/SKILL.md must use --remove flag for label-issues"


def test_flow_abort_removes_labels():
    """flow-abort SKILL.md must call bin/flow label-issues with --remove."""
    content = _read_skill("flow-abort")
    assert "label-issues" in content, "flow-abort/SKILL.md must reference label-issues"
    assert "--remove" in content, "flow-abort/SKILL.md must use --remove flag for label-issues"


# --- flow-create-issue self-invocation and step gates ---


def _create_issue_steps():
    """Parse flow-create-issue SKILL.md into numbered steps."""
    content = _read_skill("flow-create-issue")
    steps = re.findall(
        r"## Step (\d+)\b.*?\n(.*?)(?=\n## Step \d|\n## Hard Rules|\Z)",
        content,
        re.DOTALL,
    )
    return [(int(num), text) for num, text in steps]


def test_create_issue_has_step_dispatch():
    """flow-create-issue must have a Step Dispatch section with --step flag."""
    content = _read_skill("flow-create-issue")
    assert "## Step Dispatch" in content, "flow-create-issue must have a '## Step Dispatch' section"
    dispatch_match = re.search(r"## Step Dispatch\n(.*?)(?=\n## )", content, re.DOTALL)
    assert dispatch_match, "Could not find Step Dispatch section content"
    assert "--step" in dispatch_match.group(1), "Step Dispatch must reference --step flag"


def test_create_issue_usage_documents_step_flag():
    """flow-create-issue Usage must document --step forms."""
    content = _read_skill("flow-create-issue")
    usage_match = re.search(r"## Usage\n(.*?)(?=\n## )", content, re.DOTALL)
    assert usage_match, "Could not find Usage section"
    usage_text = usage_match.group(1)
    assert "--step 2" in usage_text, "Usage must document --step 2 form"
    assert "--step 3" not in usage_text, "Usage must not document --step 3 (skill has 2 steps)"
    assert "--step 4" not in usage_text, "Usage must not document --step 4 (skill has 2 steps)"


def test_create_issue_usage_documents_auto_flag():
    """flow-create-issue Usage must document --auto flag for autonomous execution."""
    content = _read_skill("flow-create-issue")
    usage_match = re.search(r"## Usage\n(.*?)(?=\n## )", content, re.DOTALL)
    assert usage_match, "Could not find Usage section"
    usage_text = usage_match.group(1)
    assert "--auto" in usage_text, "Usage must document --auto flag"


def test_create_issue_steps_have_banners():
    """Each flow-create-issue step must have a step banner."""
    steps = _create_issue_steps()
    assert len(steps) == 2, f"Expected 2 steps, found {len(steps)}"
    for step_num, step_text in steps:
        assert re.search(rf"Step {step_num} of 2", step_text), (
            f"Step {step_num} must have a banner containing 'Step {step_num} of 2'"
        )


def test_create_issue_steps_1_2_have_ask_user():
    """Steps 1 and 2 must have AskUserQuestion gates."""
    steps = _create_issue_steps()
    for step_num, step_text in steps:
        if step_num in (1, 2):
            assert "AskUserQuestion" in step_text, f"Step {step_num} must contain AskUserQuestion"


def test_create_issue_step_1_self_invokes():
    """Step 1 must self-invoke flow:flow-create-issue with --step flag."""
    steps = _create_issue_steps()
    for step_num, step_text in steps:
        if step_num == 1:
            assert "flow:flow-create-issue" in step_text, f"Step {step_num} must self-invoke flow:flow-create-issue"
            assert "--step" in step_text, f"Step {step_num} must use --step flag for self-invocation"


def test_create_issue_has_resume_check():
    """flow-create-issue must have a Resume Check section that reads create_issue_step."""
    content = _read_skill("flow-create-issue")
    rc_match = re.search(r"## Resume Check\n(.*?)(?=\n## )", content, re.DOTALL)
    assert rc_match, "flow-create-issue must have a Resume Check section"
    assert "create_issue_step" in rc_match.group(1), "Resume Check must reference create_issue_step field"


def test_create_issue_no_input_classification():
    """Tombstone: removed in PR #677. Must not return."""
    content = _read_skill("flow-create-issue")
    assert "## Input Classification" not in content


def test_create_issue_no_exploration_mode():
    """Tombstone: removed in PR #677. Must not return."""
    content = _read_skill("flow-create-issue")
    assert "## Exploration Mode" not in content


def test_create_issue_no_multi_issue_path():
    """Tombstone: removed in PR #677. Must not return."""
    content = _read_skill("flow-create-issue")
    assert "Multi-Issue Path" not in content


def test_create_issue_has_conversation_gate():
    """flow-create-issue must have a gate that rejects cold-start invocations."""
    content = _read_skill("flow-create-issue")
    assert "## Conversation Gate" in content, (
        "flow-create-issue must have a '## Conversation Gate' section "
        "that rejects invocations without prior brainstorming context"
    )
    gate_match = re.search(r"## Conversation Gate\n(.*?)(?=\n## )", content, re.DOTALL)
    assert gate_match, "Could not find Conversation Gate section content"
    gate_text = gate_match.group(1)
    assert "decompose" in gate_text.lower(), "Conversation Gate must guide user to run /decompose:decompose first"


def test_create_issue_step2_has_implementation_plan_section():
    """Step 2 must produce an Implementation Plan section matching plan file format."""
    content = _read_skill("flow-create-issue")
    step2_match = re.search(r"## Step 2.*?(?=\n## Hard Rules|\Z)", content, re.DOTALL)
    assert step2_match, "flow-create-issue must have a Step 2 section"
    step2_text = step2_match.group(0)
    assert "Implementation Plan" in step2_text, "Step 2 must reference Implementation Plan section"
    assert "Context" in step2_text, "Step 2 must reference Context subsection of the plan"
    assert "Approach" in step2_text, "Step 2 must reference Approach subsection of the plan"
    assert "Tasks" in step2_text, "Step 2 must reference Tasks subsection of the plan"


def test_create_issue_has_repo_routing():
    """flow-create-issue must route plugin bugs to benkruger/flow."""
    content = _read_skill("flow-create-issue")
    # Must contain a bash block with --repo benkruger/flow for plugin bugs
    assert re.search(r"```bash\s*\n[^`]*--repo benkruger/flow", content, re.DOTALL), (
        "flow-create-issue must have a bash block with '--repo benkruger/flow' "
        "for filing FLOW plugin bugs against the plugin repo"
    )
    # The repo routing decision must be wrapped in a HARD-GATE in Step 2
    step2_match = re.search(r"## Step 2.*?(?=\n## )", content, re.DOTALL)
    assert step2_match, "flow-create-issue must have a Step 2 section"
    step2_text = step2_match.group(0)
    assert "<HARD-GATE>" in step2_text and "AskUserQuestion" in step2_text, (
        "Step 2 must have a HARD-GATE with AskUserQuestion for repo routing"
    )


def test_create_issue_skips_repo_selection_in_flow_repo():
    """flow-create-issue must skip repo selection when working in the FLOW repo."""
    content = _read_skill("flow-create-issue")
    step2_match = re.search(r"## Step 2.*?(?=\n## )", content, re.DOTALL)
    assert step2_match, "flow-create-issue must have a Step 2 section"
    step2_text = step2_match.group(0)
    # Step 2 must detect the current repo via git remote
    assert "git remote get-url origin" in step2_text, (
        "Step 2 must detect the current repo via 'git remote get-url origin' "
        "to determine if the FLOW-repo shortcut applies"
    )
    # Step 2 must have a conditional path for the FLOW repo case
    assert "benkruger/flow" in step2_text, "Step 2 must reference 'benkruger/flow' for the FLOW-repo conditional"


def test_create_issue_step1_has_prior_decompose_detection():
    """Step 1 must detect prior implementation-focused decompose output and skip redundant re-invocation."""
    steps = _create_issue_steps()
    assert any(n == 1 for n, _ in steps), "Step 1 not found in flow-create-issue"
    for step_num, step_text in steps:
        if step_num == 1:
            assert "Prior Decompose Detection" in step_text, "Step 1 must have a 'Prior Decompose Detection' subsection"


def test_create_issue_usage_documents_force_decompose():
    """Usage section must document --force-decompose flag."""
    content = _read_skill("flow-create-issue")
    usage_match = re.search(r"## Usage\n(.*?)(?=\n## )", content, re.DOTALL)
    assert usage_match, "Could not find Usage section"
    assert "--force-decompose" in usage_match.group(1), "Usage must document --force-decompose flag"


def test_create_issue_step2_redecompose_uses_force_flag():
    """Step 2 Re-decompose path must pass --force-decompose to prevent skip-loop."""
    steps = _create_issue_steps()
    for step_num, step_text in steps:
        if step_num == 2:
            assert "--force-decompose" in step_text, (
                "Step 2 must include --force-decompose in the Re-decompose self-invocation "
                "to prevent the skip logic from looping"
            )


def test_complete_no_force_ci():
    """Tombstone: --force removed from Complete Step 4 CI command in PR #637. Must not return."""
    content = _read_skill("flow-complete")
    assert "ci --force" not in content, (
        "flow-complete must NOT use --force in bin/flow ci commands "
        "(removed in PR #637 — sentinel is now simulate-branch-aware)"
    )


def test_decompose_project_no_depends_on_text():
    """Tombstone: 'Depends on' text removed in PR #697. Must not return."""
    content = _read_skill("flow-decompose-project")
    assert "Depends on:" not in content, (
        "flow-decompose-project must NOT instruct including 'Depends on:' text in issue bodies "
        "(removed in PR #697 — native blocked-by API links are the sole dependency mechanism)"
    )


def test_skills_no_repo_tracked_files_at_project_root():
    """Skills must not direct Claude to check repo-tracked files at the project root.

    In a linked worktree, 'project root' resolves to the main repo (on main),
    not the worktree where the feature code lives. Repo-tracked executables
    (bin/test, bin/ci) must be checked in the current working directory."""
    repo_tracked_executables = ["bin/test", "bin/ci"]
    violations = []
    for skill_dir in sorted(SKILLS_DIR.iterdir()):
        if not skill_dir.is_dir():
            continue
        skill_file = skill_dir / "SKILL.md"
        if not skill_file.exists():
            continue
        content = skill_file.read_text()
        paragraphs = content.split("\n\n")
        for para in paragraphs:
            para_lower = para.lower()
            if "project root" not in para_lower:
                continue
            for exe in repo_tracked_executables:
                if exe in para:
                    violations.append(f"{skill_dir.name}: paragraph mentions both '{exe}' and 'project root'")
    assert not violations, (
        "Skills must not direct Claude to check repo-tracked files 'at the project root' — "
        "use 'current working directory' or omit the path. Violations:\n" + "\n".join(violations)
    )
