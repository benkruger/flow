// Rust port of tests/test_skill_contracts.py — SKILL.md content contracts.
//
// Validates structural invariants in skill markdown files: phase gates,
// state field references, cross-skill invocations, agent contracts,
// banner formatting, tombstone tests, and more.
//
// 258 tests (2 format_panel tests remain in Python as test_format_status.py).

mod common;

use std::collections::HashSet;
use std::fs;

use regex::Regex;
use serde_json::Value;

// --- Constants ---

const CONFIGURABLE_SKILLS: &[&str] = &[
    "flow-start",
    "flow-plan",
    "flow-code",
    "flow-code-review",
    "flow-learn",
    "flow-complete",
    "flow-abort",
];

const PHASE_ENTER_PHASES: &[&str] = &["flow-code", "flow-code-review", "flow-learn"];

fn phase_number() -> std::collections::HashMap<String, usize> {
    common::phase_order()
        .into_iter()
        .enumerate()
        .map(|(i, key)| (key, i + 1))
        .collect()
}

fn phase_skills_map() -> Vec<(String, String)> {
    let phases = common::load_phases();
    let order = common::phase_order();
    order
        .into_iter()
        .map(|key| {
            let skill = phases["phases"][&key]["command"]
                .as_str()
                .unwrap()
                .split(':')
                .nth(1)
                .unwrap()
                .to_string();
            (key, skill)
        })
        .collect()
}

fn read_agent_frontmatter(name: &str) -> serde_yaml::Value {
    let content = common::read_agent(name);
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    assert!(
        parts.len() >= 3,
        "{} missing YAML frontmatter delimiters",
        name
    );
    serde_yaml::from_str(parts[1]).unwrap_or_else(|e| panic!("{} invalid YAML: {}", name, e))
}

fn agent_files() -> Vec<String> {
    let dir = common::agents_dir();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

// --- Phase gate consistency ---

#[test]
fn phase_skills_2_through_5_have_hard_gate_checking_previous_phase() {
    let order = common::phase_order();
    let ps = phase_skills_map();
    for (key, skill) in &ps[1..ps.len() - 1] {
        let content = common::read_skill(skill);
        assert!(
            content.contains("<HARD-GATE>"),
            "Phase {} ({}) has no <HARD-GATE>",
            key,
            skill
        );
        if PHASE_ENTER_PHASES.contains(&key.as_str()) {
            assert!(
                content.contains("phase-enter"),
                "Phase {} ({}) HARD-GATE doesn't use phase-enter",
                key,
                skill
            );
        } else {
            let idx = order.iter().position(|k| k == key).unwrap();
            let prev = &order[idx - 1];
            let pat = format!("phases.{}.status", prev);
            assert!(
                content.contains(&pat),
                "Phase {} ({}) HARD-GATE doesn't check {}",
                key,
                skill,
                pat
            );
        }
    }
}

#[test]
fn utility_skills_have_no_phase_gate() {
    let re = Regex::new(r"phases\.[\w-]+\.status").unwrap();
    for name in common::utility_skills() {
        let content = common::read_skill(&name);
        assert!(
            !re.is_match(&content),
            "Utility skill '{}' has a phase status check",
            name
        );
    }
}

#[test]
fn phase_1_has_no_previous_phase_gate() {
    let content = common::read_skill("flow-start");
    let re = Regex::new(r"phases\.[\w-]+\.status").unwrap();
    assert!(
        !re.is_match(&content),
        "Phase 1 (start) should not gate on any phase status"
    );
}

#[test]
fn phase_skills_1_through_5_have_done_section_hard_gate() {
    let ps = phase_skills_map();
    let nums = phase_number();
    let re = Regex::new(r"(?s)<HARD-GATE>(.*?)</HARD-GATE>").unwrap();
    for (key, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        let gates: Vec<String> = re
            .captures_iter(&content)
            .map(|c| c[1].to_string())
            .collect();
        let has_continue = gates
            .iter()
            .any(|g| g.contains("continue=manual") && g.contains("continue=auto"));
        assert!(
            has_continue,
            "Phase {} ({}) has no HARD-GATE enforcing continue-mode branching",
            nums[key], skill
        );
    }
}

// --- State field schema ---

#[test]
fn embedded_json_blocks_are_valid() {
    let re = Regex::new(r"(?s)```json\s*\n(.*?)```").unwrap();
    let placeholder_re = Regex::new(r"<[^>]+>").unwrap();
    for name in common::all_skill_names() {
        let skill_dir = common::skills_dir().join(&name);
        for entry in fs::read_dir(&skill_dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap();
            for (i, cap) in re.captures_iter(&content).enumerate() {
                let block = &cap[1];
                if placeholder_re.is_match(block) {
                    continue;
                }
                let stripped = block.trim();
                if !stripped.starts_with('{') && !stripped.starts_with('[') {
                    continue;
                }
                if block.contains("[...]") || block.contains("...") {
                    continue;
                }
                assert!(
                    serde_json::from_str::<Value>(block).is_ok(),
                    "Invalid JSON in {}/{} block {}",
                    name,
                    path.file_name().unwrap().to_string_lossy(),
                    i
                );
            }
        }
    }
}

// --- Cross-skill invocations ---

#[test]
fn flow_references_point_to_existing_skills() {
    // Match /flow:<name> where name is a complete skill identifier with at least one hyphen
    let re = Regex::new(r"/flow:(flow-[\w-]+\w)").unwrap();
    let skills = common::all_skill_names();
    let skill_set: HashSet<&str> = skills.iter().map(|s| s.as_str()).collect();
    for name in &skills {
        let content = common::read_skill(name);
        for cap in re.captures_iter(&content) {
            let ref_name = &cap[1];
            // Skip references that are clearly part of pattern descriptions (e.g. "flow:<name>")
            if ref_name.contains('<') {
                continue;
            }
            assert!(
                skill_set.contains(ref_name),
                "skills/{}/SKILL.md references /flow:{} but skills/{}/ does not exist",
                name,
                ref_name,
                ref_name
            );
        }
    }
}

#[test]
fn phase_transitions_follow_sequence() {
    let order = common::phase_order();
    let phases = common::load_phases();
    let nums = phase_number();
    for i in 0..order.len() - 1 {
        let key = &order[i];
        let next_key = &order[i + 1];
        let skill_name = phases["phases"][key]["command"]
            .as_str()
            .unwrap()
            .split(':')
            .nth(1)
            .unwrap();
        let content = common::read_skill(skill_name);
        let next_name = phases["phases"][next_key]["name"].as_str().unwrap();
        let next_num = nums[next_key];
        let pattern = format!("Phase {}", next_num);
        assert!(
            content.contains(&pattern),
            "Phase {} ({}) transition should reference Phase {} ({})",
            nums[key],
            skill_name,
            next_num,
            next_name
        );
    }
}

// --- Sub-agent contracts ---

#[test]
fn start_uses_ci_fixer_subagent() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("ci-fixer"),
        "flow-start must reference ci-fixer sub-agent"
    );
}

#[test]
fn complete_uses_ci_fixer_subagent() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("ci-fixer"),
        "flow-complete must reference ci-fixer sub-agent"
    );
}

#[test]
fn code_review_has_six_tenants() {
    let c = common::read_skill("flow-code-review");
    for tenant in &[
        "Architecture",
        "Simplicity",
        "Maintainability",
        "Correctness",
        "Test coverage",
        "Documentation",
    ] {
        assert!(
            c.contains(tenant),
            "flow-code-review missing tenant '{}'",
            tenant
        );
    }
}

#[test]
fn complete_merge_command_no_delete_branch() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("--delete-branch"),
        "flow-complete merge must not use --delete-branch"
    );
}

#[test]
fn complete_does_not_contain_admin_flag() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("--admin"),
        "flow-complete must never mention --admin flag"
    );
}

#[test]
fn complete_navigates_to_project_root() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("project root") || c.contains("project_root"),
        "flow-complete must navigate to project root before cleanup"
    );
}

fn assert_agent_exists(filename: &str, required_keys: &[&str]) {
    let fm = read_agent_frontmatter(filename);
    let map = fm.as_mapping().unwrap();
    for key in required_keys {
        assert!(
            map.contains_key(serde_yaml::Value::String(key.to_string())),
            "{} missing '{}' in frontmatter",
            filename,
            key
        );
    }
}

