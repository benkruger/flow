//! Structural invariant tests for FLOW plugin configuration files.
//!
//! These tests verify config consistency, hook registration, version
//! definitions, agent frontmatter, version parity, and tombstone guards.

mod common;

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use regex::Regex;
use serde_json::Value;

// --- Phase structure tests ---

#[test]
fn test_phases_has_1_through_5() {
    let data = common::load_phases();
    let order = data["order"].as_array().expect("missing 'order' array");
    assert_eq!(
        order.len(),
        5,
        "Expected 5 phases in order, got {}",
        order.len()
    );
    let phases = data["phases"].as_object().expect("missing 'phases' object");
    for key_val in order {
        let key = key_val.as_str().unwrap();
        assert!(
            phases.contains_key(key),
            "Phase '{}' in order but missing from phases",
            key
        );
    }
    assert_eq!(phases.len(), 5);
}

#[test]
fn test_commands_match_flow_pattern() {
    let data = common::load_phases();
    let re = Regex::new(r"^/flow:[\w-]+$").unwrap();
    let phases = data["phases"].as_object().unwrap();
    for (key, phase) in phases {
        let cmd = phase["command"].as_str().unwrap();
        assert!(
            re.is_match(cmd),
            "Phase '{}' command '{}' doesn't match /flow:<name> pattern",
            key,
            cmd
        );
    }
}

#[test]
fn test_can_return_to_references_valid_lower_phases() {
    let data = common::load_phases();
    let order: Vec<&str> = data["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let phases = data["phases"].as_object().unwrap();
    for (key, phase) in phases {
        let key_index = order.iter().position(|&k| k == key).unwrap();
        let can_return_to = phase["can_return_to"].as_array().unwrap();
        for target_val in can_return_to {
            let target = target_val.as_str().unwrap();
            assert!(
                phases.contains_key(target),
                "Phase '{}' can_return_to references non-existent phase '{}'",
                key,
                target
            );
            let target_index = order.iter().position(|&k| k == target).unwrap();
            assert!(
                target_index < key_index,
                "Phase '{}' can_return_to references same or higher phase '{}'",
                key,
                target
            );
        }
    }
}

#[test]
fn test_commands_are_unique() {
    let data = common::load_phases();
    let phases = data["phases"].as_object().unwrap();
    let commands: Vec<&str> = phases
        .values()
        .map(|p| p["command"].as_str().unwrap())
        .collect();
    let unique: HashSet<&str> = commands.iter().copied().collect();
    assert_eq!(
        commands.len(),
        unique.len(),
        "Duplicate commands found: {:?}",
        commands
            .iter()
            .filter(|c| commands.iter().filter(|c2| c2 == c).count() > 1)
            .collect::<Vec<_>>()
    );
}

// --- Version parity ---

#[test]
fn test_version_matches_across_files() {
    let root = common::repo_root();
    let plugin: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap();
    let marketplace: Value = serde_json::from_str(
        &fs::read_to_string(root.join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    let v_plugin = plugin["version"].as_str().unwrap();
    let v_meta = marketplace["metadata"]["version"].as_str().unwrap();
    let v_entry = marketplace["plugins"][0]["version"].as_str().unwrap();
    assert_eq!(
        v_plugin, v_meta,
        "plugin.json ({}) != marketplace metadata ({})",
        v_plugin, v_meta
    );
    assert_eq!(
        v_plugin, v_entry,
        "plugin.json ({}) != marketplace plugins[0] ({})",
        v_plugin, v_entry
    );
}

// --- Skill directory invariants ---

#[test]
fn test_every_skill_dir_has_skill_md() {
    let skills = common::skills_dir();
    let mut entries: Vec<_> = fs::read_dir(&skills)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let skill_md = entry.path().join("SKILL.md");
        assert!(
            skill_md.exists(),
            "skills/{}/ has no SKILL.md",
            entry.file_name().to_string_lossy()
        );
    }
}

#[test]
fn test_every_skill_dir_starts_with_flow_prefix() {
    let skills = common::skills_dir();
    let mut entries: Vec<_> = fs::read_dir(&skills)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            name.starts_with("flow-"),
            "skills/{}/ does not start with 'flow-' prefix",
            name
        );
    }
}

