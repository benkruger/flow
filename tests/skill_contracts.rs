// SKILL.md content contracts.
//
// Validates structural invariants in skill markdown files: phase gates,
// state field references, cross-skill invocations, agent contracts,
// banner formatting, tombstone tests, and more.

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use flow_rs::tombstone_audit::extract_pr_numbers;
use regex::Regex;
use serde_json::Value;

// --- Constants ---

const CONFIGURABLE_SKILLS: &[&str] = &[
    "flow-start",
    "flow-code",
    "flow-review",
    "flow-learn",
    "flow-complete",
    "flow-abort",
];

const PHASE_ENTER_PHASES: &[&str] = &["flow-code", "flow-review", "flow-learn"];

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
fn review_has_six_tenants() {
    let c = common::read_skill("flow-review");
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
            "flow-review missing tenant '{}'",
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
fn review_no_onboarding_agent() {
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

// --- Agent Output Format subsection extractor ---
//
// Both the END-OF-FINDINGS marker contract and the code_read field
// contract assert content inside an agent's `## Output Format`
// section. Each contract uses a bounded slice so a refactor that
// guts the section is detected even when an unrelated sibling
// section still mentions the asserted token (see
// `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
// in Contract Tests"). Extracting the bounded-slice walk into a
// shared helper keeps the section boundary one source of truth.

fn read_agent_output_format_section(agent_basename: &str) -> String {
    let c = common::read_agent(agent_basename);
    let tail_at_heading = c
        .split_once("## Output Format")
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_else(|| panic!("{agent_basename} must have ## Output Format section"));
    tail_at_heading
        .split_once("\n## ")
        .map(|(section, _)| section.to_string())
        .unwrap_or(tail_at_heading)
}

// --- END-OF-FINDINGS marker contract ---
//
// Three context-rich/high-investigation agents — reviewer,
// learn-analyst, documentation — declare a literal `END-OF-FINDINGS`
// completion marker in their Output Format section so the
// flow-review skill can detect maxTurns truncation by marker
// absence rather than guessing from prose shape. Per-file siblings
// (rather than a single coordinated test) because each agent's
// regression is independent: a refactor or accidental edit to one
// agent's Output Format that drops the marker breaks the skill's
// truncation detection for THAT agent only. Per-file failure output
// names the drifted agent immediately.

fn assert_agent_output_format_declares_end_of_findings(agent_basename: &str) {
    let subsection = read_agent_output_format_section(agent_basename);
    assert!(
        subsection.contains("END-OF-FINDINGS"),
        "{agent_basename} Output Format must declare the literal `END-OF-FINDINGS` completion marker so the flow-review skill can detect maxTurns truncation by marker absence (see .claude/rules/cognitive-isolation.md \"Context Budget + Truncation Recovery\")"
    );
}

#[test]
fn reviewer_agent_declares_end_of_findings_marker() {
    assert_agent_output_format_declares_end_of_findings("reviewer.md");
}

#[test]
fn learn_analyst_agent_declares_end_of_findings_marker() {
    assert_agent_output_format_declares_end_of_findings("learn-analyst.md");
}

#[test]
fn documentation_agent_declares_end_of_findings_marker() {
    assert_agent_output_format_declares_end_of_findings("documentation.md");
}

// --- code_read field contract ---
//
// The pre-mortem agent's safety value depends on the agent actually
// executing the Premise → Trace → Conclude reasoning discipline. A
// structural `code_read` field in the Output Format finding block
// converts "the agent verified the code" from an implicit claim into
// a required output: triage that sees a non-conforming or missing
// `code_read` value can dismiss the finding immediately, and skipped
// Trace steps leave a structural gap rather than a plausible-looking
// prose finding. The contract test guards against an accidental edit
// or refactor that drops the field.
//
// Scope: pre-mortem only. The other agents that follow the same
// reasoning discipline (reviewer, ci-fixer deep-diagnosis mode,
// adversarial — see .claude/rules/semi-formal-reasoning.md) do not
// yet declare the field; they are tracked separately rather than
// scope-expanded here. The assertion is structural rather than a
// loose substring match: it requires the bullet-shaped declaration
// `- **code_read:**` so a future edit that demotes the field to a
// prose mention or a code-block example would not satisfy the test.

fn assert_agent_output_format_declares_code_read(agent_basename: &str) {
    let subsection = read_agent_output_format_section(agent_basename);
    assert!(
        subsection.contains("- **code_read:**"),
        "{agent_basename} Output Format must declare a `code_read` field as a bullet (`- **code_read:**`) naming the file:line_range the agent verified via Read or Grep, so triage can detect findings produced from the diff alone without an actual codebase trace (see .claude/rules/semi-formal-reasoning.md). The bullet-shaped assertion guards against future edits that demote the field to a prose mention or code-block example."
    );
}

#[test]
fn pre_mortem_agent_declares_code_read_field() {
    assert_agent_output_format_declares_code_read("pre-mortem.md");
}

// --- Halt instructions wrapped in fix-first HARD-GATE ---
//
// When a phase skill instructs the model to halt the workflow on an
// infrastructure failure (e.g. `bin/test --adversarial-path` exits 2,
// a phase-gate command returns a structured error), the surrounding
// prose must wrap the instruction in a `<HARD-GATE>` block that names
// the single fix-first response and cites both
// `.claude/rules/anti-patterns.md` "Never Offer to Skip Workflow Steps"
// and `.claude/rules/fix-infrastructure-bugs.md` "Fix Infrastructure
// Bugs Immediately". Without the HARD-GATE shape, the model defaults
// to enumerating multiple options ("(1) fix it, (2) skip the agent,
// (3) abort the workflow") at the moment the rule says enumeration
// is forbidden.
//
// Single coordinated test (rather than per-skill siblings) because
// the invariant is corpus-wide: every phase SKILL.md that adds a
// halt instruction must follow the same shape. Per-skill failure
// output is preserved by including the skill name and trigger line
// in every assertion message.
//
// Trigger vocabulary (closed and curated):
//
// - A line containing `halt` AND one of `exit 2` / `exits 2` /
//   `exit code 2` / `exits with 2` (case-insensitive).
// - A line containing `infrastructure halt` (case-insensitive).
//
// Compliance proof — the trigger line must sit inside an open
// `<HARD-GATE>` block, AND the enclosing block must contain:
// `single option` OR `Two options`; AND `anti-patterns.md`; AND
// `fix-infrastructure-bugs.md`. Compliance is the conjunction.

fn line_byte_offset(content: &str, line_index: usize) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == line_index {
            return offset;
        }
        offset += line.len() + 1;
    }
    offset
}

fn halt_trigger_matches(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("infrastructure halt") {
        return true;
    }
    if !lower.contains("halt") {
        return false;
    }
    lower.contains("exit 2")
        || lower.contains("exits 2")
        || lower.contains("exit code 2")
        || lower.contains("exits with 2")
}

#[test]
fn phase_skills_halt_instructions_wrapped_in_fix_first_hard_gate() {
    let ps = phase_skills_map();
    for (key, skill) in &ps {
        let content = common::read_skill(skill);
        for (idx, line) in content.lines().enumerate() {
            if !halt_trigger_matches(line) {
                continue;
            }
            let line_offset = line_byte_offset(&content, idx);
            let before = &content[..line_offset];
            let last_open = before.rfind("<HARD-GATE>");
            let last_close = before.rfind("</HARD-GATE>");
            let inside_hard_gate = match (last_open, last_close) {
                (Some(o), Some(c)) => o > c,
                (Some(_), None) => true,
                _ => false,
            };
            assert!(
                inside_hard_gate,
                "Phase {key} ({skill}) line {}: halt instruction must be wrapped in a <HARD-GATE> block per .claude/rules/anti-patterns.md \"Never Offer to Skip Workflow Steps\" and .claude/rules/fix-infrastructure-bugs.md \"Fix Infrastructure Bugs Immediately\". Trigger line:\n  {line}",
                idx + 1
            );
            let gate_start = last_open.expect("inside_hard_gate implies open");
            let after_open = &content[gate_start..];
            let gate_end_relative = after_open.find("</HARD-GATE>").unwrap_or_else(|| {
                panic!("Phase {key} ({skill}) HARD-GATE at byte {gate_start} has no closing tag")
            });
            let gate_block = &after_open[..gate_end_relative];
            assert!(
                gate_block.contains("single option") || gate_block.contains("Two options"),
                "Phase {key} ({skill}) line {}: enclosing HARD-GATE must frame the response with \"single option\" or \"Two options\" so the model cannot enumerate alternatives. Trigger line:\n  {line}",
                idx + 1
            );
            assert!(
                gate_block.contains("anti-patterns.md"),
                "Phase {key} ({skill}) line {}: enclosing HARD-GATE must cite .claude/rules/anti-patterns.md (Never Offer to Skip Workflow Steps). Trigger line:\n  {line}",
                idx + 1
            );
            assert!(
                gate_block.contains("fix-infrastructure-bugs.md"),
                "Phase {key} ({skill}) line {}: enclosing HARD-GATE must cite .claude/rules/fix-infrastructure-bugs.md (Fix Infrastructure Bugs Immediately). Trigger line:\n  {line}",
                idx + 1
            );
        }
    }
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
fn review_agents_have_sufficient_max_turns() {
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
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("CLAUDE.md") || c.contains("claude.md"),
        "Review Step 2 (Launch) must reference CLAUDE.md for reviewer context"
    );
}

// --- Code review requirements ---

#[test]
fn review_no_inline_correctness_review() {
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("### Correctness Review") && !c.contains("## Correctness Review"),
        "Tombstone: inline correctness review removed"
    );
}

#[test]
fn review_no_inline_security_step() {
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("### Security Review") && !c.contains("## Security Review"),
        "Tombstone: inline security review step removed"
    );
}

#[test]
fn review_uses_documentation_subagent() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("documentation"),
        "Review must reference documentation sub-agent"
    );
}

#[test]
fn review_step_4_handles_no_findings() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("no findings") || c.contains("No findings") || c.contains("no real findings"),
        "Step 4 (Fix) must handle no-findings path"
    );
}

#[test]
fn review_no_step_5() {
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("### Step 5"),
        "Tombstone: Step 5 merged into Step 4"
    );
}

#[test]
fn review_no_step_6() {
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("### Step 6"),
        "Tombstone: Step 6 merged into Step 4"
    );
}

#[test]
fn review_steps_have_continuation_directives() {
    let c = common::read_skill("flow-review");
    // Steps must have continuation directives (may use ## Step or ### Step format)
    assert!(
        c.contains("Step 1") && c.contains("Step 2") && c.contains("Step 3"),
        "Review must have Steps 1-3"
    );
}