#[test]
fn ci_fixer_agent_exists() {
    assert_agent_exists("ci-fixer.md", &["name", "model", "maxTurns"]);
}
#[test]
fn pre_mortem_agent_exists() {
    assert_agent_exists("pre-mortem.md", &["name", "model", "maxTurns"]);
}
#[test]
fn documentation_agent_exists() {
    assert_agent_exists("documentation.md", &["name", "model", "maxTurns"]);
}
#[test]
fn learn_analyst_agent_exists() {
    assert_agent_exists("learn-analyst.md", &["name", "model", "maxTurns"]);
}
#[test]
fn reviewer_agent_exists() {
    assert_agent_exists("reviewer.md", &["name", "model", "maxTurns"]);
}
#[test]
fn adversarial_agent_exists() {
    assert_agent_exists("adversarial.md", &["name", "model", "maxTurns"]);
}

#[test]
fn code_review_no_onboarding_agent() {
    assert!(
        !common::agents_dir().join("onboarding.md").exists(),
        "Tombstone: onboarding agent must not exist"
    );
}

#[test]
fn learn_analyst_agent_has_design_note() {
    let c = common::read_agent("learn-analyst.md");
    assert!(
        c.contains("Design Note"),
        "learn-analyst.md must have Design Note section"
    );
}

#[test]
fn learn_no_onboarding_subagent() {
    let c = common::read_skill("flow-learn");
    assert!(
        !c.contains("onboarding"),
        "flow-learn must not reference onboarding agent"
    );
}

#[test]
fn learn_uses_learn_analyst_subagent() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("learn-analyst"),
        "flow-learn must reference learn-analyst sub-agent"
    );
}

#[test]
fn code_review_agents_have_sufficient_max_turns() {
    for agent in &[
        "reviewer.md",
        "pre-mortem.md",
        "adversarial.md",
        "documentation.md",
    ] {
        let fm = read_agent_frontmatter(agent);
        let turns = fm["maxTurns"].as_u64().unwrap_or(0);
        assert!(turns >= 40, "{} maxTurns ({}) must be >= 40", agent, turns);
    }
}

#[test]
fn learn_agents_have_sufficient_max_turns() {
    let fm = read_agent_frontmatter("learn-analyst.md");
    let turns = fm["maxTurns"].as_u64().unwrap_or(0);
    assert!(
        turns >= 25,
        "learn-analyst.md maxTurns ({}) must be >= 25",
        turns
    );
}

#[test]
fn agents_have_reasoning_discipline() {
    for agent in &["pre-mortem.md", "reviewer.md", "adversarial.md"] {
        let c = common::read_agent(agent);
        assert!(
            c.contains("Reasoning Discipline") || c.contains("Semi-Formal Reasoning"),
            "{} must have Reasoning Discipline section",
            agent
        );
    }
}

#[test]
fn semi_formal_reasoning_rule_exists() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("semi-formal-reasoning.md");
    assert!(
        path.exists(),
        "semi-formal-reasoning.md rule file must exist"
    );
    let c = fs::read_to_string(&path).unwrap();
    assert!(
        c.contains("Premise"),
        "semi-formal-reasoning.md must contain 'Premise'"
    );
    assert!(
        c.contains("Trace"),
        "semi-formal-reasoning.md must contain 'Trace'"
    );
}

#[test]
fn cognitive_isolation_lists_all_context_rich_agents() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("cognitive-isolation.md");
    let c = fs::read_to_string(&path).unwrap();
    assert!(
        c.contains("reviewer"),
        "cognitive-isolation.md must list reviewer as context-rich"
    );
    assert!(
        c.contains("learn-analyst"),
        "cognitive-isolation.md must list learn-analyst as context-rich"
    );
}

#[test]
fn investigation_agents_no_inline_context() {
    for agent in &["pre-mortem.md", "documentation.md", "adversarial.md"] {
        let c = common::read_agent(agent);
        assert!(
            !c.contains("CLAUDE.md content:") && !c.contains("Rules content:"),
            "{} must NOT receive inline context (context-sparse design)",
            agent
        );
    }
}

#[test]
fn reviewer_inline_context_format_convention() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("CLAUDE.md") || c.contains("claude.md"),
        "Code Review Step 2 (Launch) must reference CLAUDE.md for reviewer context"
    );
}

// --- Code review requirements ---

#[test]
fn code_review_no_inline_correctness_review() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("### Correctness Review") && !c.contains("## Correctness Review"),
        "Tombstone: inline correctness review removed"
    );
}

#[test]
fn code_review_no_inline_security_step() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("### Security Review") && !c.contains("## Security Review"),
        "Tombstone: inline security review step removed"
    );
}

#[test]
fn code_review_uses_documentation_subagent() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("documentation"),
        "Code Review must reference documentation sub-agent"
    );
}

#[test]
fn code_review_step_4_handles_no_findings() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("no findings") || c.contains("No findings") || c.contains("no real findings"),
        "Step 4 (Fix) must handle no-findings path"
    );
}

#[test]
fn code_review_no_step_5() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("### Step 5"),
        "Tombstone: Step 5 merged into Step 4"
    );
}

#[test]
fn code_review_no_step_6() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("### Step 6"),
        "Tombstone: Step 6 merged into Step 4"
    );
}

#[test]
fn code_review_steps_have_continuation_directives() {
    let c = common::read_skill("flow-code-review");
    // Steps must have continuation directives (may use ## Step or ### Step format)
    assert!(
        c.contains("Step 1") && c.contains("Step 2") && c.contains("Step 3"),
        "Code Review must have Steps 1-3"
    );
}

#[test]
fn code_review_hard_rules_require_step_continuation() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("## Hard Rules"),
        "Code Review must have Hard Rules section"
    );
}

// --- Tool restriction ---

#[test]
fn phase_skills_have_tool_restriction_in_hard_rules() {
    let ps = phase_skills_map();
    let re_hr = Regex::new(r"(?s)## Hard Rules\n(.*)").unwrap();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if !content.contains("## Hard Rules") {
            continue;
        }
        if let Some(cap) = re_hr.captures(&content) {
            let rules = &cap[1];
            assert!(
                rules.contains("Bash") || rules.contains("bash"),
                "{} Hard Rules must mention Bash tool restrictions",
                skill
            );
        }
    }
}

// --- Banner consistency ---

#[test]
fn phase_skills_have_announce_banner() {
    let ps = phase_skills_map();
    let version = common::plugin_version();
    let nums = phase_number();
    let phases = common::load_phases();
    for (key, skill) in &ps {
        let content = common::read_skill(skill);
        let name = phases["phases"][key]["name"].as_str().unwrap();
        let num = nums[key];
        let pattern = format!("FLOW v{}", version);
        assert!(
            content.contains(&pattern),
            "Phase {} ({}) missing version in banner",
            num,
            skill
        );
        let phase_pattern = format!("Phase {}", num);
        assert!(
            content.contains(&phase_pattern),
            "Phase {} ({}) missing phase number in banner",
            num,
            skill
        );
        assert!(
            content.contains(name),
            "Phase {} ({}) missing phase name '{}' in banner",
            num,
            skill,
            name
        );
    }
}

#[test]
fn phase_skills_have_update_state_section() {
    let ps = phase_skills_map();
    for (_, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        // Phase skills should have state update instructions
        assert!(
            content.contains("phase-enter")
                || content.contains("phase-finalize")
                || content.contains("phase-transition")
                || content.contains("set-timestamp"),
            "{} should have state update instructions",
            skill
        );
    }
}

#[test]
fn phase_skills_use_phase_transition_for_entry() {
    let ps = phase_skills_map();
    for (_, skill) in &ps[1..] {
        let content = common::read_skill(skill);
        assert!(
            content.contains("phase-enter") || content.contains("phase-transition"),
            "{} must use phase entry command",
            skill
        );
    }
}

#[test]
fn phase_skills_use_phase_transition_for_completion() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        assert!(
            content.contains("phase-finalize")
                || content.contains("phase-transition")
                || content.contains("complete-finalize"),
            "{} must use phase completion command",
            skill
        );
    }
}

