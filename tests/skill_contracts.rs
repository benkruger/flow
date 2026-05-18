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

#[test]
fn test_each_agent_frontmatter_has_rationale_comment() {
    // The `model:` value in every agent's frontmatter must be preceded
    // by a YAML comment line that names the tier and a one-sentence
    // rationale (e.g. `# Opus: Reasoning depth is the job.`). The tier
    // name in the comment must match the `model:` value so a future
    // edit that changes one half without the other is caught at CI.
    //
    // The scan is bounded to the YAML frontmatter (lines between the
    // opening `---` on line 0 and the next `---` line) so a `model:`
    // line in the agent's body prose cannot mask its actual
    // frontmatter value.
    let model_re = Regex::new(r"^model: (opus|sonnet|haiku)\s*$").unwrap();
    let comment_re = Regex::new(r"^# (Opus|Sonnet|Haiku): .+\.$").unwrap();
    for filename in agent_files() {
        let content = common::read_agent(&filename);
        let lines: Vec<&str> = content.lines().collect();
        assert!(
            lines.first() == Some(&"---"),
            "{} must open with YAML frontmatter delimiter '---'",
            filename
        );
        let frontmatter_end = lines
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, line)| **line == "---")
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("{} missing closing '---' frontmatter delimiter", filename));
        let (model_idx, model_value) = lines[1..frontmatter_end]
            .iter()
            .enumerate()
            .find_map(|(i, line)| model_re.captures(line).map(|c| (i + 1, c[1].to_string())))
            .unwrap_or_else(|| panic!("{} missing 'model: <tier>' line in frontmatter", filename));
        assert!(
            model_idx > 1,
            "{} has 'model:' immediately after opening '---' — no preceding line for rationale comment",
            filename
        );
        let prev = lines[model_idx - 1];
        let cap = comment_re.captures(prev).unwrap_or_else(|| {
            panic!(
                "{} line preceding 'model:' must be '# <Tier>: <sentence>.' — got: {:?}",
                filename, prev
            )
        });
        let comment_tier = cap[1].to_ascii_lowercase();
        assert_eq!(
            comment_tier, model_value,
            "{} rationale comment tier ({}) does not match model value ({})",
            filename, &cap[1], model_value
        );
    }
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

// --- Planning-tier agent contracts ---
//
// Three planning-tier sub-agents — `agents/pm.md`,
// `agents/tech-lead.md`, `agents/cto.md` — represent concentric
// scope authority: PM authorizes copy/content/small changes,
// Tech Lead authorizes changes within current architecture and
// design patterns, CTO authorizes novel work and is the
// escalation terminus. The tests below lock in the scope
// boundary, the structured `## SCOPE REFUSAL` escalation
// protocol, and (for Tech Lead) the Reasoning Discipline
// section per `.claude/rules/semi-formal-reasoning.md`. Per-file
// siblings rather than a single coordinated test because each
// agent's regression is independent: weakening one boundary,
// dropping one refusal template, or accidentally adding a
// refusal to CTO each break a distinct invariant.
//
// Section-bounded assertion helpers (refusal_section,
// section_by_heading) anchor on line-level heading matches rather
// than substring scans. Prose mentions of the literal heading
// text (inline backticked references explaining what the section
// emits) must not satisfy the assertion. Per
// `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
// in Contract Tests," section-scoped tests bound the search to
// the heading-to-next-heading slice rather than walking the file
// or taking a fixed-line window.

/// Return the body of a Markdown section beginning at the line
/// whose trimmed content equals `heading`. The body extends to the
/// next top-or-same-level heading (`## `) line. Returns `None` if
/// no line in `content` exactly equals `heading` after trimming
/// trailing whitespace — a prose mention of the heading text
/// (inline backticked reference, code-block example) does not
/// satisfy the match.
fn section_by_heading<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let mut idx = 0usize;
    for line in content.split_inclusive('\n') {
        if line.trim_end() == heading {
            let start = idx + line.len();
            // Scan forward for the next "## " heading line; the
            // section ends just before that line.
            let tail = &content[start..];
            let mut local = 0usize;
            for next_line in tail.split_inclusive('\n') {
                let trimmed = next_line.trim_end_matches('\n');
                if trimmed.starts_with("## ") {
                    return Some(&tail[..local]);
                }
                local += next_line.len();
            }
            return Some(tail);
        }
        idx += line.len();
    }
    None
}

/// Convenience: return the body of an agent's `## SCOPE REFUSAL`
/// section. Anchors on a line-level heading match so prose
/// mentions cannot satisfy the lookup.
fn refusal_section(content: &str) -> Option<&str> {
    section_by_heading(content, "## SCOPE REFUSAL")
}

#[test]
fn agents_planning_have_scope_section() {
    for agent in &["pm.md", "tech-lead.md", "cto.md"] {
        let c = common::read_agent(agent);
        let has_heading = c.lines().any(|l| l.trim_end() == "## Scope");
        assert!(
            has_heading,
            "agents/{} must declare a `## Scope` heading on its own line naming the boundary of work it authorizes",
            agent
        );
    }
}

#[test]
fn agents_planning_pm_refuses_with_template_naming_tech_lead() {
    let c = common::read_agent("pm.md");
    let section = refusal_section(&c).expect(
        "agents/pm.md must contain a `## SCOPE REFUSAL` heading on its own line naming the Tech Lead escalation target",
    );
    assert!(
        section.contains("**Escalate to:** Tech Lead"),
        "agents/pm.md `## SCOPE REFUSAL` section must contain the canonical `**Escalate to:** Tech Lead` bullet — section body checked: {}",
        section
    );
}

#[test]
fn agents_planning_tech_lead_refuses_with_template_naming_cto() {
    let c = common::read_agent("tech-lead.md");
    let section = refusal_section(&c).expect(
        "agents/tech-lead.md must contain a `## SCOPE REFUSAL` heading on its own line naming the CTO escalation target",
    );
    assert!(
        section.contains("**Escalate to:** CTO"),
        "agents/tech-lead.md `## SCOPE REFUSAL` section must contain the canonical `**Escalate to:** CTO` bullet — section body checked: {}",
        section
    );
}

#[test]
fn agents_planning_cto_is_escalation_terminus() {
    let c = common::read_agent("cto.md");
    let has_heading = c.lines().any(|l| l.trim_end() == "## SCOPE REFUSAL");
    assert!(
        !has_heading,
        "agents/cto.md must NOT contain a `## SCOPE REFUSAL` heading on its own line — CTO is the escalation terminus, the buck stops there. Prose mentions of the literal text are allowed (e.g. explaining what sibling tiers emit); only a real heading is forbidden."
    );
}