#[test]
fn review_hard_rules_require_step_continuation() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("## Hard Rules"),
        "Review must have Hard Rules section"
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

/// Every fenced bash block that invokes a CI-running subcommand
/// (`bin/flow ci`, `bin/flow start-gate`, `bin/flow finalize-commit`,
/// `bin/flow complete-fast`) must be preceded by prose that instructs
/// the model to set a 10-minute Bash tool timeout. Without the
/// instruction, the default 2-minute Bash tool timeout backgrounds
/// long-running CI invocations, defeating the gate (see
/// `.claude/rules/ci-is-a-gate.md`).
///
/// Regression guarded: a future SKILL.md refactor drops the adjacent
/// timeout-instruction prose. The test scans every SKILL.md under
/// both `skills/` and `.claude/skills/` (maintainer skills like
/// `flow-release` invoke `finalize-commit` too) and panics with the
/// file path and opening-fence line number when the instruction is
/// missing.
///
/// Scan window: the 5 immediately preceding non-blank lines of prose
/// above each opening ```bash fence. The backward walk stops at any
/// prior fenced block — each CI-invoking block must have its own
/// adjacent preamble, and inheritance across unrelated blocks is
/// prohibited. Adjacent variants in the same section each carry their
/// own preamble.
#[test]
fn skill_ci_invocations_specify_long_timeout() {
    // CI-running subcommand family. Each entry runs `ci::run_impl()`
    // directly or transitively:
    //
    // - `ci`              — the direct CI runner
    // - `start-gate`      — runs CI on main under the start lock per
    //                       CLAUDE.md "Start-Gate CI on Main as
    //                       Serialization Point"
    // - `finalize-commit` — runs `ci::run_impl()` before `git commit`
    //                       per CLAUDE.md "CI is enforced inside
    //                       `finalize-commit` itself"
    // - `complete-fast`   — runs a local CI dirty check before the
    //                       Complete merge, and dispatches to
    //                       `ci::run_impl()` on sentinel miss
    //
    // When adding a new CI-running `bin/flow` subcommand, extend this
    // regex in the same PR and update the list above.
    let ci_re = Regex::new(r"bin/flow (ci|start-gate|finalize-commit|complete-fast)\b").unwrap();
    // Numeric form: the Bash tool `timeout` parameter must equal
    // exactly 600000 (10 minutes). The trailing `\D` (or end of
    // string) anchor prevents typo'd values like `timeout: 6000000`
    // (100 minutes) from passing the gate as substring matches.
    let timeout_num_re = Regex::new(r"timeout:\s*600000(\D|$)").unwrap();
    // Prose form: the canonical phrase authors use when describing
    // the instruction in surrounding text.
    const TIMEOUT_PROSE: &str = "10-minute Bash tool timeout";
    const WINDOW_NON_BLANK_LINES: usize = 5;

    let mut violations: Vec<String> = Vec::new();

    let mut scan_dir = |dir: PathBuf, label: &str| {
        let files = common::collect_md_files(&dir);
        for (rel, content) in &files {
            if !rel.ends_with("SKILL.md") {
                continue;
            }
            let lines: Vec<&str> = content.lines().collect();

            let mut in_bash = false;
            let mut bash_body = String::new();
            let mut fence_line: usize = 0;
            let mut prev_prose: Vec<String> = Vec::new();
            let mut saw_opening_fence = false;

            let check_coverage = |prev_prose: &[String],
                                  violations: &mut Vec<String>,
                                  fence_line: usize| {
                let has_instruction = prev_prose
                    .iter()
                    .any(|l| timeout_num_re.is_match(l) || l.contains(TIMEOUT_PROSE));
                if !has_instruction {
                    violations.push(format!(
                        "{}/{}:{} — bash block invokes a CI-running `bin/flow` subcommand but the preceding {} non-blank prose lines (stopping at any prior fence) do not mention `timeout: 600000` or `10-minute Bash tool timeout`",
                        label, rel, fence_line, WINDOW_NON_BLANK_LINES
                    ));
                }
            };

            for (idx, line) in lines.iter().enumerate() {
                let trimmed_left = line.trim_start();
                if !in_bash && trimmed_left.starts_with("```bash") {
                    in_bash = true;
                    saw_opening_fence = true;
                    bash_body.clear();
                    // Line numbers are 1-based for human-readable error output.
                    fence_line = idx + 1;
                    // Walk backward collecting the preceding non-blank
                    // prose lines. Stop immediately at any prior fence
                    // line — each CI-invoking block must have its own
                    // adjacent preamble, not inherit from a distant
                    // section across unrelated blocks.
                    prev_prose.clear();
                    let mut j = idx;
                    while j > 0 && prev_prose.len() < WINDOW_NON_BLANK_LINES {
                        j -= 1;
                        let prev = lines[j];
                        let prev_t = prev.trim();
                        if prev_t.is_empty() {
                            continue;
                        }
                        if prev_t.starts_with("```") {
                            break;
                        }
                        prev_prose.push(prev.to_string());
                    }
                    continue;
                }
                if in_bash && trimmed_left.starts_with("```") {
                    in_bash = false;
                    if ci_re.is_match(&bash_body) {
                        check_coverage(&prev_prose, &mut violations, fence_line);
                    }
                    bash_body.clear();
                    continue;
                }
                if in_bash {
                    bash_body.push_str(line);
                    bash_body.push('\n');
                }
            }

            // Unclosed ```bash fence at EOF: the main loop never saw a
            // closing fence, so `bash_body` was accumulated but never
            // checked. Treat this as a violation — either the file is
            // truncated (interrupted write, merge-conflict half-save)
            // or a merge conflict dropped the closing fence. Either
            // way, the gate should surface it loudly rather than
            // silently passing.
            if in_bash && saw_opening_fence && ci_re.is_match(&bash_body) {
                violations.push(format!(
                    "{}/{}:{} — unclosed ```bash fence at EOF contains a CI-running `bin/flow` invocation. Close the fence or restore the truncated content.",
                    label, rel, fence_line
                ));
            }
        }
    };

    scan_dir(common::skills_dir(), "skills");

    assert!(
        violations.is_empty(),
        "SKILL.md bash blocks invoke CI-running commands without an adjacent 10-minute timeout instruction (see .claude/rules/ci-is-a-gate.md):\n{}",
        violations.join("\n")
    );
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

// Equals-sign banners are prohibited — only box-drawing characters allowed

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
fn prime_supports_reprime_flag() {
    let c = common::read_skill("flow-prime");
    assert!(c.contains("--reprime"), "Prime must support --reprime flag");
}

// --- Skill structure and learning ---

#[test]
fn no_skill_fragment_files() {
    // Each skill directory must contain only SKILL.md, never split
    // into multiple .md fragments. The original phrasing called these
    // "framework fragments" — the rule itself was always about
    // skill fragmentation, not framework dispatch.
    for name in common::all_skill_names() {
        let dir = common::skills_dir().join(&name);
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname != "SKILL.md" && fname.ends_with(".md") {
                panic!("No skill fragment files should exist: {}/{}", name, fname);
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
fn learning_files_plugin_issues_against_plugin_repo() {
    let c = common::read_skill("flow-learn");
    // Plugin process gaps and enforcement escalations must route to
    // the plugin repo. Issue #1405 removed the redundant `Flow`
    // label, leaving `--repo benkruger/flow` as the routing signal.
    assert!(
        c.contains("--repo benkruger/flow"),
        "Learn must file plugin issues with --repo benkruger/flow"
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
fn review_no_inline_simplify_step() {
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("simplify:simplify"),
        "Tombstone: simplify plugin removed"
    );
}

#[test]
fn review_triage_two_outcomes_only() {
    // Review has two triage outcomes: Real (fix in Step 4) and
    // False positive (dismiss). The filing path was removed — see
    // .claude/rules/review-scope.md.
    let c = common::read_skill("flow-review");
    assert!(
        !c.contains("bin/flow issue"),
        "Review skill must not invoke issue creation"
    );
    assert!(
        !c.contains("bin/flow add-issue"),
        "Review skill must not record filed issues"
    );
    assert!(
        !c.contains("--outcome \"filed\""),
        "Review skill must not record findings with the filed outcome"
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
fn generic_skills_have_no_language_conditionals() {
    // Generic skills (the always-available utility skills) must stay
    // language-agnostic. They never branch on "If Rails", "If Python",
    // etc. — every project owns its toolchain via bin/* and the skill
    // itself is the same shape regardless of language.
    let _phase_names: HashSet<String> = common::phase_order().into_iter().collect();
    let generic = vec![
        "flow-commit",
        "flow-config",
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
            "Generic skill {} must not have language conditionals",
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

/// Locks in the `code_tasks_total` writer at flow-start. The TUI's
/// X-of-Y rendering paths in the Code-phase timeline read
/// `code_tasks_total` from the per-branch state file and silently
/// no-op when the field is absent, so the writer must remain wired
/// for the counter to display. The adjacency check requires the
/// `set-timestamp` invocation to sit in a bash block whose
/// preceding non-blank prose references `plan-from-issue`,
/// anchoring the writer to the step that computes the count and
/// preventing it from drifting to an unrelated step.
#[test]
fn flow_start_writes_code_tasks_total() {
    let content = common::read_skill("flow-start");
    const NEEDLE: &str = "set-timestamp --set code_tasks_total=";
    const ADJACENT: &str = "plan-from-issue";
    const WINDOW_NON_BLANK_LINES: usize = 5;

    assert!(
        content.contains(NEEDLE),
        "flow-start must invoke `bin/flow {}` so code_tasks_total \
         is written into the per-branch state file. The TUI's \
         X-of-Y rendering paths consume this field; without the \
         writer they silently no-op.",
        NEEDLE
    );

    let lines: Vec<&str> = content.lines().collect();
    let mut in_bash = false;
    let mut bash_body = String::new();
    let mut prev_prose: Vec<String> = Vec::new();
    let mut found_with_adjacent_plan_from_issue = false;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed_left = line.trim_start();
        if !in_bash && trimmed_left.starts_with("```bash") {
            in_bash = true;
            bash_body.clear();
            prev_prose.clear();
            let mut j = idx;
            while j > 0 && prev_prose.len() < WINDOW_NON_BLANK_LINES {
                j -= 1;
                let prev = lines[j];
                let prev_t = prev.trim();
                if prev_t.is_empty() {
                    continue;
                }
                if prev_t.starts_with("```") {
                    break;
                }
                prev_prose.push(prev.to_string());
            }
            continue;
        }
        if in_bash && trimmed_left.starts_with("```") {
            in_bash = false;
            if bash_body.contains(NEEDLE) && prev_prose.iter().any(|l| l.contains(ADJACENT)) {
                found_with_adjacent_plan_from_issue = true;
            }
            bash_body.clear();
            continue;
        }
        if in_bash {
            bash_body.push_str(line);
            bash_body.push('\n');
        }
    }

    assert!(
        found_with_adjacent_plan_from_issue,
        "flow-start must invoke `{}` in a bash block whose \
         preceding {} non-blank prose lines reference `{}` — \
         anchors the writer to the step that computes the count.",
        NEEDLE, WINDOW_NON_BLANK_LINES, ADJACENT
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

/// Returns the slice of `content` between the first `phase-enter`
/// invocation and the `## Resume Check` heading. Used by per-skill
/// re-anchor tests to bound the assertion scope per
/// `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
/// in Contract Tests".
fn slice_between_phase_enter_and_resume_check(content: &str) -> &str {
    let after_enter = content
        .split_once("phase-enter --phase")
        .map(|(_, t)| t)
        .expect("phase-enter --phase invocation must exist");
    after_enter
        .split_once("\n## Resume Check")
        .map(|(s, _)| s)
        .unwrap_or(after_enter)
}

/// Returns true when `bounded` contains `target` inside a fenced
/// ```bash``` block. The model only executes `bash` fences; if the
/// instruction lives in prose or a different fence type the cwd
/// never re-anchors at runtime. The search walks every `bash` fence
/// in the slice and checks the body up to the next closing fence
/// for `target`.
fn bash_fence_contains(bounded: &str, target: &str) -> bool {
    let mut rest = bounded;
    while let Some((_, after_open)) = rest.split_once("```bash") {
        let body_end = after_open.find("\n```").unwrap_or(after_open.len());
        let body = &after_open[..body_end];
        if body.contains(target) {
            return true;
        }
        rest = &after_open[body_end..];
    }
    false
}

/// Regression: flow-code/SKILL.md must instruct `cd "<worktree_cwd>"`
/// inside a bash fence between the phase-enter HARD-GATE and the
/// Resume Check. Without this, a session resuming Code phase after
/// context loss has no way to re-anchor cwd, and every subsequent
/// bin/flow call fails with cwd_scope::enforce blocking. The bash
/// fence is load-bearing: the model only executes ` ```bash ` blocks,
/// so a future regression that moves the instruction into prose
/// would silently disable runtime cd. Consumer: every Code-phase
/// session running on a mono-repo flow.
#[test]
fn flow_code_re_anchors_cwd_after_phase_enter() {
    let c = common::read_skill("flow-code");
    let bounded = slice_between_phase_enter_and_resume_check(&c);
    assert!(
        bash_fence_contains(bounded, r#"cd "<worktree_cwd>""#),
        "flow-code/SKILL.md must instruct `cd \"<worktree_cwd>\"` inside a bash fence between phase-enter and Resume Check"
    );
}

/// Regression: flow-review/SKILL.md must instruct
/// `cd "<worktree_cwd>"` inside a bash fence between the phase-enter
/// HARD-GATE and the Resume Check. Without this, a session resuming
/// Review after context loss cannot re-anchor cwd at runtime
/// (the model only executes bash fences). Consumer: every Code-
/// Review-phase session running on a mono-repo flow.
#[test]
fn flow_review_re_anchors_cwd_after_phase_enter() {
    let c = common::read_skill("flow-review");
    let bounded = slice_between_phase_enter_and_resume_check(&c);
    assert!(
        bash_fence_contains(bounded, r#"cd "<worktree_cwd>""#),
        "flow-review/SKILL.md must instruct `cd \"<worktree_cwd>\"` inside a bash fence between phase-enter and Resume Check"
    );
}

/// Regression: flow-learn/SKILL.md must instruct `cd "<worktree_cwd>"`
/// inside a bash fence between the phase-enter HARD-GATE and the
/// Resume Check. Without this, a session resuming Learn after context
/// loss cannot re-anchor cwd at runtime. Consumer: every Learn-phase
/// session running on a mono-repo flow.
#[test]
fn flow_learn_re_anchors_cwd_after_phase_enter() {
    let c = common::read_skill("flow-learn");
    let bounded = slice_between_phase_enter_and_resume_check(&c);
    assert!(
        bash_fence_contains(bounded, r#"cd "<worktree_cwd>""#),
        "flow-learn/SKILL.md must instruct `cd \"<worktree_cwd>\"` inside a bash fence between phase-enter and Resume Check"
    );
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
    for (_, skill) in &ps[1..3] {
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
fn flow_start_documents_automatic_issue_branch_naming() {
    let c = common::read_skill("flow-start");
    assert!(
        c.contains("issue") && c.contains("branch"),
        "flow-start must document issue-aware branch naming"
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

#[test]
fn code_has_plan_test_verification() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Plan Test Verification"),
        "Code skill must have Plan Test Verification subsection"
    );
}

#[test]
fn code_skill_has_discovery_output_handling_preamble() {
    // The flow-code skill must carry a "Discovery output handling"
    // section that names the truncation problem (Bash tool display
    // buffer drops the middle of long output) and tells the model
    // which existing artifacts (CI log file, Grep tool) to read
    // instead. Without the preamble, violation enumeration based
    // on inline output silently misses entries — the bug
    // motivating this contract.
    //
    // The contract is the literal phrase "discovery output
    // handling" (case-insensitive) so a future re-titling that
    // drops the recognizable name fails the test.
    let c = common::read_skill("flow-code");
    let lower = c.to_ascii_lowercase();
    assert!(
        lower.contains("discovery output handling"),
        "Code skill must contain a 'Discovery output handling' \
         preamble that names the long-output truncation problem \
         and points at existing artifacts (CI log file, Grep \
         tool) the model should use instead of inline output. \
         The phrase 'discovery output handling' (case-insensitive) \
         was not found."
    );
}

#[test]
fn code_documents_measurement_only_task_pathway() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("### Measurement-Only Tasks"),
        "Code skill must document the measurement-only task pathway as a named `### ` subsection"
    );
    // Bound the slice to the subsection itself. Splitting on the
    // heading string alone would leave `after_heading` covering
    // everything from the heading to EOF, so a later section (e.g.
    // the standard Commit section around L443) could satisfy the
    // /flow:flow-commit and "Nothing to commit" assertions even if
    // the subsection itself were gutted. Splitting the tail on the
    // next `### ` heading keeps the checks local to the subsection.
    let tail_at_heading = c
        .split_once("### Measurement-Only Tasks")
        .map(|(_, tail)| tail)
        .expect("heading presence asserted above");
    let subsection = tail_at_heading
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_heading);
    assert!(
        subsection.contains("/flow:flow-commit"),
        "Measurement-only subsection must route through /flow:flow-commit"
    );
    assert!(
        subsection.contains("Nothing to commit"),
        "Measurement-only subsection must reference the empty-diff return path"
    );
    assert!(
        subsection.contains("bin/flow ci"),
        "Measurement-only subsection must keep the bin/flow ci Gate mandatory"
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

// --- Complete phase ---

#[test]
fn complete_skill_uses_render_pr_body() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("render-pr-body"),
        "Complete must use render-pr-body"
    );
}

// --- Complete phase (cont.) ---

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

#[test]
fn complete_done_banner_mentions_findings() {
    let c = common::read_skill("flow-complete");
    assert!(
        c.contains("Findings"),
        "Complete Done section must mention findings sections in the summary description"
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
        ("flow-review", 2),
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
fn create_issue_has_starting_banner() {
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("STARTING") || c.contains("banner"),
        "Skill must have STARTING banner"
    );
}

#[test]
fn create_issue_has_ask_user_gate() {
    // Bound the assertion to the `## File` HARD-GATE section so a future
    // edit that gutted the section (leaving "AskUserQuestion" only in
    // the Hard Rules prose) cannot satisfy the test vacuously. Per
    // .claude/rules/testing-gotchas.md "Subsection-Local Assertions in
    // Contract Tests".
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## File\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## File` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("AskUserQuestion"),
        "flow-create-issue `## File` section must fire AskUserQuestion"
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
fn create_issue_has_implementation_plan_section() {
    // Bound the assertion to the `## Transform + Draft` section so a
    // future edit that gutted the section (leaving "Implementation
    // Plan" only in intro/Hard-Rules prose) cannot satisfy the test
    // vacuously. Also assert the FLOW-PLAN sentinel is documented so
    // bin/flow plan-from-issue extraction stays valid.
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## Transform + Draft\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Transform + Draft` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("Implementation Plan"),
        "`## Transform + Draft` must produce an Implementation Plan section"
    );
    assert!(
        section.contains("FLOW-PLAN-BEGIN") && section.contains("FLOW-PLAN-END"),
        "`## Transform + Draft` must wrap the plan in FLOW-PLAN sentinels"
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
fn flow_create_issue_skip_decompose_criterion_accepts_substantive_exploration() {
    // The Decompose section's skip rule must recognize substantive
    // exploration as sufficient context — named files, identified
    // root cause, agreed approach — not only literal prior decompose
    // output. This eliminates the Skill-tool roundtrip for the common
    // case where the user has already discussed the problem in the
    // current conversation, which is the failure surface where the
    // model returns control to the user instead of continuing.
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## Decompose\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Decompose` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        lower.contains("substantive exploration"),
        "`## Decompose` skip rule must accept substantive exploration"
    );
    assert!(
        lower.contains("named files"),
        "`## Decompose` skip rule must list named files as a signal"
    );
    assert!(
        lower.contains("root cause"),
        "`## Decompose` skip rule must list identified root cause as a signal"
    );
    assert!(
        lower.contains("agreed approach") || lower.contains("approach"),
        "`## Decompose` skip rule must list an agreed approach as a signal"
    );
}

#[test]
fn flow_create_issue_skip_decompose_criterion_rejects_bare_invocation() {
    // When the conversation lacks substantive exploration, the
    // Decompose section must still invoke decompose:decompose. The
    // skip path is a fast-track for the common case, not a bypass
    // that hides bare invocations.
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## Decompose\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Decompose` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("decompose:decompose"),
        "`## Decompose` must still invoke decompose:decompose when context is missing"
    );
    assert!(
        section.contains("--force-decompose"),
        "`## Decompose` must document the --force-decompose override"
    );
}

#[test]
fn flow_create_issue_decompose_has_hard_gate_after_skill_invocation() {
    // When decompose:decompose is invoked via the Skill tool, the
    // Decompose section must close with a HARD-GATE that prevents the
    // model from stopping, summarizing, or returning control to the
    // user once the Skill tool returns. The HARD-GATE is the
    // mechanical defense for the failure mode where Claude treats the
    // Skill tool's return as a natural stopping point — the same
    // surface that produced the original bug this issue tracks.
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## Decompose\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Decompose` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("<HARD-GATE>"),
        "`## Decompose` must include a HARD-GATE block"
    );
    assert!(
        section.contains("</HARD-GATE>"),
        "`## Decompose` HARD-GATE block must be closed"
    );
    let lower = section.to_ascii_lowercase();
    assert!(
        lower.contains("do not stop") || lower.contains("must not stop"),
        "`## Decompose` HARD-GATE must prohibit stopping after Skill return"
    );
    assert!(
        lower.contains("skill tool returns") || lower.contains("when the skill returns"),
        "`## Decompose` HARD-GATE must reference the Skill tool's return point"
    );
}

#[test]
fn flow_create_issue_hard_gate_names_consequence() {
    // The HARD-GATE prose must name the consequence so a future
    // maintainer reading the gate understands why it exists. Without
    // a named consequence, the gate looks like an arbitrary stylistic
    // restriction and is at risk of being weakened or removed. Per
    // .claude/rules/forward-facing-authoring.md, the prose names the
    // current invariant (unattended completion breaks if the model
    // stops here) without citing the originating issue.
    let c = common::read_skill("flow-create-issue");
    let tail = c
        .split_once("\n## Decompose\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Decompose` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        lower.contains("unattended")
            || lower.contains("user must prompt")
            || lower.contains("breaks the flow")
            || lower.contains("returns control"),
        "`## Decompose` HARD-GATE prose must name the consequence (unattended flow breaks)"
    );
}

#[test]
fn flow_create_issue_has_title_authoring_section() {
    // Issue titles flow into branch names, PR titles, commit subjects,
    // and TUI feature lines. Without explicit guidance, the model
    // paraphrases code symbols and shorthand from the brainstorming
    // conversation, and every downstream surface inherits unreadable
    // output. The Title Authoring section is the model-facing rule;
    // this contract test guards it against regression.
    let c = common::read_skill("flow-create-issue");
    assert!(
        c.contains("## Title Authoring"),
        "flow-create-issue must have a `## Title Authoring` section"
    );
    let tail = c
        .split_once("\n## Title Authoring\n")
        .map(|(_, t)| t)
        .expect("flow-create-issue must have a `## Title Authoring` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        lower.contains("plain english"),
        "`## Title Authoring` must require plain English titles"
    );
    assert!(
        lower.contains("forbid") || lower.contains("must not"),
        "`## Title Authoring` must forbid code symbols / shorthand explicitly"
    );
    let bad_idx = lower.find("bad").or_else(|| lower.find("wrong"));
    let good_idx = lower.find("good").or_else(|| lower.find("better"));
    assert!(
        bad_idx.is_some() && good_idx.is_some(),
        "`## Title Authoring` must include at least one bad/good example pair"
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
fn review_no_two_dot_diff() {
    let c = common::read_skill("flow-review");
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

// --- base_branch flows through to Phase 6 prompt and success message ---

/// flow-complete's Step 4 squash-merge prompt interpolates the
/// integration branch from `bin/flow base-branch` rather than the
/// literal `main`. A non-main-trunk repo asking the user
/// "Squash-merge into main?" misleads them about which branch the
/// merge actually targets.
#[test]
fn flow_complete_prompt_interpolates_base_branch() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("Squash-merge '<feature>' into main?"),
        "flow-complete must not hardcode `Squash-merge '<feature>' into main?` — \
         interpolate the integration branch via `<base_branch>`"
    );
    assert!(
        c.contains("<base_branch>"),
        "flow-complete must reference `<base_branch>` somewhere — \
         the prompt resolves the integration branch from `bin/flow base-branch`"
    );
}

/// flow-complete's Step 5 success message interpolates the
/// integration branch via `<base_branch>` rather than the literal
/// `main`, so a staging-trunked repo reports `merged into staging`
/// after the merge — not a misleading `merged into main`.
#[test]
fn flow_complete_success_message_interpolates_base_branch() {
    let c = common::read_skill("flow-complete");
    assert!(
        !c.contains("merged into main."),
        "flow-complete must not hardcode `merged into main.` — \
         interpolate the integration branch via `<base_branch>`"
    );

    // Bound the assertion scope to Step 5 so a stray
    // `<base_branch>` mention elsewhere cannot satisfy the check —
    // see `.claude/rules/testing-gotchas.md` Subsection-Local
    // Assertions in Contract Tests.
    let tail_at_heading = c
        .split_once("### Step 5 — Merge PR")
        .map(|(_, tail)| tail)
        .expect("Step 5 heading must exist in flow-complete SKILL.md");
    let step5 = tail_at_heading
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_heading);
    assert!(
        step5.contains("merged into <base_branch>."),
        "Step 5 must contain the literal `merged into <base_branch>.` \
         success message so a future edit cannot drop the placeholder \
         while the negative assertion above still passes"
    );
}

/// flow-start prose generalizes "Main is broken" to a base-branch-
/// neutral phrasing so a staging-trunked repo's Phase 1 messaging
/// does not name the wrong branch when the start gate fails.
#[test]
fn flow_start_prose_no_universal_main() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("Main is broken"),
        "flow-start must not hardcode `Main is broken` — generalize to \
         `the integration branch is broken` (or equivalent base-branch-neutral wording)"
    );
}

// --- base_branch flows through to Phase 4/5 diff commands ---

/// flow-review constructs the diff range from
/// `bin/flow base-branch` rather than the hardcoded `origin/main`.
/// Locks in the cross-skill contract: skills resolve the integration
/// branch via the CLI subcommand, never via a literal.
#[test]
fn flow_review_diff_uses_base_branch_subcommand() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("bin/flow base-branch") || c.contains("bin/flow\" base-branch"),
        "flow-review SKILL.md must invoke `bin/flow base-branch` to resolve the diff range"
    );
    assert!(
        !c.contains("git diff origin/main...HEAD"),
        "flow-review SKILL.md must not embed `git diff origin/main...HEAD` — \
         resolve base_branch via `bin/flow base-branch` instead"
    );
}

/// flow-learn constructs its diff range from `bin/flow base-branch`
/// rather than the hardcoded `origin/main`. Same contract as
/// `flow_review_diff_uses_base_branch_subcommand`.
#[test]
fn flow_learn_diff_uses_base_branch_subcommand() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("bin/flow base-branch") || c.contains("bin/flow\" base-branch"),
        "flow-learn SKILL.md must invoke `bin/flow base-branch` to resolve the diff range"
    );
    assert!(
        !c.contains("git diff origin/main...HEAD"),
        "flow-learn SKILL.md must not embed `git diff origin/main...HEAD` — \
         resolve base_branch via `bin/flow base-branch` instead"
    );
}

/// flow-review Step 1 derives the adversarial probe path by
/// shelling out to `bin/test --adversarial-path` and halts on
/// exit 2. The skill must NOT hardcode the canonical
/// `.flow-states/<branch>/adversarial_test` location — that location
/// lives outside the project's test tree and language test runners
/// cannot discover it, which is the underlying reason cluster B
/// (#1284 et al.) kept producing escaped probe files. The exit-2
/// halt is the fail-closed gate that stops the agent from running
/// against an unconfigured path.
#[test]
fn flow_review_step1_derives_adversarial_path_via_bin_test() {
    let c = common::read_skill("flow-review");
    // Bound the assertion to Step 1 so a future Step that
    // legitimately mentions the canonical path (e.g. a migration
    // note) does not silently satisfy the negative assertion.
    let after = c
        .split_once("## Step 1")
        .map(|(_, t)| t)
        .expect("Step 1 must exist");
    let step1 = after
        .split_once("\n## Step 2")
        .map(|(s, _)| s)
        .unwrap_or(after);

    assert!(
        step1.contains("bin/test --adversarial-path"),
        "Step 1 must invoke `bin/test --adversarial-path` to derive the probe path"
    );
    assert!(
        !step1.contains(".flow-states/<branch>/adversarial_test"),
        "Step 1 must not hardcode the canonical .flow-states/<branch>/adversarial_test path"
    );
    assert!(
        step1.contains("exit 2") || step1.contains("exits 2"),
        "Step 1 prose must name the exit-2 halt behavior"
    );
}

/// The four Review agent Input sections (reviewer, pre-mortem,
/// adversarial, documentation) describe the diff in terms of the
/// integration branch (`<base_branch>`) — not a hardcoded `origin/main`.
/// Stale Input sections mislead the agent about the diff range it
/// receives, per `.claude/rules/docs-with-behavior.md` "Agent Input
/// Section Sync".
#[test]
fn agent_diff_input_sections_reference_base_branch_not_main() {
    for agent in &[
        "reviewer.md",
        "pre-mortem.md",
        "adversarial.md",
        "documentation.md",
    ] {
        let c = common::read_agent(agent);
        assert!(
            !c.contains("git diff origin/main...HEAD"),
            "agents/{} must not describe the diff range as `git diff origin/main...HEAD` — \
             use `<base_branch>` (or equivalent placeholder) so the description matches \
             what the skill constructs at runtime",
            agent
        );
        assert!(
            c.contains("<base_branch>")
                || c.contains("base_branch")
                || c.contains("${BASE}")
                || c.contains("$BASE"),
            "agents/{} must reference `<base_branch>` (or an equivalent placeholder) when \
             describing the diff range so the Input section stays accurate when the \
             integration branch is not `main`",
            agent
        );
    }
}

// --- Git command consolidation tombstones ---

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
fn review_has_resume_check() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("Resume Check") || c.contains("## Resume"),
        "Review must have Resume Check section"
    );
}

#[test]
fn review_steps_record_completion() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("set-timestamp"),
        "Review steps must record completion via set-timestamp"
    );
}

#[test]
fn review_steps_self_invoke() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("flow:flow-review --continue-step"),
        "Review steps must self-invoke with --continue-step"
    );
}