#[test]
fn phase_skills_no_inline_time_computation() {
    let ps = phase_skills_map();
    let re = Regex::new(r"(?i)date\s+-u|date\s+\+|datetime\.now|time\.time").unwrap();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        assert!(
            !re.is_match(&content),
            "{} must not contain inline time computation patterns",
            skill
        );
    }
}

#[test]
fn phase_transition_names_current_phase() {
    let ps = phase_skills_map();
    let phases = common::load_phases();
    let nums = phase_number();
    for (key, skill) in &ps {
        let content = common::read_skill(skill);
        let name = phases["phases"][key]["name"].as_str().unwrap();
        let num = nums[key];
        let pattern = format!("Phase {}: {}", num, name);
        if content.contains("COMPLETE") {
            assert!(
                content.contains(&pattern) || content.contains(&format!("Phase {}:", num)),
                "{} transition should include 'Phase {}: {}'",
                skill,
                num,
                name
            );
        }
    }
}

#[test]
fn phase_6_has_soft_gate_not_hard_gate() {
    let c = common::read_skill("flow-complete");
    // Phase 6 entry should use SOFT-GATE or a different gate type
    assert!(
        c.contains("<SOFT-GATE>") || c.contains("SOFT-GATE") || c.contains("phase-enter"),
        "Phase 6 entry gate should be SOFT-GATE or phase-enter, not HARD-GATE"
    );
}

#[test]
fn phase_transitions_have_note_capture_option() {
    let ps = phase_skills_map();
    for (_, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        if content.contains("AskUserQuestion") {
            assert!(
                content.contains("correction")
                    || content.contains("learning")
                    || content.contains("note"),
                "{} transition question must offer note-capture option",
                skill
            );
        }
    }
}

#[test]
fn phase_1_hard_gate_checks_feature_name() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("Feature name") || c.contains("feature name") || c.contains("arguments"),
        "Phase 1 HARD-GATE should check for feature name"
    );
}

#[test]
fn flow_start_surfaces_auto_upgrade() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("auto_upgraded"),
        "flow-start Step 1 must handle auto_upgraded"
    );
}

#[test]
fn flow_start_documents_flow_in_progress_label_step() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("Flow In-Progress") || c.contains("flow_in_progress"),
        "flow-start must document Flow In-Progress label"
    );
}

#[test]
fn phase_skills_have_logging_section() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        assert!(
            content.contains("## Logging"),
            "{} must have ## Logging section",
            skill
        );
    }
}

#[test]
fn phase_6_has_delete_state_instructions() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("delete") || c.contains("remove") || c.contains("cleanup"),
        "Phase 6 should have delete/remove instructions for state file"
    );
}

// --- Back navigation ---

#[test]
fn back_navigation_names_match_can_return_to() {
    let phases = common::load_phases();
    let order = common::phase_order();
    for key in &order {
        let can_return_to = phases["phases"][key]["can_return_to"].as_array().unwrap();
        if can_return_to.is_empty() {
            continue;
        }
        let skill = phases["phases"][key]["command"]
            .as_str()
            .unwrap()
            .split(':')
            .nth(1)
            .unwrap();
        let content = common::read_skill(skill);
        for target in can_return_to {
            let target_str = target.as_str().unwrap();
            let target_name = phases["phases"][target_str]["name"].as_str().unwrap();
            assert!(
                content.contains(target_name) || content.contains(target_str),
                "{} back navigation should reference {} ({})",
                skill,
                target_str,
                target_name
            );
        }
    }
}

#[test]
fn can_return_to_targets_are_reachable() {
    let phases = common::load_phases();
    let order = common::phase_order();
    for key in &order {
        let can_return_to = phases["phases"][key]["can_return_to"].as_array().unwrap();
        for target in can_return_to {
            let t = target.as_str().unwrap();
            assert!(
                phases["phases"].get(t).is_some(),
                "can_return_to target '{}' does not exist in phases",
                t
            );
        }
    }
}

// --- Banner formatting ---

#[test]
fn phase_skills_complete_banner_includes_timing() {
    let ps = phase_skills_map();
    let _version = common::plugin_version();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("COMPLETE") {
            assert!(
                content.contains("<formatted_time>") || content.contains("formatted_time"),
                "{} COMPLETE banner must include formatted_time",
                skill
            );
        }
    }
}

#[test]
fn utility_skill_banners_include_version() {
    let version = common::plugin_version();
    for name in common::utility_skills() {
        let content = common::read_skill(&name);
        if content.contains("STARTING") || content.contains("COMPLETE") {
            assert!(
                content.contains(&format!("v{}", version)),
                "Utility skill {} banners must include version",
                name
            );
        }
    }
}

#[test]
fn phase_complete_banners_use_formatted_time() {
    let ps = phase_skills_map();
    let banner_re = Regex::new(r"COMPLETE\s*\(.*?cumulative_seconds").unwrap();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        // Only flag if cumulative_seconds appears inside a COMPLETE banner line
        assert!(
            !banner_re.is_match(&content),
            "{} COMPLETE banner must use <formatted_time>, not <cumulative_seconds>",
            skill
        );
    }
}

#[test]
fn no_skills_use_equals_banners() {
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        assert!(
            !content.contains("============"),
            "{} should not use old ============ banner pattern",
            name
        );
    }
}

#[test]
fn starting_banners_use_light_horizontal() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("STARTING") {
            assert!(
                content.contains("──"),
                "{} STARTING banner must use ── (light horizontal) borders",
                skill
            );
        }
    }
}

#[test]
fn complete_banners_use_heavy_horizontal() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("COMPLETE") {
            assert!(
                content.contains("━━"),
                "{} COMPLETE banner must use ━━ (heavy horizontal) borders",
                skill
            );
        }
    }
}

#[test]
fn paused_banners_use_double_horizontal() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("Paused") || content.contains("PAUSED") {
            assert!(
                content.contains("══"),
                "{} PAUSED banner must use ══ (double horizontal) borders",
                skill
            );
        }
    }
}

#[test]
fn complete_banners_have_check_mark() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("COMPLETE") {
            assert!(
                content.contains("✓"),
                "{} COMPLETE banner must include ✓ marker",
                skill
            );
        }
    }
}

#[test]
fn paused_banners_have_diamond() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("Paused") || content.contains("PAUSED") {
            assert!(
                content.contains("◆"),
                "{} PAUSED banner must include ◆ marker",
                skill
            );
        }
    }
}

// format_status_no_equals_banners removed in PR #953 — lib/format-status.py deleted

#[test]
fn docs_no_equals_banners() {
    let docs = common::collect_md_files(&common::docs_dir());
    for (rel, content) in &docs {
        assert!(
            !content.contains("============"),
            "docs/{} must not use old ============ pattern",
            rel
        );
    }
}

// --- Commit skill tombstones ---

#[test]
fn commit_no_auto_manual_flags() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("--auto") && !c.contains("--manual"),
        "Tombstone: flow-commit has no approval prompt flags"
    );
}

#[test]
fn commit_no_mode_detection() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("dual-mode") && !c.contains("Dual-mode"),
        "Tombstone: dual-mode detection removed"
    );
}

#[test]
fn commit_no_flow_phases_json() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("flow-phases.json"),
        "Tombstone: flow-commit must not detect via flow-phases.json"
    );
}

#[test]
fn commit_no_maintainer_mode() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("Maintainer mode") && !c.contains("maintainer mode"),
        "Tombstone: must not reference Maintainer mode"
    );
}

#[test]
fn commit_no_approval_prompt() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("AskUserQuestion"),
        "Tombstone: must not contain AskUserQuestion"
    );
}

#[test]
fn commit_no_git_reset_head() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("git reset HEAD"),
        "Tombstone: must not unstage via git reset HEAD"
    );
}

#[test]
fn commit_no_docs_sync() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("docs sync") && !c.contains("Docs Sync") && !c.contains("docs_sync"),
        "Tombstone: must not have docs sync check"
    );
}

// --- Reset skill ---

#[test]
fn reset_guard_requires_main_branch() {
    let c = common::read_skill("flow-reset");
    assert!(
        c.contains("main") && c.contains("branch"),
        "Reset must guard against running outside main branch"
    );
}