// --- Hook invariants ---

#[test]
fn test_hooks_json_references_existing_files() {
    let root = common::repo_root();
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    for (_event, matchers) in hook_map {
        let matchers_arr = matchers.as_array().unwrap();
        for matcher in matchers_arr {
            let hook_list = matcher["hooks"].as_array().unwrap();
            for hook in hook_list {
                let cmd = hook["command"].as_str().unwrap();
                let resolved = cmd.replace("${CLAUDE_PLUGIN_ROOT}", &root.to_string_lossy());
                let script_path = resolved.split_whitespace().next().unwrap();
                let path = PathBuf::from(script_path);
                let exists = if path.is_absolute() {
                    path.exists()
                } else {
                    root.join(script_path).exists()
                };
                assert!(exists, "Hook command references non-existent file: {}", cmd);
            }
        }
    }
}

#[test]
fn test_hook_scripts_are_executable() {
    let root = common::repo_root();
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    let mut non_executable: Vec<String> = Vec::new();
    for matchers in hook_map.values() {
        let matchers_arr = matchers.as_array().unwrap();
        for matcher in matchers_arr {
            let hook_list = matcher["hooks"].as_array().unwrap();
            for hook in hook_list {
                let cmd = hook["command"].as_str().unwrap();
                let resolved = cmd.replace("${CLAUDE_PLUGIN_ROOT}", &root.to_string_lossy());
                let script_path = resolved.split_whitespace().next().unwrap();
                let path = if PathBuf::from(script_path).is_absolute() {
                    PathBuf::from(script_path)
                } else {
                    root.join(script_path)
                };
                if path.exists() {
                    let mode = fs::metadata(&path).unwrap().permissions().mode();
                    if mode & 0o111 == 0 {
                        let rel = path
                            .strip_prefix(&root)
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
                        non_executable.push(rel);
                    }
                }
            }
        }
    }
    assert!(
        non_executable.is_empty(),
        "Hook scripts missing execute permission: {}",
        non_executable.join(", ")
    );
}

#[test]
fn test_hooks_json_has_pretooluse_bash_validator() {
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    assert!(
        hook_map.contains_key("PreToolUse"),
        "hooks.json missing PreToolUse key -- the global Bash validator must be registered"
    );
    let matchers = hook_map["PreToolUse"].as_array().unwrap();
    let bash_matchers: Vec<&Value> = matchers
        .iter()
        .filter(|m| m["matcher"].as_str().is_some_and(|s| s.contains("Bash")))
        .collect();
    assert_eq!(
        bash_matchers.len(),
        1,
        "Expected exactly 1 Bash-matching matcher in PreToolUse, got {}",
        bash_matchers.len()
    );
    let matcher_str = bash_matchers[0]["matcher"].as_str().unwrap();
    assert!(
        matcher_str.contains("Agent"),
        "PreToolUse Bash validator must also match Agent tool (matcher should be 'Bash|Agent')"
    );
    let commands: Vec<&str> = bash_matchers[0]["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .any(|cmd| cmd.contains("bin/flow hook validate-pretool")),
        "PreToolUse Bash hook must reference bin/flow hook validate-pretool"
    );
}

#[test]
fn test_hooks_json_uses_bin_flow_hook_for_pretool_validators() {
    let root = common::repo_root();
    let hooks_content = fs::read_to_string(root.join("hooks/hooks.json")).unwrap();
    for name in &[
        "validate-pretool",
        "validate-claude-paths",
        "validate-worktree-paths",
        "validate-ask-user",
    ] {
        let legacy = format!("lib/{}.py", name);
        assert!(
            !hooks_content.contains(&legacy),
            "hooks.json must not reference {} -- use bin/flow hook {} instead",
            legacy,
            name
        );
    }
}

