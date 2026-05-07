//! Verify-references scanner — Plan-phase Gate 6.
//!
//! When a plan references a backtick-quoted identifier in the
//! `## Tasks` section that looks like a function or test name
//! (≥ 10 chars, snake_case), the identifier must exist as a
//! `fn <name>(` definition somewhere under `src/` or `tests/`.
//! Issue authors — including Claude in prior sessions — sometimes
//! cite test names that were never created or were renamed; this
//! scanner catches the gap before Code phase begins. See
//! `.claude/rules/skill-authoring.md` "Verify Test Function
//! References in Issues".
//!
//! ## Trigger vocabulary
//!
//! Closed and curated. The trigger is a backtick-quoted identifier
//! inside the `## Tasks` section that matches `(?i)^[a-z_][a-z0-9_]{9,}$`
//! (≥ 10 chars, snake_case). The length filter prevents
//! common-word identifiers (like `config` or `helper`) from
//! triggering. Identifiers outside the Tasks section (Context,
//! Risks, Approach, Files to Investigate) are intentionally
//! ignored — those sections cite existing code as orientation,
//! not as test-task targets. Per
//! `.claude/rules/research-target-project.md` and
//! `.claude/rules/test-placement.md`, the index walks `tests/` and
//! `src/` only.
//!
//! ## Compliance proof
//!
//! Each cited identifier must appear as `fn <name>(` somewhere in
//! `tests/**/*.rs` or `src/**/*.rs`. The scanner builds the
//! definition index lazily (on first identifier query) so plans
//! with no candidates pay no walk cost.
//!
//! ## Identifier cap
//!
//! `MAX_IDENTIFIERS_PER_PLAN` = 30 caps the unique identifier set
//! per plan to bound walk cost.
//!
//! ## Opt-outs
//!
//! `<!-- verify-references: prose-citation -->` on the trigger
//! line, the line directly above, or two lines above with one
//! blank line between. Same walk-back grammar as the sibling
//! scanners.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

const MAX_IDENTIFIERS_PER_PLAN: usize = 30;

/// Index of `fn <name>(` definitions across `tests/` and `src/`.
/// Built lazily on first identifier query to skip the walk when
/// no candidates exist.
pub struct DefinitionIndex {
    index: HashMap<String, Vec<PathBuf>>,
}

impl DefinitionIndex {
    /// Walk `<root>/tests/` and `<root>/src/`, indexing every
    /// `fn <name>(` declaration by name.
    pub fn from_repo(root: &Path) -> DefinitionIndex {
        let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for sub in &["tests", "src"] {
            let dir = root.join(sub);
            if dir.is_dir() {
                index_dir(&dir, root, &mut index);
            }
        }
        DefinitionIndex { index }
    }

    /// Returns true when the name is defined as a function
    /// somewhere in the indexed tree.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub phrase: String,
    pub context: String,
    pub identifier: String,
}

pub fn scan(content: &str, source: &Path, index: &DefinitionIndex) -> Vec<Violation> {
    let lines: Vec<&str> = content.lines().collect();
    let fenced = compute_fenced_mask(&lines);
    let in_tasks = compute_tasks_section_mask(&lines);
    let mut violations = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        if seen.len() >= MAX_IDENTIFIERS_PER_PLAN {
            break;
        }
        if fenced[idx] {
            continue;
        }
        if !in_tasks[idx] {
            continue;
        }
        if is_optout_line(&lines, idx) {
            continue;
        }
        for cap in ident_regex().captures_iter(line) {
            // `ident_regex()` is `r"`([^`\n]+)`"` — capture group 1
            // always present on a successful match. `.expect` does
            // not create a testable branch per
            // `.claude/rules/testability-means-simplicity.md`.
            let inside = cap
                .get(1)
                .expect("ident regex has capture group 1")
                .as_str()
                .trim();
            if !ident_shape_regex().is_match(inside) {
                continue;
            }
            if !seen.insert(inside.to_string()) {
                continue;
            }
            if !index.contains(inside) {
                violations.push(Violation {
                    file: source.to_path_buf(),
                    line: idx + 1,
                    phrase: inside.to_string(),
                    context: (*line).to_string(),
                    identifier: inside.to_string(),
                });
            }
        }
    }

    violations
}

fn ident_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").expect("verify-references ident regex"))
}

fn ident_shape_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[a-z_][a-z0-9_]{9,}$").expect("verify-references shape regex")
    })
}

fn fn_decl_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)\bfn\s+([a-z_][a-z0-9_]+)\s*[(<]")
            .expect("verify-references fn-decl regex")
    })
}

fn index_dir(dir: &Path, root: &Path, index: &mut HashMap<String, Vec<PathBuf>>) {
    // Caller checks `dir.is_dir()` before invoking, and the
    // recursive entry below also gates on `meta.is_dir()`. A
    // `read_dir` error after that check is a TOCTOU race window
    // — narrow enough that we treat it as a programmer-visible
    // panic instead of silently skipping per
    // `.claude/rules/testability-means-simplicity.md`.
    let entries = fs::read_dir(dir).expect("read_dir on a verified directory");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "target" || name.starts_with('.') {
            continue;
        }
        // `symlink_metadata` on a freshly-iterated entry succeeds
        // in every practical case. The `Err` arm is a TOCTOU race
        // window so narrow that we treat it as a panic rather than
        // an instrumented branch.
        let meta =
            fs::symlink_metadata(&path).expect("symlink_metadata on a freshly-iterated entry");
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            index_dir(&path, root, index);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // `read_to_string` on a regular file the kernel just
        // listed succeeds in every practical case. Treat the
        // error path as a panic per the same TOCTOU rationale.
        let content =
            fs::read_to_string(&path).expect("read_to_string on a freshly-iterated rs file");
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        for cap in fn_decl_regex().captures_iter(&content) {
            // Capture group 1 always present on a successful match.
            let m = cap.get(1).expect("fn_decl_regex has capture group 1");
            let name = m.as_str().to_string();
            index.entry(name).or_default().push(rel.clone());
        }
    }
}

/// Returns a mask where `mask[i]` is true iff line `i` is inside
/// the `## Tasks` section. The section starts at the first line
/// matching `## Tasks` (case-insensitive, leading whitespace
/// allowed) and ends at the next `## ` heading.
fn compute_tasks_section_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_tasks = false;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("## ") {
            in_tasks = trimmed.eq_ignore_ascii_case("## tasks")
                || trimmed.to_lowercase().starts_with("## tasks");
        }
        mask[i] = in_tasks;
    }
    mask
}

fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut open_at: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            open_at = match open_at {
                Some(_) => None,
                None => Some(i),
            };
            mask[i] = true;
            continue;
        }
        mask[i] = open_at.is_some();
    }
    if let Some(start) = open_at {
        for m in &mut mask[start..] {
            *m = false;
        }
    }
    mask
}

fn is_optout_line(lines: &[&str], idx: usize) -> bool {
    if line_has_optout_comment(lines[idx]) {
        return true;
    }
    if idx >= 1 && line_has_optout_comment(lines[idx - 1]) {
        return true;
    }
    if idx >= 2 && lines[idx - 1].trim().is_empty() && line_has_optout_comment(lines[idx - 2]) {
        return true;
    }
    false
}

fn optout_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)<!--\s*verify-references\s*:\s*prose-citation\s*-->")
            .expect("verify-references opt-out regex")
    })
}

fn line_has_optout_comment(line: &str) -> bool {
    optout_regex().is_match(line)
}