#[test]
fn review_steps_await_background_agents() {
    let c = common::read_skill("flow-review");
    for agent in &["reviewer", "pre-mortem", "adversarial", "documentation"] {
        assert!(
            c.contains(agent),
            "Step 2 (Launch) must reference {} agent",
            agent
        );
    }
}

#[test]
fn review_has_self_invocation_check() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("Self-Invocation"),
        "Review must have Self-Invocation Check section"
    );
}

#[test]
fn review_has_bash_binflow_check() {
    let c = common::read_skill("flow-review");
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
fn start_no_flaky_test_filing() {
    let c = common::read_skill("flow-start");
    assert!(
        !c.contains("Flaky Test"),
        "Tombstone: start-gate retry was removed; Flaky Test filing branch \
         removed alongside. Must not return — re-introducing retries on the \
         integration-branch gate produces 11 minutes of identical output for \
         a deterministic failure (see start_gate.rs module doc)."
    );
    assert!(
        !c.contains("ci_flaky"),
        "Tombstone: ci_flaky status removed when start-gate retry was eliminated"
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
fn review_step_3_has_triage() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("Triage") || c.contains("triage"),
        "Step 3 (Triage) must classify findings"
    );
}

#[test]
fn review_has_supersession_check() {
    let c = common::read_skill("flow-review");
    let lower = c.to_lowercase();
    assert!(
        lower.contains("supersession"),
        "flow-review/SKILL.md Step 3 Triage must include a supersession check \
         per .claude/rules/supersession.md (Review Phase section)"
    );
}

#[test]
fn extract_helper_refactor_rule_has_expected_structure() {
    // The SKILL.md Extract-Helper Branch Enumeration subsection
    // cross-references .claude/rules/extract-helper-refactor.md for the
    // full trigger vocabulary, the three classifications, and the
    // opt-out grammar. This test asserts that rule file exists and
    // contains the canonical elements the SKILL.md cross-reference
    // promises, so a broken cross-reference or a missing section fails
    // CI instead of silently shipping.
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("extract-helper-refactor.md");
    let content = std::fs::read_to_string(&path)
        .expect(".claude/rules/extract-helper-refactor.md must exist");

    for cls in [
        "Testable via seam",
        "Testable directly",
        "Testable via subprocess",
    ] {
        assert!(
            content.contains(cls),
            "extract-helper-refactor.md must name classification: {cls}"
        );
    }

    assert!(
        content.contains("extract-helper-refactor: not-an-extraction"),
        "extract-helper-refactor.md must document the opt-out comment token \
         'extract-helper-refactor: not-an-extraction'"
    );

    // The rule file must carry the canonical section structure the
    // SKILL.md cross-reference promises. A future edit that removes
    // Why, The Rule, The Three Classifications, or Enforcement
    // leaves the rule without its substantive scaffolding; these
    // assertions fail CI on that regression.
    for section in [
        "## Vocabulary",
        "## Why",
        "## The Rule",
        "## The Three Classifications",
        "## Enforcement",
        "## Opt-Out Grammar",
        "## How to Apply",
    ] {
        assert!(
            content.contains(section),
            "extract-helper-refactor.md must contain section heading: {section}"
        );
    }

    // The canonical four-column Branch Enumeration Table must appear
    // in the rule file as the reference for Plan authors.
    assert!(
        content.contains("| Branch | Condition | Classification | Test |"),
        "extract-helper-refactor.md must include the four-column \
         Branch Enumeration Table header"
    );
}

#[test]
fn review_step_2_launches_four_agents() {
    let c = common::read_skill("flow-review");
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
fn review_no_plugin_step() {
    let c = common::read_skill("flow-review");
    let forbidden = concat!("code", "-", "review:code", "-", "review");
    assert!(
        !c.contains(forbidden),
        "Tombstone: {} plugin removed",
        forbidden
    );
}

#[test]
fn review_no_plugin_config_axis() {
    let c = common::read_skill("flow-review");
    let forbidden = concat!("code", "_", "review_plugin");
    assert!(
        !c.contains(forbidden),
        "Tombstone: {} config removed",
        forbidden
    );
}

#[test]
fn adversarial_agent_has_verify_step() {
    let c = common::read_agent("adversarial.md");
    assert!(
        c.contains("**Verify."),
        "adversarial.md Reasoning Discipline must contain a Verify step"
    );
}

#[test]
fn review_adversarial_uses_temp_test_file_placeholder() {
    // The adversarial step parameterizes the temp file path so the
    // agent can write a single test file under .flow-states/ without
    // hardcoding language. The framework concept is gone; the agent
    // picks the file extension itself by inspecting the diff.
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("<temp_test_file>"),
        "SKILL.md must parameterize the adversarial temp file path"
    );
    assert!(
        c.contains("<test_command>"),
        "SKILL.md must parameterize the adversarial test command"
    );
}

#[test]
fn adversarial_agent_uses_test_command_placeholder() {
    let c = common::read_agent("adversarial.md");
    assert!(
        c.contains("<test_command>"),
        "adversarial.md must reference <test_command> parameterized runner"
    );
}

// --- Tombstone audit fixture contamination prevention ---

/// `scan_test_files()` reads ALL `tests/*.rs` files and runs `extract_pr_numbers()`
/// on each. Literal `Tombstone:...PR #N` patterns in `tests/tombstone_audit.rs`
/// would be detected as real tombstones during `bin/flow tombstone-audit`, producing
/// phantom stale entries. The builders in that file (`tombstone_line()`, etc.)
/// construct patterns at runtime to keep the source clean.
#[test]
fn tombstone_audit_fixture_no_literal_tombstone_patterns() {
    let path = common::repo_root().join("tests/tombstone_audit.rs");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    let prs = extract_pr_numbers(&content);
    assert!(
        prs.is_empty(),
        "tests/tombstone_audit.rs contains literal tombstone patterns matching the scanner regex. \
         Found PR references: {:?}. Use the runtime builders defined in tests/tombstone_audit.rs \
         (tombstone_line, tombstone_doc_line, tombstone_str_line) instead of literal patterns \
         to avoid contaminating scan_test_files() results.",
        prs
    );
}

// --- Review tombstone audit integration ---

#[test]
fn review_mentions_tombstone_audit() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("tombstone-audit"),
        "Review Step 1 must run tombstone-audit for stale tombstone detection"
    );
}

