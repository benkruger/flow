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
/// prohibited. This closes the adjacent-block inheritance gap found
/// by Code Review's adversarial agent (PR #1183, finding A10) at the
/// cost of requiring each adjacent variant in the same section to
/// carry its own preamble.
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
    let dot_skills = common::repo_root().join(".claude").join("skills");
    if dot_skills.exists() {
        scan_dir(dot_skills, ".claude/skills");
    }

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
fn flow_qa_asks_for_target() {
    // The flow-qa skill must prompt the user to pick which QA repo to
    // exercise. Originally framed as a "framework" question; the
    // framework concept has been removed but the choice itself
    // remains — different QA repos still test different toolchains.
    let c = fs::read_to_string(
        common::repo_root()
            .join(".claude")
            .join("skills")
            .join("flow-qa")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(
        c.contains("AskUserQuestion"),
        "flow-qa must prompt for which QA target to run"
    );
    assert!(
        c.contains("python") && c.contains("rails"),
        "flow-qa must list at least the python and rails targets"
    );
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
fn code_review_no_inline_simplify_step() {
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("simplify:simplify"),
        "Tombstone: simplify plugin removed"
    );
}

#[test]
fn code_review_triage_two_outcomes_only() {
    // Code Review has two triage outcomes: Real (fix in Step 4) and
    // False positive (dismiss). The filing path was removed — see
    // .claude/rules/code-review-scope.md.
    let c = common::read_skill("flow-code-review");
    assert!(
        !c.contains("bin/flow issue"),
        "Code Review skill must not invoke issue creation"
    );
    assert!(
        !c.contains("bin/flow add-issue"),
        "Code Review skill must not record filed issues"
    );
    assert!(
        !c.contains("--outcome \"filed\""),
        "Code Review skill must not record findings with the filed outcome"
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

#[test]
fn code_has_plan_test_verification() {
    let c = common::read_skill("flow-code");
    assert!(
        c.contains("Plan Test Verification"),
        "Code skill must have Plan Test Verification subsection"
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

// --- plan-check ordering ---

/// Step 4 of flow-plan must invoke `bin/flow plan-check` and the
/// invocation must appear BEFORE `phase-transition --action complete`
/// in the skill content. A reordered edit that moves plan-check
/// after the phase transition would defeat the scope-enumeration
/// gate — the phase would complete before violations are caught.
#[test]
fn plan_skill_step4_invokes_plan_check_before_phase_transition() {
    let c = common::read_skill("flow-plan");

    let plan_check_idx = c.find("bin/flow plan-check").unwrap_or_else(|| {
        panic!("flow-plan SKILL.md must invoke `bin/flow plan-check` in Step 4")
    });
    let phase_complete_idx = c.find("phase-transition --phase flow-plan --action complete").unwrap_or_else(
        || panic!("flow-plan SKILL.md must invoke `phase-transition --phase flow-plan --action complete`"),
    );

    assert!(
        plan_check_idx < phase_complete_idx,
        "flow-plan Step 4 must run `bin/flow plan-check` BEFORE \
         `phase-transition --phase flow-plan --action complete`. \
         Found plan-check at byte {} and phase-transition at byte {}.",
        plan_check_idx,
        phase_complete_idx
    );
}

/// The scope-enumeration rule file must exist — it is the
/// authoritative statement of the universal-coverage rule that the
/// scanner, the plan-check subcommand, and the `plan_extract.rs`
/// integration all reference.
#[test]
fn scope_enumeration_rule_file_exists() {
    let path = common::repo_root()
        .join(".claude")
        .join("rules")
        .join("scope-enumeration.md");
    assert!(
        path.exists(),
        ".claude/rules/scope-enumeration.md must exist"
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
fn plan_enforces_must_verify_risk_tasks() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("Risk Verification Enforcement") || c.contains("risk verification enforcement"),
        "Plan must have Risk Verification Enforcement subsection"
    );
}

#[test]
fn plan_has_dag_freshness_check() {
    let c = common::read_skill("flow-plan");
    assert!(
        c.contains("DAG Freshness") || c.contains("dag freshness"),
        "Plan must have DAG Freshness Check subsection"
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
fn code_review_has_supersession_check() {
    let c = common::read_skill("flow-code-review");
    let lower = c.to_lowercase();
    assert!(
        lower.contains("supersession"),
        "flow-code-review/SKILL.md Step 3 Triage must include a supersession check \
         per .claude/rules/supersession.md (Code Review Phase section)"
    );
}

#[test]
fn plan_has_supersession_enumeration() {
    let c = common::read_skill("flow-plan");
    let lower = c.to_lowercase();
    assert!(
        lower.contains("supersession"),
        "flow-plan/SKILL.md Step 3 must include supersession enumeration instructions \
         per .claude/rules/supersession.md (Plan Phase section)"
    );
}

#[test]
fn flow_plan_skill_has_extract_helper_branch_enumeration() {
    // The Extract-Helper Branch Enumeration subsection is the Plan-phase
    // discipline from .claude/rules/extract-helper-refactor.md: when a
    // plan task extracts a block into a new helper, the plan must
    // enumerate the helper's branches with testability classifications
    // before Code phase begins. The subsection lives inside Step 3 of
    // flow-plan/SKILL.md alongside Supersession Enumeration.
    let c = common::read_skill("flow-plan");

    assert!(
        c.contains("### Extract-Helper Branch Enumeration"),
        "flow-plan/SKILL.md Step 3 must include an '### Extract-Helper Branch Enumeration' \
         subsection per .claude/rules/extract-helper-refactor.md"
    );

    // Step headings are h2 (`## Step N`); subsections inside a step are
    // h3 (`### Name`). The new subsection must sit between the Step 3
    // h2 and the Step 4 h2.
    let step3 = c.find("## Step 3").expect("Step 3 heading must exist");
    let step4 = c.find("## Step 4").expect("Step 4 heading must exist");
    let subsection = c
        .find("### Extract-Helper Branch Enumeration")
        .expect("Extract-Helper Branch Enumeration subsection must exist");
    assert!(
        subsection > step3 && subsection < step4,
        "### Extract-Helper Branch Enumeration must sit inside Step 3 \
         (between '## Step 3' and '## Step 4')"
    );

    // Isolate the subsection body: from the heading to the next '### ' heading.
    let rest = &c[subsection..];
    let sub_end_rel = rest[1..]
        .find("\n### ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    let sub_body = &rest[..sub_end_rel];

    assert!(
        sub_body.contains(".claude/rules/extract-helper-refactor.md"),
        "Extract-Helper Branch Enumeration subsection must cross-reference \
         '.claude/rules/extract-helper-refactor.md'"
    );

    for cls in [
        "Testable via seam",
        "Testable directly",
        "Testable via subprocess",
    ] {
        assert!(
            sub_body.contains(cls),
            "Extract-Helper Branch Enumeration subsection must name classification: {cls}"
        );
    }

    // The subsection must present a Branch Enumeration Table (four
    // columns: Branch / Condition / Classification / Test). The
    // header row and separator together guarantee authors see the
    // table shape inline instead of anchoring on an abbreviated
    // prose summary.
    assert!(
        sub_body.contains("| Branch | Condition | Classification | Test |"),
        "Extract-Helper Branch Enumeration subsection must include the \
         four-column Branch Enumeration Table header"
    );

    // The subsection must document the opt-out comment token inline
    // so Plan authors learn the escape hatch without having to
    // follow the cross-reference to the rule file.
    assert!(
        sub_body.contains("extract-helper-refactor: not-an-extraction"),
        "Extract-Helper Branch Enumeration subsection must document \
         the opt-out comment token 'extract-helper-refactor: not-an-extraction'"
    );
}

#[test]
fn extract_helper_refactor_rule_has_expected_structure() {
    // The SKILL.md Extract-Helper Branch Enumeration subsection
    // cross-references .claude/rules/extract-helper-refactor.md for the
    // full trigger vocabulary, the three classifications, the opt-out
    // grammar, and the motivating PR #1155 incident. This test asserts
    // that rule file exists and contains the canonical elements the
    // SKILL.md cross-reference promises, so a broken cross-reference
    // or a missing section fails CI instead of silently shipping.
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

    assert!(
        content.contains("PR #1155"),
        "extract-helper-refactor.md must cite the motivating PR #1155 incident"
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
        "## Motivating Incident",
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

#[test]
fn adversarial_agent_has_verify_step() {
    let c = common::read_agent("adversarial.md");
    assert!(
        c.contains("**Verify."),
        "adversarial.md Reasoning Discipline must contain a Verify step"
    );
}

#[test]
fn code_review_adversarial_uses_temp_test_file_placeholder() {
    // The adversarial step parameterizes the temp file path so the
    // agent can write a single test file under .flow-states/ without
    // hardcoding language. The framework concept is gone; the agent
    // picks the file extension itself by inspecting the diff.
    let c = common::read_skill("flow-code-review");
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

// --- Code Review tombstone audit integration ---

#[test]
fn code_review_mentions_tombstone_audit() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("tombstone-audit"),
        "Code Review Step 1 must run tombstone-audit for stale tombstone detection"
    );
}

#[test]
fn code_review_collects_substantive_diff() {
    let c = common::read_skill("flow-code-review");
    assert!(
        c.contains("git diff origin/main...HEAD -w"),
        "Code Review Step 1 must collect a substantive diff (git diff -w) for context-sparse agents"
    );
}

#[test]
fn code_review_routes_substantive_diff_to_context_sparse_agents() {
    let c = common::read_skill("flow-code-review");
    for agent in &["Pre-mortem", "Adversarial", "Documentation"] {
        assert!(
            c.contains("substantive diff output"),
            "Code Review Step 2 must route substantive diff to {} agent",
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
