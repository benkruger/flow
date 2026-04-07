//! Tests that documentation stays in sync with skills and flow-phases.json.
//!
//! Ports tests/test_docs_sync.py to Rust integration tests.
//! Skills are hand-authored for different audiences (Claude vs. public users)
//! so auto-generation isn't appropriate. These tests catch structural drift —
//! missing files, wrong names, stale references.

mod common;

use std::collections::HashMap;
use std::fs;

use regex::Regex;

/// Returns a map of phase_key → 1-indexed phase number.
fn phase_number() -> HashMap<String, usize> {
    common::phase_order()
        .into_iter()
        .enumerate()
        .map(|(i, key)| (key, i + 1))
        .collect()
}

/// Returns set of skill names that correspond to phases (from flow-phases.json commands).
fn phase_skill_names() -> Vec<String> {
    let phases = common::load_phases();
    let phase_map = phases["phases"].as_object().unwrap();
    phase_map
        .values()
        .map(|p| {
            p["command"]
                .as_str()
                .unwrap()
                .split(':')
                .nth(1)
                .unwrap()
                .to_string()
        })
        .collect()
}

/// Returns sorted list of skill names that are NOT phase skills.
fn utility_skill_names() -> Vec<String> {
    let phase_names: Vec<String> = phase_skill_names();
    let mut utils: Vec<String> = common::all_skill_names()
        .into_iter()
        .filter(|name| !phase_names.contains(name))
        .collect();
    utils.sort();
    utils
}

// Required features that README and landing page must mention
fn required_features() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("Autonomy config", vec!["autonomy"]),
        ("Learning system", vec!["learning system"]),
        ("DAG decomposition", vec!["dag", "decompose"]),
        ("Zero dependencies", vec!["zero dependencies"]),
        ("Minimal repo artifacts", vec![".flow-states"]),
        ("Multi-language", vec!["rails"]),
        ("Issue auto-close", vec!["close issues"]),
        ("Batch orchestration", vec!["orchestrat"]),
    ]
}

fn assert_covers_key_features(content: &str, source_label: &str) {
    let lower = content.to_lowercase();
    for (feature, keywords) in required_features() {
        let found = keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()));
        assert!(
            found,
            "{} does not mention feature '{}' (looked for: {:?})",
            source_label, feature, keywords
        );
    }
}

// --- Skill docs existence (bidirectional) ---

/// Every skills/<name>/ must have a docs/skills/<name>.md.
#[test]
fn every_skill_has_a_docs_page() {
    for name in common::all_skill_names() {
        let doc = common::docs_dir().join("skills").join(format!("{}.md", name));
        assert!(
            doc.exists(),
            "skills/{}/ exists but docs/skills/{}.md is missing",
            name,
            name
        );
    }
}

/// Every docs/skills/<name>.md must have a skills/<name>/.
#[test]
fn every_docs_skill_page_has_a_skill_dir() {
    let skills_docs = common::docs_dir().join("skills");
    for entry in fs::read_dir(&skills_docs).unwrap().flatten() {
        let path = entry.path();
        if path.file_name().unwrap() == "index.md" || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let skill_name = path.file_stem().unwrap().to_string_lossy().to_string();
        assert!(
            common::skills_dir().join(&skill_name).is_dir(),
            "docs/skills/{}.md exists but skills/{}/ is missing",
            skill_name,
            skill_name
        );
    }
}

// --- Phase docs match flow-phases.json ---

/// Every phase in flow-phases.json must have a docs/phases/phase-<N>-<name>.md.
#[test]
fn every_phase_has_a_docs_page() {
    let phases = common::load_phases();
    let numbers = phase_number();
    for (key, phase) in phases["phases"].as_object().unwrap() {
        let num = numbers[key];
        let name = phase["name"].as_str().unwrap().to_lowercase().replace(' ', "-");
        let doc = common::docs_dir()
            .join("phases")
            .join(format!("phase-{}-{}.md", num, name));
        assert!(
            doc.exists(),
            "Phase {} ({}) has no docs/phases/phase-{}-{}.md",
            num,
            phase["name"].as_str().unwrap(),
            num,
            name
        );
    }
}

/// Each phase doc must contain the command from flow-phases.json.
#[test]
fn phase_docs_contain_correct_command() {
    let phases = common::load_phases();
    let numbers = phase_number();
    for (key, phase) in phases["phases"].as_object().unwrap() {
        let num = numbers[key];
        let name = phase["name"].as_str().unwrap().to_lowercase().replace(' ', "-");
        let doc_path = common::docs_dir()
            .join("phases")
            .join(format!("phase-{}-{}.md", num, name));
        let content = fs::read_to_string(&doc_path).unwrap();
        // Docs use /flow-start, not /flow:flow-start
        let user_command = phase["command"]
            .as_str()
            .unwrap()
            .replace("/flow:", "/");
        assert!(
            content.contains(&user_command),
            "docs/phases/phase-{}-{}.md does not mention command '{}'",
            num,
            name,
            user_command
        );
    }
}

/// Each phase doc title must contain 'Phase N: Name'.
#[test]
fn phase_docs_have_correct_title() {
    let phases = common::load_phases();
    let numbers = phase_number();
    for (key, phase) in phases["phases"].as_object().unwrap() {
        let num = numbers[key];
        let phase_name = phase["name"].as_str().unwrap();
        let name_lower = phase_name.to_lowercase().replace(' ', "-");
        let doc_path = common::docs_dir()
            .join("phases")
            .join(format!("phase-{}-{}.md", num, name_lower));
        let content = fs::read_to_string(&doc_path).unwrap();
        let pattern = format!(r"Phase {}\s*:\s*{}", num, regex::escape(phase_name));
        let re = Regex::new(&pattern).unwrap();
        assert!(
            re.is_match(&content),
            "docs/phases/phase-{}-{}.md missing 'Phase {}: {}' in title",
            num,
            name_lower,
            num,
            phase_name
        );
    }
}