#[test]
fn test_bin_flow_fails_closed_for_hook_subcommand() {
    // bin/flow must exit 2 (block) not 1 (error) when the hook subcommand has no handler.
    let root = common::repo_root();
    let bin_flow = fs::read_to_string(root.join("bin/flow")).unwrap();
    assert!(
        bin_flow.contains(r#"if [ "$subcmd" = "hook" ]; then"#),
        "bin/flow must have a hook-specific fail-closed branch"
    );
    let hook_branch_start = bin_flow.find(r#"if [ "$subcmd" = "hook" ]; then"#).unwrap();
    let hook_branch_end = bin_flow[hook_branch_start..].find("fi").unwrap() + hook_branch_start;
    let hook_branch = &bin_flow[hook_branch_start..hook_branch_end];
    assert!(
        hook_branch.contains("exit 2"),
        "Hook fail-closed branch must use exit 2 (block), not exit 1 (error)"
    );
}

#[test]
fn test_hooks_json_read_glob_grep_consolidated() {
    // Read, Glob, Grep must share a single matcher entry in hooks.json.
    let hooks = common::load_hooks();
    let matchers = hooks["hooks"]["PreToolUse"].as_array().unwrap();
    let read_matchers: Vec<&Value> = matchers
        .iter()
        .filter(|m| m["matcher"].as_str() == Some("Read"))
        .collect();
    let glob_matchers: Vec<&Value> = matchers
        .iter()
        .filter(|m| m["matcher"].as_str() == Some("Glob"))
        .collect();
    let grep_matchers: Vec<&Value> = matchers
        .iter()
        .filter(|m| m["matcher"].as_str() == Some("Grep"))
        .collect();
    assert!(
        read_matchers.is_empty(),
        "Read should not have a separate matcher entry"
    );
    assert!(
        glob_matchers.is_empty(),
        "Glob should not have a separate matcher entry"
    );
    assert!(
        grep_matchers.is_empty(),
        "Grep should not have a separate matcher entry"
    );
    let combined: Vec<&Value> = matchers
        .iter()
        .filter(|m| {
            m["matcher"]
                .as_str()
                .is_some_and(|s| s.contains("Read") && s.contains("Glob") && s.contains("Grep"))
        })
        .collect();
    assert_eq!(
        combined.len(),
        1,
        "Expected exactly 1 combined Read|Glob|Grep matcher, got {}",
        combined.len()
    );
}

#[test]
fn test_hooks_json_has_no_exit_plan_validator() {
    // Tombstone: hooks.json must NOT register an ExitPlanMode hook -- plan mode removed.
    let hooks = common::load_hooks();
    let matchers = hooks["hooks"]["PreToolUse"].as_array().unwrap();
    let exit_plan_matchers: Vec<&Value> = matchers
        .iter()
        .filter(|m| m["matcher"].as_str() == Some("ExitPlanMode"))
        .collect();
    assert!(
        exit_plan_matchers.is_empty(),
        "ExitPlanMode hook should not exist -- plan mode was removed. Found {} matcher(s)",
        exit_plan_matchers.len()
    );
}

#[test]
fn test_hooks_json_has_post_compact_hook() {
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    assert!(
        hook_map.contains_key("PostCompact"),
        "hooks.json missing PostCompact key -- the compaction data capture hook must be registered"
    );
    let matchers = hook_map["PostCompact"].as_array().unwrap();
    assert!(
        !matchers.is_empty(),
        "PostCompact hook must have at least one entry"
    );
    let commands: Vec<&str> = matchers
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        commands.iter().any(|cmd| cmd.contains("hook post-compact")),
        "PostCompact hook must reference bin/flow hook post-compact"
    );
}

#[test]
fn test_hooks_json_has_stop_continue_hook() {
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    assert!(
        hook_map.contains_key("Stop"),
        "hooks.json missing Stop key -- the continuation hook must be registered"
    );
    let matchers = hook_map["Stop"].as_array().unwrap();
    assert!(
        !matchers.is_empty(),
        "Stop hook must have at least one entry"
    );
    let commands: Vec<&str> = matchers
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .any(|cmd| cmd.contains("hook stop-continue")),
        "Stop hook must reference bin/flow hook stop-continue"
    );
}