#[test]
fn reset_has_inventory_step() {
    let c = common::read_skill("flow-reset");
    assert!(
        c.contains("inventory") || c.contains("Inventory"),
        "Reset must inventory artifacts before destroying"
    );
}

#[test]
fn reset_has_confirmation() {
    let c = common::read_skill("flow-reset");
    assert!(
        c.contains("confirm") || c.contains("Confirm"),
        "Reset must confirm before destroying"
    );
}

#[test]
fn reset_clears_start_lock_queue() {
    let c = common::read_skill("flow-reset");
    assert!(
        c.contains("start-queue") || c.contains("lock"),
        "Reset must clean up start-queue lock directory"
    );
}

// --- QA skill ---

#[test]
fn flow_qa_has_setup_check() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        c.contains(".qa-repos") || c.contains("qa-repos"),
        "QA must check .qa-repos/ for setup status"
    );
}

#[test]
fn flow_qa_has_setup_commands() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        c.contains("prime-setup") || c.contains("prime"),
        "QA must reference prime-setup"
    );
}

#[test]
fn flow_qa_asks_for_framework() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(c.contains("framework"), "flow-qa must prompt for framework");
}

#[test]
fn flow_qa_no_create_issue_step() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        !c.contains("flow-create-issue"),
        "flow-qa must not reference flow-create-issue"
    );
}

// --- Commit configuration ---

#[test]
fn commit_no_mode_resolution() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("## Mode Resolution"),
        "Tombstone: dual-mode detection removed from commit"
    );
}

#[test]
fn commit_no_separate_ci_step() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("bin/flow ci") && !c.contains("bin/ci"),
        "Tombstone: CI runs inside finalize-commit, not as separate step"
    );
}

#[test]
fn commit_has_commit_format_support() {
    let c = common::read_skill("flow-commit");
    assert!(
        c.contains("commit_format"),
        "Commit must support commit_format"
    );
    assert!(
        c.contains("title-only") || c.contains("full"),
        "Commit must support format options"
    );
}

#[test]
fn no_skill_invokes_commit_with_auto() {
    for name in common::all_skill_names() {
        if name == "flow-commit" {
            continue;
        }
        let content = common::read_skill(&name);
        assert!(
            !content.contains("flow-commit --auto") && !content.contains("flow:flow-commit --auto"),
            "Tombstone: {} must not pass --auto to flow-commit",
            name
        );
    }
}

// --- Release and prime ---

#[test]
fn release_manual_requires_approval() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-release")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        c.contains("--manual"),
        "Release --manual flag must pause for approval"
    );
}

#[test]
fn prime_supports_reprime_flag() {
    let c = common::read_skill("flow-prime");
    assert!(c.contains("--reprime"), "Prime must support --reprime flag");
}

// --- Framework and learning ---

#[test]
fn no_framework_fragment_files() {
    for name in common::all_skill_names() {
        let dir = common::skills_dir().join(&name);
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname != "SKILL.md" && fname.ends_with(".md") {
                panic!(
                    "No framework fragment files should exist: {}/{}",
                    name, fname
                );
            }
        }
    }
}

#[test]
fn learning_has_no_worktree_memory_rescue() {
    let c = common::read_skill("flow-learn");
    assert!(
        !c.contains("memory rescue") && !c.contains("rescue memory"),
        "Learning must not rescue worktree memory"
    );
}

#[test]
fn learning_repo_destinations_use_worktree_path() {
    let c = common::read_skill("flow-learn");
    if c.contains("CLAUDE.md") || c.contains(".claude/rules/") {
        assert!(
            !c.contains("project_root/CLAUDE.md") && !c.contains("project_root/.claude"),
            "Learning repo destinations must use worktree path, not project root"
        );
    }
}

#[test]
fn learning_has_no_private_destination_paths() {
    let c = common::read_skill("flow-learn");
    assert!(
        !c.contains("~/.claude/rules/") && !c.contains("~/.claude/CLAUDE.md"),
        "Learning must not use private destination paths"
    );
}

#[test]
fn learning_destinations_are_repo_only() {
    let c = common::read_skill("flow-learn");
    // If the skill mentions destination paths, they should be repo-level
    assert!(
        !c.contains("user-level") || c.contains("never"),
        "Learning destinations must be repo-only"
    );
}

#[test]
fn learning_detects_dangling_async_operations() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("dangling") || c.contains("async") || c.contains("background"),
        "Learning must detect dangling async operations"
    );
}

#[test]
fn learning_edits_rules_directly() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("write-rule") || c.contains("Edit") || c.contains("bin/flow write-rule"),
        "Learning must edit rules directly"
    );
}

#[test]
fn learning_files_flow_issues_not_learning() {
    let c = common::read_skill("flow-learn");
    // Step 6 should use label 'Flow', not 'learning'
    assert!(
        c.contains("\"Flow\"")
            || c.contains("'Flow'")
            || c.contains("--label") && c.contains("Flow"),
        "Learn Step 6 must use label 'Flow'"
    );
}

#[test]
fn learn_step3_excludes_flow_process_gaps() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("process gap") || c.contains("Process Gap"),
        "Learn Step 3 must handle process gaps"
    );
}

// --- Issue filing ---

#[test]
fn code_files_flaky_test_issues() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Flaky Test"),
        "Code skill CI Gate must file Flaky Test issues"
    );
}

#[test]
fn code_review_files_tech_debt_issues() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("Tech Debt") || c.contains("tech debt"),
        "Code Review must file Tech Debt issues"
    );
}

#[test]
fn code_review_no_inline_simplify_step() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("simplify:simplify"),
        "Tombstone: simplify plugin removed"
    );
}

#[test]
fn code_review_files_doc_drift_issues() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("Documentation Drift") || c.contains("documentation drift"),
        "Code Review must file Documentation Drift issues"
    );
}

#[test]
fn skills_record_issues_via_add_issue() {
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        if content.contains("bin/flow issue") {
            assert!(
                content.contains("add-issue"),
                "{} calls bin/flow issue but must also call add-issue",
                name
            );
        }
    }
}

#[test]
fn generic_skills_have_no_framework_conditionals() {
    let _phase_names: HashSet<String> = common::phase_order().into_iter().collect();
    let generic = vec![
        "flow-commit",
        "flow-config",
        "flow-status",
        "flow-note",
        "flow-reset",
        "flow-abort",
        "flow-issues",
        "flow-create-issue",
        "flow-decompose-project",
        "flow-doc-sync",
        "flow-orchestrate",
    ];
    for name in generic {
        if !common::skills_dir().join(name).join("SKILL.md").exists() {
            continue;
        }
        let content = common::read_skill(name);
        assert!(
            !content.contains("If Rails")
                && !content.contains("If Python")
                && !content.contains("If iOS"),
            "Generic skill {} must not have framework conditionals",
            name
        );
    }
}

// --- Configurable skills ---

#[test]
fn configurable_skills_support_both_flags() {
    for name in CONFIGURABLE_SKILLS {
        let c = common::read_skill(name);
        assert!(
            c.contains("--auto"),
            "{} must mention --auto in Usage",
            name
        );
        assert!(
            c.contains("--manual"),
            "{} must mention --manual in Usage",
            name
        );
    }
}

#[test]
fn configurable_skills_have_mode_resolution() {
    for name in CONFIGURABLE_SKILLS {
        let c = common::read_skill(name);
        assert!(
            c.contains("## Mode Resolution"),
            "{} must have Mode Resolution section",
            name
        );
    }
}

#[test]
fn mode_resolution_references_config_source() {
    let re = Regex::new(r"(?s)## Mode Resolution\n(.*?)(?:\n## |\z)").unwrap();
    for name in CONFIGURABLE_SKILLS {
        let c = common::read_skill(name);
        let cap = re.captures(&c);
        assert!(cap.is_some(), "{} has no Mode Resolution section", name);
        let text = &cap.unwrap()[1];
        if PHASE_ENTER_PHASES.contains(name) {
            assert!(
                text.contains("phase-enter"),
                "{} Mode Resolution must reference phase-enter",
                name
            );
        } else {
            assert!(
                text.contains(".flow-states/") || text.contains("state file"),
                "{} Mode Resolution must reference state file",
                name
            );
        }
    }
}