// --- Index completeness ---

/// docs/skills/index.md must mention every /<name> command.
#[test]
fn index_mentions_every_skill_command() {
    let index = fs::read_to_string(common::docs_dir().join("skills").join("index.md")).unwrap();
    for name in common::all_skill_names() {
        let command = format!("/{}", name);
        assert!(
            index.contains(&command),
            "docs/skills/index.md does not mention {}",
            command
        );
    }
}

/// docs/skills/index.md phase table must show 'N — Name' for all 6 phases.
#[test]
fn index_phase_table_shows_all_phases() {
    let phases = common::load_phases();
    let numbers = phase_number();
    let index = fs::read_to_string(common::docs_dir().join("skills").join("index.md")).unwrap();
    for (key, phase) in phases["phases"].as_object().unwrap() {
        let num = numbers[key];
        let phase_name = phase["name"].as_str().unwrap();
        let pattern = format!(r"{}\s*—\s*{}", num, regex::escape(phase_name));
        let re = Regex::new(&pattern).unwrap();
        assert!(
            re.is_match(&index),
            "docs/skills/index.md missing '{} — {}' in phase table",
            num,
            phase_name
        );
    }
}

// --- README completeness ---

/// README.md must mention all 6 phase commands and 'N: Name' strings.
#[test]
fn readme_mentions_all_phase_commands() {
    let readme = fs::read_to_string(common::repo_root().join("README.md")).unwrap();
    let phases = common::load_phases();
    let numbers = phase_number();
    for (key, phase) in phases["phases"].as_object().unwrap() {
        let num = numbers[key];
        let phase_name = phase["name"].as_str().unwrap();
        let user_command = phase["command"]
            .as_str()
            .unwrap()
            .replace("/flow:", "/");
        assert!(
            readme.contains(&user_command),
            "README.md does not mention phase command '{}'",
            user_command
        );
        let pattern = format!(r"{}:\s*{}", num, regex::escape(phase_name));
        let re = Regex::new(&pattern).unwrap();
        assert!(
            re.is_match(&readme),
            "README.md does not mention '{}: {}'",
            num,
            phase_name
        );
    }
}

/// README.md must mention all maintainer skill commands as /<name>.
#[test]
fn readme_mentions_all_maintainer_commands() {
    let readme = fs::read_to_string(common::repo_root().join("README.md")).unwrap();
    let maintainer_dir = common::repo_root().join(".claude").join("skills");
    for entry in fs::read_dir(&maintainer_dir).unwrap().flatten() {
        if entry.path().is_dir() && entry.path().join("SKILL.md").exists() {
            let name = entry.file_name().to_string_lossy().to_string();
            let command = format!("/{}", name);
            assert!(
                readme.contains(&command),
                "README.md does not mention maintainer command '{}'",
                command
            );
        }
    }
}

/// README.md must mention all utility skill commands.
#[test]
fn readme_mentions_all_utility_commands() {
    let readme = fs::read_to_string(common::repo_root().join("README.md")).unwrap();
    for name in utility_skill_names() {
        let command = format!("/{}", name);
        assert!(
            readme.contains(&command),
            "README.md does not mention utility command '{}'",
            command
        );
    }
}

// --- Landing page completeness ---

/// docs/index.html must mention all 6 phase names.
#[test]
fn landing_page_mentions_all_phase_names() {
    let html = fs::read_to_string(common::docs_dir().join("index.html")).unwrap();
    let phases = common::load_phases();
    for phase in phases["phases"].as_object().unwrap().values() {
        let name = phase["name"].as_str().unwrap();
        assert!(
            html.contains(name),
            "docs/index.html does not mention phase name '{}'",
            name
        );
    }
}

// --- State schema coverage ---

/// Schema doc must document all phase-level fields from make_state().
#[test]
fn schema_doc_covers_phase_fields() {
    let schema = fs::read_to_string(
        common::docs_dir()
            .join("reference")
            .join("flow-state-schema.md"),
    )
    .unwrap();
    let phase_fields = [
        "name",
        "status",
        "started_at",
        "completed_at",
        "session_started_at",
        "cumulative_seconds",
        "visit_count",
    ];
    for field in phase_fields {
        let pattern = format!("`{}`", field);
        assert!(
            schema.contains(&pattern),
            "docs/reference/flow-state-schema.md does not document phase field '{}'",
            field
        );
    }
}

/// Schema doc must document all top-level fields from make_state().
#[test]
fn schema_doc_covers_top_level_fields() {
    let schema = fs::read_to_string(
        common::docs_dir()
            .join("reference")
            .join("flow-state-schema.md"),
    )
    .unwrap();
    let top_level_fields = [
        "schema_version",
        "branch",
        "repo",
        "pr_number",
        "pr_url",
        "started_at",
        "current_phase",
        "prompt",
        "notes",
        "phase_transitions",
    ];
    for field in top_level_fields {
        let pattern = format!("`{}`", field);
        assert!(
            schema.contains(&pattern),
            "docs/reference/flow-state-schema.md does not document top-level field '{}'",
            field
        );
    }
}

// --- Key feature coverage ---

/// README.md must mention all key features by keyword.
#[test]
fn readme_covers_key_features() {
    let content = fs::read_to_string(common::repo_root().join("README.md")).unwrap();
    assert_covers_key_features(&content, "README.md");
}

/// docs/index.html must mention all key features by keyword.
#[test]
fn landing_page_covers_key_features() {
    let content = fs::read_to_string(common::docs_dir().join("index.html")).unwrap();
    assert_covers_key_features(&content, "docs/index.html");
}