#[test]
fn test_hooks_json_has_stop_failure_hook() {
    let hooks = common::load_hooks();
    let hook_map = hooks["hooks"].as_object().unwrap();
    assert!(
        hook_map.contains_key("StopFailure"),
        "hooks.json missing StopFailure key -- the API error capture hook must be registered"
    );
    let matchers = hook_map["StopFailure"].as_array().unwrap();
    assert!(
        !matchers.is_empty(),
        "StopFailure hook must have at least one entry"
    );
    let commands: Vec<&str> = matchers
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        commands.iter().any(|cmd| cmd.contains("hook stop-failure")),
        "StopFailure hook must reference bin/flow hook stop-failure"
    );
}

// --- Script test file coverage ---

#[test]
fn test_every_script_has_a_test_file() {
    // Every shell script in hooks/ and executable in bin/ must have a test file.
    let root = common::repo_root();
    let hooks = common::hooks_dir();
    let bin = common::bin_dir();
    let mut missing: Vec<String> = Vec::new();

    // Check hooks/*.sh
    let mut sh_files: Vec<_> = fs::read_dir(&hooks)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "sh"))
        .collect();
    sh_files.sort_by_key(|e| e.file_name());
    for sh in &sh_files {
        let stem = sh
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace('-', "_");
        let rs_test = root.join(format!("tests/{}.rs", stem));
        if !rs_test.exists() {
            let rel = sh
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            missing.push(rel);
        }
    }

    // Check bin/ executables
    let mut bin_files: Vec<_> = fs::read_dir(&bin)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                && fs::metadata(e.path())
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        })
        .collect();
    bin_files.sort_by_key(|e| e.file_name());
    for f in &bin_files {
        let stem = f
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace('-', "_");
        let rs_test = root.join(format!("tests/bin_{}.rs", stem));
        if !rs_test.exists() {
            let rel = f
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            missing.push(rel);
        }
    }

    assert!(
        missing.is_empty(),
        "Scripts without test files: {}",
        missing.join(", ")
    );
}

// --- CLAUDE.md invariants ---

#[test]
fn test_claude_md_has_no_lessons_learned_section() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(
        !content.contains("## Lessons Learned"),
        "CLAUDE.md still has a '## Lessons Learned' section -- learnings belong in rules files, not CLAUDE.md"
    );
}

// --- Plugin invariants ---

// --- plugin.json invariants ---

#[test]
fn test_plugin_json_has_no_config_hash() {
    let root = common::repo_root();
    let content = fs::read_to_string(root.join(".claude-plugin/plugin.json")).unwrap();
    let plugin: Value = serde_json::from_str(&content).unwrap();
    assert!(
        plugin.get("config_hash").is_none(),
        "plugin.json must not contain config_hash -- Claude Code's plugin validator rejects unrecognized keys"
    );
}

// --- Agent frontmatter tests ---

const SUPPORTED_AGENT_FRONTMATTER_KEYS: &[&str] = &[
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
];

fn parse_agent_frontmatter(agent_file: &PathBuf) -> serde_yaml::Value {
    let content = fs::read_to_string(agent_file).unwrap();
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    assert!(
        parts.len() >= 3,
        "{} missing YAML frontmatter delimiters",
        agent_file.file_name().unwrap().to_string_lossy()
    );
    let frontmatter: serde_yaml::Value = serde_yaml::from_str(parts[1]).unwrap_or_else(|e| {
        panic!(
            "{} has invalid YAML frontmatter: {}",
            agent_file.file_name().unwrap().to_string_lossy(),
            e
        )
    });
    assert!(
        frontmatter.is_mapping(),
        "{} frontmatter is not a dict",
        agent_file.file_name().unwrap().to_string_lossy()
    );
    frontmatter
}

fn agent_md_files() -> Vec<PathBuf> {
    let agents = common::agents_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&agents)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}