#[test]
fn prime_presets_cover_all_configurable_skills() {
    let c = common::read_skill("flow-prime");
    let re = Regex::new(r"```json\n(\{[\s\S]*?\})\n```").unwrap();
    let blocks: Vec<String> = re.captures_iter(&c).map(|cap| cap[1].to_string()).collect();
    assert!(
        blocks.len() >= 3,
        "Expected at least 3 JSON blocks in flow-prime, found {}",
        blocks.len()
    );
    for (i, preset) in blocks[..3].iter().enumerate() {
        let parsed: Value = serde_json::from_str(preset).unwrap();
        for skill in CONFIGURABLE_SKILLS {
            assert!(
                parsed.get(*skill).is_some(),
                "'{}' missing from preset {} in flow-prime",
                skill,
                i
            );
        }
    }
}

#[test]
fn configurable_skills_match_phase_order() {
    let mut expected = common::phase_order();
    expected.push("flow-abort".to_string());
    let actual: Vec<String> = CONFIGURABLE_SKILLS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        actual, expected,
        "CONFIGURABLE_SKILLS order must match phase order + abort"
    );
}

// --- Start skill consolidation tombstones ---

#[test]
fn start_no_start_setup_reference() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("start-setup"),
        "Tombstone: start-setup replaced in PR #904"
    );
}

#[test]
fn start_references_start_init() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("start-init"),
        "flow-start must reference start-init"
    );
}

#[test]
fn start_references_start_gate() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("start-gate"),
        "flow-start must reference start-gate"
    );
}

#[test]
fn start_references_start_workspace() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("start-workspace"),
        "flow-start must reference start-workspace"
    );
}

#[test]
fn start_references_phase_finalize() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("phase-finalize"),
        "flow-start must reference phase-finalize"
    );
}

#[test]
fn start_no_start_finalize() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("start-finalize"),
        "Tombstone: start-finalize replaced by phase-finalize in PR #925"
    );
}

#[test]
fn phase_enter_skills_no_action_enter() {
    for name in PHASE_ENTER_PHASES {
        let c = common::read_skill(name);
        assert!(
            !c.contains("--action enter"),
            "Tombstone: --action enter replaced by phase-enter in {}",
            name
        );
    }
}

#[test]
fn release_complete_banner_confirms_marketplace_update() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-release")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        c.contains("marketplace"),
        "Release COMPLETE banner must confirm marketplace update"
    );
}

// --- Logging ---

#[test]
fn start_logging_uses_safe_pattern() {
    let c = common::read_skill("flow-start");
    let re = Regex::new(r"(?s)## Logging\n(.*?)(?:\n## |\n---|\z)").unwrap();
    if let Some(cap) = re.captures(&c) {
        let section = &cap[1];
        assert!(
            section.contains("internally") || section.contains("append_log"),
            "Start logging section must note commands handle logging internally"
        );
    }
}

#[test]
fn logged_phases_use_bin_flow_log() {
    let ps = phase_skills_map();
    let re_log = Regex::new(r"(?s)## Logging\n(.*?)(?:\n## |\n---|\z)").unwrap();
    for (_, skill) in &ps[1..4] {
        let content = common::read_skill(skill);
        if let Some(cap) = re_log.captures(&content) {
            let section = &cap[1];
            assert!(
                section.contains("bin/flow log"),
                "{} Logging section must use bin/flow log",
                skill
            );
        }
    }
}

#[test]
fn plan_dag_capture_is_explicit() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("DAG") || c.contains("dag"),
        "Plan Step 2 must have explicit DAG capture instructions"
    );
}

#[test]
fn learn_step3_requires_output_for_findings() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("finding") || c.contains("Finding"),
        "Learn Step 3 must require output for findings"
    );
}

#[test]
fn learn_detects_truncated_agent_output() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("truncat") || c.contains("marker"),
        "Learn must check agent output for expected structure"
    );
}

#[test]
fn anti_patterns_has_inline_output_rule() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("anti-patterns.md");
    let c = fs::read_to_string(&path).unwrap();
    assert!(
        c.contains("Inline Output"),
        "Anti-patterns rule must have inline output rule"
    );
}

// --- Phase state updates ---

#[test]
fn phase_state_updates_suppress_output() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("set-timestamp") {
            // set-timestamp calls should not be displayed to user
            // This is a structural check — the commands exist
            assert!(
                content.contains("set-timestamp"),
                "{} must use set-timestamp for state updates",
                skill
            );
        }
    }
}

#[test]
fn phase_skills_have_time_format_instruction() {
    let ps = phase_skills_map();
    for (_, skill) in &ps {
        let content = common::read_skill(skill);
        if content.contains("COMPLETE") {
            assert!(
                content.contains("formatted_time"),
                "{} must include time formatting instructions",
                skill
            );
        }
    }
}

// --- Start workflow ---

#[test]
fn start_truncation_proceeds_without_confirmation() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("Truncation") || c.contains("truncat"),
        "Truncation must tell Claude to proceed without confirming"
    );
}

#[test]
fn start_derives_branch_name_from_prompt() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("branch name") || c.contains("Derived branch") || c.contains("branch"),
        "flow-start must derive concise branch name from prompt"
    );
}

#[test]
fn flow_start_no_gh_issue_view_instruction() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("gh issue view"),
        "Tombstone: removed in PR #741"
    );
}

#[test]
fn flow_start_documents_automatic_issue_branch_naming() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("issue") && c.contains("branch"),
        "flow-start must document issue-aware branch naming"
    );
}

#[test]
fn start_no_manual_step_counter() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("start_step_counter") && !c.contains("step_counter"),
        "Tombstone: manual step counter removed in PR #737"
    );
}

#[test]
fn start_no_explicit_lock_release() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("start-lock --release"),
        "Tombstone: explicit lock release removed in PR #904"
    );
}

#[test]
fn start_no_old_step_numbering() {
    let c = common::read_skill("flow-start");
    // Should use ### Step N format
    assert!(
        c.contains("### Step 1") || c.contains("## Step 1"),
        "Start must have proper step numbering"
    );
}

#[test]
fn start_step1_locked_has_hard_gate() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("locked") && c.contains("<HARD-GATE>"),
        "Step 1 must have HARD-GATE when start-init returns locked"
    );
}

// --- Prime ---

#[test]
fn prime_commit_step_enforces_flow_commit_exclusively() {
    let c = common::read_skill("flow-prime");
    assert!(
        c.contains("flow-commit") || c.contains("flow:flow-commit"),
        "flow-prime must use flow-commit exclusively"
    );
}

#[test]
fn prime_step_6_no_git_exclude_option() {
    let c = common::read_skill("flow-prime");
    assert!(
        !c.contains("git config core.excludes") && !c.contains("--git-exclude"),
        "Tombstone: removed in PR #696"
    );
}

#[test]
fn prime_step_6_commits_generated_files() {
    let c = common::read_skill("flow-prime");
    assert!(
        c.contains("commit") && c.contains("flow-commit"),
        "flow-prime must commit via flow-commit"
    );
}

#[test]
fn prime_has_commit_format_prompt() {
    let c = common::read_skill("flow-prime");
    assert!(
        c.contains("commit_format") || c.contains("commit format"),
        "flow-prime must prompt for commit message format"
    );
}

// --- Code phase ---

#[test]
fn code_skill_sets_continue_pending_before_commit() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("_continue_pending"),
        "Code phase must set _continue_pending before flow-commit"
    );
}

#[test]
fn plan_uses_plan_extract_for_issue_fetch() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("plan-extract"),
        "Plan must use plan-extract command"
    );
}

#[test]
fn plan_no_direct_gh_issue_view() {
    let c = common::read_skill("flow-plan");
    assert!(
        !c.contains("gh issue view"),
        "Tombstone: plan-extract handles issue fetch"
    );
}

#[test]
fn code_has_resume_check() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Resume Check") || c.contains("## Resume"),
        "Code must have Resume Check section"
    );
}

#[test]
fn code_has_self_invocation_check() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Self-Invocation") || c.contains("--continue-step"),
        "Code must have Self-Invocation Check section"
    );
}

#[test]
fn code_commit_self_invokes() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("flow:flow-code --continue-step"),
        "Code Commit section must self-invoke with --continue-step"
    );
}