#[test]
fn agents_planning_tech_lead_uses_reasoning_discipline() {
    let c = common::read_agent("tech-lead.md");
    let section = section_by_heading(&c, "## Reasoning Discipline").expect(
        "agents/tech-lead.md must declare a `## Reasoning Discipline` heading on its own line (per .claude/rules/semi-formal-reasoning.md) since its findings reason about code behavior",
    );
    for term in ["Premise", "Trace", "Conclude"] {
        assert!(
            section.contains(term),
            "agents/tech-lead.md `## Reasoning Discipline` section must contain `{}` (Premise -> Trace -> Conclude template) — section body checked: {}",
            term,
            section
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

/// Tombstone-style guard against reintroducing a hardcoded `main`
/// literal in user-visible flow-reset prose. Parallels
/// `flow_start_prose_no_universal_main`. The skill runs from any cwd
/// via `${CLAUDE_PLUGIN_ROOT}/bin/reset` and names no integration
/// branch in user-visible prose; a regression that re-introduced a
/// hardcoded `main` literal would mis-document the skill's scope on
/// staging/develop/master-trunked repos.
#[test]
fn reset_no_universal_main() {
    let c = common::read_skill("flow-reset");
    assert!(
        !c.contains("Must be on main"),
        "flow-reset must not hardcode `Must be on main` — the skill \
         runs from any cwd via `${{CLAUDE_PLUGIN_ROOT}}/bin/reset`"
    );
    assert!(
        !c.contains("Available from `main` only"),
        "flow-reset Rules must not name `main` as the only valid branch \
         — the skill runs from any cwd"
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

/// flow-learn Step 2 declares `correction` notes mandatory user
/// directives that always route to a durable rule. Regression guard:
/// an edit removing the mandatory-routing instruction would let a
/// session silently drop a user's `/flow:flow-note` correction.
#[test]
fn learn_routes_correction_notes_as_mandatory_directives() {
    let c = common::read_skill("flow-learn");
    assert!(
        c.contains("Every `correction` note is a non-negotiable user directive: it MUST be routed to a durable rule and never dropped"),
        "flow-learn must declare correction notes mandatory user directives"
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
fn flow_plan_step_6_files_decomposed_issue_with_assignee_me() {
    // The flow-plan Step 6 "Validate + File + Link" subsection files
    // the decomposed issue via `bin/flow issue`. That invocation must
    // carry `--label decomposed` (so flow-issues / flow-orchestrate
    // recognize it as ready-for-flow-start work) AND `--assignee @me`
    // (so the decomposed issue is assigned to the planner who ran
    // flow-plan). The assertion is scoped to the `bin/flow issue`
    // invocation line itself — not the whole subsection — because the
    // subsection also contains a `bin/flow add-issue --label decomposed`
    // call, so a subsection-wide `--label decomposed` check would still
    // pass even if the filing invocation dropped the flag.
    let c = common::read_skill("flow-plan");
    let tail = c
        .split_once("### Validate + File + Link")
        .map(|(_, t)| t)
        .expect("flow-plan must have a `Validate + File + Link` subsection");
    let subsection = tail
        .split_once("\n## ")
        .map(|(section, _)| section)
        .unwrap_or(tail);
    let issue_invocation = subsection
        .lines()
        .find(|l| l.contains("bin/flow issue --title"))
        .expect("flow-plan Step 6 must contain a `bin/flow issue --title` filing invocation");
    assert!(
        issue_invocation.contains("--label decomposed"),
        "flow-plan Step 6 bin/flow issue invocation must carry --label decomposed"
    );
    assert!(
        issue_invocation.contains("--assignee @me"),
        "flow-plan Step 6 bin/flow issue invocation must carry --assignee @me \
         so the decomposed issue is assigned to its planner"
    );
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
        "flow-explore",
        "flow-plan",
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
fn flow_complete_mode_resolution_invokes_resolve_skill_mode() {
    // The flow-complete `## Mode Resolution` section must resolve the
    // configured mode through `bin/flow resolve-skill-mode` rather than
    // hand-rolling the `skills.flow-complete` state-file read. Regression:
    // a future SKILL.md edit reverts the section to the hand-rolled prose,
    // reintroducing the bare-string-vs-object shape ambiguity.
    let c = common::read_skill("flow-complete");
    let re = Regex::new(r"(?s)## Mode Resolution\n(.*?)(?:\n## |\z)").unwrap();
    let cap = re.captures(&c);
    assert!(
        cap.is_some(),
        "flow-complete has no Mode Resolution section"
    );
    let section = &cap.unwrap()[1];
    assert!(
        section.contains("bin/flow resolve-skill-mode --skill flow-complete"),
        "flow-complete Mode Resolution must invoke `bin/flow resolve-skill-mode --skill flow-complete`"
    );
}

#[test]
fn flow_abort_mode_resolution_invokes_resolve_skill_mode() {
    // The flow-abort `## Mode Resolution` section must resolve the
    // configured mode through `bin/flow resolve-skill-mode` rather than
    // hand-rolling the `skills.flow-abort` state-file read. Regression:
    // a future SKILL.md edit reverts the section to the hand-rolled prose,
    // reintroducing the bare-string-vs-object shape ambiguity.
    let c = common::read_skill("flow-abort");
    let re = Regex::new(r"(?s)## Mode Resolution\n(.*?)(?:\n## |\z)").unwrap();
    let cap = re.captures(&c);
    assert!(cap.is_some(), "flow-abort has no Mode Resolution section");
    let section = &cap.unwrap()[1];
    assert!(
        section.contains("bin/flow resolve-skill-mode --skill flow-abort"),
        "flow-abort Mode Resolution must invoke `bin/flow resolve-skill-mode --skill flow-abort`"
    );
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

/// flow-release must rebuild and stage the committed prebuilt binary
/// (bin/flow-rs-darwin-arm64) at every release so its bytes match the
/// current source generation. Both the build and the staging copy run
/// inside `bin/setup --stage-binary` — the `cargo` deny-list entry does
/// not reach the script's internal `cargo build --release` subprocess,
/// and the copy runs inside the script rather than as a
/// permission-denied `cp` Bash tool call.
/// Regression guarded: a flow-release edit that drops the rebuild step,
/// or one that reintroduces a permission-denied command (`cargo build
/// --release`, `cp`) or the fragile git-plumbing staging dance
/// (`hash-object` / `update-index` / `checkout-index`).
#[test]
fn flow_release_skill_builds_and_commits_binary() {
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-release")
            .join("SKILL.md"),
    )
    .unwrap();
    let tail = c
        .split_once("## Step 6 — Rebuild and stage the prebuilt binary")
        .map(|(_, t)| t)
        .expect("flow-release/SKILL.md must contain the Step 6 binary-rebuild heading");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("bin/setup --stage-binary"),
        "flow-release Step 6 must build and stage the binary via `bin/setup --stage-binary`"
    );
    for forbidden in [
        "cargo build --release",
        "cp target/release/flow-rs",
        "hash-object",
        "update-index",
        "checkout-index",
    ] {
        assert!(
            !section.contains(forbidden),
            "flow-release Step 6 must not reintroduce the permission-denied or fragile command `{}`",
            forbidden
        );
    }
}

/// flow-qa is the project-local maintainer skill that files a
/// pre-decomposed QA issue for full-lifecycle regression testing
/// of the FLOW plugin. The SKILL.md must declare valid frontmatter
/// (name + non-empty description) and emit the canonical announce
/// banner so the user sees a consistent "STARTING" line when they
/// type `/flow-qa`.
///
/// Regression guarded: a future edit that removes the frontmatter,
/// changes the `name:` field away from `flow-qa`, drops the
/// `description:` field, or omits the announce banner string.
#[test]
fn flow_qa_skill_exists_with_proper_frontmatter() {
    let content = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .expect(".claude/skills/flow-qa/SKILL.md must exist");

    assert!(
        content.starts_with("---\n"),
        "flow-qa SKILL.md must open with YAML frontmatter delimiter"
    );

    let after_open = content
        .strip_prefix("---\n")
        .expect("frontmatter open delimiter checked above");
    let (frontmatter, _body) = after_open
        .split_once("\n---\n")
        .expect("flow-qa SKILL.md frontmatter must close with `\\n---\\n`");

    assert!(
        frontmatter
            .lines()
            .any(|line| line.trim() == "name: flow-qa"),
        "flow-qa SKILL.md frontmatter must declare `name: flow-qa`"
    );

    let desc_line = frontmatter
        .lines()
        .find(|line| line.trim_start().starts_with("description:"))
        .expect("flow-qa SKILL.md frontmatter must declare a `description:` field");
    let desc_value = desc_line
        .trim_start()
        .strip_prefix("description:")
        .map(|v| v.trim().trim_matches('"').trim())
        .unwrap_or("");
    assert!(
        !desc_value.is_empty(),
        "flow-qa SKILL.md `description:` field must be non-empty"
    );

    // Match a structural banner shape — `FLOW v<MAJOR>.<MINOR>.<PATCH> — flow-qa — STARTING` —
    // rather than pinning the literal version `v2.2.0`. The pinned-version form forced
    // every release to update this test in lockstep; the structural form names the
    // regression target (banner present, naming the skill) without the version drift cost.
    let banner_re =
        Regex::new(r"FLOW v\d+\.\d+\.\d+ — flow-qa — STARTING").expect("banner regex must compile");
    assert!(
        banner_re.is_match(&content),
        "flow-qa SKILL.md must contain the announce banner matching `FLOW v<x>.<y>.<z> — flow-qa — STARTING`"
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

#[test]
fn flow_prime_has_role_selection_step() {
    let c = common::read_skill("flow-prime");
    let role_marker = c
        .lines()
        .find(|l| l.starts_with("### Step ") && l.to_lowercase().contains("role"))
        .expect(
            "flow-prime must contain a `### Step` heading whose subject names role selection \
             (e.g., 'Choose primary role')",
        );
    let role_offset = c
        .find(role_marker)
        .expect("role-selection Step heading must be locatable in skill content");
    let subsection_start = &c[role_offset..];
    let subsection = subsection_start
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(subsection_start);
    for option in ["PM", "Tech Lead", "Founder / Solo Dev"] {
        assert!(
            subsection.contains(option),
            "role-selection Step must list option `{}` within its body",
            option
        );
    }
    let askuser_idx = subsection
        .find("\"What is your primary role?")
        .expect("role-selection Step must contain the AskUserQuestion prompt");
    let after_prompt = &subsection[askuser_idx..];
    let close_idx = after_prompt
        .find("</HARD-GATE>")
        .expect("role-selection AskUserQuestion must be bounded by the closing HARD-GATE");
    let prompt_window = &after_prompt[..close_idx];
    let bullet_count = prompt_window
        .lines()
        .filter(|l| l.starts_with("> - **"))
        .count();
    assert_eq!(
        bullet_count, 3,
        "role-selection AskUserQuestion must offer exactly three role bullets \
         (PM, Tech Lead, Founder / Solo Dev) — found {} bullets, which means a \
         resurrected Skip option (or a new bullet) would pass the per-option \
         presence checks above without tripping any tombstone",
        bullet_count
    );
}

#[test]
fn flow_prime_step_headings_in_role_commit_autonomy_order() {
    let c = common::read_skill("flow-prime");
    let tail_at_steps = c
        .split_once("\n## Steps\n")
        .map(|(_, tail)| tail)
        .expect("flow-prime must declare a `## Steps` section");
    let steps_section = tail_at_steps
        .split_once("\n## ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_steps);
    let headings: Vec<&str> = steps_section
        .lines()
        .filter(|l| {
            l.starts_with("### Step 1 ")
                || l.starts_with("### Step 2 ")
                || l.starts_with("### Step 3 ")
        })
        .collect();
    assert!(
        headings.len() >= 3,
        "flow-prime `## Steps` section must declare Step 1, Step 2, and Step 3 headings — found {}",
        headings.len()
    );
    let step1 = headings[0];
    let step2 = headings[1];
    let step3 = headings[2];
    assert!(
        step1.contains("Choose primary role"),
        "Step 1 must be 'Choose primary role'; got: {}",
        step1
    );
    assert!(
        step2.contains("Choose commit message format"),
        "Step 2 must be 'Choose commit message format'; got: {}",
        step2
    );
    assert!(
        step3.contains("Choose autonomy level"),
        "Step 3 must be 'Choose autonomy level'; got: {}",
        step3
    );
}

#[test]
fn flow_prime_recommended_preset_matches_new_shape() {
    let c = common::read_skill("flow-prime");
    let tail_at_step3 = c
        .split_once("### Step 3 — Choose autonomy level")
        .map(|(_, tail)| tail)
        .expect("flow-prime must declare a `### Step 3 — Choose autonomy level` heading");
    let step3_section = tail_at_step3
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_step3);
    let tail_at_label = step3_section
        .split_once("**Recommended** — safe defaults:")
        .map(|(_, tail)| tail)
        .expect(
            "Step 3 Autonomy section must label the Recommended preset \
             with '**Recommended** — safe defaults:'",
        );
    let re = Regex::new(r"```json\n(\{[\s\S]*?\})\n```").unwrap();
    let recommended_block = re
        .captures(tail_at_label)
        .expect("Recommended preset must be followed by a ```json fenced block")[1]
        .to_string();
    let recommended: Value =
        serde_json::from_str(&recommended_block).expect("Recommended preset must be valid JSON");
    assert_eq!(
        recommended["flow-start"]["continue"], "auto",
        "Recommended preset: flow-start.continue must be 'auto'"
    );
    assert_eq!(
        recommended["flow-code"]["commit"], "auto",
        "Recommended preset: flow-code.commit must be 'auto'"
    );
    assert_eq!(
        recommended["flow-code"]["continue"], "auto",
        "Recommended preset: flow-code.continue must be 'auto'"
    );
    assert_eq!(
        recommended["flow-review"]["commit"], "auto",
        "Recommended preset: flow-review.commit must be 'auto'"
    );
    assert_eq!(
        recommended["flow-review"]["continue"], "auto",
        "Recommended preset: flow-review.continue must be 'auto'"
    );
    assert_eq!(
        recommended["flow-learn"]["commit"], "auto",
        "Recommended preset: flow-learn.commit must be 'auto'"
    );
    assert_eq!(
        recommended["flow-learn"]["continue"], "auto",
        "Recommended preset: flow-learn.continue must be 'auto'"
    );
    assert_eq!(
        recommended["flow-complete"], "manual",
        "Recommended preset: flow-complete must be 'manual'"
    );
    assert_eq!(
        recommended["flow-abort"], "manual",
        "Recommended preset: flow-abort must be 'manual'"
    );
}

#[test]
fn flow_prime_fully_manual_preset_keeps_start_continue_auto() {
    let c = common::read_skill("flow-prime");
    let tail_at_step3 = c
        .split_once("### Step 3 — Choose autonomy level")
        .map(|(_, tail)| tail)
        .expect("flow-prime must declare a `### Step 3 — Choose autonomy level` heading");
    let step3_section = tail_at_step3
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_step3);
    let tail_at_label = step3_section
        .split_once("**Fully manual** — all manual:")
        .map(|(_, tail)| tail)
        .expect(
            "Step 3 Autonomy section must label the Fully manual preset \
             with '**Fully manual** — all manual:'",
        );
    let re = Regex::new(r"```json\n(\{[\s\S]*?\})\n```").unwrap();
    let fully_manual_block = re
        .captures(tail_at_label)
        .expect("Fully manual preset must be followed by a ```json fenced block")[1]
        .to_string();
    let fully_manual: Value =
        serde_json::from_str(&fully_manual_block).expect("Fully manual preset must be valid JSON");
    assert_eq!(
        fully_manual["flow-start"]["continue"], "auto",
        "Fully manual preset: flow-start.continue must be 'auto' (Start is never prompted)"
    );
    assert_eq!(
        fully_manual["flow-code"]["commit"], "manual",
        "Fully manual preset: flow-code.commit must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-code"]["continue"], "manual",
        "Fully manual preset: flow-code.continue must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-review"]["commit"], "manual",
        "Fully manual preset: flow-review.commit must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-review"]["continue"], "manual",
        "Fully manual preset: flow-review.continue must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-learn"]["commit"], "manual",
        "Fully manual preset: flow-learn.commit must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-learn"]["continue"], "manual",
        "Fully manual preset: flow-learn.continue must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-complete"], "manual",
        "Fully manual preset: flow-complete must be 'manual'"
    );
    assert_eq!(
        fully_manual["flow-abort"], "manual",
        "Fully manual preset: flow-abort must be 'manual'"
    );
}

#[test]
fn flow_prime_customize_section_never_prompts_for_flow_start() {
    let c = common::read_skill("flow-prime");
    let tail_at_step3 = c
        .split_once("### Step 3 — Choose autonomy level")
        .map(|(_, tail)| tail)
        .expect("flow-prime must declare a `### Step 3 — Choose autonomy level` heading");
    let step3_section = tail_at_step3
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_step3);
    let tail_at_customize = step3_section
        .split_once("**Customize** — ask per skill")
        .map(|(_, tail)| tail)
        .expect(
            "Step 3 Autonomy section must declare the Customize branch \
             with '**Customize** — ask per skill'",
        );
    let askuser_re = Regex::new(r#">[^\n]*"[^"\n]*/flow:flow-start[^"\n]*\?""#).unwrap();
    let askuser_hit = askuser_re.find(tail_at_customize);
    assert!(
        askuser_hit.is_none(),
        "Step 3 Customize section must not contain any AskUserQuestion prompt that \
         targets /flow:flow-start — Start is hardcoded to continue=auto across \
         every autonomy path. Any prompt of the form `\"<verb> for /flow:flow-start?\"` \
         resurrects the deleted Customize-Start sub-question. Found: {}",
        askuser_hit
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    );
}

#[test]
fn flow_prime_reprime_extracts_role() {
    let c = common::read_skill("flow-prime");
    let tail_at_heading = c
        .split_once("## Reprime Check")
        .map(|(_, t)| t)
        .expect("flow-prime must declare a Reprime Check section");
    let reprime = tail_at_heading
        .split_once("\n## ")
        .map(|(section, _)| section)
        .unwrap_or(tail_at_heading);
    assert!(
        reprime.contains("role"),
        "Reprime Check must mention extracting `role` alongside skills and commit_format"
    );
    assert!(
        reprime.contains("skills") && reprime.contains("commit_format"),
        "Reprime Check still extracts skills and commit_format"
    );
}

#[test]
fn flow_prime_invokes_setup_with_role_flag() {
    let c = common::read_skill("flow-prime");
    let setup_step = c
        .lines()
        .find(|l| l.starts_with("### Step ") && l.to_lowercase().contains("run prime setup script"))
        .expect("flow-prime must contain a `Run prime setup script` Step heading");
    let setup_offset = c
        .find(setup_step)
        .expect("setup-script Step heading must be locatable in skill content");
    let subsection_start = &c[setup_offset..];
    let subsection = subsection_start
        .split_once("\n### ")
        .map(|(section, _)| section)
        .unwrap_or(subsection_start);
    let bash_blocks: Vec<&str> = subsection
        .split("```bash")
        .skip(1)
        .filter_map(|tail| tail.split_once("```").map(|(body, _)| body))
        .collect();
    assert!(
        !bash_blocks.is_empty(),
        "setup-script Step must contain at least one fenced bash block",
    );
    assert!(
        bash_blocks.iter().any(|body| body.contains("--role")),
        "setup-script Step must include `--role` inside a fenced bash block so role flows into prime-setup",
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

/// flow-complete's Step 1 must dispatch `ci_drift` before `ci_failed`,
/// since the drift signal (sentinel-hit + remote-fail) is a subset of
/// what the generic `ci_failed` would otherwise consume. The handler
/// must also reference `bin/dependencies` (the toolchain refresh
/// surface), `bin/flow ci --force` (the sentinel invalidate + re-run),
/// and carry a 10-minute Bash timeout preamble for the CI re-run per
/// `.claude/rules/ci-is-a-gate.md`.
#[test]
fn flow_complete_skill_contains_ci_drift_handler_before_ci_failed() {
    let content = common::read_skill("flow-complete");

    // Bounded slice: just Step 1. Pattern from
    // .claude/rules/testing-gotchas.md "Subsection-Local Assertions".
    let tail = content
        .split_once("### Step 1")
        .map(|(_, t)| t)
        .expect("Step 1 heading must exist in flow-complete SKILL.md");
    let step1 = tail
        .split_once("\n### Step 2")
        .map(|(s, _)| s)
        .unwrap_or(tail);

    let drift_marker = r#"`"path": "ci_drift"`"#;
    let failed_marker = r#"`"path": "ci_failed"`"#;
    let drift_off = step1.find(drift_marker).unwrap_or_else(|| {
        panic!(
            "Step 1 must contain `\"path\": \"ci_drift\"` marker — \
             handler missing for the toolchain-drift recovery path"
        )
    });
    let failed_off = step1.find(failed_marker).unwrap_or_else(|| {
        panic!(
            "Step 1 must contain `\"path\": \"ci_failed\"` marker — \
             handler missing for the generic ci_failed dispatch"
        )
    });
    assert!(
        drift_off < failed_off,
        "ci_drift handler must appear before ci_failed handler in Step 1 \
         (drift_off={}, failed_off={}). Order matters: ci_drift is a \
         specialization of the local-pass + remote-fail combination and \
         must be dispatched before the generic ci_failed branch.",
        drift_off,
        failed_off
    );

    assert!(
        step1.contains("bin/dependencies"),
        "Step 1 ci_drift handler must invoke `bin/dependencies` to \
         refresh the local toolchain"
    );
    assert!(
        step1.contains("bin/flow ci --force"),
        "Step 1 ci_drift handler must run `bin/flow ci --force` to \
         invalidate the local CI sentinel and re-run on the refreshed \
         toolchain"
    );

    // Timeout preamble: within 5 non-blank lines before the
    // `bin/flow ci --force` opening fence, expect either the numeric
    // `timeout: 600000` form or the canonical prose phrase.
    let force_marker = "bin/flow ci --force";
    let force_off = step1
        .find(force_marker)
        .expect("bin/flow ci --force must appear in ci_drift handler");
    // Find the opening ```bash fence immediately preceding force_off.
    let prefix = &step1[..force_off];
    let fence_off = prefix
        .rfind("```bash")
        .expect("bin/flow ci --force must be inside a ```bash fence");
    let preamble_slice = &step1[..fence_off];
    let preamble_lines: Vec<&str> = preamble_slice
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();
    let has_timeout = preamble_lines
        .iter()
        .any(|l| l.contains("timeout: 600000") || l.contains("10-minute Bash tool timeout"));
    assert!(
        has_timeout,
        "Step 1 ci_drift handler must include a 10-minute Bash timeout \
         preamble within the 5 non-blank lines preceding the \
         `bin/flow ci --force` block (looked for `timeout: 600000` or \
         `10-minute Bash tool timeout`)"
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

/// Bounded slice helper: return the Step-2 ("Render the four
/// sections") subsection of flow-issues SKILL.md. Used by the
/// four-section contract tests so assertions can't be satisfied
/// by content in unrelated subsections.
fn flow_issues_step_2_subsection() -> String {
    let c = common::read_skill("flow-issues");
    let tail = c
        .split_once("## Step 2 — Render the four sections")
        .map(|(_, t)| t)
        .expect("flow-issues SKILL.md must have a Step 2 — Render the four sections section");
    let body = tail.split_once("\n## ").map(|(b, _)| b).unwrap_or(tail);
    body.to_string()
}

#[test]
fn flow_issues_has_four_sections_in_order() {
    let body = flow_issues_step_2_subsection();
    let blocked_idx = body.find("**Blocked**").expect("Blocked section name");
    let other_idx = body.find("**Other**").expect("Other section name");
    let vanilla_idx = body.find("**Vanilla**").expect("Vanilla section name");
    let decomposed_idx = body
        .find("**Decomposed**")
        .expect("Decomposed section name");
    assert!(
        blocked_idx < other_idx && other_idx < vanilla_idx && vanilla_idx < decomposed_idx,
        "flow-issues Step 2 must name Blocked, Other, Vanilla, Decomposed in that order"
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
fn flow_issues_has_triage_in_progress_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Triage In-Progress") || c.contains("triage_in_progress"),
        "flow-issues must reference the Triage In-Progress signal"
    );
}

#[test]
fn flow_issues_has_vanilla_detection() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("Vanilla") || c.contains("vanilla"),
        "flow-issues must reference the Vanilla bucket"
    );
}

#[test]
fn flow_issues_has_start_commands() {
    let c = common::read_skill("flow-issues");
    assert!(
        c.contains("flow-start") || c.contains("flow:flow-start"),
        "flow-issues Decomposed section must include flow-start commands"
    );
}

#[test]
fn flow_issues_has_explore_command_for_other_bucket() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("flow-explore") || body.contains("flow:flow-explore"),
        "flow-issues Other section must include flow-explore commands"
    );
}

#[test]
fn flow_issues_has_plan_command_for_vanilla_bucket() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("flow-plan") || body.contains("flow:flow-plan"),
        "flow-issues Vanilla section must include flow-plan commands"
    );
}