#[test]
fn test_agent_frontmatter_only_supported_keys() {
    let supported: HashSet<&str> = SUPPORTED_AGENT_FRONTMATTER_KEYS.iter().copied().collect();

    for agent_file in agent_md_files() {
        let frontmatter = parse_agent_frontmatter(&agent_file);
        let mapping = frontmatter.as_mapping().unwrap();
        let file_name = agent_file
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut unsupported: Vec<String> = Vec::new();
        for key in mapping.keys() {
            let key_str = key.as_str().unwrap();
            if !supported.contains(key_str) {
                unsupported.push(key_str.to_string());
            }
        }
        assert!(
            unsupported.is_empty(),
            "{} has unsupported frontmatter keys: {:?}. Supported keys: {:?}",
            file_name,
            unsupported,
            {
                let mut s: Vec<&str> = supported.iter().copied().collect();
                s.sort();
                s
            }
        );
    }
}

#[test]
fn test_all_agents_specify_model() {
    let expected_models: std::collections::HashMap<&str, &str> = [
        ("ci-fixer.md", "opus"),
        ("adversarial.md", "opus"),
        ("reviewer.md", "sonnet"),
        ("pre-mortem.md", "sonnet"),
        ("learn-analyst.md", "haiku"),
        ("documentation.md", "haiku"),
        ("issue-triage.md", "sonnet"),
        ("pm.md", "haiku"),
        ("tech-lead.md", "sonnet"),
        ("cto.md", "opus"),
    ]
    .into_iter()
    .collect();

    for agent_file in agent_md_files() {
        let file_name = agent_file
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let frontmatter = parse_agent_frontmatter(&agent_file);
        let mapping = frontmatter.as_mapping().unwrap();
        assert!(
            mapping.contains_key(serde_yaml::Value::String("model".to_string())),
            "{} missing 'model' key in frontmatter -- agents without an explicit model inherit from the parent session",
            file_name
        );
        let model = mapping
            .get(serde_yaml::Value::String("model".to_string()))
            .unwrap()
            .as_str()
            .unwrap();
        let expected = expected_models.get(file_name.as_str());
        assert!(
            expected.is_some(),
            "{} not in expected_models map -- add it when creating a new agent",
            file_name
        );
        assert_eq!(
            model,
            *expected.unwrap(),
            "{} has model={:?}, expected {:?}",
            file_name,
            model,
            expected.unwrap()
        );
    }
}

// --- issue-triage agent contract ---
//
// `agents/issue-triage.md` is the PM-lens triage agent. The three
// contracts below lock in (a) frontmatter shape (name, model, tools,
// disallowedTools), (b) the canonical 10-question lens count and
// ordering, and (c) the 5-field verdict card. Each test guards a
// distinct regression: silent model downgrade or permission
// widening, dropped/added questions, verdict-card field drift.

fn read_issue_triage_agent() -> String {
    let path = common::agents_dir().join("issue-triage.md");
    fs::read_to_string(&path).expect("agents/issue-triage.md must exist")
}