#[test]
fn review_collects_substantive_diff() {
    let c = common::read_skill("flow-review");
    assert!(
        c.contains("git diff origin/<base_branch>...HEAD -w"),
        "Review Step 1 must collect a substantive diff \
         (`git diff origin/<base_branch>...HEAD -w`) for context-sparse agents"
    );
}

#[test]
fn review_routes_substantive_diff_to_context_sparse_agents() {
    let c = common::read_skill("flow-review");
    for agent in &["Pre-mortem", "Adversarial", "Documentation"] {
        assert!(
            c.contains("substantive diff output"),
            "Review Step 2 must route substantive diff to {} agent",
            agent
        );
    }
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

// --- File-tool preflight invariants ---
//
// Regression the two tests below guard:
//   A SKILL.md instruction writes to (or edits) a file whose target may
//   already exist on disk when the skill runs. Claude Code's Write tool
//   and Edit tool each have a Read-first-in-session preflight — Write
//   errors when the target exists and has not been Read, Edit errors
//   when any edit is attempted before a prior Read. When the preflight
//   fires mid-skill the tool call surfaces a user-visible error and the
//   flow cannot continue until the model manually works around it.
//
// Code path that produces the regression:
//   - Write side: a SKILL.md instructs the model to Write to one of the
//     persistent monitored paths (plan/DAG file, commit-msg, issue-body,
//     orchestrate queue) without first routing through the
//     `bin/flow write-rule` subcommand, whose `fs::write` call bypasses
//     the preflight.
//   - Edit side: a SKILL.md instructs the model to Edit a named plan or
//     DAG file without a preceding explicit Read-tool instruction on
//     the same file in the same `### Step` block.
//
// Consumers:
//   - Every FLOW skill that writes to `.flow-states/` or project-root
//     persistent files (flow-plan, flow-commit, flow-start, flow-code,
//     flow-learn, flow-orchestrate) relies on the Write-side invariant
//     to not block mid-phase.
//   - `flow-plan`'s plan-check fix loop relies on the Edit-side
//     invariant so the Edit tool can open the plan on re-entry.
//   - `.claude/rules/file-tool-preflights.md` authorizes the scans.

/// Target paths whose Write-tool invocations must route through
/// `bin/flow write-rule`.
///
/// Branch-scoped and literal paths only. Session-scoped `-<id>` temp files
/// used by `flow-create-issue` and `flow-decompose-project` are excluded
/// because the unique id makes cross-invocation collision unlikely.
/// Intermediate input files used BY `bin/flow write-rule` (e.g. paths
/// ending in `-content.md` that the Rust code reads and deletes) are
/// also not monitored — they are the Write-tool input, not a persistent
/// target.
const WRITE_MONITORED_PATHS: &[&str] = &[
    ".flow-states/<branch>-dag.md",
    ".flow-states/<branch>-plan.md",
    ".flow-states/<branch>-commit-msg.txt",
    ".flow-issue-body",
    "orchestrate-queue.json",
];

/// Non-blank lines of forward scan after a Write-tool instruction to
/// locate the matching `bin/flow write-rule` call. The window spans a
/// few prose lines, a description of the content, and a following bash
/// block — 30 lines covers the longest pattern in the corpus today.
const WRITE_RULE_FORWARD_WINDOW: usize = 30;

/// Check whether a monitored literal path match is bounded on BOTH sides
/// so it is not embedded in a longer unrelated path.
///
/// - Prefix boundary: the byte before `start` must not be a character
///   that would make the path a suffix of a longer path (e.g.
///   `my-orchestrate-queue.json` must not match `orchestrate-queue.json`).
/// - Suffix boundary: the byte after the match must not extend the path
///   (e.g. `.flow-issue-body-<id>` is session-scoped, out of scope;
///   `.flow-commit-msg.bak` is a different file). `.md` and `.json`
///   suffixes are themselves terminating so the check short-circuits.
fn write_path_is_bounded(haystack: &str, path: &str, start: usize) -> bool {
    let bytes = haystack.as_bytes();
    // Prefix boundary check — reject if the byte before `start` extends
    // the path (hyphen, dot, alnum, underscore).
    if start > 0 {
        let prev = bytes[start - 1];
        if prev == b'-' || prev == b'.' || prev == b'_' || prev.is_ascii_alphanumeric() {
            return false;
        }
    }
    // Suffix boundary check — file-extension suffixes are self-
    // terminating; otherwise reject byte-extensions into another path.
    if path.ends_with(".md") || path.ends_with(".json") || path.ends_with(".txt") {
        return true;
    }
    let end = start + path.len();
    match bytes.get(end) {
        Some(b) => {
            let c = *b;
            !(c == b'-' || c == b'.' || c == b'_' || c.is_ascii_alphanumeric())
        }
        None => true,
    }
}

/// Tool-instruction verb vocabulary matching the curated-closed style of
/// `.claude/rules/scope-enumeration.md`. Each verb below is a realistic
/// phrasing a skill author might use ("using the Write tool", "invoke
/// the Write tool", "call the Write tool", "run the Write tool"). Novel
/// phrasings slip through intentionally — the rule file is the primary
/// instrument; future reviewers add verbs here when a new false-negative
/// is observed.
const TOOL_VERB_PATTERN: &str = r"(?:us(?:e|ing)|invok(?:e|ing)|call(?:s|ing)?|run(?:s|ning)?)";

/// Build the phrase-detection regex for a given tool name ("Write" or
/// "Edit"). Matches, case-insensitive and with `\s+` absorbing newlines,
/// any of the verb forms above followed by "the <Tool> tool".
fn tool_phrase_regex(tool: &str) -> Regex {
    let pattern = format!(r"(?si)\b{}\s+the\s+{}\s+tool", TOOL_VERB_PATTERN, tool);
    Regex::new(&pattern).unwrap()
}

/// Walk forward-window lines and return true when any single line
/// contains BOTH `bin/flow write-rule` AND the monitored path. Requiring
/// co-occurrence on the same line closes the disconnected-substring
/// bypass (a write-rule call targeting a DIFFERENT path plus an
/// unrelated mention of the monitored path both present in the window
/// without being wired together).
fn forward_has_write_rule_line(
    lines: &[&str],
    start_idx: usize,
    end_idx: usize,
    path: &str,
) -> bool {
    let end = end_idx.min(lines.len());
    for line in &lines[start_idx..end] {
        if line.contains("bin/flow write-rule") && line.contains(path) {
            return true;
        }
    }
    false
}

#[test]
fn file_tool_preflight_write_paths_route_through_write_rule() {
    let phrase_re = tool_phrase_regex("Write");

    let skills_dir = common::skills_dir();
    let files = common::collect_md_files(&skills_dir);
    let mut violations: Vec<String> = Vec::new();

    for (rel, content) in &files {
        if !rel.ends_with("SKILL.md") {
            continue;
        }
        let lines: Vec<&str> = content.lines().collect();

        for m in phrase_re.find_iter(content) {
            let line_num = content[..m.start()].matches('\n').count() + 1;
            let idx = line_num - 1;

            // Identify the monitored target by looking at a small window
            // around the instruction (3 lines back, current, plus next
            // two) because the target path is typically mentioned on the
            // line right before the Write-tool instruction.
            let start_back = idx.saturating_sub(3);
            let end_ctx = (idx + 3).min(lines.len());
            let surrounding = lines[start_back..end_ctx].join("\n");
            let matched_path = WRITE_MONITORED_PATHS.iter().find(|p| {
                surrounding
                    .match_indices(**p)
                    .any(|(pos, _)| write_path_is_bounded(&surrounding, p, pos))
            });
            let Some(path) = matched_path else { continue };

            // Same-line co-occurrence of `bin/flow write-rule` + the
            // path inside the forward window. See `forward_has_write_rule_line`
            // for the rationale.
            let end_fwd = idx + WRITE_RULE_FORWARD_WINDOW;
            if !forward_has_write_rule_line(&lines, idx, end_fwd, path) {
                violations.push(format!(
                    "{}:{} — Write-tool instruction targets monitored path `{}` but no `bin/flow write-rule --path <...{}>` call on a single line follows within {} lines",
                    rel,
                    line_num,
                    path,
                    path,
                    WRITE_RULE_FORWARD_WINDOW,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "SKILL.md Write-tool instructions target paths that may pre-exist and trip Claude Code's Write preflight, but do not route through `bin/flow write-rule`. See `.claude/rules/file-tool-preflights.md`:\n{}",
        violations.join("\n")
    );
}

/// Named plan-file paths whose Edit-tool invocations must be preceded
/// by an explicit Read-tool instruction. The Edit tool's preflight
/// ("You must use your Read tool at least once in the conversation
/// before editing") fires when the model has not naturally Read the
/// file in the current turn — for example, re-entering the plan-check
/// fix loop after a `--continue-step` resume.
const EDIT_MONITORED_PATHS: &[&str] = &[
    ".flow-states/<branch>-plan.md",
    ".flow-states/<branch>-dag.md",
];

/// Non-blank lines backward from an Edit-tool instruction to look for
/// a paired Read-tool instruction on the same path. Twelve lines covers
/// a short prose preamble plus an intervening bash fence. The scan
/// stops at any `### Step N`, `### ` subsection, or `## Section`
/// heading encountered during the walk so a Read in a prior step does
/// not credit an Edit in a later step (a `--continue-step` re-entry
/// would invalidate the prior Read).
const EDIT_READ_BACKWARD_WINDOW: usize = 12;

/// Walk backward from `idx` up to `window` non-blank lines, stopping at
/// any Markdown heading line (`## ` or `### `). Returns the slice of
/// lines between the first encountered boundary and `idx` (inclusive)
/// as a joined string. Callers then scan the returned slice for a
/// Read-tool instruction co-occurring with the monitored path on the
/// SAME line.
fn backward_read_window(lines: &[&str], idx: usize, window: usize) -> String {
    let mut start = idx;
    let mut taken = 0;
    while start > 0 && taken < window {
        start -= 1;
        let trimmed = lines[start].trim_start();
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            // Include the heading itself for context, then stop.
            break;
        }
        taken += 1;
    }
    lines[start..=idx.min(lines.len().saturating_sub(1))].join("\n")
}

#[test]
fn file_tool_preflight_edit_paths_preceded_by_read() {
    let phrase_re = tool_phrase_regex("Edit");
    // Require an explicit Read-tool instruction ("use/using/invoke/
    // invoking/call/run the Read tool"). Prose like "Read the plan"
    // or "Read the current state" no longer counts — the preflight
    // requires an actual Read tool call, and the scanner must match
    // that discipline.
    let read_re = tool_phrase_regex("Read");

    let skills_dir = common::skills_dir();
    let files = common::collect_md_files(&skills_dir);
    let mut violations: Vec<String> = Vec::new();

    for (rel, content) in &files {
        if !rel.ends_with("SKILL.md") {
            continue;
        }
        let lines: Vec<&str> = content.lines().collect();

        for m in phrase_re.find_iter(content) {
            let line_num = content[..m.start()].matches('\n').count() + 1;
            let idx = line_num - 1;

            // Window around the Edit instruction to find a monitored path.
            let start_back = idx.saturating_sub(3);
            let end_ctx = (idx + 3).min(lines.len());
            let surrounding = lines[start_back..end_ctx].join("\n");
            let matched_path = EDIT_MONITORED_PATHS.iter().find(|p| {
                surrounding
                    .match_indices(**p)
                    .any(|(pos, _)| write_path_is_bounded(&surrounding, p, pos))
            });
            let Some(path) = matched_path else { continue };

            // Step-scoped backward window (stops at `### Step N` /
            // `## Section` headings) so a Read in a prior step cannot
            // credit an Edit in a later step.
            let backward = backward_read_window(&lines, idx, EDIT_READ_BACKWARD_WINDOW);

            // Require both the Read-tool phrase and the path somewhere
            // in the window. Perfect same-line co-occurrence is too
            // strict for Edit (skill authors often phrase the Read on
            // one line and identify the path on an adjacent line).
            let has_read = read_re.is_match(&backward) && backward.contains(*path);
            if !has_read {
                violations.push(format!(
                    "{}:{} — Edit-tool instruction on monitored path `{}` but no `Read` tool instruction on the same path in the preceding {} lines (scan stops at `## ` / `### ` headings)",
                    rel,
                    line_num,
                    path,
                    EDIT_READ_BACKWARD_WINDOW,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "SKILL.md Edit-tool instructions on named plan/DAG files must be preceded by an explicit Read-tool instruction to satisfy Claude Code's Edit preflight. See `.claude/rules/file-tool-preflights.md`:\n{}",
        violations.join("\n")
    );
}

// --- flow-reset SKILL.md delegates to bin/flow cleanup --all ---
//
// The flow-reset skill is a thin wrapper around `bin/flow cleanup --all`.
// The contract test below locks in the canonical delegation: the skill
// must invoke both the dry-run inventory form (Step 1) and the live
// execute form (Step 3). If either is missing, the skill cannot fulfil
// its purpose.

#[test]
fn flow_reset_invokes_cleanup_all_dry_run_and_live() {
    let content = common::read_skill("flow-reset");
    assert!(
        content.contains("${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all --dry-run"),
        "skills/flow-reset/SKILL.md must invoke `cleanup . --all --dry-run` (Step 1 inventory)"
    );
    // Live invocation must NOT carry --dry-run on the same line.
    let live_present = content.lines().any(|line| {
        line.contains("${CLAUDE_PLUGIN_ROOT}/bin/flow cleanup . --all")
            && !line.contains("--dry-run")
    });
    assert!(
        live_present,
        "skills/flow-reset/SKILL.md must invoke `cleanup . --all` without --dry-run (Step 3 execute)"
    );
}

// --- assess-issues rule content contracts ---
//
// `.claude/rules/assess-issues.md` is the rule the issue-triage agent
// depends on. The three contracts below lock in (a) the canonical
// "code actually does" phrasing against the historical `issue actually
// does` typo shape, (b) the unreferenced-files coverage bullet, and
// (c) the `gh pr list --search` / `git log --grep` shipped-but-not-
// closed investigation move. Each test guards a distinct regression:
// a deletion of any of the three lines fails CI immediately.

fn read_assess_issues_rule() -> String {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("assess-issues.md");
    std::fs::read_to_string(&path).expect(".claude/rules/assess-issues.md must exist")
}

#[test]
fn test_assess_issues_rule_has_no_typo() {
    let content = read_assess_issues_rule();
    assert!(
        content.contains("what the existing code\nactually does")
            || content.contains("what the existing code actually does")
            || content.contains("the code actually does"),
        ".claude/rules/assess-issues.md must phrase the comparison as 'what the (existing) code actually does'"
    );
    assert!(
        !content.contains("what the issue actually does"),
        ".claude/rules/assess-issues.md must NOT contain the typo 'what the issue actually does'"
    );
}

#[test]
fn test_assess_issues_rule_covers_unreferenced_files() {
    let content = read_assess_issues_rule();
    assert!(
        content.contains("If the issue names no files"),
        ".claude/rules/assess-issues.md must cover the unreferenced-files case starting with 'If the issue names no files'"
    );
    assert!(
        content.contains("search the codebase for the behavior"),
        ".claude/rules/assess-issues.md must instruct searching the codebase for the described behavior when no files are referenced"
    );
}

#[test]
fn test_assess_issues_rule_includes_pr_search_step() {
    let content = read_assess_issues_rule();
    assert!(
        content.contains("gh pr list --search"),
        ".claude/rules/assess-issues.md must instruct checking `gh pr list --search` for already-shipped work"
    );
    assert!(
        content.contains("git log --all --grep"),
        ".claude/rules/assess-issues.md must instruct checking `git log --all --grep` for already-shipped work"
    );
}

// --- flow-triage-issue skill content contracts ---
//
// `skills/flow-triage-issue/SKILL.md` is a thin dispatcher. The three
// contracts below lock in (a) the no-side-effects HARD-GATE that
// forbids auto-close, auto-label, auto-comment, and auto-skill
// invocation; (b) the canonical 4-disposition closed set; and (c) the
// dispatch target — the issue-triage sub-agent, never general-purpose
// or any other agent. Each test guards a distinct regression: a
// missing HARD-GATE invites side-effect creep, a drifted disposition
// set invents new outcomes the agent never produces, and a wrong
// dispatch target sends the model into an unbounded sub-agent.

/// Extract the body of the FIRST `<HARD-GATE>...</HARD-GATE>` block
/// in the SKILL.md so contract assertions about HARD-GATE content
/// can be bound to the gate scope rather than satisfied by passive
/// prose anywhere in the file. Returns the inner content of the
/// block (between the opening and closing tags). Asserts that both
/// tags exist in the file. Panics if the block is malformed.
fn extract_hard_gate_block(content: &str) -> String {
    let open = content
        .find("<HARD-GATE>")
        .expect("skills/flow-triage-issue/SKILL.md must contain <HARD-GATE> opening tag");
    let after_open = open + "<HARD-GATE>".len();
    let close_offset = content[after_open..]
        .find("</HARD-GATE>")
        .expect("skills/flow-triage-issue/SKILL.md must contain </HARD-GATE> closing tag");
    content[after_open..after_open + close_offset].to_string()
}

#[test]
fn test_flow_triage_issue_skill_has_no_side_effects_hard_gate() {
    let content = common::read_skill("flow-triage-issue");
    // Bind the assertions to the actual <HARD-GATE>...</HARD-GATE>
    // block so prose elsewhere in the file cannot satisfy the
    // checks (per adversarial findings A2/A6/A9/A12/A13/A16).
    let gate = extract_hard_gate_block(&content);
    let gate_lower = gate.to_lowercase();
    for forbidden in ["auto-close", "auto-label", "auto-comment"] {
        assert!(
            gate_lower.contains(forbidden),
            "skills/flow-triage-issue/SKILL.md HARD-GATE block must explicitly forbid {forbidden}"
        );
    }
    // The "never close" prohibition must live inside the HARD-GATE
    // block so a removed or empty gate fails the test.
    assert!(
        gate_lower.contains("close") && gate_lower.contains("not"),
        "skills/flow-triage-issue/SKILL.md HARD-GATE block must forbid closing issues"
    );
    // The "no auto-invocation of skills" prohibition must live
    // inside the HARD-GATE block.
    assert!(
        gate_lower.contains("invoke any skill") || gate_lower.contains("auto-invocation"),
        "skills/flow-triage-issue/SKILL.md HARD-GATE block must forbid Skill tool invocation after rendering the verdict"
    );
}

#[test]
fn test_flow_triage_issue_skill_disposition_set_is_canonical() {
    let content = common::read_skill("flow-triage-issue");
    let lower = content.to_lowercase();
    // Canonical two must be present.
    for disposition in ["close", "decompose"] {
        assert!(
            content.contains(disposition),
            "skills/flow-triage-issue/SKILL.md must enumerate disposition: {disposition}"
        );
    }
    // Closed-set check: extract every quoted token inside
    // backticks that follows a `**` disposition-marker pattern in
    // the Step 5 hint section. The Step 5 hint enumerates one
    // bullet per allowed disposition (`**close**`, `**decompose**`,
    // `**Out of scope**`). Any additional `**<token>**` bullet
    // inside the HARD-GATE disposition list is an unsanctioned
    // extension. Locks the closed set to two canonical dispositions
    // plus the out-of-scope envelope label.
    let gate = extract_hard_gate_block(&content);
    let bullet_re = regex::Regex::new(r"(?m)^- \*\*([a-zA-Z][a-zA-Z0-9 -]*)\*\*")
        .expect("disposition bullet regex");
    let mut bullet_tokens: Vec<String> = bullet_re
        .captures_iter(&gate)
        .map(|cap| cap[1].trim().to_lowercase())
        .collect();
    bullet_tokens.sort();
    bullet_tokens.dedup();
    let allowed: std::collections::HashSet<&str> =
        ["close", "decompose", "out of scope"].into_iter().collect();
    for token in &bullet_tokens {
        assert!(
            allowed.contains(token.as_str()),
            "skills/flow-triage-issue/SKILL.md HARD-GATE enumerates unsanctioned disposition bullet: {token:?}. The closed set is exactly {{close, decompose}} plus the Out-of-scope envelope."
        );
    }
    // Defense in depth: forbid common alternative tokens
    // anywhere outside fenced code blocks. Use word-boundary
    // shape so legitimate prose like "decompose" doesn't false-
    // match. Alternative tokens are never names of valid
    // dispositions in this v1 — their presence in body prose
    // signals drift even if the test above passed because the
    // bullet list was unchanged.
    let forbidden_re = regex::Regex::new(
        r"(?i)\b(wontfix|won't fix|stale|invalid|reopened|pending|wip|needs[- ]info)\b",
    )
    .expect("forbidden disposition regex");
    if let Some(m) = forbidden_re.find(&lower) {
        panic!(
            "skills/flow-triage-issue/SKILL.md must NOT mention forbidden alternative disposition token: {:?}",
            m.as_str()
        );
    }
}

#[test]
fn test_flow_triage_issue_skill_dispatches_issue_triage_agent() {
    let content = common::read_skill("flow-triage-issue");
    // The skill MUST dispatch issue-triage in its Step 2 dispatch
    // instruction. Bind the check to the dispatch-instruction
    // context (the line containing "Invoke the" + "sub-agent")
    // so prose mentions of `issue-triage` elsewhere in the file
    // cannot satisfy the assertion (per reviewer finding R3 and
    // adversarial test verification).
    let dispatch_line_present = content
        .lines()
        .any(|line| line.contains("issue-triage") && line.contains("sub-agent"));
    assert!(
        dispatch_line_present,
        "skills/flow-triage-issue/SKILL.md must contain a dispatch instruction line that names the `issue-triage` sub-agent (e.g. 'Invoke the `issue-triage` sub-agent ...')"
    );
    // The skill must NOT route through general-purpose — that agent
    // ignores tool restrictions in its prompt and is forbidden during
    // active flows by .claude/rules/skill-authoring.md "Sub-Agent Safety".
    assert!(
        !content.contains("general-purpose"),
        "skills/flow-triage-issue/SKILL.md must NOT use general-purpose sub-agent"
    );
}

#[test]
fn issue_triage_agent_declares_end_of_findings_marker() {
    // Per .claude/rules/cognitive-isolation.md "Completion-marker
    // contract": every high-investigation agent must declare
    // `## END-OF-FINDINGS` as the final structural element of its
    // Output Format. Adversarial findings A7/A11/A14 demonstrated
    // that the SKILL.md's `### Verdict` substring check is gameable
    // by echoed instruction templates; the END-OF-FINDINGS marker
    // is the canonical truncation signal.
    let path = common::repo_root().join("agents").join("issue-triage.md");
    let content = std::fs::read_to_string(&path).expect("agents/issue-triage.md must exist");
    assert!(
        content.contains("## END-OF-FINDINGS"),
        "agents/issue-triage.md must declare the literal `## END-OF-FINDINGS` completion marker"
    );
}

#[test]
fn test_flow_triage_issue_skill_applies_triage_in_progress_label() {
    let content = common::read_skill("flow-triage-issue");
    assert!(
        content.contains(r#"--add-label "Triage In-Progress""#),
        "skills/flow-triage-issue/SKILL.md must apply the Triage In-Progress \
         label at the start of triage via `gh issue edit ... --add-label \
         \"Triage In-Progress\"`"
    );
}

#[test]
fn test_flow_triage_issue_skill_removes_triage_in_progress_label() {
    let content = common::read_skill("flow-triage-issue");
    assert!(
        content.contains(r#"--remove-label "Triage In-Progress""#),
        "skills/flow-triage-issue/SKILL.md must remove the Triage In-Progress \
         label before the COMPLETE banner via `gh issue edit ... --remove-label \
         \"Triage In-Progress\"`"
    );
}

// --- flow-skills coverage and admin/maintainer membership ---

#[test]
fn flow_skills_lists_every_skill_exactly_once() {
    // Named regression: a new skill is added under `skills/<name>/`
    // OR `.claude/skills/<name>/` but `flow-skills` SKILL.md is
    // not updated, so `/flow:flow-skills` shows a stale list.
    // Named consumer: the user typing `/flow:flow-skills`. Each
    // skill must appear exactly once across the bucket tables,
    // formatted as either `` `/flow:<name>` `` for plugin skills
    // or `` `/<name>` `` for maintainer-private skills under
    // `.claude/skills/`.
    let content = common::read_skill("flow-skills");
    let mut expected: HashSet<String> = common::all_skill_names().into_iter().collect();
    let claude_skills_dir = common::repo_root().join(".claude").join("skills");
    if let Ok(entries) = fs::read_dir(&claude_skills_dir) {
        for entry in entries.flatten() {
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            expected.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    assert!(
        !expected.is_empty(),
        "expected skill universe must not be empty; check skills/ and .claude/skills/"
    );
    for name in &expected {
        let primary = format!("`/flow:{}`", name);
        let alt = format!("`/{}`", name);
        let count = content.matches(&primary).count() + content.matches(&alt).count();
        assert_eq!(
            count, 1,
            "skills/flow-skills/SKILL.md must reference {} exactly once across its bucket tables (found {})",
            name, count
        );
    }
}

#[test]
fn flow_skills_admin_and_maintainer_match_user_only() {
    // Named regression: `USER_ONLY_SKILLS` in
    // `src/hooks/transcript_walker.rs` is edited (skill added or
    // removed) but `flow-skills` SKILL.md is not updated, so the
    // documentation drifts from mechanical enforcement. Named
    // consumer: the user typing `/flow:flow-skills` to learn which
    // skills are user-only.
    //
    // Section assertions are bounded via line-anchored heading
    // search and a same-or-higher-level end marker per
    // `.claude/rules/testing-gotchas.md` "Subsection-Local
    // Assertions in Contract Tests" so the slice covers ONLY the
    // headed subsection — not the entire remainder of the file.
    let walker = common::repo_root()
        .join("src")
        .join("hooks")
        .join("transcript_walker.rs");
    let walker_src =
        std::fs::read_to_string(&walker).expect("src/hooks/transcript_walker.rs must exist");

    // Extract USER_ONLY_SKILLS members from the constant literal.
    let const_tail = walker_src
        .split_once("pub const USER_ONLY_SKILLS:")
        .map(|(_, tail)| tail)
        .expect("USER_ONLY_SKILLS constant must exist");
    let const_body = const_tail
        .split_once("];")
        .map(|(body, _)| body)
        .expect("USER_ONLY_SKILLS constant must close with `];`");
    let entry_re = Regex::new(r#""(flow:flow-[a-z0-9-]+)""#).expect("regex must compile");
    let user_only_entries: Vec<String> = entry_re
        .captures_iter(const_body)
        .map(|c| c[1].to_string())
        .collect();
    assert!(
        !user_only_entries.is_empty(),
        "expected USER_ONLY_SKILLS to declare at least one entry"
    );

    let content = common::read_skill("flow-skills");

    // Bound the slice to the `heading` subsection only. `heading`
    // is the FULL heading line (e.g. `#### Admin`); the start is
    // line-anchored so `### Admin` cannot substring-match into
    // `#### Admin`. The end is the earliest occurrence of any
    // heading marker (`## `, `### `, or `#### `) at the start of
    // a subsequent line, so a level-4 subsection ends at the next
    // level-4 heading even when no level-2 or level-3 heading
    // appears before EOF.
    fn subsection<'a>(content: &'a str, heading: &str) -> &'a str {
        let needle = format!("\n{}\n", heading);
        let tail = content
            .split_once(&needle)
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("flow-skills SKILL.md missing heading `{}`", heading));
        let mut end = tail.len();
        for marker in &["\n## ", "\n### ", "\n#### "] {
            if let Some((before, _)) = tail.split_once(marker) {
                if before.len() < end {
                    end = before.len();
                }
            }
        }
        &tail[..end]
    }

    let admin_section = subsection(&content, "#### Admin");
    let maintainer_section = subsection(&content, "#### Maintainer");

    for entry in &user_only_entries {
        let bare = entry.strip_prefix("flow:").unwrap_or(entry.as_str());
        if entry == "flow:flow-release" {
            assert!(
                maintainer_section.contains(bare),
                "Maintainer section of skills/flow-skills/SKILL.md must reference `{}`",
                bare
            );
        } else {
            assert!(
                admin_section.contains(bare),
                "Admin section of skills/flow-skills/SKILL.md must reference `{}`",
                bare
            );
        }
    }
}

// --- no-backwards-reasoning rule + skill scans ---

/// The four canonical scan phrasings the SKILL bodies enumerate. Each phrase
/// represents a distinct backward-facing reasoning shape; the rule must
/// retain the body content that authorizes the scans.
const SCAN_PHRASINGS: &[&str] = &[
    "PR #<N> decided",
    "kept for backward compatibility",
    "older plugin versions",
    "as PR #<N> chose to",
];

#[test]
fn no_backwards_reasoning_rule_states_current_merits_principle() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("no-backwards-reasoning.md");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "expected `.claude/rules/no-backwards-reasoning.md` to exist: {}",
            e
        )
    });

    assert!(
        content.contains("current merits"),
        "rule must state the load-bearing `current merits` invariant phrase"
    );
    assert!(
        content.contains("plugin version"),
        "rule must explicitly cover the plugin-version-compat sub-case"
    );

    const FORBIDDEN_PATTERN_KEYWORDS: &[&str] = &[
        "commit message",
        "PR description",
        "doc comment",
        "git log",
        "git blame",
    ];
    let hits: Vec<&&str> = FORBIDDEN_PATTERN_KEYWORDS
        .iter()
        .filter(|k| content.contains(**k))
        .collect();
    assert!(
        hits.len() >= 3,
        "rule must enumerate at least three forbidden-pattern keywords from {:?}; found {:?}",
        FORBIDDEN_PATTERN_KEYWORDS,
        hits
    );

    for phrase in SCAN_PHRASINGS {
        assert!(
            content.contains(phrase),
            "rule must enumerate the SKILL scan phrasing `{}` so the rule remains the authoritative source for what the scans target",
            phrase
        );
    }
}

#[test]
fn flow_create_issue_skill_has_pre_draft_backwards_reasoning_scan() {
    let content = common::read_skill("flow-create-issue");

    let transform_idx = content
        .find("## Transform + Draft")
        .expect("flow-create-issue SKILL.md missing `## Transform + Draft`");
    let scan_idx = content.find("Pre-Draft Backwards-Reasoning Scan").expect(
        "flow-create-issue SKILL.md missing `Pre-Draft Backwards-Reasoning Scan` subsection",
    );
    let draft_idx = content
        .find("### Draft Presentation")
        .expect("flow-create-issue SKILL.md missing `### Draft Presentation`");

    assert!(
        content.contains(".claude/rules/no-backwards-reasoning.md"),
        "Pre-Draft scan must cross-reference `.claude/rules/no-backwards-reasoning.md`"
    );
    assert!(
        transform_idx < scan_idx,
        "Pre-Draft scan must appear AFTER `## Transform + Draft`"
    );
    assert!(
        scan_idx < draft_idx,
        "Pre-Draft scan must appear BEFORE `### Draft Presentation`"
    );

    // Bound to the scan's body so a future drift cannot leave a stub heading
    // and move the body elsewhere. The scan body sits between its `### `
    // heading and the next `### ` (or the next `## `) heading.
    let scan_tail = &content[scan_idx..];
    let after_heading = scan_tail
        .split_once('\n')
        .map(|(_, t)| t)
        .expect("scan heading must be followed by content");
    let mut body_end = after_heading.len();
    for marker in &["\n### ", "\n## "] {
        if let Some((before, _)) = after_heading.split_once(marker) {
            if before.len() < body_end {
                body_end = before.len();
            }
        }
    }
    let scan_body = &after_heading[..body_end];
    for phrase in SCAN_PHRASINGS {
        assert!(
            scan_body.contains(phrase),
            "Pre-Draft scan body must enumerate the canonical scan phrasing `{}` (a stub heading without the body content does not satisfy the contract)",
            phrase
        );
    }
}

#[test]
fn flow_decompose_project_skill_has_backwards_reasoning_scan() {
    let content = common::read_skill("flow-decompose-project");

    assert!(
        content.contains("Backwards-Reasoning Scan"),
        "flow-decompose-project SKILL.md must include a `Backwards-Reasoning Scan` step"
    );
    assert!(
        content.contains(".claude/rules/no-backwards-reasoning.md"),
        "scan step must cross-reference `.claude/rules/no-backwards-reasoning.md`"
    );

    let scan_idx = content
        .find("Backwards-Reasoning Scan")
        .expect("scan heading checked above");
    let present_idx = content
        .find("Present the full issue list")
        .expect("flow-decompose-project must contain `Present the full issue list` where children are surfaced");
    assert!(
        scan_idx < present_idx,
        "Backwards-Reasoning Scan must appear BEFORE child issues are presented"
    );

    // Bound to the scan's body so a future drift cannot leave a stub heading
    // and move the body content elsewhere.
    let scan_tail = &content[scan_idx..];
    let after_heading = scan_tail
        .split_once('\n')
        .map(|(_, t)| t)
        .expect("scan heading must be followed by content");
    let mut body_end = after_heading.len();
    for marker in &["\n### ", "\n## "] {
        if let Some((before, _)) = after_heading.split_once(marker) {
            if before.len() < body_end {
                body_end = before.len();
            }
        }
    }
    let scan_body = &after_heading[..body_end];
    for phrase in SCAN_PHRASINGS {
        assert!(
            scan_body.contains(phrase),
            "Backwards-Reasoning Scan body must enumerate the canonical scan phrasing `{}` (a stub heading without the body content does not satisfy the contract)",
            phrase
        );
    }
}

// --- include-bias-in-issues rule content contract ---
//
// The contract test below pins four load-bearing invariants in
// the rule body so a future paraphrase or refactor cannot
// silently drop them or invert their meaning: the structural
// shape of the principle (bold opening sentence, not a quoted
// or negated phrase), the bad-reasoning patterns enumeration,
// the lifecycle-cost framing, and the absence of inversion
// vocabulary. The fourth assertion blocks the inversion bypass
// where every required substring is present in a context that
// negates the principle.

fn read_include_bias_rule() -> String {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("include-bias-in-issues.md");
    std::fs::read_to_string(&path).expect(".claude/rules/include-bias-in-issues.md must exist")
}

#[test]
fn include_bias_rule_states_default_to_inclusion() {
    let content = read_include_bias_rule();

    // Structural shape: the principle MUST appear as a bold
    // opening sentence (`**Default to inclusion ...`), not as a
    // quoted or negated phrase. The bold form is the prescriptive
    // shape; a plain reference inside a sentence does not lock
    // the rule's intent.
    assert!(
        content.contains("**Default to inclusion"),
        ".claude/rules/include-bias-in-issues.md must state the principle as a bold prescriptive opening (`**Default to inclusion ...`), not as a quoted or referenced phrase"
    );

    let bad_patterns: &[&str] = &[
        "prior PR did",
        "user owns this",
        "separate code surface",
        "would expand scope",
    ];
    let hits = bad_patterns.iter().filter(|p| content.contains(*p)).count();
    assert!(
        hits >= 3,
        ".claude/rules/include-bias-in-issues.md must enumerate at least three of four bad-reasoning patterns ({:?}); found {}",
        bad_patterns,
        hits
    );

    assert!(
        content.contains("lifecycle cost"),
        ".claude/rules/include-bias-in-issues.md must include the 'lifecycle cost' framing"
    );

    // Inversion guard: the rule MUST NOT contain any phrasing
    // that negates the principle. A future rewrite that keeps
    // every required substring while flipping the meaning would
    // satisfy the substring assertions above; this list locks
    // out the canonical inversions.
    let inversion_patterns: &[&str] = &[
        "Default to inclusion is wrong",
        "Default to inclusion is the wrong",
        "Default to inclusion is incorrect",
        "Default to exclusion",
        "Defer aggressively",
        "Bad Reasoning Patterns Are Actually Good",
    ];
    for inversion in inversion_patterns {
        assert!(
            !content.contains(inversion),
            ".claude/rules/include-bias-in-issues.md must not contain inversion phrase `{}` — the rule's principle prescribes inclusion, not exclusion",
            inversion
        );
    }
}

// --- persistence-routing rule invariant ---
//
// `validate-claude-paths` block message points the model at this rule
// when an Edit/Write under `~/.claude/projects/` is rejected. The
// message asserts the rule names "Rules are the default" and
// "Memory is the exception" — locking in those two phrases as the
// load-bearing invariants future readers consult when deciding where
// to persist a behavioral constraint vs. user preference.

#[test]
fn persistence_routing_rule_states_rules_are_default() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("persistence-routing.md");
    let content =
        std::fs::read_to_string(&path).expect(".claude/rules/persistence-routing.md must exist");
    assert!(
        content.contains("Rules are the default"),
        ".claude/rules/persistence-routing.md must state 'Rules are the default' as the load-bearing invariant"
    );
    assert!(
        content.contains("Memory is the exception"),
        ".claude/rules/persistence-routing.md must state 'Memory is the exception' as the corollary"
    );
}