#[test]
fn flow_issues_names_canonical_columns() {
    let body = flow_issues_step_2_subsection();
    for col in ["Issue #", "Title", "Assignee", "Command"] {
        assert!(
            body.contains(col),
            "flow-issues Step 2 must name the `{}` column",
            col,
        );
    }
    assert!(
        body.contains("Blocked By"),
        "flow-issues Step 2 must name the `Blocked By` column for the Blocked section"
    );
}

#[test]
fn flow_issues_names_color_prefixes() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("🟡"),
        "flow-issues Step 2 must name the 🟡 prefix for Flow-In-Progress rows"
    );
    assert!(
        body.contains("🔍"),
        "flow-issues Step 2 must name the 🔍 prefix for Triage-In-Progress rows"
    );
    assert!(
        body.contains("Bold") || body.contains("bold"),
        "flow-issues Step 2 must instruct bold Title for colored rows"
    );
}

#[test]
fn flow_issues_names_link_format() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("[#N](url)"),
        "flow-issues Step 2 must render Issue # cells as `[#N](url)` markdown links"
    );
}

#[test]
fn flow_issues_names_empty_cell_convention() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("`—`"),
        "flow-issues Step 2 must name `—` as the empty-cell convention"
    );
}

#[test]
fn flow_issues_names_sort_rules() {
    let body = flow_issues_step_2_subsection();
    assert!(
        body.contains("number") && body.contains("descending"),
        "flow-issues Step 2 must name issue-number descending as the sort rule"
    );
    assert!(
        body.contains("colored rows first") || body.contains("colored first"),
        "flow-issues Step 2 must instruct colored-first sort for Other and Decomposed"
    );
}

/// Tombstone: removed in PR #1549. Stability argument:
/// `Recommended Work Order`, `Start Commands`, and the
/// `### In Progress` heading are distinctive multi-word strings
/// that cannot be assembled by `concat!`/`format!` or split
/// across method-chained `.arg()` calls — they appear as
/// Markdown headings in prose, not as shell-tool arguments. The
/// summary-line directive includes the literal token "in
/// progress, " from the rendered template, which is also
/// non-reassemblable. Bypasses considered and rejected: macro
/// concat (Markdown headings cannot be runtime-assembled),
/// constant ref (would still leave the string on a source line),
/// hex escapes (would still appear in source).
#[test]
fn test_flow_issues_no_recommended_work_order_heading() {
    let c = common::read_skill("flow-issues");
    assert!(
        !c.contains("Recommended Work Order"),
        "flow-issues SKILL.md must not contain `Recommended Work Order` — \
         the four-section dashboard in PR #1549 replaces the work-order table."
    );
}

/// Tombstone: removed in PR #1549. Stability argument: the
/// `Start Commands` heading is a distinctive two-word phrase
/// with no `concat!`/`format!`/constant reassembly path; it
/// appears as a Markdown heading in prose, not as a shell-tool
/// argument that could split across method chains.
#[test]
fn test_flow_issues_no_start_commands_heading() {
    let c = common::read_skill("flow-issues");
    assert!(
        !c.contains("Start Commands"),
        "flow-issues SKILL.md must not contain `Start Commands` — \
         the four-section dashboard in PR #1549 surfaces commands per row in the Command cell."
    );
}

/// Tombstone: removed in PR #1549. Stability argument: the
/// `### In Progress` heading is a distinctive Markdown-level-3
/// heading string with no `concat!`/`format!`/constant
/// reassembly path. The substring is unique to the subsection
/// heading; the `Flow In-Progress` label name remains valid
/// elsewhere because it includes the hyphenated suffix.
#[test]
fn test_flow_issues_no_in_progress_subsection_heading() {
    let c = common::read_skill("flow-issues");
    assert!(
        !c.contains("### In Progress"),
        "flow-issues SKILL.md must not contain `### In Progress` — \
         Flow-In-Progress rows are bucketed into the Decomposed section in PR #1549."
    );
}

/// Tombstone: removed in PR #1549. Stability argument: the
/// `Impact` and `Rationale` column headers are common English
/// words; the assertion is bounded to the Step 2 subsection
/// (the table-rendering region) so prose mentions of the words
/// elsewhere remain valid. No `concat!`/`format!`/constant
/// reassembly path produces a column header at runtime.
#[test]
fn test_flow_issues_no_impact_or_rationale_column_in_step_2() {
    let body = flow_issues_step_2_subsection();
    assert!(
        !body.contains("Impact"),
        "flow-issues Step 2 must not name an `Impact` column — \
         impact ranking was dropped in PR #1549."
    );
    assert!(
        !body.contains("Rationale"),
        "flow-issues Step 2 must not name a `Rationale` column — \
         rationale ranking was dropped in PR #1549."
    );
}