/// Parse a comma-separated YAML scalar value into a sorted list of
/// trimmed tokens. The frontmatter `tools` and `disallowedTools`
/// values are written as YAML comma-separated strings (e.g.
/// `tools: Read, Glob, Grep, Bash`); `as_str()` returns the
/// concatenated string, and we split-trim to recover the canonical
/// list so contract assertions can do exact-equality checks rather
/// than substring matches (per adversarial findings A4/A5 — a
/// substring check accepts `Read_Editor` as if it were `Read`).
fn split_tools(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[test]
fn test_issue_triage_agent_frontmatter() {
    let path = common::agents_dir().join("issue-triage.md");
    let frontmatter = parse_agent_frontmatter(&path);
    let mapping = frontmatter.as_mapping().unwrap();

    let name = mapping
        .get(serde_yaml::Value::String("name".to_string()))
        .and_then(|v| v.as_str())
        .expect("issue-triage frontmatter missing name");
    assert_eq!(
        name, "issue-triage",
        "issue-triage agent name must be 'issue-triage'"
    );

    let model = mapping
        .get(serde_yaml::Value::String("model".to_string()))
        .and_then(|v| v.as_str())
        .expect("issue-triage frontmatter missing model");
    assert_eq!(
        model, "sonnet",
        "issue-triage agent must use model=sonnet for parity with reviewer/pre-mortem"
    );

    // Exact-equality check on each tool token: split on `,`, trim,
    // require the set to equal {Read, Glob, Grep, Bash}. Substring
    // matching would accept malformed `Read_Editor` as if it were
    // `Read` (A4).
    let tools_raw = mapping
        .get(serde_yaml::Value::String("tools".to_string()))
        .and_then(|v| v.as_str())
        .expect("issue-triage frontmatter missing tools");
    let tools = split_tools(tools_raw);
    let expected_tools: std::collections::HashSet<&str> =
        ["Read", "Glob", "Grep", "Bash"].into_iter().collect();
    let actual_tools: std::collections::HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        actual_tools, expected_tools,
        "issue-triage tools must be exactly {{Read, Glob, Grep, Bash}}: got {tools_raw:?}"
    );

    // Same exact-equality discipline for disallowedTools (A5 —
    // `Edit_Self` would substring-match `Edit` while leaving the
    // real Edit tool enabled).
    let disallowed_raw = mapping
        .get(serde_yaml::Value::String("disallowedTools".to_string()))
        .and_then(|v| v.as_str())
        .expect("issue-triage frontmatter missing disallowedTools — Edit and Write must be explicitly forbidden");
    let disallowed = split_tools(disallowed_raw);
    let expected_disallowed: std::collections::HashSet<&str> =
        ["Edit", "Write"].into_iter().collect();
    let actual_disallowed: std::collections::HashSet<&str> =
        disallowed.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        actual_disallowed, expected_disallowed,
        "issue-triage disallowedTools must be exactly {{Edit, Write}}: got {disallowed_raw:?}"
    );
}

#[test]
fn test_issue_triage_agent_enumerates_ten_questions() {
    let content = read_issue_triage_agent();
    // Each marker must appear as a line whose trimmed prefix
    // begins with `### N. <heading>`. Line-prefix anchoring
    // defeats prose-mention bypass (A3) — the previous
    // `content.find()` accepted matches anywhere in the file
    // including inside running prose like "answer question 1.
    // Real?". The line must START with the marker, which
    // structurally identifies a heading regardless of whether
    // it lives inside the agent's Output Format fenced
    // template or in prose elsewhere.
    let markers = [
        "### 1. Real",
        "### 2. Still real",
        "### 3. Framing",
        "### 4. What",
        "### 5. Why care",
        "### 6. Who",
        "### 7. Urgency",
        "### 8. How would this be fixed",
        "### 9. What success looks like",
        "### 10. Risk of the fix",
    ];

    let mut found: Vec<Option<usize>> = vec![None; markers.len()];
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        for (i, marker) in markers.iter().enumerate() {
            if found[i].is_none() && trimmed.starts_with(marker) {
                found[i] = Some(idx);
                break;
            }
        }
    }

    let mut last_line: Option<usize> = None;
    for (i, marker) in markers.iter().enumerate() {
        let line = found[i].unwrap_or_else(|| {
            panic!("agents/issue-triage.md must enumerate question marker {marker:?} as a `### ` line prefix")
        });
        if let Some(prev) = last_line {
            assert!(
                line > prev,
                "agents/issue-triage.md question {marker:?} appears out of order (line {line}, previous {prev})"
            );
        }
        last_line = Some(line);
    }
}

#[test]
fn test_issue_triage_agent_verdict_card_fields() {
    let content = read_issue_triage_agent();
    // Lock the five verdict-card fields and the canonical disposition set.
    for field in [
        "Disposition",
        "Summary",
        "Evidence",
        "Confidence",
        "This flips if",
    ] {
        assert!(
            content.contains(field),
            "agents/issue-triage.md verdict card must include field: {field}"
        );
    }
    // Strict assertion: pin the canonical disposition markers — the
    // verdict-template bracketed form `{close | decompose}` AND the
    // Disposition Semantics bullet shape `- **close**` /
    // `- **decompose**`. A bare `content.contains("close")` was
    // satisfied by non-disposition prose ("Refuse closed", "the closed
    // set", "gh issue close"), so dropping the canonical markers while
    // leaving prose intact would have passed silently.
    assert!(
        content.contains("{close | decompose}"),
        "agents/issue-triage.md verdict template must contain the canonical disposition form `{{close | decompose}}`"
    );
    for bullet in ["- **close**", "- **decompose**"] {
        assert!(
            content.contains(bullet),
            "agents/issue-triage.md Disposition Semantics must contain bullet: {bullet}"
        );
    }
}

