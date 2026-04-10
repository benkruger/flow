// Bash permission coverage tests.
//
// Every Bash command in every skill must have a matching permission entry.
// Prohibition tests enforce that bash blocks never contain patterns that
// break permission matching or shell behavior.

mod common;

use std::collections::HashSet;
use std::fs;

use flow_rs::utils::permission_to_regex;
use regex::Regex;
use serde_json::Value;

// --- Helpers ---

fn all_plugin_skill_files() -> Vec<(String, String)> {
    let skills_dir = common::skills_dir();
    let repo = common::repo_root();
    let mut result = Vec::new();
    for entry in fs::read_dir(&skills_dir).unwrap().flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        for md in fs::read_dir(entry.path()).unwrap().flatten() {
            let path = md.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let rel = path
                    .strip_prefix(&repo)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let content = fs::read_to_string(&path).unwrap();
                result.push((rel, content));
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

fn all_docs_files() -> Vec<(String, String)> {
    common::collect_md_files(&common::docs_dir())
        .into_iter()
        .map(|(rel, content)| (format!("docs/{}", rel), content))
        .collect()
}

fn maintainer_files() -> Vec<(String, String)> {
    let dir = common::repo_root().join(".claude").join("skills");
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let content = fs::read_to_string(&skill_md).unwrap();
                    files.push((format!(".claude/skills/{}/SKILL.md", name), content));
                }
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn all_check_files() -> Vec<(String, String)> {
    let mut files = all_plugin_skill_files();
    files.extend(all_docs_files());
    files
}

fn all_check_files_with_maintainer() -> Vec<(String, String)> {
    let mut files = all_check_files();
    files.extend(maintainer_files());
    files
}

fn extract_bash_blocks_from_content(content: &str) -> Vec<String> {
    common::extract_bash_blocks(content)
}

fn extract_prime_permissions_block() -> Value {
    let content = common::read_skill("flow-prime");
    let re = Regex::new(r"```json\s*\n([\s\S]*?)```").unwrap();
    let placeholder_cleaner = Regex::new(r"<[^>]+>").unwrap();
    for cap in re.captures_iter(&content) {
        let block = &cap[1];
        if block.contains("\"permissions\"") && block.contains("\"allow\"") {
            let cleaned = placeholder_cleaner
                .replace_all(block, "placeholder")
                .to_string();
            if let Ok(parsed) = serde_json::from_str::<Value>(&cleaned) {
                if let Some(perms) = parsed.get("permissions") {
                    return perms.clone();
                }
            }
        }
    }
    panic!("Could not find permissions JSON in prime/SKILL.md");
}

fn extract_prime_allow() -> Vec<String> {
    let perms = extract_prime_permissions_block();
    perms["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn extract_prime_deny() -> Vec<String> {
    let perms = extract_prime_permissions_block();
    perms["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn build_regexes(permissions: &[String]) -> Vec<Regex> {
    permissions
        .iter()
        .filter_map(|p| permission_to_regex(p))
        .collect()
}

fn concrete_example(perm: &str) -> Option<String> {
    let re = Regex::new(r"^Bash\((.+)\)$").unwrap();
    let cap = re.captures(perm)?;
    Some(cap[1].replace('*', "test-value"))
}

const PLACEHOLDER_SUBS: &[(&str, &str)] = &[
    ("<feature-name>", "test-feature"),
    ("<branch>", "test-branch"),
    ("<project_root>", "/tmp/test"),
    ("<framework>", "rails"),
    ("<worktree_path>", ".worktrees/test-branch"),
    ("<pr_number>", "123"),
    ("<note_text>", "test note"),
    ("<current-branch>", "test-branch"),
    ("<test/path/to/file_test.rb>", "test/models/user_test.rb"),
    ("<tests/path/to/test_file.py>", "tests/test_foo.py"),
    ("<N>", "flow-code"),
    ("<name>", "flow-code"),
    ("<path>", "design.approved_at"),
    ("<plan_file_path>", ".flow-states/test-branch-plan.md"),
    ("<dag_file_path>", ".flow-states/test-branch-dag.md"),
    ("<issue_title>", "Test issue title"),
    (
        "<transcript_path>",
        "~/.claude/projects/-tmp-test/abc123.jsonl",
    ),
    ("<issue_number>", "107"),
    ("<issue_url>", "https://github.com/test/test/issues/1"),
    ("<n>", "3"),
    ("<file>", "lib/foo.py"),
    (
        "<skills_dict_json>",
        r#"{"flow-start":{"continue":"manual"}}"#,
    ),
    ("<commit_format>", "full"),
    ("<index>", "0"),
    ("<outcome>", "completed"),
    ("<pr_url>", "https://github.com/test/test/pull/1"),
    ("<title>", "Test title"),
    ("<reason>", "CI failed"),
    ("<ts>", "1234567890.123456"),
    ("<thread_ts>", "1234567890.123456"),
    ("<message_text>", "Phase complete"),
    ("<id>", "abc12345"),
    ("<repo>", "owner/repo"),
    ("<project_name>", "My Project"),
    ("<due_date>", "2026-06-01"),
    ("<child_number>", "5"),
    ("<epic_number>", "1"),
    ("<dep_number>", "3"),
    ("<topic>", "testing-gotchas"),
    ("<description>", "Update contract tests"),
    ("<slack_thread_ts>", "1234567890.123456"),
];

fn substitute_placeholders(line: &str) -> Option<String> {
    let repo = common::repo_root();
    let mut result = line.replace("${CLAUDE_PLUGIN_ROOT}", &repo.to_string_lossy());
    for (placeholder, value) in PLACEHOLDER_SUBS {
        result = result.replace(placeholder, value);
    }
    let re = Regex::new(r"<[a-zA-Z_/-]+>").unwrap();
    if re.is_match(&result) {
        return None;
    }
    Some(result)
}

fn extract_primary_command(bash_block: &str) -> Option<String> {
    let blockquote_re = Regex::new(r"^>\s*").unwrap();
    let lines: Vec<String> = bash_block
        .trim()
        .lines()
        .map(|l| blockquote_re.replace(l, "").to_string())
        .collect();
    let line = lines.join("\n").trim().to_string();

    if line.contains("COMMAND") {
        return None;
    }

    let line = substitute_placeholders(&line)?;
    // Strip cd prefix
    let cd_re = Regex::new(r"^cd\s+\S+\s*&&\s*").unwrap();
    let line = cd_re.replace(&line, "").to_string();
    // Take first command before ;
    let line = if line.contains(';') && !line.contains("&&") {
        line.split(';').next().unwrap().to_string()
    } else {
        line
    };
    let line = line.trim().to_string();
    // Collapse multi-line
    let line = Regex::new(r"\s*\\\n\s*")
        .unwrap()
        .replace_all(&line, " ")
        .to_string();
    // Take first line
    let line = line.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

fn extract_full_command(bash_block: &str) -> Option<String> {
    let blockquote_re = Regex::new(r"^>\s*").unwrap();
    let lines: Vec<String> = bash_block
        .trim()
        .lines()
        .map(|l| blockquote_re.replace(l, "").to_string())
        .collect();
    let line = lines.join("\n").trim().to_string();

    if line.contains("COMMAND") {
        return None;
    }

    let line = substitute_placeholders(&line)?;
    // NOTE: cd prefix NOT stripped
    let line = if line.contains(';') && !line.contains("&&") {
        line.split(';').next().unwrap().to_string()
    } else {
        line
    };
    let line = line.trim().to_string();
    let line = Regex::new(r"\s*\\\n\s*")
        .unwrap()
        .replace_all(&line, " ")
        .to_string();
    let line = line.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

const AUTO_ALLOWED: &[&str] = &["cd"];

fn is_auto_allowed(cmd: &str) -> bool {
    AUTO_ALLOWED
        .iter()
        .any(|a| cmd == *a || cmd.starts_with(&format!("{} ", a)))
}

const SENSITIVE_PATH_COMMANDS: &[&str] = &["rm -rf ~/.claude/plugins/cache/flow-marketplace"];

fn logging_skills() -> Vec<String> {
    let re = Regex::new(r"## Logging\n(.*?)(?:\n## |\n---|\z)").unwrap();
    common::all_skill_names()
        .into_iter()
        .filter(|name| {
            let content = common::read_skill(name);
            if !content.contains("## Logging") {
                return false;
            }
            if let Some(cap) = re.captures(&content) {
                !cap[1].contains("No logging")
            } else {
                false
            }
        })
        .collect()
}

fn load_settings_allow() -> Vec<String> {
    let settings = common::load_settings();
    settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn load_settings_deny() -> Vec<String> {
    let settings = common::load_settings();
    settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

// --- Tests ---

#[test]
fn no_bash_commands_reference_tmp() {
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files_with_maintainer() {
        for block in extract_bash_blocks_from_content(&content) {
            if block.contains("/tmp/") {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!(
                    "{}: bash block references /tmp/: '{}'",
                    filepath, cmd
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) referencing /tmp/:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_command_substitution_in_bash_blocks() {
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if block.contains("$(") {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!("{}: bash block contains $(): '{}'", filepath, cmd));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) containing $():\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_bash_redirects_to_dot_claude() {
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if block.contains(">>") && block.contains(".claude/") {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!(
                    "{}: bash block redirects to .claude/: '{}'",
                    filepath, cmd
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) using >> to .claude/:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn logging_uses_project_local_path() {
    let log_re = Regex::new(r"## Logging\n(.*?)(?:\n## |\n---|\z)").unwrap();
    for name in logging_skills() {
        let content = common::read_skill(&name);
        let cap = log_re.captures(&content).unwrap();
        let section = &cap[1];
        assert!(
            !section.contains("/tmp/"),
            "skills/{}/SKILL.md ## Logging references /tmp/",
            name
        );
        assert!(
            section.contains(".flow-states/"),
            "skills/{}/SKILL.md ## Logging missing .flow-states/",
            name
        );
    }
}

#[test]
fn logging_template_is_command_first() {
    let log_re = Regex::new(r"## Logging\n(.*?)(?:\n## |\n---|\z)").unwrap();
    let bash_re = Regex::new(r"```bash\s*\n(.+?)```").unwrap();
    for name in logging_skills() {
        let content = common::read_skill(&name);
        let cap = log_re.captures(&content).unwrap();
        let section = &cap[1];
        if let Some(bash_cap) = bash_re.captures(section) {
            let bash_content = bash_cap[1].trim();
            let first_line = bash_content.lines().next().unwrap_or("");
            assert!(
                bash_content.starts_with("COMMAND") || first_line.contains("bin/flow log"),
                "skills/{}/SKILL.md ## Logging bash template must start with COMMAND or bin/flow log",
                name
            );
        }
    }
}

#[test]
fn plugin_skills_use_plugin_root_for_bin_flow() {
    let mut errors = Vec::new();
    for (rel, content) in all_plugin_skill_files() {
        for block in extract_bash_blocks_from_content(&content) {
            for line in block.lines() {
                let stripped = line.trim();
                if stripped.starts_with("bin/flow") {
                    errors.push(format!(
                        "{}: bare 'bin/flow' must use ${{CLAUDE_PLUGIN_ROOT}}/bin/flow — got: {}",
                        rel,
                        &stripped[..stripped.len().min(60)]
                    ));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Plugin skill bash blocks must not use bare bin/flow:\n{}",
        errors.join("\n")
    );
}

#[test]
fn no_exit_in_bash_blocks() {
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if block.contains("; exit") || block.contains("exit $") {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!("{}: bash block contains exit: '{}'", filepath, cmd));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) containing exit:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_heredoc_in_bash_blocks() {
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if block.contains("<<") {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!(
                    "{}: bash block contains heredoc (<<): '{}'",
                    filepath, cmd
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) containing heredoc:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_cd_compound_in_bash_blocks() {
    let cd_re = Regex::new(r"\bcd\s+\S+\s*&&").unwrap();
    let mut errors = Vec::new();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if cd_re.is_match(&block) {
                let cmd = block.lines().next().unwrap_or("");
                errors.push(format!(
                    "{}: bash block contains cd && compound: '{}'",
                    filepath, cmd
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) containing cd && compound:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn all_bash_commands_have_permission_coverage() {
    let permissions = extract_prime_allow();
    let regexes = build_regexes(&permissions);
    let mut errors = Vec::new();

    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(cmd) = extract_primary_command(&block) {
                if is_auto_allowed(&cmd) {
                    continue;
                }
                if !regexes.iter().any(|r| r.is_match(&cmd)) {
                    errors.push(format!(
                        "{}: command '{}' has no matching permission",
                        filepath, cmd
                    ));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} command(s) without permission coverage:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn cd_prefixed_commands_have_full_permission_coverage() {
    let permissions = extract_prime_allow();
    let regexes = build_regexes(&permissions);
    let cd_re = Regex::new(r"^cd\s+\S+\s*&&\s*").unwrap();
    let mut errors = Vec::new();

    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(full_cmd) = extract_full_command(&block) {
                if !cd_re.is_match(&full_cmd) {
                    continue;
                }
                if !regexes.iter().any(|r| r.is_match(&full_cmd)) {
                    errors.push(format!(
                        "{}: cd-prefixed command '{}' has no matching permission",
                        filepath, full_cmd
                    ));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} cd-prefixed command(s) without coverage:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn worktree_cd_persists_no_repeated_cd() {
    let content = common::read_skill("flow-start");
    let step_header_re = Regex::new(r"### Step (\d+) — ").unwrap();

    // Find all step header positions and extract (step_num, section_content)
    let matches: Vec<_> = step_header_re.find_iter(&content).collect();
    let mut sections = Vec::new();
    for (i, m) in matches.iter().enumerate() {
        let start = m.start();
        let end = if i + 1 < matches.len() {
            matches[i + 1].start()
        } else {
            content.len()
        };
        // Extract step number from the match
        let cap = step_header_re.captures(&content[start..end]).unwrap();
        let step_num = cap[1].to_string();
        let section = &content[start..end];
        sections.push((step_num, section.to_string()));
    }

    let mut bare_cd_count = 0;
    let mut compound_cd_blocks = Vec::new();

    for (step_num, section) in &sections {
        for block in extract_bash_blocks_from_content(section) {
            if !block.contains("cd .worktrees/") {
                continue;
            }
            if block.starts_with("cd .worktrees/") && !block.contains("&&") {
                bare_cd_count += 1;
            } else {
                let first_line = block.lines().next().unwrap_or("");
                compound_cd_blocks.push(format!("Step {}: '{}'", step_num, first_line));
            }
        }
    }

    assert_eq!(
        bare_cd_count, 1,
        "Expected exactly 1 bare 'cd .worktrees/' block, found {}",
        bare_cd_count
    );
    assert!(
        compound_cd_blocks.is_empty(),
        "Found {} compound 'cd .worktrees/ && ...' block(s):\n{}",
        compound_cd_blocks.len(),
        compound_cd_blocks.join("\n  ")
    );
}

#[test]
fn maintainer_bash_commands_have_settings_coverage() {
    let permissions = load_settings_allow();
    let regexes = build_regexes(&permissions);
    let mut errors = Vec::new();

    for (filepath, content) in maintainer_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(cmd) = extract_primary_command(&block) {
                if is_auto_allowed(&cmd) {
                    continue;
                }
                if SENSITIVE_PATH_COMMANDS.contains(&cmd.as_str()) {
                    continue;
                }
                if !regexes.iter().any(|r| r.is_match(&cmd)) {
                    errors.push(format!(
                        "{}: command '{}' has no matching permission in settings.json",
                        filepath, cmd
                    ));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} maintainer command(s) without settings.json coverage:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

const REQUIRED_DENY_ENTRIES: &[&str] = &[
    "Bash(git rebase *)",
    "Bash(git push --force *)",
    "Bash(git push -f *)",
    "Bash(git reset --hard *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git clean *)",
    "Bash(git commit *)",
    "Bash(gh pr merge * --admin*)",
    "Bash(gh * --admin*)",
];

const REQUIRED_TOOL_ALTERNATIVE_DENIES: &[&str] = &[
    "Bash(cat *)",
    "Bash(head *)",
    "Bash(tail *)",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(find *)",
    "Bash(ls *)",
    "Bash(* && *)",
    "Bash(* | *)",
];

#[test]
fn plugin_permissions_deny_destructive_git() {
    let perms = extract_prime_permissions_block();
    let deny = perms["deny"].as_array().unwrap();
    let deny_strs: Vec<&str> = deny.iter().map(|v| v.as_str().unwrap()).collect();
    for entry in REQUIRED_DENY_ENTRIES {
        assert!(
            deny_strs.contains(entry),
            "Missing deny entry in prime/SKILL.md: {}",
            entry
        );
    }
}

#[test]
fn maintainer_permissions_deny_destructive_git() {
    let deny = load_settings_deny();
    for entry in REQUIRED_DENY_ENTRIES {
        assert!(
            deny.iter().any(|d| d == entry),
            "Missing deny entry in settings.json: {}",
            entry
        );
    }
}

#[test]
fn no_unrecognized_placeholders_in_bash_blocks() {
    let placeholder_re = Regex::new(r"<[a-zA-Z_/-]+>").unwrap();
    let mut errors = Vec::new();

    let blockquote_re = Regex::new(r"^>\s*").unwrap();
    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            let lines: Vec<String> = block
                .lines()
                .map(|l| blockquote_re.replace(l, "").to_string())
                .collect();
            let mut line = lines.join("\n").trim().to_string();

            if line.contains("COMMAND") {
                continue;
            }

            for (placeholder, value) in PLACEHOLDER_SUBS {
                line = line.replace(placeholder, value);
            }

            let remaining: Vec<String> = placeholder_re
                .find_iter(&line)
                .map(|m| m.as_str().to_string())
                .collect();
            if !remaining.is_empty() {
                let unique: HashSet<_> = remaining.into_iter().collect();
                let mut sorted: Vec<_> = unique.into_iter().collect();
                sorted.sort();
                errors.push(format!(
                    "{}: unrecognized placeholder(s) {:?}",
                    filepath, sorted
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) with unrecognized placeholders:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn permission_to_regex_known_conversions() {
    // Exact match
    let r = permission_to_regex("Bash(git push)").unwrap();
    assert!(r.is_match("git push"));
    assert!(!r.is_match("git push origin"));

    // Trailing glob
    let r = permission_to_regex("Bash(git push *)").unwrap();
    assert!(r.is_match("git push origin main"));
    assert!(!r.is_match("git pull"));

    // Exact binary
    let r = permission_to_regex("Bash(bin/ci)").unwrap();
    assert!(r.is_match("bin/ci"));
    assert!(!r.is_match("bin/ci --if-dirty"));

    // git -C prefix with glob
    let r = permission_to_regex("Bash(git -C *)").unwrap();
    assert!(r.is_match("git -C .worktrees/my-branch status"));

    // rm with glob suffix
    let r = permission_to_regex("Bash(rm .flow-*)").unwrap();
    assert!(r.is_match("rm .flow-commit-msg"));

    // Pipe deny pattern
    let r = permission_to_regex("Bash(* | *)").unwrap();
    assert!(r.is_match("git show HEAD:file.py | sed 's/foo/bar/'"));
    assert!(!r.is_match("git status"));

    // Non-Bash Type(pattern) entries now return a regex
    let r = permission_to_regex("Write(*)").unwrap();
    assert!(r.is_match("anything"));

    // Malformed entries (no Type(pattern) format) return None
    assert!(permission_to_regex("plain string").is_none());
}

#[test]
fn no_skill_command_matches_deny() {
    let deny = extract_prime_deny();
    let deny_regexes = build_regexes(&deny);
    let mut errors = Vec::new();

    for (filepath, content) in all_check_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(cmd) = extract_primary_command(&block) {
                for (i, regex) in deny_regexes.iter().enumerate() {
                    if regex.is_match(&cmd) {
                        errors.push(format!(
                            "{}: command '{}' matches deny entry '{}'",
                            filepath, cmd, deny[i]
                        ));
                        break;
                    }
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} command(s) matching deny patterns:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_maintainer_command_matches_deny() {
    let deny = load_settings_deny();
    let deny_regexes = build_regexes(&deny);
    let mut errors = Vec::new();

    for (filepath, content) in maintainer_files() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(cmd) = extract_primary_command(&block) {
                for (i, regex) in deny_regexes.iter().enumerate() {
                    if regex.is_match(&cmd) {
                        errors.push(format!(
                            "{}: command '{}' matches deny entry '{}'",
                            filepath, cmd, deny[i]
                        ));
                        break;
                    }
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} maintainer command(s) matching deny:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_allow_deny_overlap_in_plugin_permissions() {
    let perms = extract_prime_permissions_block();
    let allow: Vec<String> = perms["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let deny: Vec<String> = perms["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let deny_regexes = build_regexes(&deny);
    let mut errors = Vec::new();

    for entry in &allow {
        if let Some(example) = concrete_example(entry) {
            for (i, regex) in deny_regexes.iter().enumerate() {
                if regex.is_match(&example) {
                    errors.push(format!(
                        "allow '{}' (example: '{}') matches deny '{}'",
                        entry, example, deny[i]
                    ));
                    break;
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} allow/deny overlap(s) in prime/SKILL.md:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn no_allow_deny_overlap_in_maintainer_settings() {
    let allow = load_settings_allow();
    let deny = load_settings_deny();
    let deny_regexes = build_regexes(&deny);
    let mut errors = Vec::new();

    for entry in &allow {
        if let Some(example) = concrete_example(entry) {
            for (i, regex) in deny_regexes.iter().enumerate() {
                if regex.is_match(&example) {
                    errors.push(format!(
                        "allow '{}' (example: '{}') matches deny '{}'",
                        entry, example, deny[i]
                    ));
                    break;
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} allow/deny overlap(s) in settings.json:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn tool_alternative_denies_in_project_settings() {
    let deny = load_settings_deny();
    for entry in REQUIRED_TOOL_ALTERNATIVE_DENIES {
        assert!(
            deny.iter().any(|d| d == entry),
            "Missing deny entry in settings.json: {}",
            entry
        );
    }
}

#[test]
fn no_dedicated_tool_commands_in_bash_blocks() {
    let denied_prefixes: HashSet<&str> = ["cat", "head", "tail", "grep", "rg", "find", "ls"]
        .iter()
        .copied()
        .collect();
    let mut errors = Vec::new();

    for (filepath, content) in all_check_files_with_maintainer() {
        for block in extract_bash_blocks_from_content(&content) {
            if let Some(cmd) = extract_primary_command(&block) {
                let first_word = cmd.split_whitespace().next().unwrap_or("");
                if denied_prefixes.contains(first_word) {
                    errors.push(format!(
                        "{}: bash block starts with '{}': '{}'",
                        filepath, first_word, cmd
                    ));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Found {} bash block(s) using commands with dedicated tool alternatives:\n{}",
        errors.len(),
        errors.join("\n  ")
    );
}

#[test]
fn prime_setup_lists_match_skill_md_reference() {
    // Extract Rust constants from src/prime_check.rs
    let prime_check =
        fs::read_to_string(common::repo_root().join("src").join("prime_check.rs")).unwrap();

    fn extract_const(content: &str, name: &str) -> Vec<String> {
        let pattern = format!(
            r#"(?s)(?:pub\s+)?const {}:\s*&\[&str\]\s*=\s*&\[(.*?)\];"#,
            regex::escape(name)
        );
        let re = Regex::new(&pattern).unwrap();
        let cap = re
            .captures(content)
            .unwrap_or_else(|| panic!("Could not find const {} in prime_check.rs", name));
        let body = &cap[1];
        let str_re = Regex::new(r#""((?:[^"\\]|\\.)*)""#).unwrap();
        str_re
            .captures_iter(body)
            .map(|c| c[1].to_string())
            .collect()
    }

    let universal_allow = extract_const(&prime_check, "UNIVERSAL_ALLOW");
    let flow_deny = extract_const(&prime_check, "FLOW_DENY");

    // Collect all framework permissions
    let mut all_framework_perms = Vec::new();
    let frameworks_dir = common::frameworks_dir();
    for entry in fs::read_dir(&frameworks_dir).unwrap().flatten() {
        if entry.path().is_dir() {
            let perms_file = entry.path().join("permissions.json");
            if perms_file.exists() {
                let data: Value =
                    serde_json::from_str(&fs::read_to_string(&perms_file).unwrap()).unwrap();
                if let Some(allow) = data["allow"].as_array() {
                    for v in allow {
                        all_framework_perms.push(v.as_str().unwrap().to_string());
                    }
                }
            }
        }
    }

    let code_allow: HashSet<String> = universal_allow
        .into_iter()
        .chain(all_framework_perms)
        .collect();
    let code_deny: HashSet<String> = flow_deny.into_iter().collect();

    let perms = extract_prime_permissions_block();
    let skill_allow: HashSet<String> = perms["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let skill_deny: HashSet<String> = perms["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let mut errors = Vec::new();
    let only_code_allow: Vec<_> = code_allow.difference(&skill_allow).collect();
    if !only_code_allow.is_empty() {
        errors.push(format!(
            "In prime_check.rs allow but not prime/SKILL.md: {:?}",
            only_code_allow
        ));
    }
    let only_skill_allow: Vec<_> = skill_allow.difference(&code_allow).collect();
    if !only_skill_allow.is_empty() {
        errors.push(format!(
            "In prime/SKILL.md allow but not prime_check.rs: {:?}",
            only_skill_allow
        ));
    }
    let only_code_deny: Vec<_> = code_deny.difference(&skill_deny).collect();
    if !only_code_deny.is_empty() {
        errors.push(format!(
            "In prime_check.rs deny but not prime/SKILL.md: {:?}",
            only_code_deny
        ));
    }
    let only_skill_deny: Vec<_> = skill_deny.difference(&code_deny).collect();
    if !only_skill_deny.is_empty() {
        errors.push(format!(
            "In prime/SKILL.md deny but not prime_check.rs: {:?}",
            only_skill_deny
        ));
    }
    assert!(
        errors.is_empty(),
        "Permission lists out of sync:\n{}",
        errors.join("\n  ")
    );
}

#[test]
fn settings_allow_list_ordered_by_category() {
    let categories: Vec<(&str, Vec<&str>)> = vec![
        ("git", vec!["Bash(git "]),
        ("github-cli", vec!["Bash(gh "]),
        ("bin", vec!["Bash(*bin/"]),
        ("plugins", vec!["Bash(claude plugin"]),
        ("cleanup", vec!["Bash(rm "]),
        (
            "build",
            vec![
                "Bash(cargo ",
                "Bash(cd ",
                "Bash(pwd)",
                "Bash(chmod ",
                "Bash(make ",
                "Bash(diff ",
                "Bash(.venv/",
                "Bash(curl ",
                "Bash(open:",
            ],
        ),
        ("read", vec!["Read("]),
        ("skills", vec!["Skill("]),
        ("agents", vec!["Agent("]),
    ];

    let categorize = |entry: &str| -> Option<&str> {
        for (cat, prefixes) in &categories {
            for prefix in prefixes {
                if entry.starts_with(prefix) {
                    return Some(cat);
                }
            }
        }
        None
    };

    let category_index: std::collections::HashMap<&str, usize> = categories
        .iter()
        .enumerate()
        .map(|(i, (cat, _))| (*cat, i))
        .collect();
    let permissions = load_settings_allow();
    let mut errors = Vec::new();
    let mut last_index: i32 = -1;
    let mut last_cat = "";

    for entry in &permissions {
        match categorize(entry) {
            None => errors.push(format!(
                "Entry '{}' does not match any known category",
                entry
            )),
            Some(cat) => {
                let idx = category_index[cat] as i32;
                if idx < last_index {
                    errors.push(format!(
                        "Entry '{}' (category: {}) appears after '{}' entries",
                        entry, cat, last_cat
                    ));
                } else {
                    last_index = idx;
                    last_cat = cat;
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Allow list not ordered by category:\n{}",
        errors.join("\n  ")
    );
}
