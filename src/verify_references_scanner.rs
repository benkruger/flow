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
//! inside any "Tasks" heading section (`## Tasks`, `### Tasks`,
//! `#### Tasks`, etc.) that matches `(?i)^[a-z_][a-z0-9_]{9,}$`
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
//! ## Path-prefixed identifier form
//!
//! Identifiers prefixed with a file scope —
//! `tests/foo.rs::bar_baz_quux` or `src/foo.rs::bar_baz_quux` —
//! are honored: the scanner verifies the named function exists in
//! THE NAMED FILE rather than anywhere in the index. A path-prefixed
//! identifier with the right name in a different file is still a
//! violation.
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
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

const MAX_IDENTIFIERS_PER_PLAN: usize = 30;

/// Per-file byte cap when reading source files into the definition
/// index. Bounds memory cost of a generated/golden source file that
/// happens to live under `tests/` or `src/`. 4 MB comfortably fits
/// every hand-authored Rust source file in this tree while
/// preventing OOM on pathological generated files. Per
/// `.claude/rules/external-input-path-construction.md` Rule 3.
const INDEX_RS_BYTE_CAP: u64 = 4 * 1024 * 1024;

/// Index of `fn <name>(` definitions across `tests/` and `src/`.
/// Built lazily on first identifier query to skip the walk when
/// no candidates exist.
pub struct DefinitionIndex {
    index: HashMap<String, Vec<PathBuf>>,
}

impl DefinitionIndex {
    /// Walk `<root>/tests/` and `<root>/src/`, indexing every
    /// `fn <name>(` declaration by name.
    ///
    /// Filesystem errors (permission denied, transient I/O failure,
    /// race with concurrent file removal) are swallowed silently —
    /// the affected file is skipped and indexing continues. The
    /// scanner gate is best-effort: a missed definition in this
    /// path produces a false-positive violation that the author can
    /// resolve via opt-out, but a panic here would crash the entire
    /// `bin/flow plan-check` subprocess and block Plan-phase
    /// completion.
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

    /// Returns true when the name is defined as a function in the
    /// file whose path (relative to the repo root) matches
    /// `file_rel`. Used by the path-prefixed identifier form
    /// (`tests/foo.rs::bar_baz_quux`) so a citation that names a
    /// specific file can be verified against that file rather than
    /// any file in the index.
    pub fn contains_in_file(&self, name: &str, file_rel: &Path) -> bool {
        match self.index.get(name) {
            Some(paths) => paths.iter().any(|p| p == file_rel),
            None => false,
        }
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
            // Path-prefixed form: `tests/foo.rs::bar_baz_quux` or
            // `src/foo.rs::bar_baz_quux`. The path part scopes the
            // lookup to a specific file; the name part must match
            // the snake_case shape filter. Plan Tasks 19/20 named
            // the test cases for this form per
            // `.claude/rules/skill-authoring.md` "Verify Test
            // Function References in Issues".
            if let Some((file_rel, name)) = parse_path_prefixed_ident(inside) {
                if !ident_shape_regex().is_match(&name) {
                    continue;
                }
                if !seen.insert(inside.to_string()) {
                    continue;
                }
                if !index.contains_in_file(&name, &file_rel) {
                    violations.push(Violation {
                        file: source.to_path_buf(),
                        line: idx + 1,
                        phrase: inside.to_string(),
                        context: (*line).to_string(),
                        identifier: inside.to_string(),
                    });
                }
                continue;
            }
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

/// Parse a path-prefixed identifier like
/// `tests/foo.rs::bar_baz_quux` into `(PathBuf("tests/foo.rs"),
/// "bar_baz_quux")`. Returns `None` when the input does not contain
/// `::` or the path part does not start with `tests/` or `src/`.
fn parse_path_prefixed_ident(s: &str) -> Option<(PathBuf, String)> {
    let (path_part, name_part) = s.split_once("::")?;
    if !(path_part.starts_with("tests/") || path_part.starts_with("src/")) {
        return None;
    }
    if name_part.is_empty() {
        return None;
    }
    Some((PathBuf::from(path_part), name_part.to_string()))
}

fn fn_decl_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)\bfn\s+([a-z_][a-z0-9_]+)\s*[(<]")
            .expect("verify-references fn-decl regex")
    })
}