// --- Checksum/version invariant ---

#[test]
fn test_checksum_version_invariant() {
    // Validate hash computation works and the upgrade mechanism is documented.
    use sha2::{Digest, Sha256};

    let root = common::repo_root();

    // 1. Verify setup_hash from Rust source
    let rust_source = root.join("src/prime_setup.rs");
    let content = fs::read(&rust_source).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let setup_hash: String = format!("{:x}", hasher.finalize())
        .chars()
        .take(12)
        .collect();
    assert_eq!(setup_hash.len(), 12);
    let hex_re = Regex::new(r"^[0-9a-f]{12}$").unwrap();
    assert!(
        hex_re.is_match(&setup_hash),
        "setup_hash is not 12 hex chars: {}",
        setup_hash
    );

    // 2. Verify config_hash via prime-setup subprocess
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    // Initialize a git repo in the temp directory
    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert!(git_init.status.success(), "git init failed");

    // Configure git identity for CI environments without global config
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(tmp_path)
        .output()
        .unwrap();

    let git_commit = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert!(git_commit.status.success(), "git commit failed");

    let result = std::process::Command::new(root.join("bin/flow").to_str().unwrap())
        .args(["prime-setup", tmp_path.to_str().unwrap()])
        .output()
        .expect("Failed to run bin/flow prime-setup");
    assert!(
        result.status.success(),
        "prime-setup failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let flow_json = fs::read_to_string(tmp_path.join(".flow.json")).unwrap();
    let flow_data: Value = serde_json::from_str(&flow_json).unwrap();
    let config_hash = flow_data["config_hash"].as_str().unwrap();
    assert_eq!(config_hash.len(), 12);
    assert!(
        hex_re.is_match(config_hash),
        "config_hash is not 12 hex chars: {}",
        config_hash
    );

    // 3. Verify CLAUDE.md documents the invariant
    let claude_md = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(
        claude_md.contains("Checksum \u{2192} Version Invariant"),
        "CLAUDE.md must document the checksum -> version invariant"
    );
}

// --- Adversarial probe must not be tracked by git ---

/// The Review adversarial probe at `tests/test_adversarial_flow.rs`
/// is an ephemeral file: the adversarial agent writes it during the
/// Review phase, and `src/cleanup.rs::delete_adversarial_probe` removes it
/// from the worktree at Phase 5 Complete. The path is excluded from
/// staging via the `test_adversarial_flow.*` pattern in
/// `src/prime_check.rs::EXCLUDE_ENTRIES`.
///
/// The exclude pattern only stops new untracked files from being
/// staged — once the path becomes tracked, `git add -A` stages
/// modifications to it and the worktree-side cleanup cannot remove
/// the tracked state on the integration branch. This test asserts
/// the path is absent from `git ls-files`, catching any commit that
/// would resurrect a tracked stub or commit a probe.
#[test]
fn adversarial_probe_must_not_be_tracked() {
    let root = common::repo_root();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "tests/test_adversarial_flow.rs"])
        .output()
        .expect("git ls-files must succeed");
    assert!(
        output.status.success(),
        "git ls-files exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "tests/test_adversarial_flow.rs must not be tracked by git. \
         The file is the Review adversarial probe — it is \
         written ephemerally during the Review phase and disposed of by \
         `src/cleanup.rs::delete_adversarial_probe` at Phase 5. \
         A tracked stub or committed probe defeats the cleanup. \
         Found in git ls-files: {}",
        stdout.trim()
    );
}