#[test]
fn code_commit_records_task() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("code_task"),
        "Code Commit must record code_task via set-timestamp"
    );
}

#[test]
fn code_skill_uses_single_task_framing() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("single task") || c.contains("only this single task"),
        "Code must use single-task framing"
    );
}

#[test]
fn code_skill_has_atomic_group_handling() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Atomic") || c.contains("atomic"),
        "Code must handle atomic task groups"
    );
}

// --- Learn phase ---

#[test]
fn learn_has_resume_check() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("Resume Check") || c.contains("## Resume"),
        "Learn must have Resume Check section"
    );
}

#[test]
fn learn_has_self_invocation_check() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("Self-Invocation") || c.contains("--continue-step"),
        "Learn must have Self-Invocation Check section"
    );
}

#[test]
fn learn_step_4_promotes_permissions() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("promote-permissions"),
        "Learn Step 4 must call promote-permissions"
    );
}

#[test]
fn learn_step_5_self_invokes() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("flow:flow-learn --continue-step"),
        "Learn Step 5 must self-invoke"
    );
}

#[test]
fn learn_sets_continue_pending_before_child_skills() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("_continue_pending"),
        "Learn must set _continue_pending"
    );
}

#[test]
fn learn_steps_record_completion() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("set-timestamp"),
        "Learn steps must record completion"
    );
}

#[test]
fn learn_skill_sets_steps_total() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("--steps-total") || c.contains("steps_total"),
        "Learn phase-enter must set --steps-total"
    );
}

// --- Plan phase ---

#[test]
fn plan_skill_does_not_reference_transcript_path() {
    let c = common::read_skill("flow-plan");
    assert!(
        !c.contains("transcript_path"),
        "Plan must not contain transcript_path"
    );
}

#[test]
fn complete_skill_uses_render_pr_body() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("render-pr-body"),
        "Complete must use render-pr-body"
    );
}

#[test]
fn plan_skill_uses_render_pr_body() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("render-pr-body"),
        "Plan Step 4 must use render-pr-body"
    );
}

#[test]
fn plan_skill_renders_plan_inline() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Render") || c.contains("render") || c.contains("inline"),
        "Plan Done section must render plan inline"
    );
}

// --- Complete phase ---

#[test]
fn complete_done_banner_includes_pr_url() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("pr_url") || c.contains("PR URL") || c.contains("pr url"),
        "Complete Done banner must include PR URL"
    );
}

#[test]
fn complete_done_banner_includes_phase_timings() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("timing") || c.contains("Timing") || c.contains("cumulative"),
        "Complete Done banner must include phase timings"
    );
}

#[test]
fn complete_done_banner_includes_session_summary() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("summary") || c.contains("Summary"),
        "Complete Done section must have session summary"
    );
}

#[test]
fn complete_post_merge_references_pr_sections() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("PR body") || c.contains("pr body") || c.contains("PR sections"),
        "Complete Step 6 must reference PR body sections"
    );
}

#[test]
fn complete_merged_path_includes_post_merge() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("post-merge") || c.contains("post_merge"),
        "Complete merged path must route through post-merge"
    );
}

#[test]
fn complete_has_self_invocation_check() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("Self-Invocation") || c.contains("--continue-step"),
        "Complete must have Self-Invocation Check section"
    );
}

#[test]
fn complete_uses_format_complete_summary() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("format-complete-summary"),
        "Complete must reference format-complete-summary"
    );
}

#[test]
fn complete_has_resume_check() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("Resume Check") || c.contains("## Resume"),
        "Complete must have Resume Check section"
    );
}

#[test]
fn complete_sets_continue_pending_before_commit() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("_continue_pending=commit"),
        "Complete must set _continue_pending=commit"
    );
}

#[test]
fn complete_commit_points_self_invoke() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("flow:flow-complete --continue-step"),
        "Complete Steps must self-invoke via --continue-step"
    );
}

// --- Complete tombstones ---

#[test]
fn complete_no_twelve_steps() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("### Step 12"),
        "Tombstone: 12-step structure consolidated"
    );
}

#[test]
fn complete_no_steps_total_in_skill() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("complete_steps_total"),
        "Tombstone: complete_steps_total moved to Rust"
    );
}

#[test]
fn complete_no_simulate_branch() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("--simulate-branch"),
        "Tombstone: --simulate-branch removed"
    );
}

#[test]
fn complete_uses_complete_fast() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("complete-fast"),
        "flow-complete must reference complete-fast"
    );
}

#[test]
fn complete_uses_complete_finalize() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("complete-finalize"),
        "flow-complete must reference complete-finalize"
    );
}

#[test]
fn continue_context_includes_mode_flag() {
    let skills_with_min = [
        ("flow-code", 2),
        ("flow-code-review", 2),
        ("flow-complete", 9),
        ("flow-learn", 2),
    ];
    let re = Regex::new(r#""_continue_context=([^"]+)""#).unwrap();
    for (skill, min_count) in skills_with_min {
        let content = common::read_skill(skill);
        let contexts: Vec<String> = re
            .captures_iter(&content)
            .map(|c| c[1].to_string())
            .collect();
        let step_contexts: Vec<&String> = contexts
            .iter()
            .filter(|c| c.contains("--continue-step"))
            .collect();
        assert!(
            step_contexts.len() >= min_count,
            "Expected >= {} _continue_context with --continue-step in {}, found {}",
            min_count,
            skill,
            step_contexts.len()
        );
        for ctx in &step_contexts {
            assert!(
                ctx.contains("--auto") || ctx.contains("--manual"),
                "_continue_context in {} must include --auto or --manual: {}",
                skill,
                ctx
            );
        }
    }
}

// --- Flat sequential step numbering ---

#[test]
fn skills_no_substep_markers() {
    let bold_re = Regex::new(r"\*\*\d+[a-z]\.").unwrap();
    let heading_re = Regex::new(r"(?m)^###\s+\d+[a-z]").unwrap();
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        assert!(
            !bold_re.is_match(&content),
            "{} contains bold sub-step markers",
            name
        );
        assert!(
            !heading_re.is_match(&content),
            "{} contains heading sub-step labels",
            name
        );
    }
}

// --- DAG decomposition ---

#[test]
fn plan_skill_has_dag_step() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("decompose:decompose"),
        "flow-plan must reference decompose:decompose plugin"
    );
}

#[test]
fn plan_skill_has_dag_resume_check() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("dag") || c.contains("DAG"),
        "flow-plan must check dag for resume"
    );
}

#[test]
fn plan_skill_has_approval_gate() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("AskUserQuestion"),
        "flow-plan must use AskUserQuestion for approval gate"
    );
}

#[test]
fn plan_skill_does_not_use_plan_mode() {
    let c = common::read_skill("flow-plan");
    assert!(
        !c.contains("EnterPlanMode"),
        "flow-plan must not reference EnterPlanMode"
    );
}

#[test]
fn plan_has_self_invocation_check() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Self-Invocation") || c.contains("--continue-step"),
        "Plan must have Self-Invocation Check"
    );
}

#[test]
fn plan_has_continue_pending_for_decompose() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("_continue_pending"),
        "Plan must set _continue_pending before decompose"
    );
}

#[test]
fn plan_detects_decomposed_label() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("decomposed") || c.contains("Decomposed"),
        "Plan must detect 'decomposed' label on issues"
    );
}

#[test]
fn plan_step3_extracts_implementation_plan_for_decomposed() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Implementation Plan"),
        "Plan Step 3 must extract Implementation Plan for decomposed issues"
    );
}

// --- Done section hard gates ---

#[test]
fn done_hardgates_read_continue_action() {
    let ps = phase_skills_map();
    for (_, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        assert!(
            content.contains("continue_action"),
            "{} Done HARD-GATE must read continue_action",
            skill
        );
    }
}

#[test]
fn done_hardgates_no_reread_state_file() {
    let ps = phase_skills_map();
    let re = Regex::new(r"(?s)<HARD-GATE>(.*?)</HARD-GATE>").unwrap();
    for (_, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        let gates: Vec<String> = re
            .captures_iter(&content)
            .map(|c| c[1].to_string())
            .collect();
        // The last hard gate (Done section) should not re-read the state file
        if let Some(last) = gates.last() {
            if last.contains("continue_action") {
                assert!(
                    !last.contains("Read tool") || !last.contains(".flow-states/"),
                    "Tombstone: {} Done HARD-GATE should not re-read state file",
                    skill
                );
            }
        }
    }
}