/// Tombstone: removed in PR #1549. Stability argument: the
/// summary-line directive includes the literal token sequence
/// "in progress, " (lowercase, with comma) from the rendered
/// template prose. The exact phrase has no
/// `concat!`/`format!`/constant reassembly path and does not
/// collide with the `Flow In-Progress` capitalized label name.
#[test]
fn test_flow_issues_no_summary_line_directive() {
    let c = common::read_skill("flow-issues");
    assert!(
        !c.contains("in progress, "),
        "flow-issues SKILL.md must not contain the summary-line `in progress,` template — \
         the four-section dashboard in PR #1549 replaces the summary line."
    );
    assert!(
        !c.contains("available for work"),
        "flow-issues SKILL.md must not contain `available for work` — \
         the summary-line template was dropped in PR #1549."
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
//
// Tombstone: PR #1477 retired the flow-create-issue skill.
// Its responsibilities split into /flow:flow-explore (PM voice,
// vanilla What/Why/Acceptance Criteria filing) and /flow:flow-plan
// #N (Tech Lead voice, decompose+file pipeline against a vanilla
// parent). Contract tests for the deleted skill have been removed
// and replaced by file-existence and prose-absence tombstones in
// tests/tombstones.rs.

// --- More tombstones ---

#[test]
fn decompose_project_no_depends_on_text() {
    let c = common::read_skill("flow-decompose-project");
    assert!(
        !c.contains("Depends on") || c.contains("Depends On"),
        "Tombstone: 'Depends on' text removed from decompose-project"
    );
}

// --- flow-continue skill contract ---
//
// `/flow:flow-continue` is the user-typed slash command that clears
// `_halt_pending` so an autonomous flow resumes. The four tests
// below guard distinct regressions per
// `.claude/rules/tests-guard-real-regressions.md`:
//
// - `flow_continue_skill_exists` — accidental deletion of
//   `skills/flow-continue/SKILL.md`. Consumer: users typing
//   `/flow:flow-continue` to resume a halted autonomous flow.
// - `flow_continue_skill_has_starting_banner` /
//   `flow_continue_skill_has_complete_banner` — drift in the
//   skill's user-facing banners away from the FLOW convention.
//   Consumer: visual consistency across every FLOW skill the user
//   invokes.
// - `flow_continue_skill_invokes_clear_halt` — silent removal of
//   the `bin/flow clear-halt` invocation, which would leave the
//   skill a no-op while still appearing to run. Consumer: the
//   user-typed slash command must mutate state.
// - `flow_continue_skill_has_description_frontmatter` — drift in
//   the YAML frontmatter that Claude Code reads to discover the
//   skill. Consumer: Claude Code skill discovery.

#[test]
fn flow_continue_skill_exists() {
    assert!(
        common::skills_dir()
            .join("flow-continue")
            .join("SKILL.md")
            .exists(),
        "skills/flow-continue/SKILL.md must exist"
    );
}

#[test]
fn flow_continue_skill_has_starting_banner() {
    let c = common::read_skill("flow-continue");
    assert!(
        c.contains("flow:flow-continue") && c.contains("STARTING"),
        "flow-continue SKILL.md must include the STARTING announce banner naming flow:flow-continue"
    );
}

#[test]
fn flow_continue_skill_has_complete_banner() {
    let c = common::read_skill("flow-continue");
    assert!(
        c.contains("flow:flow-continue") && c.contains("COMPLETE"),
        "flow-continue SKILL.md must include the COMPLETE banner naming flow:flow-continue"
    );
}

#[test]
fn flow_continue_skill_invokes_clear_halt() {
    let c = common::read_skill("flow-continue");
    assert!(
        c.contains("bin/flow clear-halt"),
        "flow-continue SKILL.md must invoke `bin/flow clear-halt` as its only step"
    );
}

#[test]
fn flow_continue_skill_has_description_frontmatter() {
    let c = common::read_skill("flow-continue");
    // Claude Code reads `description:` from the YAML frontmatter
    // for skill discovery. The frontmatter sits between `---`
    // delimiters at the top of the file.
    let frontmatter = c
        .split_once("---\n")
        .and_then(|(_, tail)| tail.split_once("\n---"))
        .map(|(fm, _)| fm)
        .unwrap_or("");
    assert!(
        frontmatter.contains("description:"),
        "flow-continue SKILL.md frontmatter must carry a `description:` field"
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
    // Review Step 1 captures the substantive diff via `bin/flow
    // capture-diff` (which runs `git diff origin/<base_branch>...HEAD -w`
    // internally and writes the bytes to a canonical
    // `.flow-states/<branch>/substantive-diff.diff` file). The contract
    // is that Step 1 invokes capture-diff with the branch+base args; the
    // skill no longer embeds the `git diff` command literally because
    // agents read the diff via the Read tool on the returned path.
    assert!(
        c.contains("capture-diff --branch <branch> --base <base_branch>"),
        "Review Step 1 must invoke `bin/flow capture-diff --branch <branch> --base <base_branch>` \
         so context-sparse agents receive the substantive diff via file handoff"
    );
}

#[test]
fn review_routes_substantive_diff_to_context_sparse_agents() {
    let c = common::read_skill("flow-review");
    // Each of the three context-sparse agents receives the substantive
    // diff via the `SUBSTANTIVE_DIFF_FILE: <substantive_diff_file>`
    // file-path handoff. The assertion is per-agent and bounded to
    // each agent's block (see `.claude/rules/testing-gotchas.md`
    // "Subsection-Local Assertions in Contract Tests") so a regression
    // that drops the file-path form from any single agent's block
    // fails — the loop body checking the same substring against the
    // full skill would silently pass when one agent loses the handoff
    // because the other two still mention it.
    const HANDOFF: &str = "SUBSTANTIVE_DIFF_FILE: <substantive_diff_file>";
    for agent in &["Pre-mortem", "Adversarial", "Documentation"] {
        let heading = format!("**{} agent**", agent);
        let tail = c
            .split_once(heading.as_str())
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("Review Step 2 must contain `{}` heading", heading));
        // Bound the slice to this agent's block: the next agent
        // heading or the post-agent section ("Wait for all agents")
        // closes the scope.
        let block = tail.split_once("\n**").map(|(b, _)| b).unwrap_or(tail);
        assert!(
            block.contains(HANDOFF),
            "Review Step 2 must route substantive diff via `{}` inside the {} agent's block",
            HANDOFF,
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

#[test]
fn skills_no_mkdir_on_claude_paths() {
    // Forbid `mkdir <flags?> <path>.claude/...` in any skill or agent
    // bash block. Claude Code's platform protection refuses tool
    // calls under `.claude/` regardless of allow-list patterns
    // (`.claude/rules/skill-authoring.md` "Platform Constraints");
    // the sanctioned path for creating skill or rule directories is
    // `bin/flow write-rule`, which calls `fs::create_dir_all(parent)`
    // from inside the Rust subprocess. A direct `mkdir` under
    // `.claude/` would surface a permission prompt mid-autonomous-flow
    // — exactly the system-initiated-prompts class
    // `.claude/rules/autonomous-phase-discipline.md` forbids.
    //
    // The pattern allows optional short flags between `mkdir` and the
    // path so `mkdir -p .claude/foo` and `mkdir -pv .claude/foo` both
    // fail. The path captures any non-whitespace prefix followed by
    // `.claude/` — bare `.claude/x`, `<plugin>/.claude/x`, and
    // `~/.claude/x` are all caught.
    let mkdir_re = Regex::new(r"mkdir(\s+-[a-z]+)?\s+[^\s]*\.claude/").unwrap();
    let mut violations = Vec::new();

    // Walk plugin-marketplace skills under `skills/`.
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        for block in common::extract_bash_blocks(&content) {
            for line in block.lines() {
                if mkdir_re.is_match(line) {
                    violations.push(format!("skills/{}/SKILL.md: {}", name, line.trim()));
                }
            }
        }
    }

    // Walk project-local maintainer skills under `.claude/skills/`.
    // These are not in `common::all_skill_names()` (which only
    // enumerates the plugin-marketplace `skills/` tree).
    let project_skills_dir = common::repo_root().join(".claude").join("skills");
    if let Ok(entries) = fs::read_dir(&project_skills_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            let Ok(content) = fs::read_to_string(&skill_md) else {
                continue;
            };
            let skill_name = entry.file_name().to_string_lossy().to_string();
            for block in common::extract_bash_blocks(&content) {
                for line in block.lines() {
                    if mkdir_re.is_match(line) {
                        violations.push(format!(
                            ".claude/skills/{}/SKILL.md: {}",
                            skill_name,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }

    // Walk agents under `agents/`.
    for agent in agent_files() {
        let content = common::read_agent(&agent);
        for block in common::extract_bash_blocks(&content) {
            for line in block.lines() {
                if mkdir_re.is_match(line) {
                    violations.push(format!("agents/{}: {}", agent, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Skill and agent bash blocks must not run `mkdir` on `.claude/` paths — Claude Code platform protection blocks these regardless of settings.json. Route directory creation through `bin/flow write-rule` (which calls `fs::create_dir_all(parent)` internally). Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn agents_no_transcript_jsonl_reads() {
    // Forbid references to the per-session transcript file Claude Code
    // persists at `~/.claude/projects/.../<session>.jsonl` inside any
    // agent prompt. Reading that file from inside a flow would surface
    // a permission prompt (the path sits outside the project root) AND
    // duplicates work — the sanctioned recovery surface is
    // `compact_summary` in `.flow-states/<branch>/state.json` per
    // `.claude/rules/post-compaction-recovery.md`. The
    // `validate-claude-paths` hook mechanically blocks the Read at the
    // tool layer; the scanner here prevents an agent prompt from
    // instructing the model to attempt the read in the first place.
    //
    // Match pattern: any path containing `.claude/projects/` followed by
    // a non-whitespace path tail ending in `.jsonl`. Anchored to
    // `.claude/projects/` rather than `~/` so absolute paths
    // (`/Users/<name>/.claude/projects/...`) and env-var-expanded
    // forms (`$HOME/.claude/projects/...`) are all caught. The
    // `.jsonl` suffix anchors the match to transcript files
    // specifically — legitimate `~/.claude/projects/*/memory/*`
    // references (used by other tooling for auto-memory) do not end
    // in `.jsonl` and are not flagged. Match is case-insensitive on
    // the `.jsonl` extension via the `(?i)` flag.
    let re = Regex::new(r"(?i)\.claude/projects/[^\s]*\.jsonl").unwrap();
    let mut violations = Vec::new();
    for agent in agent_files() {
        let content = common::read_agent(&agent);
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                violations.push(format!("agents/{}:{}: {}", agent, i + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Agent prompts must not instruct the model to read the persisted transcript file. Use the `compact_summary` field in `.flow-states/<branch>/state.json` per `.claude/rules/post-compaction-recovery.md`. Violations:\n{}",
        violations.join("\n")
    );
}

/// Scan every skill and agent for `Write(/tmp/<path>.<ext>)` references
/// where `<ext>` falls outside the closed allow set
/// (`.txt`, `.diff`, `.patch`, `.md`, `.json`, `.jsonl`). Extensions
/// outside that set continue to require an explicit permission prompt
/// by design — per `.claude/rules/permissions.md` "Symmetric R+W /tmp/
/// Extension Policy" — so a skill instruction that names one would
/// surface a permission prompt mid-autonomous-flow.
///
/// The match operates on the literal source text (skill/agent prose
/// AND bash blocks). The plan's permission-coverage tests for
/// allow-list entries cover `Bash(...)` patterns; this scanner covers
/// the `Write(...)` tool-call shape that the symmetric /tmp/ policy
/// concerns.
#[test]
fn skills_no_tmp_writes_outside_allowed_extensions() {
    // The closed extension set per `.claude/rules/permissions.md`
    // "Symmetric R+W /tmp/ Extension Policy."
    const ALLOWED: &[&str] = &["txt", "diff", "patch", "md", "json", "jsonl"];

    // Match `Write(/tmp/...)` or `Write(//tmp/...)`. UNIVERSAL_ALLOW
    // patterns use the double-slash form; skill prose may use either.
    // The optional second slash is captured via `/?`. The extension
    // capture is optional — a missing extension (`Write(/tmp/scratch)`)
    // is itself a violation because no closed-set entry covers it,
    // surfacing a permission prompt at runtime.
    let re = Regex::new(r"Write\(//?tmp/([^)]+)\)").unwrap();
    let mut violations = Vec::new();

    let classify = |inner: &str| -> Option<String> {
        // Extract the extension if present (text after the last
        // `.` in the path tail). Missing extension → return None,
        // which the caller treats as a violation.
        inner
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
    };

    let check = |label: String, line_no: usize, line: &str, violations: &mut Vec<String>| {
        for cap in re.captures_iter(line) {
            let inner = cap.get(1).unwrap().as_str();
            let match_text = cap.get(0).unwrap().as_str();
            match classify(inner) {
                Some(ext) if ALLOWED.contains(&ext.as_str()) => {} // in-set, allowed
                Some(ext) => violations.push(format!(
                    "{}:{}: {} — extension `.{}` is outside the closed allow set",
                    label,
                    line_no + 1,
                    match_text,
                    ext
                )),
                None => violations.push(format!(
                    "{}:{}: {} — extensionless /tmp/ writes have no allow-list entry",
                    label,
                    line_no + 1,
                    match_text
                )),
            }
        }
    };

    // Plugin-marketplace skills.
    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        for (i, line) in content.lines().enumerate() {
            check(
                format!("skills/{}/SKILL.md", name),
                i,
                line,
                &mut violations,
            );
        }
    }

    // Project-local maintainer skills.
    let project_skills_dir = common::repo_root().join(".claude").join("skills");
    if let Ok(entries) = fs::read_dir(&project_skills_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            let Ok(content) = fs::read_to_string(&skill_md) else {
                continue;
            };
            let skill_name = entry.file_name().to_string_lossy().to_string();
            for (i, line) in content.lines().enumerate() {
                check(
                    format!(".claude/skills/{}/SKILL.md", skill_name),
                    i,
                    line,
                    &mut violations,
                );
            }
        }
    }

    // Agents.
    for agent in agent_files() {
        let content = common::read_agent(&agent);
        for (i, line) in content.lines().enumerate() {
            check(format!("agents/{}", agent), i, line, &mut violations);
        }
    }

    assert!(
        violations.is_empty(),
        "Skill/agent Write(/tmp/) references must use one of the closed extension set: txt, diff, patch, md, json, jsonl. Per `.claude/rules/permissions.md` 'Symmetric R+W /tmp/ Extension Policy', extensions outside the set require an explicit permission prompt. Violations:\n{}",
        violations.join("\n")
    );
}

/// Scan every skill and agent for placeholder-anchor language that
/// describes the forbidden "write a stub file, then redirect command
/// output into it" pattern. Three trigger phrasings:
///
/// 1. `placeholder` and `anchor` co-occurring within 5 lines of each
///    other.
/// 2. `anchor` followed by `redirect` on the same line.
/// 3. `empty` ... `file` ... `then` ... `redirect` on the same line.
///
/// See `.claude/rules/no-placeholder-anchors.md` for the rule the
/// scanner enforces and the sanctioned alternatives (Read tool on
/// persisted log files, atomic Write tool calls,
/// `bin/flow write-rule` for managed artifacts).
#[test]
fn skills_no_placeholder_anchor_language() {
    // The forbidden mechanic is "write a stub file, then redirect
    // command output into it." Trigger vocabulary targets that exact
    // pattern. `placeholder.*redirect` on a single line is more
    // specific than `anchor.*redirect` (the latter catches legitimate
    // prose like "anchor the discovery step and redirect to ...").
    // `empty.*file.*then.*redirect` catches the imperative form.
    let placeholder_redirect_re = Regex::new(r"(?i)placeholder.*redirect").unwrap();
    let empty_then_redirect_re = Regex::new(r"(?i)empty.*file.*then.*redirect").unwrap();
    let mut violations = Vec::new();

    let scan = |label: &str, content: &str, violations: &mut Vec<String>| {
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let lower = line.to_ascii_lowercase();
            // Opt-out comment grammar: a line that carries the
            // marker `<!-- no-placeholder-anchors: not-anchored -->`
            // suppresses every trigger on that line. The marker is
            // the standard escape hatch for legitimate prose that
            // happens to use the trigger vocabulary in a non-
            // forbidden sense (e.g., describing the anti-pattern
            // itself).
            if line.contains("<!-- no-placeholder-anchors: not-anchored -->") {
                continue;
            }
            // Trigger 1: placeholder ↔ anchor within 5 lines.
            // The proximity check catches multi-line prose that
            // walks the model through stub-file-then-anchor steps.
            if lower.contains("placeholder") {
                let lo = i.saturating_sub(5);
                let hi = (i + 6).min(lines.len());
                for (j, near) in lines[lo..hi].iter().enumerate() {
                    if near.to_ascii_lowercase().contains("anchor") {
                        violations.push(format!(
                            "{}:{}: 'placeholder' near 'anchor' (line {})",
                            label,
                            i + 1,
                            lo + j + 1
                        ));
                        break;
                    }
                }
            }
            // Trigger 2 + 3: same-line patterns targeting the actual
            // forbidden mechanic.
            if placeholder_redirect_re.is_match(&lower) {
                violations.push(format!(
                    "{}:{}: 'placeholder.*redirect': {}",
                    label,
                    i + 1,
                    line.trim()
                ));
            }
            if empty_then_redirect_re.is_match(&lower) {
                violations.push(format!(
                    "{}:{}: 'empty.*file.*then.*redirect': {}",
                    label,
                    i + 1,
                    line.trim()
                ));
            }
        }
    };

    for name in common::all_skill_names() {
        let content = common::read_skill(&name);
        scan(
            &format!("skills/{}/SKILL.md", name),
            &content,
            &mut violations,
        );
    }
    let project_skills_dir = common::repo_root().join(".claude").join("skills");
    if let Ok(entries) = fs::read_dir(&project_skills_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            let Ok(content) = fs::read_to_string(&skill_md) else {
                continue;
            };
            let skill_name = entry.file_name().to_string_lossy().to_string();
            scan(
                &format!(".claude/skills/{}/SKILL.md", skill_name),
                &content,
                &mut violations,
            );
        }
    }
    for agent in agent_files() {
        let content = common::read_agent(&agent);
        scan(&format!("agents/{}", agent), &content, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "Skill/agent prose must not describe the placeholder-anchor pattern (write a stub file, then redirect command output into it). Per `.claude/rules/no-placeholder-anchors.md`, the redirect is blocked by `validate-pretool` and the placeholder anchors nothing. Use the Read tool on persisted log files, an atomic Write tool call, or `bin/flow write-rule` instead. Violations:\n{}",
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
/// used by `flow-explore`, `flow-plan`, and `flow-decompose-project` are
/// excluded because the unique id makes cross-invocation collision unlikely.
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

// --- flow-reset SKILL.md delegates to ${CLAUDE_PLUGIN_ROOT}/bin/reset ---
//
// The flow-reset skill dispatches the destructive wipe through
// `bin/flow reset` so model-invoked calls land on the canonical
// `Bash(*bin/flow *)` allow entry rather than a script-specific
// wildcard. The Rust shim at `src/reset.rs` exec's the existing
// `${CLAUDE_PLUGIN_ROOT}/bin/reset` bash script, which resolves the
// main repo root via `git rev-parse --git-common-dir` and removes
// `.flow-states/` via `rm -rf`. The contract test below locks in the
// canonical delegation: the skill must invoke the dispatcher in
// Step 2 so the bash body stays reachable from the sanctioned
// permission surface.

#[test]
fn flow_reset_invokes_bin_flow_reset_dispatcher() {
    let content = common::read_skill("flow-reset");
    assert!(
        content.contains("${CLAUDE_PLUGIN_ROOT}/bin/flow reset"),
        "skills/flow-reset/SKILL.md must invoke `${{CLAUDE_PLUGIN_ROOT}}/bin/flow reset` \
         (Step 2 execute) so the dispatch lands on Bash(*bin/flow *)"
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
        // The project-local maintainer skill at
        // `.claude/skills/flow-release/` emits the bare name; every
        // other user-only skill lives at `skills/<name>/` and emits
        // the namespaced `flow:<name>` form.
        if entry == "flow-release" {
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

/// Named regression: a future edit removes the `/flow-qa` row from
/// the Maintainer table in `skills/flow-skills/SKILL.md`, so the
/// catalog of maintainer-invokable skills drifts out of sync with
/// the project-local `.claude/skills/flow-qa/` resident. Named
/// consumer: a maintainer typing `/flow:flow-skills` to discover
/// which maintainer skills they can invoke.
///
/// The bare-name regex in `flow_skills_admin_and_maintainer_match_user_only`
/// captures only `flow:flow-...` prefixed entries from
/// `USER_ONLY_SKILLS`; `flow-qa` is bare-name and invisible to that
/// scan. This test provides direct coverage for the `/flow-qa` row.
#[test]
fn flow_skills_maintainer_section_references_flow_qa() {
    let content = common::read_skill("flow-skills");
    let needle = "\n#### Maintainer\n";
    let tail = content
        .split_once(needle)
        .map(|(_, t)| t)
        .expect("flow-skills SKILL.md must contain a `#### Maintainer` subsection");
    let mut end = tail.len();
    for marker in &["\n## ", "\n### ", "\n#### "] {
        if let Some((before, _)) = tail.split_once(marker) {
            if before.len() < end {
                end = before.len();
            }
        }
    }
    let section = &tail[..end];
    assert!(
        section.contains("/flow-qa"),
        "Maintainer section of skills/flow-skills/SKILL.md must reference `/flow-qa`"
    );
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

// Three flow_create_issue contract tests retired alongside the
// flow-create-issue skill — Tombstone: PR #1477 (covers
// pre_draft_backwards_reasoning_scan, no_current_session_id_subcommand,
// and marker_invocations_omit_session_id_flag).
// The Pre-Draft Backwards-Reasoning Scan invariant is now pinned
// for the surviving issue-filing skills (flow-explore via the new
// flow_explore_skill_* tests above and flow-decompose-project via
// flow_decompose_project_skill_has_backwards_reasoning_scan below).

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

// --- include-bias rule + skill scans ---

/// The four canonical scan phrasings the SKILL bodies enumerate. Each
/// phrase represents a distinct defensive-scope shape; the rule must
/// retain the body content that authorizes the scans. The lowercase
/// `"Out of scope"` form is the canonical anchor — the title-case
/// variant is intentionally left out of the constant because the
/// SKILL scan instruction reads case-flexibly in practice (the model
/// interprets the phrasing as a concept and catches title-case
/// occurrences in issue bodies without requiring the literal byte
/// string in the SKILL prose).
const INCLUDE_BIAS_SCAN_PHRASINGS: &[&str] = &[
    "Out of scope",
    "Non-goals",
    "would expand scope",
    "separate code surface",
];

/// Action-verb tokens that prove an include-bias scan SKILL section
/// instructs the model to do something — not just enumerate the
/// canonical phrasings. At least one must appear in each SKILL's
/// scan body so a future stub-form rewrite (phrase list only) fails
/// the contract test.
const INCLUDE_BIAS_SCAN_ACTION_VERBS: &[&str] = &["scan", "revise", "evaluate", "convert"];

#[test]
fn include_bias_rule_states_inclusion_default_principle() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("include-bias-in-issues.md");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "expected `.claude/rules/include-bias-in-issues.md` to exist: {}",
            e
        )
    });

    assert!(
        content.contains("Default to inclusion"),
        "rule must state the load-bearing `Default to inclusion` invariant phrase"
    );

    for phrase in INCLUDE_BIAS_SCAN_PHRASINGS {
        assert!(
            content.contains(phrase),
            "rule must enumerate the SKILL scan phrasing `{}` so the rule remains the authoritative source for what the scans target",
            phrase
        );
    }
}

// flow_create_issue_skill_has_pre_draft_include_bias_scan retired
// alongside the flow-create-issue skill — Tombstone: PR #1477.
// The Include-Bias Scan invariant is now pinned for the surviving
// issue-filing skills (flow-explore via the new flow_explore_skill_*
// tests above and flow-decompose-project via
// flow_decompose_project_skill_has_include_bias_scan below).

#[test]
fn flow_decompose_project_skill_has_include_bias_scan() {
    let content = common::read_skill("flow-decompose-project");

    let scan_idx = content
        .find("\n### Include-Bias Scan\n")
        .expect("flow-decompose-project SKILL.md missing `### Include-Bias Scan` heading");
    assert!(
        content.contains(".claude/rules/include-bias-in-issues.md"),
        "Include-Bias Scan must cross-reference `.claude/rules/include-bias-in-issues.md`"
    );

    let present_idx = content
        .find("Present the full issue list")
        .expect("flow-decompose-project must contain `Present the full issue list` where children are surfaced");
    assert!(
        scan_idx < present_idx,
        "Include-Bias Scan must appear BEFORE child issues are presented"
    );

    let scan_tail = &content[scan_idx + 1..];
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
    for phrase in INCLUDE_BIAS_SCAN_PHRASINGS {
        assert!(
            scan_body.contains(phrase),
            "Include-Bias Scan body must enumerate the canonical scan phrasing `{}` (a stub heading without the body content does not satisfy the contract)",
            phrase
        );
    }
    let scan_body_lower = scan_body.to_ascii_lowercase();
    let has_action_verb = INCLUDE_BIAS_SCAN_ACTION_VERBS
        .iter()
        .any(|verb| scan_body_lower.contains(verb));
    assert!(
        has_action_verb,
        "Include-Bias Scan body must contain at least one action verb from {:?} so the section actually instructs the model — a bare phrase listing satisfies the phrase assertion but does not fulfill the scan's purpose",
        INCLUDE_BIAS_SCAN_ACTION_VERBS
    );
}

#[test]
fn flow_decompose_project_announce_sets_utility_marker() {
    // The Announce section must write the per-session
    // "utility skill in progress" marker so the Stop hook's
    // `check_in_progress_utility_skill` predicate refuses turn-end
    // while the multi-step decompose-project skill is running. A
    // missing marker breaks the unattended-flow contract whenever
    // the decompose:decompose Skill tool returns mid-pipeline at
    // Step 1, allowing the model to stop and return control to
    // the user before the issue graph is filed.
    //
    // The `--session-id` flag is intentionally absent from the
    // assertion set: Rust resolves the active session_id at the CLI
    // boundary by reading the `CLAUDE_CODE_SESSION_ID` env var, so
    // every multi-step utility skill's marker invocation omits it.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Announce\n")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Announce` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("set-utility-in-progress"),
        "`## Announce` must invoke `bin/flow set-utility-in-progress` so the Stop hook refuses turn-end while the multi-step skill is running"
    );
    assert!(
        section.contains("--skill flow:flow-decompose-project"),
        "`## Announce` must pass `--skill flow:flow-decompose-project` so the marker is scoped to this skill's identifier"
    );
}

#[test]
fn flow_decompose_project_step_1_no_dag_review_ask_user_question() {
    // Regression: a future edit re-introduces the Step 1 DAG-review
    // AskUserQuestion gate. Per AC#4 of issue #1488, the user's
    // invocation of `/flow:flow-decompose-project` is the single
    // authorization for the decompose-and-file pipeline; a second
    // confirmation between Step 1 (decompose) and Step 2 (issue list)
    // broke the single-signal contract. The forbidden phrasing is
    // the exact AskUserQuestion option label the removed gate used.
    let c = common::read_skill("flow-decompose-project");
    assert!(
        !c.contains("Proceed to review"),
        "skills/flow-decompose-project/SKILL.md must not contain the Step 1 `Proceed to review` AskUserQuestion option — Step 2 fires directly after the DAG synthesis is presented"
    );
}

#[test]
fn flow_decompose_project_step_2_no_due_date_prompt() {
    // Regression: a future edit re-introduces the Step 2 milestone
    // due-date AskUserQuestion. The milestone path is removed
    // entirely; the skill no longer creates milestones in Step 3
    // and `bin/flow create-milestone` is deleted. A re-introduced
    // due-date prompt would have no consumer for the captured
    // value.
    let c = common::read_skill("flow-decompose-project");
    assert!(
        !c.contains("milestone due date"),
        "skills/flow-decompose-project/SKILL.md must not prompt for milestone due date — the milestone path is removed"
    );
}

#[test]
fn flow_decompose_project_step_2_no_milestone_mention() {
    // Regression: a future edit re-introduces a milestone bullet,
    // `--milestone` flag pass-through, or `due_date` session field
    // in Step 2. With `bin/flow create-milestone` deleted, every
    // milestone reference in the skill is orphan infrastructure.
    let c = common::read_skill("flow-decompose-project");
    let step2_tail = c
        .split_once("\n## Step 2")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 2` section");
    let step2 = step2_tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(step2_tail);
    assert!(
        !step2.to_ascii_lowercase().contains("milestone"),
        "skills/flow-decompose-project/SKILL.md Step 2 must not mention milestones — the milestone path is removed"
    );
    assert!(
        !step2.contains("due_date") && !step2.contains("due-date"),
        "skills/flow-decompose-project/SKILL.md Step 2 must not carry a `due_date` session field — the milestone path is removed"
    );
}

#[test]
fn flow_decompose_project_step_3_no_create_milestone_call() {
    // Regression: a future edit re-introduces the
    // `bin/flow create-milestone` invocation in Step 3. The
    // subcommand is deleted from `src/create_milestone.rs`,
    // `src/lib.rs`, and `src/main.rs`; resurrecting the SKILL.md
    // call would surface as a runtime error rather than a
    // compile error.
    let c = common::read_skill("flow-decompose-project");
    assert!(
        !c.contains("create-milestone"),
        "skills/flow-decompose-project/SKILL.md must not invoke `bin/flow create-milestone` — the subcommand is deleted"
    );
}

#[test]
fn flow_decompose_project_step_3_validator_auto_fix_loop() {
    // Regression: a future edit drops the bounded auto-fix loop on
    // Step 3 epic-validator failure and replaces it with either an
    // unbounded loop or a prompt-the-user gate. The
    // `validator_max_retries` reason is the contract the
    // COMPLETE-FAILED banner depends on.
    let c = common::read_skill("flow-decompose-project");
    let step3_tail = c
        .split_once("\n## Step 3")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 3` section");
    let step3 = step3_tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(step3_tail);
    assert!(
        step3.contains("validator_max_retries"),
        "Step 3 must name `validator_max_retries` so the structured-error contract is locked in"
    );
    assert!(
        step3.contains("5 attempts") || step3.contains("5 retries"),
        "Step 3 must name the 5-attempt cap so the bounded-loop contract is locked in"
    );
}

#[test]
fn flow_decompose_project_step_4_validator_auto_fix_loop() {
    // Regression: a future edit drops the Step 4 per-child auto-fix
    // loop and replaces it with either an unbounded loop or a
    // prompt-the-user gate. Step 4's failure mode is skip-on-cap
    // (continue to next child) rather than halt-on-cap (Step 3's
    // epic-failure path), so the prose must signal the skip
    // semantics.
    let c = common::read_skill("flow-decompose-project");
    let step4_tail = c
        .split_once("\n## Step 4")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 4` section");
    let step4 = step4_tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(step4_tail);
    assert!(
        step4.contains("5 attempts") || step4.contains("5 retries"),
        "Step 4 must name the 5-attempt cap so the bounded-loop contract is locked in"
    );
    assert!(
        step4.to_ascii_lowercase().contains("skip"),
        "Step 4 must name the skip-on-cap semantics so the child-failure recovery contract is locked in"
    );
}

#[test]
fn flow_decompose_project_step_3_no_milestone_flag_in_issue_call() {
    // Regression: a future edit re-introduces `--milestone` on the
    // Step 3 epic-filing `bin/flow issue` call. The flag is deleted
    // from `src/issue.rs`; resurrecting the SKILL.md pass-through
    // would surface as a runtime error.
    let c = common::read_skill("flow-decompose-project");
    let step3_tail = c
        .split_once("\n## Step 3")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 3` section");
    let step3 = step3_tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(step3_tail);
    assert!(
        !step3.contains("--milestone"),
        "skills/flow-decompose-project/SKILL.md Step 3 must not pass `--milestone` to `bin/flow issue` — the flag is deleted"
    );
}

#[test]
fn flow_decompose_project_step_4_no_milestone_flag_in_issue_call() {
    // Same regression guard as Step 3 but scoped to the per-child
    // filing loop in Step 4.
    let c = common::read_skill("flow-decompose-project");
    let step4_tail = c
        .split_once("\n## Step 4")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 4` section");
    let step4 = step4_tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(step4_tail);
    assert!(
        !step4.contains("--milestone"),
        "skills/flow-decompose-project/SKILL.md Step 4 must not pass `--milestone` to `bin/flow issue` — the flag is deleted"
    );
}

#[test]
fn flow_decompose_project_step6_clears_utility_marker() {
    // Step 6 is the success path that finishes the multi-step
    // skill. The marker must clear here so the Stop hook releases
    // turn-end immediately after the COMPLETE banner — otherwise
    // the user returns to a session that refuses to stop even
    // though the skill has finished its work.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 6")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 6` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("clear-utility-in-progress"),
        "Step 6 success path must invoke `bin/flow clear-utility-in-progress` so the Stop hook releases turn-end after the skill completes"
    );
}

#[test]
fn flow_decompose_project_step2_names_sentinel_wrapping() {
    // Step 2 drafts the body shape for both the parent epic and
    // each child issue. The body must wrap its Implementation Plan
    // in the FLOW-PLAN sentinel pair so `bin/flow plan-from-issue`
    // can extract the plan at flow-start. A draft without the
    // sentinels produces an issue that the decomposed-body
    // validators (`bin/flow validate-issue-body --mode decomposed`)
    // reject with `plan_markers_missing` — the issue files but
    // cannot be consumed downstream.
    // End delimiter is `\n## Step ` so an intra-section heading
    // rendered inside a fenced markdown example block — like
    // `## Implementation Plan` inside the Body Shape Contract
    // example — cannot truncate the slice before the assertion
    // targets.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 2")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 2` section");
    let section = tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(tail);
    assert!(
        section.contains("FLOW-PLAN-BEGIN"),
        "Step 2 must name the FLOW-PLAN-BEGIN sentinel that wraps the Implementation Plan block"
    );
    assert!(
        section.contains("FLOW-PLAN-END"),
        "Step 2 must name the FLOW-PLAN-END sentinel that closes the Implementation Plan block"
    );
}

#[test]
fn flow_decompose_project_step2_names_task_header_format() {
    // The Implementation Plan's Tasks subsection uses `#### Task N:`
    // headers — this is the heading shape `bin/flow plan-from-issue`
    // counts via `count_tasks` to populate `code_tasks_total`. A
    // future drift that reverted to numbered list items would break
    // that count and produce a wrong X-of-Y annotation in the
    // Code-phase TUI. The header presence locks the format.
    // End delimiter is `\n## Step ` so an intra-section heading
    // rendered inside a fenced markdown example block — like
    // `## Implementation Plan` inside the Body Shape Contract
    // example — cannot truncate the slice before the assertion
    // targets.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 2")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 2` section");
    let section = tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        section.contains("#### Task "),
        "Step 2 must name the `#### Task N:` header format used by the Tasks subsection"
    );
    assert!(
        lower.contains("header"),
        "Step 2 must describe `#### Task N:` as the header format (not a numbered list)"
    );
}

#[test]
fn flow_decompose_project_step2_names_paraphrase_rule() {
    // Every prose reference to the FLOW-PLAN sentinel pair must
    // paraphrase the marker strings so `bin/flow plan-from-issue`
    // extraction matches the correct slice. A literal marker
    // mid-prose silently redirects extraction to the wrong bytes
    // and the validator rejects the body downstream.
    // End delimiter is `\n## Step ` so an intra-section heading
    // rendered inside a fenced markdown example block — like
    // `## Implementation Plan` inside the Body Shape Contract
    // example — cannot truncate the slice before the assertion
    // targets.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 2")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 2` section");
    let section = tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        lower.contains("paraphrase"),
        "Step 2 must name the paraphrase rule for sentinel-marker prose references"
    );
    assert!(
        lower.contains("sentinel") || lower.contains("marker"),
        "Step 2 must scope the paraphrase rule to the plan-sentinel/marker pair"
    );
}

#[test]
fn flow_decompose_project_step3_validates_before_issue() {
    // The Step 3 epic-body filing path must invoke
    // `bin/flow validate-issue-body` BEFORE `bin/flow issue` so a
    // body that `bin/flow plan-from-issue` cannot consume at
    // flow-start is rejected before it reaches GitHub. Ordering
    // matters: validating after filing makes the gate post-hoc and
    // useless. Regression: a future edit that moved the validator
    // call below `bin/flow issue`, dropped it, or gated it behind
    // a conditional would surface here.
    //
    // End delimiter `\n## Step ` bounds to Step 3 even when
    // intra-section subheadings appear.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 3")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 3` section");
    let section = tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(tail);
    let validate_pos = section
        .find("bin/flow validate-issue-body")
        .expect("`## Step 3` must invoke `bin/flow validate-issue-body`");
    let issue_pos = section
        .find("bin/flow issue")
        .expect("`## Step 3` must invoke `bin/flow issue`");
    assert!(
        validate_pos < issue_pos,
        "`bin/flow validate-issue-body` (at {}) must appear BEFORE `bin/flow issue` (at {}) in the `## Step 3` section",
        validate_pos,
        issue_pos
    );
}

#[test]
fn flow_decompose_project_hard_rules_name_validator_and_sentinels() {
    // The Hard Rules section enumerates load-bearing invariants —
    // a future maintainer reading the section must see the four
    // discipline anchors the rest of the SKILL.md depends on:
    // validator-before-filer, FLOW-PLAN sentinel wrap, `#### Task N:`
    // header format, paraphrase rule. Without these four entries
    // a future paraphrase of Step 2/3/4 could silently drop the
    // discipline and the Hard Rules wouldn't reveal the gap.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Hard Rules\n")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Hard Rules` section");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    let lower = section.to_ascii_lowercase();
    assert!(
        section.contains("validate-issue-body"),
        "Hard Rules must name `bin/flow validate-issue-body` so the pre-filing gate is locked into the discipline list"
    );
    assert!(
        section.contains("FLOW-PLAN"),
        "Hard Rules must name the FLOW-PLAN sentinel pair so the wrap discipline is locked in"
    );
    assert!(
        section.contains("#### Task"),
        "Hard Rules must name the `#### Task N:` header format so the count_tasks-compatible heading shape is locked in"
    );
    assert!(
        lower.contains("paraphrase"),
        "Hard Rules must name the paraphrase rule for sentinel-marker prose references"
    );
}

#[test]
fn flow_decompose_project_step4_validates_before_issue() {
    // The Step 4 per-child filing loop must invoke
    // `bin/flow validate-issue-body` BEFORE `bin/flow issue` for
    // each child. Same gate-contract as Step 3 — every child body
    // that `bin/flow plan-from-issue` cannot consume at flow-start
    // is rejected before it reaches GitHub. The per-child loop
    // pattern means an un-gated Step 4 would file an entire issue
    // graph of unconsumable children, requiring manual cleanup of
    // every one.
    //
    // End delimiter `\n## Step ` bounds to Step 4 even when
    // intra-section subheadings appear.
    let c = common::read_skill("flow-decompose-project");
    let tail = c
        .split_once("\n## Step 4")
        .map(|(_, t)| t)
        .expect("flow-decompose-project must have a `## Step 4` section");
    let section = tail
        .split_once("\n## Step ")
        .map(|(s, _)| s)
        .unwrap_or(tail);
    let validate_pos = section
        .find("bin/flow validate-issue-body")
        .expect("`## Step 4` must invoke `bin/flow validate-issue-body`");
    let issue_pos = section
        .find("bin/flow issue")
        .expect("`## Step 4` must invoke `bin/flow issue`");
    assert!(
        validate_pos < issue_pos,
        "`bin/flow validate-issue-body` (at {}) must appear BEFORE `bin/flow issue` (at {}) in the `## Step 4` section",
        validate_pos,
        issue_pos
    );
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

// --- flow-plan skill content contracts ---
//
// The `flow-plan` skill drives discussion-mode planning conversations
// and dispatches to PM/Tech Lead/CTO sub-agents on explicit user
// request. The four assertions below pin the load-bearing invariants
// the SKILL.md must hold so a future paraphrase or refactor cannot
// silently weaken them:
//
// 1. Refusals surface verbatim — when an agent returns a `## SCOPE
//    REFUSAL` block, the skill renders the block as-is and waits for
//    user direction. Auto-escalation, soft-re-prompting, or
//    silently-performing-the-refused-analysis would defeat the
//    scope-authority hierarchy the planning-tier agents enforce.
// 2. No state-file writes — the skill carries planning context via
//    the shared session conversation, never via `.flow-states/`
//    artifacts. A future addition of state writes would couple the
//    skill to per-branch persistence it does not need.
// 3. Role-read from `.flow.json` — the skill reads the optional
//    `role` field at session start to suggest a complementary
//    planning default. A future paraphrase that dropped the
//    role-read would break the default-persona signal.
// 4. Utility-in-progress marker — the skill sets the per-session
//    marker so the Stop hook refuses turn-end while the skill is
//    running, and clears it on every exit boundary. A future
//    paraphrase that dropped either side would either leave the
//    session deadlocked (missing clear) or break the unattended
//    contract (missing set).

#[test]
fn flow_plan_skill_surfaces_refusals_verbatim() {
    // Regression: a future edit weakens the refusal-handling
    // HARD-GATE so the model auto-escalates, re-prompts the agent
    // with softer framing, or performs the refused analysis itself
    // instead of surfacing the `## SCOPE REFUSAL` block verbatim.
    //
    // Consumer: planning-tier scope authority. PM/Tech Lead/CTO
    // produce structured refusals to escalate decisions; the skill
    // must surface them as-is so the user — not the orchestrating
    // model — chooses the next move.
    let c = common::read_skill("flow-plan");
    let mut found_gate = false;
    for (idx, _) in c.match_indices("<HARD-GATE>") {
        let tail = &c[idx..];
        let window: String = tail.lines().take(30).collect::<Vec<_>>().join("\n");
        let lower = window.to_ascii_lowercase();
        if lower.contains("render")
            && lower.contains("verbatim")
            && window.contains("SCOPE REFUSAL")
        {
            found_gate = true;
            break;
        }
    }
    assert!(
        found_gate,
        "skills/flow-plan/SKILL.md must contain a `<HARD-GATE>` block whose body within 30 lines names `render`, `verbatim`, and `SCOPE REFUSAL` so the refusal-surfacing discipline is locked in"
    );
}

#[test]
fn flow_plan_skill_no_per_branch_state_mutations() {
    // Regression: a future edit introduces per-branch FLOW state
    // mutations. flow-plan files a new GitHub issue and writes the
    // issue body file as its persistent surface; it must NOT mutate
    // a per-branch state file (which would couple the planning skill
    // to a flow that does not yet exist — the decomposed issue is
    // filed BEFORE any flow-start picks it up). Forbidden mutators:
    // set-timestamp, phase-enter, phase-finalize, phase-transition,
    // add-finding, init-state.
    //
    // Consumer: the planning lifecycle contract. flow-plan produces
    // a filed decomposed issue; the per-branch state file for any
    // future flow only comes into existence when /flow:flow-start
    // #M is invoked against that issue. Mutating per-branch state
    // here would either fail (no state file exists) or write to a
    // stale state file from an unrelated branch.
    let c = common::read_skill("flow-plan");
    let forbidden_mutators = [
        "bin/flow set-timestamp",
        "bin/flow phase-enter",
        "bin/flow phase-finalize",
        "bin/flow phase-transition",
        "bin/flow add-finding",
        "bin/flow init-state",
    ];
    for mutator in forbidden_mutators {
        assert!(
            !c.contains(mutator),
            "skills/flow-plan/SKILL.md must not invoke `{}` — the skill files a GitHub issue but does not mutate per-branch FLOW state (no flow exists yet for the decomposed issue until /flow:flow-start picks it up)",
            mutator
        );
    }
}

#[test]
fn flow_plan_skill_reads_role_from_flow_json() {
    // Regression: a future edit drops the `.flow.json` `role` read
    // at session start. Without the role read, the skill cannot
    // suggest a complementary planning default (PM → Tech Lead,
    // Tech Lead → PM, founder-solo → no preset) and the per-user
    // role-selection captured by `/flow:flow-prime` becomes
    // invisible to the discussion-mode entry point.
    //
    // Consumer: the user's primary-role selection in `.flow.json`,
    // written by `/flow:flow-prime` Step 3 and consumed by this
    // skill at session start.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains(".flow.json"),
        "skills/flow-plan/SKILL.md must reference `.flow.json` so the role-read step is locked in"
    );
    assert!(
        c.contains("role"),
        "skills/flow-plan/SKILL.md must name the `role` field so the complementary-default mapping is locked in"
    );
}

#[test]
fn every_marker_writing_skill_is_in_multi_step_allowlist() {
    // Regression: a future utility skill writes a per-session
    // marker via `bin/flow set-utility-in-progress --skill flow:<n>`
    // but is not registered in
    // `src/commands/utility_marker.rs::MULTI_STEP_UTILITY_SKILLS`.
    // The Stop hook's `check_in_progress_utility_skill` predicate
    // (src/hooks/stop_continue.rs) silently drops markers naming
    // skills outside the allowlist — the unattended-flow contract
    // breaks the first time a Skill tool returns mid-pipeline.
    //
    // Consumer: the Stop hook predicate above. Every skill that
    // sets a marker depends on the allowlist to honor it; without
    // the allowlist entry the marker is invisible to the hook and
    // the model returns control to the user mid-skill.
    use regex::Regex;
    let allowlist_path = common::repo_root()
        .join("src")
        .join("commands")
        .join("utility_marker.rs");
    let allowlist_src = std::fs::read_to_string(&allowlist_path)
        .expect("src/commands/utility_marker.rs must exist");
    let anchor = "MULTI_STEP_UTILITY_SKILLS";
    let tail = allowlist_src
        .split_once(anchor)
        .map(|(_, t)| t)
        .expect("src/commands/utility_marker.rs must declare MULTI_STEP_UTILITY_SKILLS");
    let value = tail
        .split_once(';')
        .map(|(v, _)| v)
        .expect("MULTI_STEP_UTILITY_SKILLS declaration must end with `;`");
    // Accept both `flow:`-prefixed plugin-marketplace skills (`skills/<name>/`)
    // and bare-name project-local maintainer skills (`.claude/skills/<name>/`).
    // The two skill roots emit different `input.skill` shapes per
    // `.claude/rules/user-only-skills.md` "Namespacing asymmetry," so the
    // scanner must capture both forms to honor the marker invariant across
    // every skill family.
    let marker_re =
        Regex::new(r"set-utility-in-progress\s+--skill\s+(\S+)").expect("regex must compile");
    let mut missing: Vec<(String, String)> = Vec::new();

    // Walk plugin-marketplace skills under `skills/`.
    for skill_name in common::all_skill_names() {
        let content = common::read_skill(&skill_name);
        for cap in marker_re.captures_iter(&content) {
            let skill_id = cap.get(1).unwrap().as_str().to_string();
            let needle = format!("\"{}\"", skill_id);
            if !value.contains(&needle) {
                missing.push((skill_name.clone(), skill_id));
            }
        }
    }

    // Walk project-local maintainer skills under `.claude/skills/`. These
    // are not in `common::all_skill_names()` (which only enumerates the
    // plugin-marketplace `skills/` tree). Without this branch, a future
    // bare-name maintainer skill that writes a per-session marker would
    // silently bypass the allowlist check.
    let project_skills_dir = common::repo_root().join(".claude").join("skills");
    if let Ok(entries) = std::fs::read_dir(&project_skills_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            let Ok(content) = std::fs::read_to_string(&skill_md) else {
                continue;
            };
            let skill_name = entry.file_name().to_string_lossy().to_string();
            for cap in marker_re.captures_iter(&content) {
                let skill_id = cap.get(1).unwrap().as_str().to_string();
                let needle = format!("\"{}\"", skill_id);
                if !value.contains(&needle) {
                    missing.push((format!(".claude/skills/{}", skill_name), skill_id));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Every utility skill that writes a per-session marker must be registered in MULTI_STEP_UTILITY_SKILLS so the Stop hook honors the marker. Missing entries: {:?}. Current allowlist value: `{}`",
        missing,
        value.trim()
    );
}

#[test]
fn flow_plan_skill_uses_utility_in_progress_marker() {
    // Regression: a future edit drops either the set or the clear
    // side of the per-session utility-in-progress marker. Without
    // `set-utility-in-progress`, the Stop hook returns control to
    // the user mid-conversation when a sub-agent Skill tool returns
    // — breaking the unattended-discussion contract. Without
    // `clear-utility-in-progress`, the session deadlocks because
    // the Stop hook keeps refusing turn-end after the skill has
    // already completed or cancelled.
    //
    // Consumer: the Stop hook's `check_in_progress_utility_skill`
    // predicate, which refuses turn-end while a per-session marker
    // is present for `flow:flow-plan`.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("set-utility-in-progress"),
        "skills/flow-plan/SKILL.md must invoke `bin/flow set-utility-in-progress` so the Stop hook refuses turn-end while the discussion-mode skill is running"
    );
    assert!(
        c.contains("clear-utility-in-progress"),
        "skills/flow-plan/SKILL.md must invoke `bin/flow clear-utility-in-progress` so the Stop hook releases turn-end after every exit boundary"
    );
    assert!(
        c.contains("--skill flow:flow-plan"),
        "skills/flow-plan/SKILL.md must pass `--skill flow:flow-plan` so the marker is scoped to this skill's identifier"
    );
}

#[test]
fn flow_plan_has_no_wrap_up_ask_user_question() {
    // Regression: a future edit re-introduces a wrap-up
    // AskUserQuestion gate into the Step 6 filing path. Per
    // AC#4 of issue #1488, the user's readiness signal from
    // Step 4 (Discussion Mode) is the single authorization to
    // file. The decompose pass + transform that precede Step 6
    // are unattended infrastructure; a second confirmation
    // question between the signal and the success banner breaks
    // the single-signal contract. The specific phrasing the
    // obsolete gate used was "Review the draft above. Ready to
    // file?"; catching that exact prompt locks in the
    // discipline against accidental resurrection.
    let c = common::read_skill("flow-plan");
    assert!(
        !c.contains("Review the draft above. Ready to file?"),
        "skills/flow-plan/SKILL.md must not contain the wrap-up AskUserQuestion prompt — Step 6 files directly after the decompose + transform pipeline"
    );
}

#[test]
fn flow_plan_validator_auto_fix_loop() {
    // Regression: a future edit drops the bounded auto-fix loop
    // on validator failure and replaces it with either an
    // unbounded loop (would silently file a malformed body if
    // the validator passes after many retries) or a prompt-the-
    // user gate (breaks the single-signal contract). The
    // `validator_max_retries` reason is the contract the
    // COMPLETE-FAILED banner depends on.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("validator_max_retries"),
        "skills/flow-plan/SKILL.md must name the `validator_max_retries` error reason so the structured-error contract is locked in"
    );
}

#[test]
fn flow_plan_validator_retry_cap_is_five() {
    // Regression: a future edit raises or lowers the retry cap.
    // Five attempts is the documented bound chosen so the
    // skill can iterate through every reasonable mechanical fix
    // class (sentinel placement, missing subsection, heading
    // level) but cannot loop indefinitely on a body the
    // validator will never accept. Lowering the cap would
    // prematurely fail on legitimate fix sequences; raising it
    // would mask validator bugs as productive retries.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("5 attempts") || c.contains("5 retries"),
        "skills/flow-plan/SKILL.md must name the 5-attempt cap so the bounded-loop contract is locked in"
    );
}

// --- flow-plan rewrite contract tests ---
//
// `/flow:flow-plan #N` consumes a vanilla problem-statement issue
// filed by `/flow:flow-explore` and produces a decomposed issue
// linked as blocked-by the parent. The contracts below pin the
// load-bearing invariants of the new shape: argument is `#N`,
// validator runs in decomposed mode, filer applies the decomposed
// label, link-blocked-by ties decomposed back to vanilla, and the
// issue fetch reads title/body/number/labels.

#[test]
fn flow_plan_skill_usage_requires_issue_number_argument() {
    // Regression: a future edit reverts the Conversation Gate to
    // accept bare-topic invocations (the pre-rewrite shape). The
    // role-based pipeline depends on flow-plan operating against a
    // pre-filed vanilla issue — without the `#N` argument the skill
    // would have no problem statement to plan against.
    //
    // Consumer: the role-based pipeline contract — the user types
    // `/flow:flow-explore <topic>` to file a vanilla issue, then
    // `/flow:flow-plan #N` against that issue. A bare-topic
    // flow-plan invocation breaks the contract.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("/flow:flow-plan #N"),
        "skills/flow-plan/SKILL.md Usage must show `/flow:flow-plan #N` so the issue-reference shape is documented"
    );
    // The Conversation Gate must reject bare-topic invocations with
    // a migration message naming /flow:flow-explore. Match either
    // the explicit `^#[1-9][0-9]*$` regex contract or a prose hint
    // that the `#N` form is required.
    assert!(
        c.contains("^#[1-9][0-9]*$") || c.contains("must be `#N`"),
        "skills/flow-plan/SKILL.md must reject bare-topic invocations — name the `#N` argument shape in the Conversation Gate"
    );
}

#[test]
fn flow_plan_skill_invokes_decompose() {
    // Regression: a future edit drops the `decompose:decompose`
    // Skill tool invocation in the wrap-up. Without decompose the
    // Implementation Plan would have to be hand-written by the
    // model — exactly the failure mode that motivated structuring
    // the wrap-up around decompose's DAG output in the first place.
    //
    // Consumer: the Plan-phase consumers of the decomposed issue
    // (flow-start's plan-from-issue extractor, flow-code's task
    // execution loop). Both depend on the structured task list
    // that decompose produces.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("decompose:decompose"),
        "skills/flow-plan/SKILL.md must invoke `decompose:decompose` so the Implementation Plan derives from structured DAG output"
    );
}

#[test]
fn flow_plan_skill_validates_with_decomposed_mode() {
    // Regression: a future edit drops `--mode decomposed` from the
    // validate-issue-body invocation. Without the mode flag the
    // validator defaults to decomposed (which is what we want), but
    // an explicit mode is the load-bearing contract — if the
    // default ever changes, the skill must continue to validate
    // against the decomposed shape.
    //
    // Consumer: `bin/flow validate-issue-body --mode decomposed` —
    // the validator branch that requires FLOW-PLAN sentinels and
    // an `## Implementation Plan` heading with at least one
    // `#### Task ` entry. flow-plan's wrap-up writes exactly that
    // shape; mismatched validator mode would silently accept a
    // body that plan-from-issue rejects at flow-start.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("validate-issue-body --mode decomposed"),
        "skills/flow-plan/SKILL.md must invoke `bin/flow validate-issue-body --mode decomposed` so decomposed bodies are validated against the sentinel-and-Implementation-Plan contract"
    );
}

#[test]
fn flow_plan_skill_files_with_decomposed_label() {
    // Regression: a future edit drops the `--label decomposed`
    // flag from the filing call. Without the label, `flow-issues`
    // and `flow-orchestrate` won't recognize the new issue as
    // ready-for-flow-start work — engineers picking from the
    // backlog would treat it as a bare problem statement.
    //
    // Consumer: `flow-issues` / `flow-orchestrate`, which select
    // `decomposed`-labeled issues. flow-plan's output must carry
    // the label or it becomes invisible to those readers.
    let c = common::read_skill("flow-plan");
    let mut found = false;
    for line in c.lines() {
        let trimmed = line.trim();
        if trimmed.contains("bin/flow issue") && trimmed.contains("--label decomposed") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "skills/flow-plan/SKILL.md must file the decomposed issue with `--label decomposed` on a single bin/flow issue invocation line"
    );
}

#[test]
fn flow_plan_skill_closes_parent_with_comment() {
    // Regression: a future edit drops the `bin/flow close-issue`
    // call after filing, or drops the `--comment` flag from it.
    // Without the parent-closure call, the vanilla problem-statement
    // issue stays open alongside the decomposed child, duplicating
    // the open-artifact surface for the same problem. Without the
    // `--comment` flag the closure carries no pointer to the
    // decomposed child, leaving readers no breadcrumb back from the
    // closed parent to the work that supersedes it.
    //
    // Consumer: the Step 6 wrap-up's `bin/flow close-issue --comment`
    // invocation. The Step 6 subsection scope (bounded between
    // `## Step 6 — Wrap-up` and the next `## ` heading per
    // `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
    // in Contract Tests") keeps the assertion from being satisfied
    // by an unrelated mention elsewhere in the file.
    let c = common::read_skill("flow-plan");
    let subsection = c
        .split_once("## Step 6 — Wrap-up")
        .map(|(_, tail)| tail)
        .expect("flow-plan SKILL.md must contain `## Step 6 — Wrap-up` heading");
    let subsection = subsection
        .split_once("\n## ")
        .map(|(section, _)| section)
        .unwrap_or(subsection);
    let mut found = false;
    for line in subsection.lines() {
        let trimmed = line.trim();
        if trimmed.contains("bin/flow close-issue") && trimmed.contains("--comment") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "skills/flow-plan/SKILL.md Step 6 must invoke `bin/flow close-issue` with `--comment` on a single line so the parent vanilla issue closes with a pointer to the decomposed child"
    );
}

#[test]
fn flow_plan_skill_fetches_issue_with_required_fields() {
    // Regression: a future edit changes the gh issue view JSON
    // field list. The skill needs `title` (for the decomposed
    // issue's title), `body` (for the parent context section in
    // the new body), `number` (for the close-issue call that
    // closes the vanilla parent), and `labels` (for the gate that
    // rejects already-decomposed issues). Dropping any field
    // breaks a downstream step.
    //
    // Consumer: Step 2's Fetch Vanilla Issue + the Combine into
    // Issue Body and Close Parent steps in Step 6. Each downstream
    // consumer depends on a specific field from this fetch.
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("gh issue view"),
        "skills/flow-plan/SKILL.md must invoke `gh issue view` to fetch the parent vanilla issue at Step 2"
    );
    assert!(
        c.contains("--json"),
        "skills/flow-plan/SKILL.md gh issue view must use --json to fetch structured fields"
    );
    let required_fields = ["title", "body", "number", "labels", "state"];
    for field in required_fields {
        assert!(
            c.contains(field),
            "skills/flow-plan/SKILL.md gh issue view --json field list must include `{}` so downstream steps can read it",
            field
        );
    }
}

// --- flow-explore skill content contracts ---
//
// `flow-explore` opens a problem-statement conversation (PM voice)
// and files a vanilla `## What` / `## Why` / `## Acceptance Criteria`
// issue. The contracts below pin the discipline that distinguishes
// it from `/flow:flow-plan #N` (which is the Tech-Lead-default
// implementation-decomposition pipeline): vanilla bodies must not
// carry sentinels, must not carry `## Implementation Plan`, must not
// be filed with the `decomposed` label, must validate via
// `--mode vanilla`, and must not invoke `decompose:decompose`.

#[test]
fn flow_explore_skill_does_not_invoke_decompose() {
    // Regression: a future edit adds a `decompose:decompose` Skill
    // tool invocation to flow-explore. Decomposition is implementation
    // work and belongs in `/flow:flow-plan #N` against a filed
    // problem-statement issue; embedding it in flow-explore would
    // collapse the role separation the new pipeline depends on.
    //
    // Consumer: the role-based pipeline contract — `/flow:flow-explore`
    // produces a vanilla problem statement; `/flow:flow-plan #N`
    // produces a decomposed implementation plan. Mixing the two
    // breaks both `--mode vanilla` validation and the Tech-Lead
    // role boundary.
    //
    // Implementation: scan each line containing `decompose:decompose`
    // and assert the surrounding context is prohibitive (Hard Rule
    // mention) rather than imperative (a directive to invoke the
    // Skill). Prohibitive cues: `never`, `not`, `do not`, `must not`,
    // `forbids`. Imperative cues that would fail the gate:
    // `Invoke <name>`, `via the Skill tool`, `using the Skill tool`.
    let c = common::read_skill("flow-explore");
    for (line_idx, line) in c.lines().enumerate() {
        if !line.contains("decompose:decompose") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let prohibitive = lower.contains("never")
            || lower.contains("not invoke")
            || lower.contains("must not")
            || lower.contains("do not")
            || lower.contains("forbid")
            || lower.contains("forbids");
        // Prohibitive context wins: a line whose surface matches an
        // imperative pattern but which is wrapped in `Never invoke`
        // / `must not invoke` is prohibitive and acceptable. Only
        // flag imperative mentions that lack any prohibitive cue.
        assert!(
            prohibitive,
            "skills/flow-explore/SKILL.md line {} mentions `decompose:decompose` outside a prohibitive context. Every mention must be a Hard Rule or in-prose prohibition (containing `never`, `not invoke`, `must not`, `do not`, or `forbid`). Decomposition belongs in `/flow:flow-plan #N`.",
            line_idx + 1
        );
    }
}

#[test]
fn flow_explore_skill_uses_vanilla_validator_mode() {
    // Regression: a future edit drops `--mode vanilla` from the
    // validate-issue-body invocation, or invokes the validator
    // without any mode flag (which defaults to `decomposed` and
    // would reject every flow-explore body for missing FLOW-PLAN
    // sentinels).
    //
    // Consumer: `bin/flow validate-issue-body --mode vanilla` —
    // the only validator branch that accepts a What/Why/Acceptance
    // body without sentinels or an Implementation Plan heading.
    let c = common::read_skill("flow-explore");
    assert!(
        c.contains("validate-issue-body --mode vanilla"),
        "skills/flow-explore/SKILL.md must invoke `bin/flow validate-issue-body --mode vanilla` so vanilla bodies are validated against the problem-statement contract, not the decomposed contract"
    );
}

#[test]
fn flow_explore_skill_files_with_vanilla_label() {
    // Regression: a future edit drops `--label vanilla` from the
    // flow-explore filing call. Without the label, vanilla problem-
    // statement issues land unlabeled — `gh issue list`, the
    // `/flow:flow-issues` dashboard, and any future label-based
    // triage tooling cannot distinguish vanilla problem statements
    // from decomposed implementation issues at a glance.
    //
    // Consumer: `gh issue list` and `/flow:flow-issues` filter and
    // group issues by origin label. The paired origin labels
    // (`vanilla` for problem statements, `decomposed` for
    // pre-planned implementation issues) make provenance visible
    // without opening the issue body.
    let c = common::read_skill("flow-explore");
    let mut found = false;
    for line in c.lines() {
        let trimmed = line.trim();
        if trimmed.contains("bin/flow issue") && trimmed.contains("--label vanilla") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "skills/flow-explore/SKILL.md must file the vanilla issue with `--label vanilla` on a single bin/flow issue invocation line"
    );
}

#[test]
fn flow_explore_skill_files_without_decomposed_label() {
    // Regression: a future edit adds `--label decomposed` to the
    // flow-explore filing call. The `decomposed` label is reserved
    // for issues filed by `/flow:flow-plan #N` and
    // `/flow:flow-decompose-project`; flow-explore files vanilla
    // problem statements that `flow-issues` and `flow-orchestrate`
    // must not pick up as ready-for-flow-start work.
    //
    // Consumer: `flow-issues` / `flow-orchestrate`, which select
    // `decomposed`-labeled issues. Mis-labeling a vanilla
    // problem-statement issue would let an engineer or the
    // overnight orchestrator try to start a flow on an issue that
    // has no Implementation Plan.
    let c = common::read_skill("flow-explore");
    // Find every `bin/flow issue ` invocation and verify none carry
    // `--label decomposed`. The skill may legitimately mention the
    // label in prose ("without --label decomposed"); the assertion
    // is scoped to lines that are actual filing invocations.
    for (line_idx, line) in c.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.contains("bin/flow issue ") && !trimmed.contains("bin/flow issue\t") {
            continue;
        }
        // A line that contains both `bin/flow issue` and `--label decomposed`
        // is a filing invocation that violates the contract.
        assert!(
            !trimmed.contains("--label decomposed"),
            "skills/flow-explore/SKILL.md line {} files an issue with `--label decomposed`; vanilla problem statements must not carry the decomposed label",
            line_idx + 1
        );
    }
}

#[test]
fn flow_explore_has_no_utility_marker_calls() {
    // Regression: a future edit re-introduces marker calls into
    // `skills/flow-explore/SKILL.md`. The skill is excluded from
    // `crate::commands::utility_marker::MULTI_STEP_UTILITY_SKILLS`
    // because it never invokes `decompose:decompose` — the Stop
    // hook's decompose-return gate cannot fire on its behalf.
    // Adding marker calls back would create dead code: the marker
    // would be written but the predicate would always drop it via
    // the allowlist check, leaving the user mystified about why
    // discussion replies still ended the turn cleanly. The
    // regression ships silent unless this scan catches it.
    let c = common::read_skill("flow-explore");
    assert!(
        !c.contains("set-utility-in-progress"),
        "skills/flow-explore/SKILL.md must not invoke `bin/flow set-utility-in-progress` — flow-explore is excluded from MULTI_STEP_UTILITY_SKILLS and the Stop hook ignores its marker"
    );
    assert!(
        !c.contains("clear-utility-in-progress"),
        "skills/flow-explore/SKILL.md must not invoke `bin/flow clear-utility-in-progress` — there is no marker to clear"
    );
}

#[test]
fn flow_explore_has_no_wrap_up_ask_user_question() {
    // Regression: a future edit re-introduces a wrap-up
    // AskUserQuestion gate into the Step 5 filing path. Per the
    // discussion-mode contract, the user's readiness signal
    // ("ready", "file it", "let's go") is the single authorization
    // to file — a second confirmation prompt breaks AC#4
    // (single-signal filing). The specific phrasing the obsolete
    // gate used was "Review the draft above. Ready to file?";
    // catching that exact prompt locks in the discipline against
    // accidental resurrection.
    let c = common::read_skill("flow-explore");
    assert!(
        !c.contains("Review the draft above. Ready to file?"),
        "skills/flow-explore/SKILL.md must not contain the wrap-up AskUserQuestion prompt — Step 5 files directly on the user's readiness signal"
    );
}

#[test]
fn flow_explore_validator_auto_fix_loop() {
    // Regression: a future edit drops the bounded auto-fix loop and
    // replaces it with either an unbounded loop (would silently
    // file a malformed body if the validator passes after many
    // retries) or a prompt-the-user gate (breaks the single-signal
    // contract). The 5-attempt cap is the documented bound and the
    // `validator_max_retries` reason is the contract the COMPLETE-
    // FAILED banner depends on.
    let c = common::read_skill("flow-explore");
    assert!(
        c.contains("validator_max_retries"),
        "skills/flow-explore/SKILL.md must name the `validator_max_retries` error reason so the structured-error contract is locked in"
    );
    assert!(
        c.contains("5 attempts") || c.contains("5 retries"),
        "skills/flow-explore/SKILL.md must name the 5-attempt cap so the bounded-loop contract is locked in"
    );
}

// --- validate_pretool escape-hatch citation contract ---
//
// Every escape-hatch-class block message in
// `src/hooks/validate_pretool.rs` must cite
// `.claude/rules/no-escape-hatches.md`. The citation lets a future
// reader looking at a block message trace the rule that the layer
// enforces. Five classes are escape-hatch-class: Layer 1 (compound
// commands and command substitution), Layer 2 (shell redirection),
// Layer 3 (exec prefix), Layer 4 (destructive find), and Layer 7
// (settings-driven deny list).
//
// Layer 7.5 (structural escape-hatch program/flag block) and
// Layer 9-active-flow (skill-commit gate) are also escape-hatch-class
// — Task 6 and Tasks 7-10 already added the citation. They are
// included in the assertion below to lock the citation in place
// across future refactors.
//
// Layer 5 (`git restore .`), Layer 6 (`git diff` with file args),
// Layer 8 (whitelist enforcement is config-driven, not escape-hatch),
// and the Layer 9-integration-branch path (workflow protection
// rather than escape-hatch) are exempt because their block messages
// describe a different protection class. The integration-branch
// message keeps the citation as a bonus — it was added alongside
// the active-flow citation in Tasks 7-10 — but the contract only
// asserts the escape-hatch-class layers.

#[test]
fn validate_pretool_escape_hatch_messages_cite_rule() {
    let root = common::repo_root();
    let src_path = root.join("src").join("hooks").join("validate_pretool.rs");
    let content = fs::read_to_string(&src_path).expect("validate_pretool.rs must exist");

    // Bounded-slice helper: walk to the first occurrence of `start`,
    // then walk to the first occurrence of `end` in the tail,
    // returning the substring between. The end marker is the
    // following layer's section header so each scope is exactly the
    // layer's block-message section.
    fn slice<'a>(content: &'a str, start: &str, end: &str) -> &'a str {
        let tail = content
            .split_once(start)
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("missing start marker `{}` in validate_pretool.rs", start));
        tail.split_once(end)
            .map(|(s, _)| s)
            .unwrap_or_else(|| panic!("missing end marker `{}` in validate_pretool.rs", end))
    }

    const CITATION: &str = "See .claude/rules/no-escape-hatches.md";

    // Layer 1 — compound commands and command substitution. The
    // block message lives between `// Layer 1` and `// Layer 2`.
    let layer1 = slice(&content, "// Layer 1:", "// Layer 2:");
    assert!(
        layer1.contains(CITATION),
        "Layer 1 (compound commands) block message must cite no-escape-hatches.md; layer body:\n{}",
        layer1
    );

    // Layer 2 — shell redirection.
    let layer2 = slice(&content, "// Layer 2:", "// Layer 3:");
    assert!(
        layer2.contains(CITATION),
        "Layer 2 (shell redirection) block message must cite no-escape-hatches.md; layer body:\n{}",
        layer2
    );

    // Layer 3 — exec prefix.
    let layer3 = slice(&content, "// Layer 3:", "// Layer 4:");
    assert!(
        layer3.contains(CITATION),
        "Layer 3 (exec prefix) block message must cite no-escape-hatches.md; layer body:\n{}",
        layer3
    );

    // Layer 4 — destructive find flags.
    let layer4 = slice(&content, "// Layer 4:", "// Layer 5:");
    assert!(
        layer4.contains(CITATION),
        "Layer 4 (destructive find) block message must cite no-escape-hatches.md; layer body:\n{}",
        layer4
    );

    // Layer 7 — settings-driven deny list. Exempt layers (5, 6)
    // sit between Layer 4 and Layer 7.
    let layer7 = slice(&content, "// Layer 7:", "// Layer 7.5:");
    assert!(
        layer7.contains(CITATION),
        "Layer 7 (deny list) block message must cite no-escape-hatches.md; layer body:\n{}",
        layer7
    );

    // Layer 7.5 — structural escape-hatch program/flag block (Task
    // 6). The actual block messages live inside the
    // `check_escape_hatch_structural` helper function — Layer 7.5's
    // section in `validate()` just dispatches to the helper. Scope
    // the citation assertion to the helper's function body so the
    // contract tests each block-message string produced by Layer
    // 7.5's match arms (one per escape-hatch family). The end
    // marker is the next function definition
    // (`fn strip_env_and_wrappers`) — function-boundary markers
    // prevent a future refactor from accidentally shrinking the
    // slice via a common Rust pattern like `_ => None,` appearing
    // elsewhere in the file.
    let layer7_5_helper = slice(
        &content,
        "fn check_escape_hatch_structural",
        "\nfn strip_env_and_wrappers",
    );
    assert!(
        layer7_5_helper.contains(CITATION),
        "Layer 7.5 helper `check_escape_hatch_structural` block messages must cite no-escape-hatches.md; function body:\n{}",
        layer7_5_helper
    );

    // Layer 9 active-flow message function. The
    // `commit_block_message_active_flow` definition is the source of
    // every active-flow block message — assert the citation lives
    // inside the function body. End marker is the next item
    // declaration so the slice covers the whole function body
    // including the `format!` interpolation's `{}` braces.
    let layer9_active = slice(
        &content,
        "fn commit_block_message_active_flow",
        "/// Run Layer 9",
    );
    assert!(
        layer9_active.contains(CITATION),
        "Layer 9 active-flow block message must cite no-escape-hatches.md; function body:\n{}",
        layer9_active
    );
}

// --- REQUIRED_AGENTS ↔ SKILL.md binding ---

/// `flow_rs::required_agents::REQUIRED_AGENTS` is the authoritative
/// per-phase required-agent set the `phase-finalize` gate composes
/// against `agents_returned` / `agents_skipped`. This contract test
/// binds the constant to the matching SKILL.md invocation set: a
/// SKILL.md edit that adds, removes, or renames an
/// `subagent_type: "flow:<name>"` invocation without updating the
/// constant fails CI.
#[test]
fn required_agents_matches_skill_invocations() {
    let re = Regex::new("subagent_type[^\"]*\"flow:([a-z][a-z0-9_-]*)\"").unwrap();
    for (phase, expected) in flow_rs::required_agents::REQUIRED_AGENTS {
        let skill = common::read_skill(phase);
        let mut found: Vec<String> = re
            .captures_iter(&skill)
            .map(|cap| cap[1].to_string())
            .collect();
        found.sort_unstable();
        found.dedup();
        let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        want.sort_unstable();
        assert_eq!(
            found, want,
            "REQUIRED_AGENTS for {} does not match SKILL.md `subagent_type: \"flow:<name>\"` invocations.\n  REQUIRED_AGENTS: {:?}\n  found in SKILL: {:?}",
            phase, want, found
        );
    }
}

// --- flow-review Step 2 cross-launch-window prohibition ---

/// flow-review Step 2's launch HARD-GATE must forbid tool calls
/// between the first agent's launch and the fourth agent's return,
/// and Step 2 must keep the post-launch anchor that marks where
/// classify-and-record work resumes.
///
/// Regression guarded: a future Step 2 edit reorders, removes,
/// rewords, inverts, or fragments the cross-launch-window
/// prohibition, letting the model interleave per-agent
/// `record-agent-return` / `set-timestamp --set agent_retry_counts`
/// / `add-skipped-agent` calls between agent launches. Step 2's
/// `### Per-agent accounting` subsection reads as a per-agent
/// narrative, so without the explicit gate the model's most
/// mechanical reading of Step 2 is launch-wait-classify-record per
/// agent — serializing four launches that are designed to run
/// concurrently.
///
/// Code path: a refactor of Step 2 that reorders, removes, or
/// rewords the launch HARD-GATE's protective phrases, or that
/// deletes the post-launch anchor.
///
/// Named consumer: the parallel-isolation invariant in
/// `.claude/rules/cognitive-isolation.md` and the wall-clock budget
/// Review pays per flow — four sequential agent runs instead of one
/// concurrent batch.
///
/// Assertion strength: substring-presence checks alone are
/// bypassable — a permissive reword keeps the substrings, and
/// fragmented incidental mentions of the endpoints keep them too.
/// The assertions below pin a prohibition keyword, the contiguous
/// launch-window phrase, and the concrete `record-agent-return`
/// call name — and anchor the gate slice to the launch directive
/// rather than gate position, so reordering Step 2's two HARD-GATE
/// blocks cannot redirect the assertions onto the wrong block.
#[test]
fn flow_review_step_2_hard_gate_forbids_per_agent_bash_during_launch() {
    let c = common::read_skill("flow-review");

    // Bounded slice: Step 2 only (per .claude/rules/testing-gotchas.md
    // "Subsection-Local Assertions in Contract Tests").
    let step2 = c
        .split_once("## Step 2 — Launch agents")
        .map(|(_, t)| t)
        .expect("flow-review SKILL.md must contain `## Step 2 — Launch agents`");
    let step2 = step2
        .split_once("## Step 3 — Triage")
        .map(|(s, _)| s)
        .unwrap_or(step2);

    // Step 2 contains more than one <HARD-GATE> block. Anchor to the
    // LAUNCH gate by content (the block mandating single-response
    // launch of all agents), not by position — reordering the two
    // HARD-GATE blocks must not redirect the assertions.
    let launch_gate = step2
        .split("<HARD-GATE>")
        .filter_map(|tail| tail.split_once("</HARD-GATE>").map(|(block, _)| block))
        .find(|block| block.contains("launch ALL applicable agents"))
        .expect(
            "flow-review Step 2 must contain a <HARD-GATE> block that mandates \
             launching ALL applicable agents in a single response",
        );

    // The launch-window constraint must read as an explicit
    // prohibition — an inverted permissive reword must not pass.
    assert!(
        launch_gate.contains("Issue NO other tool call")
            || launch_gate.contains("MUST NOT run during this launch window"),
        "flow-review Step 2 launch HARD-GATE must state the launch-window \
         constraint as an explicit prohibition (`Issue NO other tool call` or \
         `MUST NOT run during this launch window`) — a permissive reword must \
         not pass — see .claude/rules/cognitive-isolation.md"
    );
    // The launch window must be named as a single contiguous phrase
    // so fragmented incidental mentions of the two endpoints cannot
    // satisfy the gate.
    assert!(
        launch_gate.contains("between the first agent's launch and the fourth agent's return"),
        "flow-review Step 2 launch HARD-GATE must name the launch window as the \
         contiguous phrase `between the first agent's launch and the fourth \
         agent's return` — see .claude/rules/cognitive-isolation.md"
    );
    // The concrete forbidden-action name pins the prohibition to a
    // specific call rather than a generic `Bash` mention.
    assert!(
        launch_gate.contains("record-agent-return"),
        "flow-review Step 2 launch HARD-GATE must name `record-agent-return` as \
         a classify-and-record call forbidden during the launch window — see \
         .claude/rules/cognitive-isolation.md"
    );
    // The protective change has a second part: the post-launch
    // anchor marking where classify-and-record work resumes. It
    // lives below the launch HARD-GATE, so assert it against the
    // full Step 2 slice.
    assert!(
        step2.contains("**After all four agents have returned.**"),
        "flow-review Step 2 must keep the `**After all four agents have \
         returned.**` post-launch anchor that marks where classify-and-record \
         work resumes — see .claude/rules/cognitive-isolation.md"
    );
}

// --- persistence-routing CLAUDE.md scope ---

/// `.claude/rules/persistence-routing.md` must carry the obey-vs-describe
/// test that gates CLAUDE.md routing, name the three alternative
/// destinations for descriptive content, and expose two sections that
/// scope what CLAUDE.md is and is not for. Without the gate, descriptive
/// project knowledge routes to CLAUDE.md and compounds token cost across
/// every session.
///
/// The destination-name assertions are bounded to the
/// `## What CLAUDE.md Is Not For` slice via the bounded-slice pattern
/// from `.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
/// in Contract Tests". Without the bound, a future edit that guts the
/// destination bullets from `## What CLAUDE.md Is Not For` would still
/// pass because the same lowercase phrasing appears in the file's Tests
/// section bullet.
#[test]
fn persistence_routing_has_obey_vs_describe_test() {
    let path = PathBuf::from(".claude/rules/persistence-routing.md");
    let content =
        fs::read_to_string(&path).expect(".claude/rules/persistence-routing.md must exist");
    assert!(
        content.contains("obey-vs-describe test"),
        "persistence-routing.md must name the `obey-vs-describe test` as the \
         gate on CLAUDE.md routing"
    );
    assert!(
        content.contains("## What CLAUDE.md Is For"),
        "persistence-routing.md must include a `## What CLAUDE.md Is For` \
         section naming the two acceptable CLAUDE.md content shapes"
    );
    assert!(
        content.contains("## What CLAUDE.md Is Not For"),
        "persistence-routing.md must include a `## What CLAUDE.md Is Not \
         For` section naming the three alternative destinations"
    );
    // Bound destination-name assertions to the `## What CLAUDE.md Is
    // Not For` slice so the substring matches only when the destination
    // section itself names them — not when an unrelated section
    // mentions the same lowercase phrase. Assertions target the
    // canonical bullet shapes (`- **<Name>**`) the section uses to
    // enumerate the three destinations.
    let tail = content
        .split_once("\n## What CLAUDE.md Is Not For\n")
        .map(|(_, t)| t)
        .expect("persistence-routing.md must contain `## What CLAUDE.md Is Not For` heading");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("- **Module doc comment**"),
        "`## What CLAUDE.md Is Not For` section must list `Module doc \
         comment` as a bullet destination"
    );
    assert!(
        section.contains("- **`docs/` subtree**"),
        "`## What CLAUDE.md Is Not For` section must list the `docs/` \
         subtree as a bullet destination"
    );
    assert!(
        section.contains("- **Discard**"),
        "`## What CLAUDE.md Is Not For` section must list `Discard` as a \
         bullet destination"
    );
}

// --- flow-learn Step 3 obey-vs-describe gate ---

/// `flow-learn` Step 3's `### Apply CLAUDE.md changes` subsection must
/// gate on the obey-vs-describe test before routing any finding into
/// CLAUDE.md. Without the gate, descriptive findings route into
/// CLAUDE.md and compound token cost across every session.
///
/// The gate must appear BEFORE the `**Compose**` instruction so a
/// future edit cannot bury the gate after the write-mechanics
/// instructions. Burying the gate at the bottom of the subsection
/// would silently break the gating semantics — the model would
/// execute Compose/Read/Write/Apply before ever consulting the
/// gate, defeating the protection.
///
/// The split anchor `"\n### Apply CLAUDE.md changes\n"` bounds the
/// match to the heading at column 0 followed by a newline. Without
/// the heading-shape anchor, inline prose mentioning the
/// subsection by name in the Routing section's cross-reference
/// would satisfy the split and produce a sliced subsection that
/// includes unrelated content from earlier in the file.
#[test]
fn flow_learn_step_3_has_obey_vs_describe_gate() {
    let content = common::read_skill("flow-learn");
    let tail = content
        .split_once("\n### Apply CLAUDE.md changes\n")
        .map(|(_, t)| t)
        .expect("flow-learn must contain `### Apply CLAUDE.md changes` heading");
    let subsection = tail.split_once("\n### ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        subsection.contains("obey-vs-describe test"),
        "flow-learn Step 3 `### Apply CLAUDE.md changes` must gate on the \
         `obey-vs-describe test` before routing into CLAUDE.md"
    );
    assert!(
        subsection.contains("module doc comment"),
        "flow-learn Step 3 `### Apply CLAUDE.md changes` must name `module \
         doc comment` as an alternative destination"
    );
    assert!(
        subsection.contains("`docs/`"),
        "flow-learn Step 3 `### Apply CLAUDE.md changes` must name the \
         `docs/` subtree as an alternative destination"
    );
    assert!(
        subsection.contains("discard"),
        "flow-learn Step 3 `### Apply CLAUDE.md changes` must name `discard` \
         as an alternative destination"
    );
    // Gate ordering: the obey-vs-describe gate must appear BEFORE
    // the first `**Compose**` instruction. A future edit that
    // moves the gate after Compose would silently break the
    // gating semantics.
    let gate_pos = subsection
        .find("obey-vs-describe test")
        .expect("gate phrase asserted above");
    let compose_pos = subsection.find("**Compose**").expect(
        "flow-learn Step 3 `### Apply CLAUDE.md changes` must contain a `**Compose**` instruction",
    );
    assert!(
        gate_pos < compose_pos,
        "flow-learn Step 3 obey-vs-describe gate must appear BEFORE the \
         `**Compose**` instruction; burying the gate after Compose defeats \
         the gating semantics"
    );
}

// --- docs-with-behavior Key Files entry shape ---

/// `.claude/rules/docs-with-behavior.md` `## What Counts` section must
/// clarify that Key Files entries are name + 1-line purpose only.
/// Without the bound, descriptions of how the artifact works route into
/// CLAUDE.md instead of the module doc comment.
#[test]
fn docs_with_behavior_key_files_bullet_clarifies_shape() {
    let path = PathBuf::from(".claude/rules/docs-with-behavior.md");
    let content =
        fs::read_to_string(&path).expect(".claude/rules/docs-with-behavior.md must exist");
    let tail = content
        .split_once("## What Counts")
        .map(|(_, t)| t)
        .expect("docs-with-behavior.md must contain `## What Counts`");
    let section = tail.split_once("\n## ").map(|(s, _)| s).unwrap_or(tail);
    assert!(
        section.contains("name + 1-line purpose only"),
        "docs-with-behavior.md `## What Counts` must clarify Key Files \
         entries as `name + 1-line purpose only`"
    );
    assert!(
        section.contains("module doc comment"),
        "docs-with-behavior.md `## What Counts` must route descriptions \
         of how the artifact works to the `module doc comment`"
    );
}

/// Project-local skills at `.claude/skills/<name>/SKILL.md` run only
/// in the FLOW repo where bare `bin/flow` resolves directly. The
/// `${CLAUDE_PLUGIN_ROOT}` prefix is for plugin-marketplace skills
/// under `skills/<name>/SKILL.md` that need to resolve in target
/// projects — applying it to a project-local skill triggers Claude
/// Code's "Contains expansion" permission prompt mid-autonomous-flow
/// with no benefit. See `.claude/rules/skill-authoring.md` "Plugin
/// Root for bin/flow".
///
/// The scanner matches the literal `${CLAUDE_PLUGIN_ROOT}` anywhere
/// in each project-local SKILL.md (not just inside ` ```bash `
/// fences). The broader match closes bypass surfaces that surfaced
/// during Review: sibling fence shapes (` ```sh `, ` ```shell `,
/// ` ```zsh `, ` ~~~bash `, indented code blocks) and bash composition
/// (`${CLAUDE_PLUGIN_ROOT}""/bin/flow`, `${CLAUDE_PLUGIN_ROOT}/bin/$(printf flow)`)
/// all collapse to the single check "does any occurrence of
/// `${CLAUDE_PLUGIN_ROOT}` exist in this file?" Project-local skills
/// have no legitimate need for the prefix in any context — teaching
/// commentary belongs in `.claude/rules/skill-authoring.md`, which
/// the scanner does not walk.
///
/// The scanner walks direct-child entries under `.claude/skills/`
/// only — nested `.claude/skills/<group>/<name>/SKILL.md` layouts
/// are out of scope by design (no such layout exists in the project).
/// When the directory itself cannot be read, the scanner fails closed
/// per `.claude/rules/security-gates.md` "Fail Closed When State Is
/// Unreliable" — a missing or unreadable `.claude/skills/` directory
/// is a repo-structure regression, not a benign empty state.
#[test]
fn no_claude_skills_use_plugin_root_expansion() {
    const FORBIDDEN: &str = "${CLAUDE_PLUGIN_ROOT}";
    let mut violations = Vec::new();

    let project_skills_dir = common::repo_root().join(".claude").join("skills");
    let entries = fs::read_dir(&project_skills_dir).unwrap_or_else(|e| {
        panic!(
            "fail-closed: `.claude/skills/` must exist and be readable for the scanner to enforce the project-local bin/flow rule (read_dir error: {})",
            e
        )
    });
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        let Ok(content) = fs::read_to_string(&skill_md) else {
            continue;
        };
        let skill_name = entry.file_name().to_string_lossy().to_string();
        for (line_no, line) in content.lines().enumerate() {
            if line.contains(FORBIDDEN) {
                violations.push(format!(
                    ".claude/skills/{}/SKILL.md:{} — {}",
                    skill_name,
                    line_no + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Project-local skills at `.claude/skills/<name>/SKILL.md` must not contain `${{CLAUDE_PLUGIN_ROOT}}` anywhere — the prefix exists only for plugin-marketplace skills under `skills/<name>/SKILL.md`, and any occurrence in a project-local skill triggers Claude Code's \"Contains expansion\" permission prompt. Teaching commentary about the prefix belongs in `.claude/rules/skill-authoring.md`. See `.claude/rules/skill-authoring.md` \"Plugin Root for bin/flow\". Violations:\n{}",
        violations.join("\n")
    );
}