fn index_dir(dir: &Path, root: &Path, index: &mut HashMap<String, Vec<PathBuf>>) {
    // All filesystem errors are swallowed — see the doc comment on
    // `DefinitionIndex::from_repo` for the rationale. A `read_dir`
    // failure on this directory means we cannot enumerate its
    // children; the index for this subtree is incomplete but the
    // scanner remains functional.
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "target" || name.starts_with('.') {
            continue;
        }
        // `symlink_metadata` reads cached directory metadata; it
        // succeeds for every entry the kernel just yielded from
        // `read_dir`. The `.expect` here is the unreachable-arm
        // pattern per `.claude/rules/testability-means-simplicity.md`
        // "When the test resists the real production path" — the
        // only failure mode is a TOCTOU race after `read_dir`
        // returned the entry, which no test environment can
        // reliably reproduce. The `.expect` produces no branch and
        // does not contribute to coverage. The earlier `read_dir`,
        // `File::open`, and `read_to_string` errors (which are
        // testable via chmod and invalid UTF-8) remain non-panicking
        // because those failures are reachable in practice.
        let meta = fs::symlink_metadata(&path)
            .expect("symlink_metadata succeeds on freshly-iterated read_dir entry");
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
        // Capped read: a file larger than `INDEX_RS_BYTE_CAP` is
        // truncated to the cap; definitions past the cap are not
        // indexed. The caller treats a missing definition as a
        // possible violation, which the author can resolve via
        // opt-out — the failure mode is a benign false positive,
        // not a crash.
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut content = String::new();
        if file
            .take(INDEX_RS_BYTE_CAP)
            .read_to_string(&mut content)
            .is_err()
        {
            continue;
        }
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
/// a "Tasks" section. The section starts at any heading line whose
/// stripped-of-`#`s text is "Tasks" (case-insensitive,
/// leading-whitespace tolerant) — `## Tasks`, `### Tasks`,
/// `#### Tasks`, etc. — and ends at the next heading whose level
/// is the same or shallower than the Tasks heading. A nested
/// deeper heading (e.g. `#### Task N` inside `## Tasks`) does NOT
/// end the section.
fn compute_tasks_section_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut tasks_level: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(level) = heading_level(trimmed) {
            let header_text = trimmed[level..].trim_start_matches(' ').trim();
            if header_text.eq_ignore_ascii_case("Tasks") {
                tasks_level = Some(level);
                mask[i] = true;
                continue;
            }
            if let Some(l) = tasks_level {
                if level <= l {
                    tasks_level = None;
                }
            }
        }
        mask[i] = tasks_level.is_some();
    }
    mask
}

/// Returns the heading level (count of leading `#`s) when the line
/// begins with one or more `#`s followed by a space, otherwise
/// `None`. A hash-only line with no trailing space is not a
/// heading.
fn heading_level(trimmed: &str) -> Option<usize> {
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    if i > 0 && i < bytes.len() && bytes[i] == b' ' {
        Some(i)
    } else {
        None
    }
}

/// Returns `true` for every line index inside or on a fenced code
/// block. Both backtick (` ``` `) and tilde (`~~~`) fences are
/// recognized per CommonMark; a fence opened by one marker can
/// only be closed by the same marker, so backtick-inside-tilde and
/// tilde-inside-backtick code blocks remain correctly masked.
///
/// Unclosed fences fail open: when a fence opens and never closes
/// before EOF, the mask for those interior lines is reset to
/// `false`. This means an unclosed fence in a partially-truncated
/// plan can produce false-positive identifier violations from
/// content the author intended to be inside the fence. Callers
/// that run before `detect_truncation` see this directly; callers
/// that run after it are protected because `detect_truncation`
/// itself flags the unclosed fence and refuses to write the
/// affected plan file.
fn compute_fenced_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut open: Option<(usize, char)> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(m) = marker {
            match open {
                Some((_, current)) if current == m => {
                    open = None;
                }
                Some(_) => {
                    // Different fence type while already open —
                    // the inner marker is content, not a closer.
                }
                None => {
                    open = Some((i, m));
                }
            }
            mask[i] = true;
            continue;
        }
        mask[i] = open.is_some();
    }
    if let Some((start, _)) = open {
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