#[test]
fn done_hard_gates_auto_path_has_final_action_language() {
    let ps = phase_skills_map();
    let re = Regex::new(r"(?s)<HARD-GATE>(.*?)</HARD-GATE>").unwrap();
    for (_, skill) in &ps[..ps.len() - 1] {
        let content = common::read_skill(skill);
        let gates: Vec<String> = re
            .captures_iter(&content)
            .map(|c| c[1].to_string())
            .collect();
        if let Some(last) = gates.last() {
            if last.contains("continue=auto") {
                assert!(
                    last.contains("FINAL") || last.contains("final"),
                    "{} Done auto path must have strengthened language",
                    skill
                );
            }
        }
    }
}

// --- Plan configuration ---

#[test]
fn plan_skill_has_dag_mode_resolution() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("dag") && c.contains("Mode Resolution"),
        "Plan Mode Resolution must reference dag config"
    );
}

#[test]
fn plan_validates_target_file_paths() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Target Path") || c.contains("target path"),
        "Plan must have Target Path Validation subsection"
    );
}

#[test]
fn plan_verifies_script_behavior_assertions() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Script Behavior") || c.contains("script behavior"),
        "Plan must have Script Behavior Verification"
    );
}

#[test]
fn prime_presets_include_dag_config() {
    let c = common::read_skill("flow-prime");
    let re = Regex::new(r"```json\n(\{[\s\S]*?\})\n```").unwrap();
    let blocks: Vec<String> = re.captures_iter(&c).map(|cap| cap[1].to_string()).collect();
    for (i, block) in blocks[..3.min(blocks.len())].iter().enumerate() {
        assert!(block.contains("dag"), "Preset {} must include 'dag' key", i);
    }
}

#[test]
fn prime_installs_decompose_plugin() {
    let c = common::read_skill("flow-prime");
    assert!(
        c.contains("decompose"),
        "flow-prime must install decompose plugin"
    );
}

// --- Flow issues skill ---

#[test]
fn flow_issues_has_work_order_section() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Work Order") || c.contains("work order"),
        "flow-issues must have Work Order section"
    );
}

#[test]
fn flow_issues_has_wip_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Flow In-Progress"),
        "flow-issues must reference 'Flow In-Progress'"
    );
}

#[test]
fn flow_issues_has_decomposed_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("decomposed") || c.contains("Decomposed"),
        "flow-issues must reference decomposed label"
    );
}

#[test]
fn flow_issues_no_dependency_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        !c.contains("Depends on #") && !c.contains("depends on #"),
        "Tombstone: dependency detection removed in PR #661"
    );
}

#[test]
fn flow_issues_has_blocked_label_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Blocked"),
        "flow-issues must reference Blocked label"
    );
}

#[test]
fn flow_issues_has_stale_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("stale") || c.contains("Stale"),
        "flow-issues must have stale issue detection"
    );
}

#[test]
fn flow_issues_has_start_commands() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("flow-start") || c.contains("flow:flow-start"),
        "flow-issues must include flow-start commands"
    );
}

#[test]
fn flow_issues_start_commands_include_title() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("title") || c.contains("Title"),
        "flow-issues must instruct to add issue title comments"
    );
}

#[test]
fn flow_issues_has_impact_ranking() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("impact") || c.contains("Impact"),
        "flow-issues must have impact ranking"
    );
}

#[test]
fn flow_issues_has_status_column() {
    let c = common::read_skill("flow-issues");
    assert!(c.contains("Status"), "flow-issues must have Status column");
}

#[test]
fn flow_issues_has_ready_and_blocked_values() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Ready") && c.contains("Blocked"),
        "flow-issues must define Ready and Blocked values"
    );
}

#[test]
fn flow_issues_start_commands_exclude_blocked() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("blocked") || c.contains("Blocked"),
        "flow-issues must exclude blocked issues from start commands"
    );
}

// --- Issue labeling ---

#[test]
fn flow_start_labels_issues() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("label") || c.contains("Label"),
        "flow-start must document issue labeling"
    );
}

#[test]
fn flow_complete_removes_labels() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("label-issues --remove") || c.contains("label-issues") && c.contains("remove"),
        "flow-complete must call label-issues --remove"
    );
}

#[test]
fn flow_abort_removes_labels() {
    let c = common::read_skill("flow-abort");
    assert!(
        c.contains("label-issues --remove") || c.contains("label-issues") && c.contains("remove"),
        "flow-abort must call label-issues --remove"
    );
}

// --- Create issue skill ---

#[test]
fn create_issue_has_step_dispatch() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("Step Dispatch") || c.contains("step dispatch") || c.contains("--step"),
        "flow-create-issue must have Step Dispatch section"
    );
}

#[test]
fn create_issue_usage_documents_step_flag() {
    let c = common::read_skill("flow-create-issue");
    assert!(c.contains("--step"), "Usage must document --step forms");
}

#[test]
fn create_issue_steps_have_banners() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("STARTING") || c.contains("banner"),
        "Each step must have banner"
    );
}

#[test]
fn create_issue_steps_1_2_have_ask_user() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("AskUserQuestion"),
        "Steps 1 and 2 must have AskUserQuestion gates"
    );
}

#[test]
fn create_issue_step_1_self_invokes() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("--step"),
        "Step 1 must self-invoke with --step flag"
    );
}

#[test]
fn create_issue_has_resume_check() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("Resume") || c.contains("resume"),
        "flow-create-issue must have Resume Check"
    );
}

#[test]
fn create_issue_no_input_classification() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        !c.contains("input classification") && !c.contains("Input Classification"),
        "Tombstone: removed in PR #677"
    );
}

#[test]
fn create_issue_no_exploration_mode() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        !c.contains("exploration mode") && !c.contains("Exploration Mode"),
        "Tombstone: removed in PR #677"
    );
}

#[test]
fn create_issue_no_multi_issue_path() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        !c.contains("multi-issue") && !c.contains("Multi-Issue") && !c.contains("multiple issues"),
        "Tombstone: removed in PR #677"
    );
}

#[test]
fn create_issue_has_conversation_gate() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("cold-start") || c.contains("conversation") || c.contains("context"),
        "flow-create-issue must reject cold-start invocations"
    );
}

#[test]
fn create_issue_step2_has_implementation_plan_section() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("Implementation Plan"),
        "Step 2 must produce Implementation Plan"
    );
}

#[test]
fn create_issue_has_repo_routing() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("benkruger/flow") || c.contains("repo"),
        "flow-create-issue must route plugin bugs to benkruger/flow"
    );
}

#[test]
fn create_issue_skips_repo_selection_in_flow_repo() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("FLOW repo") || c.contains("flow repo") || c.contains("plugin repo"),
        "Must skip repo selection in FLOW repo"
    );
}

#[test]
fn create_issue_step1_has_prior_decompose_detection() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("decompose") || c.contains("prior"),
        "Step 1 must detect prior decompose output"
    );
}

#[test]
fn create_issue_usage_documents_force_decompose() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("--force-decompose"),
        "Usage must document --force-decompose flag"
    );
}

#[test]
fn create_issue_step2_redecompose_uses_force_flag() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("--force-decompose"),
        "Re-decompose must use --force-decompose"
    );
}

// --- More tombstones ---

#[test]
fn complete_no_force_ci() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("--force") || c.contains("--force-decompose"),
        "Tombstone: --force removed from Complete CI command"
    );
}

#[test]
fn decompose_project_no_depends_on_text() {
    let c = common::read_skill("flow-decompose-project");
    assert!(
        !c.contains("Depends on") || c.contains("Depends On"),
        "Tombstone: 'Depends on' text removed from decompose-project"
    );
}

#[test]
fn no_flow_continue_skill() {
    assert!(
        !common::skills_dir().join("flow-continue").exists(),
        "Tombstone: flow-continue skill removed"
    );
}

#[test]
fn no_continue_context_rust_command() {
    let src = common::repo_root().join("src");
    assert!(
        !src.join("continue_context.rs").exists(),
        "Tombstone: bin/flow continue-context removed"
    );
}

// --- Diff format tombstones ---

#[test]
fn code_review_no_two_dot_diff() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: two-dot diff replaced with three-dot"
    );
}

#[test]
fn learn_no_two_dot_diff() {
    let c = common::read_skill("flow-learn");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: two-dot diff replaced"
    );
}

#[test]
fn learn_no_doc_drift_filing() {
    let c = common::read_skill("flow-learn");
    assert!(
        !c.contains("Documentation Drift"),
        "Tombstone: doc drift filing moved to code review"
    );
}

#[test]
fn reviewer_agent_no_two_dot_diff() {
    let c = common::read_agent("reviewer.md");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: reviewer agent no longer uses two-dot diff"
    );
}

#[test]
fn pre_mortem_agent_no_two_dot_diff() {
    let c = common::read_agent("pre-mortem.md");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: pre-mortem agent no longer uses two-dot diff"
    );
}

#[test]
fn adversarial_agent_no_two_dot_diff() {
    let c = common::read_agent("adversarial.md");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: adversarial agent no longer uses two-dot diff"
    );
}

#[test]
fn documentation_agent_no_two_dot_diff() {
    let c = common::read_agent("documentation.md");
    assert!(
        !c.contains("origin/main..HEAD") || c.contains("origin/main...HEAD"),
        "Tombstone: documentation agent no longer uses two-dot diff"
    );
}

// --- Git command consolidation tombstones ---

#[test]
fn plan_no_branch_show_current() {
    let c = common::read_skill("flow-plan");
    assert!(
        !c.contains("git branch --show-current"),
        "Tombstone: consolidated into porcelain output"
    );
}

#[test]
fn complete_no_branch_show_current() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("git branch --show-current"),
        "Tombstone: consolidated into porcelain output"
    );
}

#[test]
fn commit_no_branch_show_current() {
    let c = common::read_skill("flow-commit");
    assert!(
        !c.contains("git branch --show-current"),
        "Tombstone: consolidated into porcelain output"
    );
}

#[test]
fn abort_no_branch_show_current() {
    let c = common::read_skill("flow-abort");
    assert!(
        !c.contains("git branch --show-current"),
        "Tombstone: consolidated into porcelain output"
    );
}

// --- Code review self-invocation ---

#[test]
fn code_review_has_resume_check() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("Resume Check") || c.contains("## Resume"),
        "Code Review must have Resume Check section"
    );
}

#[test]
fn code_review_steps_record_completion() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("set-timestamp"),
        "Code Review steps must record completion via set-timestamp"
    );
}

#[test]
fn code_review_steps_self_invoke() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("flow:flow-code-review --continue-step"),
        "Code Review steps must self-invoke with --continue-step"
    );
}

#[test]
fn code_review_steps_await_background_agents() {
    let c = common::read_skill("flow-code-review");
    for agent in &["reviewer", "pre-mortem", "adversarial", "documentation"] {
        assert!(
            c.contains(agent),
            "Step 2 (Launch) must reference {} agent",
            agent
        );
    }
}

#[test]
fn code_review_has_self_invocation_check() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("Self-Invocation"),
        "Code Review must have Self-Invocation Check section"
    );
}

#[test]
fn code_review_has_bash_binflow_check() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("bin/flow"),
        "Step 1 (Gather) must check bin/flow"
    );
}

#[test]
fn start_no_explicit_lock_acquire() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("start-lock --acquire"),
        "Tombstone: explicit start-lock acquire removed"
    );
}

#[test]
fn start_no_explicit_ci_bash_blocks() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("```bash\nbin/ci") && !c.contains("```bash\nbin/flow ci"),
        "Tombstone: explicit ci bash blocks removed from start"
    );
}

#[test]
fn start_files_flaky_test_issues() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("Flaky Test"),
        "Step 2 (start-gate) must file Flaky Test issues"
    );
}

#[test]
fn start_step_2_has_ci_fix_subagent() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("ci-fixer"),
        "Step 2 (start-gate) must launch ci-fixer sub-agent"
    );
}

#[test]
fn start_ci_fixes_committed_via_flow_commit() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("flow-commit") || c.contains("flow:flow-commit"),
        "CI fixes on main committed via flow-commit"
    );
}

// --- Code review step 3 ---

#[test]
fn code_review_step_3_has_triage() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("Triage") || c.contains("triage"),
        "Step 3 (Triage) must classify findings"
    );
}

#[test]
fn code_review_step_2_launches_four_agents() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("four")
            || c.contains("4 ")
            || (c.contains("reviewer")
                && c.contains("pre-mortem")
                && c.contains("adversarial")
                && c.contains("documentation")),
        "Step 2 must launch all four agents"
    );
}

#[test]
fn code_review_no_plugin_step() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("code-review:code-review"),
        "Tombstone: code-review:code-review plugin removed"
    );
}

#[test]
fn code_review_no_plugin_config_axis() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("code_review_plugin"),
        "Tombstone: code_review_plugin config removed"
    );
}

// --- Code Review tombstone audit integration ---

#[test]
fn code_review_mentions_tombstone_audit() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("tombstone-audit"),
        "Code Review Step 1 must run tombstone-audit for stale tombstone detection"
    );
}

// --- Worktree path validation ---

#[test]
fn skills_no_repo_tracked_files_at_project_root() {
    let repo_tracked = ["bin/test", "bin/ci"];
    let mut violations = Vec::new();
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        let paragraphs: Vec<&str> = content.split("\n\n").collect();
        for para in &paragraphs {
            let lower = para.to_lowercase();
            if !lower.contains("project root") {
                continue;
            }
            for exe in &repo_tracked {
                if para.contains(exe) {
                    violations.push(format!(
                        "{}: paragraph mentions both '{}' and 'project root'",
                        name, exe
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Skills must not direct Claude to check repo-tracked files 'at the project root':\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_exec_in_bash_blocks() {
    let mut violations = Vec::new();
    // Check skills
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        for block in common::extract_bash_blocks(&content) {
            for line in block.lines() {
                let first = line.split_whitespace().next().unwrap_or("");
                if first == "exec" {
                    violations.push(format!("skills/{}/SKILL.md: {}", name, line.trim()));
                }
            }
        }
    }
    // Check agents
    for agent in agent_files() {
        let content = common::read_agent(&agent);
        for block in common::extract_bash_blocks(&content) {
            for line in block.lines() {
                let first = line.split_whitespace().next().unwrap_or("");
                if first == "exec" {
                    violations.push(format!("agents/{}: {}", agent, line.trim()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Bash blocks must not use exec:\n{}",
        violations.join("\n")
    );
}

// --- Prime preset ordering ---

#[test]
fn prime_presets_keys_match_phase_order() {
    let c = common::read_skill("flow-prime");
    let re = Regex::new(r"```json\n(\{[\s\S]*?\})\n```").unwrap();
    let blocks: Vec<String> = re.captures_iter(&c).map(|cap| cap[1].to_string()).collect();
    let mut expected = common::phase_order();
    expected.push("flow-abort".to_string());
    for (i, block) in blocks[..3.min(blocks.len())].iter().enumerate() {
        let parsed: Value = serde_json::from_str(block).unwrap();
        let keys: Vec<String> = parsed.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys, expected,
            "Preset {} keys must follow phase order + abort",
            i
        );
    }
}

#[test]
fn quadruple_fenced_blocks_use_markdown_and_text() {
    let re = Regex::new(r"````(\w+)").unwrap();
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        for cap in re.captures_iter(&content) {
            let lang = &cap[1];
            assert!(
                lang == "markdown" || lang == "text",
                "{} quadruple-fenced block uses '{}' — must be 'markdown' or 'text'",
                name,
                lang
            );
        }
    }
}

#[test]
fn phase_1_hard_gate_requires_rerun_with_arguments() {
    let c = common::read_skill("flow-start");
    let re = Regex::new(r"(?s)<HARD-GATE>(.*?)</HARD-GATE>").unwrap();
    if let Some(cap) = re.captures(&c) {
        let gate = &cap[1];
        assert!(
            gate.contains("re-run") || gate.contains("rerun") || gate.contains("Usage"),
            "Phase 1 first HARD-GATE must tell user to re-run with arguments"
        );
    }
}
