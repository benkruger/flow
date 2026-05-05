//! Version gate — verify `/flow:flow-prime` has been run with a
//! matching version.
//!
//! Usage: `bin/flow prime-check`
//!
//! Output (JSON to stdout):
//!   Success: `{"status": "ok"}`
//!   Auto-upgrade: `{"status": "ok", "auto_upgraded": true, "old_version": "...", "new_version": "..."}`
//!   Failure: `{"status": "error", "message": "..."}`
//!
//! # Constants
//!
//! `UNIVERSAL_ALLOW`, `FLOW_DENY`, and `EXCLUDE_ENTRIES` are the
//! canonical source for permission and exclude lists. They are shared
//! with `src/prime_setup.rs` which imports them via `pub use`.
//!
//! # JSON Separator Format for Config Hashing
//!
//! `compute_config_hash` must produce SHA-256 digests that match
//! existing `.flow.json` files, which use `(", ", ": ")` separators.
//! Rust's `serde_json::to_string` default is `(",", ":")` — without
//! a custom formatter the digests differ, breaking hash comparisons
//! on upgrade. `PythonDefaultFormatter` below implements the three
//! `serde_json::ser::Formatter` methods needed to emit the expected
//! separators. Renaming the struct or changing its method bodies
//! would alter the SHA-256 output and invalidate every stored
//! `config_hash` in users' `.flow.json` files, forcing a re-prime.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use serde::Serialize;
use serde_json::ser::Formatter;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Universal allow list — canonical source for all permission merging.
/// Shared with `prime_setup.rs` via pub import.
pub const UNIVERSAL_ALLOW: &[&str] = &[
    "Bash(git add *)",
    "Bash(git blame *)",
    "Bash(git branch *)",
    "Bash(git cat-file *)",
    "Bash(git -C *)",
    "Bash(git diff *)",
    "Bash(git fetch *)",
    "Bash(git for-each-ref *)",
    "Bash(git grep *)",
    "Bash(git log *)",
    "Bash(git ls-files *)",
    "Bash(git ls-tree *)",
    "Bash(git merge *)",
    "Bash(git pull *)",
    "Bash(git push)",
    "Bash(git push *)",
    "Bash(git remote *)",
    "Bash(git restore *)",
    "Bash(git rev-list *)",
    "Bash(git rev-parse *)",
    "Bash(git rm *)",
    "Bash(git show *)",
    "Bash(git status)",
    "Bash(git status *)",
    "Bash(git symbolic-ref *)",
    "Bash(git worktree *)",
    "Bash(cd *)",
    "Bash(pwd)",
    "Bash(chmod +x *)",
    "Bash(awk *)",
    "Bash(bash -n *)",
    "Bash(cat *)",
    "Bash(cmp *)",
    "Bash(command -v *)",
    "Bash(cut *)",
    "Bash(date)",
    "Bash(date *)",
    "Bash(diff *)",
    "Bash(file *)",
    "Bash(find *)",
    "Bash(grep *)",
    "Bash(head *)",
    "Bash(id)",
    "Bash(jq *)",
    "Bash(ls *)",
    "Bash(mkdir *)",
    "Bash(mktemp)",
    "Bash(mktemp *)",
    "Bash(psql *)",
    "Bash(rg *)",
    "Bash(sed *)",
    "Bash(shellcheck *)",
    "Bash(sort *)",
    "Bash(stat *)",
    "Bash(tail *)",
    "Bash(test -d *)",
    "Bash(touch *)",
    "Bash(tr *)",
    "Bash(uname *)",
    "Bash(uniq *)",
    "Bash(wc *)",
    "Bash(which *)",
    "Bash(whoami)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(gh pr list *)",
    "Bash(gh pr view *)",
    "Bash(gh pr checks *)",
    "Bash(gh pr merge *)",
    "Bash(gh pr comment *)",
    "Bash(gh pr diff *)",
    "Bash(gh pr ready *)",
    "Bash(gh pr reopen *)",
    "Bash(gh pr review *)",
    "Bash(gh pr status *)",
    "Bash(gh issue *)",
    "Bash(gh label *)",
    "Bash(gh browse *)",
    "Bash(gh search *)",
    "Bash(gh status)",
    "Bash(gh status *)",
    "Bash(gh repo view *)",
    "Bash(gh repo list *)",
    "Bash(gh run list *)",
    "Bash(gh run view *)",
    "Bash(gh run watch *)",
    "Bash(gh workflow list *)",
    "Bash(gh workflow view *)",
    "Bash(gh release list *)",
    "Bash(gh release view *)",
    "Bash(gh release create *)",
    "Bash(gh -C *)",
    "Bash(*bin/flow *)",
    "Bash(bin/test --adversarial-path)",
    "Bash(rm .flow-*)",
    "Bash(test -f *)",
    "Bash(claude plugin list)",
    "Bash(claude plugin marketplace add *)",
    "Bash(claude plugin install *)",
    "Bash(curl *)",
    "Read(~/.claude/rules/*)",
    "Read(~/.claude/projects/*/memory/*)",
    "Read(//tmp/*.txt)",
    "Read(//tmp/*.diff)",
    "Read(//tmp/*.patch)",
    "Read(//tmp/*.md)",
    "Read(//tmp/*.json)",
    "Agent(flow:ci-fixer)",
    "Skill(decompose:decompose)",
];

/// FLOW deny list — canonical source for deny permissions.
/// Shared with `prime_setup.rs` via pub import.
pub const FLOW_DENY: &[&str] = &[
    "Bash(git rebase *)",
    "Bash(git push --force *)",
    "Bash(git push -f *)",
    "Bash(git reset *)",
    "Bash(git reset --hard *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git clean *)",
    "Bash(git commit *)",
    "Bash(git config *)",
    "Bash(git branch -d *)",
    "Bash(git branch -D *)",
    "Bash(git symbolic-ref HEAD refs/*)",
    "Bash(git -C * checkout *)",
    "Bash(git -C * clean *)",
    "Bash(git -C * commit *)",
    "Bash(git -C * config *)",
    "Bash(git -C * push --force*)",
    "Bash(git -C * push -f*)",
    "Bash(git -C * rebase *)",
    "Bash(git -C * reset *)",
    "Bash(git -C * stash *)",
    "Bash(sed -i*)",
    "Bash(sed * -i*)",
    "Bash(gh pr merge * --admin*)",
    "Bash(gh pr merge --admin*)",
    "Bash(gh * --admin*)",
    "Bash(gh --admin*)",
    "Bash(gh auth login*)",
    "Bash(gh auth logout*)",
    "Bash(gh auth refresh*)",
    "Bash(gh auth setup-git*)",
    "Bash(gh auth switch*)",
    "Bash(gh auth token*)",
    "Bash(gh extension install *)",
    "Bash(gh issue delete *)",
    "Bash(gh issue lock *)",
    "Bash(gh issue transfer *)",
    "Bash(gh issue unlock *)",
    "Bash(gh label clone *)",
    "Bash(gh label delete *)",
    "Bash(gh release delete *)",
    "Bash(gh repo archive *)",
    "Bash(gh repo delete *)",
    "Bash(gh run cancel *)",
    "Bash(gh run delete *)",
    "Bash(gh secret *)",
    "Bash(gh ssh-key *)",
    "Bash(gh variable *)",
    "Bash(cargo *)",
    "Bash(rustc *)",
    "Bash(go *)",
    "Bash(bundle *)",
    "Bash(rubocop *)",
    "Bash(ruby *)",
    "Bash(rails *)",
    "Bash(xcodebuild *)",
    "Bash(xcrun *)",
    "Bash(swift *)",
    "Bash(swiftlint *)",
    "Bash(.venv/bin/*)",
    "Bash(python3 -m pytest *)",
    "Bash(pytest *)",
    "Bash(python *)",
    "Bash(python3 *)",
    "Bash(python3.10 *)",
    "Bash(python3.11 *)",
    "Bash(python3.12 *)",
    "Bash(python3.13 *)",
    "Bash(pip *)",
    "Bash(pip3 *)",
    "Bash(ruff *)",
    "Bash(pyenv *)",
    "Bash(poetry *)",
    "Bash(uv *)",
    "Bash(npm *)",
    "Bash(npx *)",
    "Bash(yarn *)",
    "Bash(pnpm *)",
    "Bash(gradle *)",
    "Bash(gradlew *)",
    "Bash(./gradlew *)",
    "Bash(mvn *)",
    "Bash(./mvnw *)",
    "Bash(mix *)",
    "Bash(elixir *)",
    "Bash(dotnet *)",
    "Bash(* && *)",
    "Bash(* ; *)",
    "Bash(* | *)",
];

/// Excluded paths — canonical source for git exclude entries.
/// Shared with `prime_setup.rs` via pub import.
///
/// The first five entries cover FLOW's own per-machine state
/// (`.flow-states/`, `.worktrees/`), the priming marker
/// (`.flow.json`), and ambient cost/lock state under `.claude/`.
///
/// The two adversarial-probe basename globs land per-language probe
/// files (e.g. `tests/test_adversarial_flow.py`,
/// `test/foo_adversarial_flow_test.rb`) in `.git/info/exclude` so
/// `git status` inside the worktree does not surface the throwaway
/// probe alongside intentional changes. The probe lives inside the
/// project's test tree so the language test runner can discover and
/// execute it; worktree removal at Phase 6 Complete then disposes of
/// the file as a side effect of removing the worktree directory.
pub const EXCLUDE_ENTRIES: &[&str] = &[
    ".flow-states/",
    ".worktrees/",
    ".flow.json",
    ".claude/cost/",
    ".claude/scheduled_tasks.lock",
    "test_adversarial_flow.*",
    "*_adversarial_flow_test.rb",
];

/// Custom `serde_json` formatter that emits `(", ", ": ")` separators
/// to match the format used by existing `.flow.json` files. Required
/// for hash stability on upgrade. Only the three separator methods are
/// overridden; everything else uses the default (compact) behavior.
struct PythonDefaultFormatter;

impl Formatter for PythonDefaultFormatter {
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        writer.write_all(b": ")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
}

/// Build the canonical config map for hashing.
///
/// Top-level keys are stored in a `BTreeMap` so serialization is
/// alphabetically sorted — required for the SHA-256 hash to be
/// stable across runs and machines. The canonical config is derived
/// from `UNIVERSAL_ALLOW`, `FLOW_DENY`, and `EXCLUDE_ENTRIES`.
fn canonical_config() -> BTreeMap<String, Value> {
    let mut allow: Vec<String> = UNIVERSAL_ALLOW.iter().map(|s| s.to_string()).collect();
    allow.sort();

    let mut deny: Vec<String> = FLOW_DENY.iter().map(|s| s.to_string()).collect();
    deny.sort();

    let mut exclude: Vec<String> = EXCLUDE_ENTRIES.iter().map(|s| s.to_string()).collect();
    exclude.sort();

    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    map.insert("allow".to_string(), json!(allow));
    map.insert("defaultMode".to_string(), json!("acceptEdits"));
    map.insert("deny".to_string(), json!(deny));
    map.insert("exclude".to_string(), json!(exclude));
    map
}

/// Compute a deterministic 12-char hex digest of the canonical config.
/// The byte sequence fed to SHA-256 must remain stable across plugin
/// versions because users' stored `.flow.json` config_hash values are
/// compared against this output to decide whether a re-prime is needed.
/// Any change to the formatter, key order, or value shape invalidates
/// every existing hash.
pub fn compute_config_hash() -> String {
    let canonical = canonical_config();
    let mut buf: Vec<u8> = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonDefaultFormatter);
    // BTreeMap<String, Value> built from compile-time constants always
    // serializes successfully — no I/O, no user-supplied values, no
    // non-serializable types. A failure here is a programmer bug, not a
    // runtime error.
    canonical
        .serialize(&mut ser)
        .expect("canonical config always serializes");
    let mut hasher = Sha256::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    hex_prefix(&digest, 12)
}

/// Compute a 12-char hex digest of src/prime_setup.rs bytes.
/// The hash covers every installation artifact (hooks, excludes,
/// priming, dependencies). When the source file changes, the hash
/// changes and `prime_check` forces a re-prime so users pick up the
/// new setup. Pre-existing stored hashes that no longer match will
/// trigger a forced re-prime, which is the intended behavior.
pub fn compute_setup_hash(plugin_root: &Path) -> Result<String, String> {
    let path = plugin_root.join("src").join("prime_setup.rs");
    let bytes = fs::read(&path).map_err(|e| format!("Could not read {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    Ok(hex_prefix(&digest, 12))
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    use std::fmt::Write;
    // (n + 1) / 2 bytes provide enough hex chars to cover n output
    // characters; `truncate` trims the final char when n is odd.
    let take = n.div_ceil(2);
    let mut s = String::with_capacity(take * 2);
    for b in bytes.iter().take(take) {
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s.truncate(n);
    s
}

/// Read and parse `.flow.json` from the project root. Returns
/// `None` on any I/O or parse error so the caller decides whether
/// the missing or malformed file is fatal — most callers treat it
/// as "FLOW not initialized in this project".
///
/// `.flow.json` lives at `<project_root>/.flow.json` regardless of
/// the user's current working directory. Mono-repo subdirectory
/// flows still find the project's marker because callers pass
/// `project_root` here, not `cwd`.
fn read_flow_json(project_root: &Path) -> Option<Value> {
    let content = fs::read_to_string(project_root.join(".flow.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Filter `Some("")` as falsy — both missing keys and empty strings
/// should be treated as absent. See rust-patterns.md
/// "Empty-String vs Missing-Key Equivalence".
fn as_nonempty_str(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty())
}

#[derive(ClapArgs)]
pub struct Args {}

/// Build the prime-check result as a JSON value.
///
/// Returns `Ok` for both `status: ok` (happy path, auto-upgrade) and
/// `status: error` results so the CLI prints the result and exits 0
/// in either case — the caller skill always parses the JSON regardless
/// of whether the prime check passed. `Err` is reserved for
/// infrastructure failures (plugin root not found, plugin.json
/// unreadable) that should exit 1.
///
/// `project_root` is the directory containing `.flow.json` — typically
/// the git repo root. Callers must resolve this via `git::project_root()`
/// (or equivalent) before invoking, so mono-repo subdirectory flows
/// (cwd inside `synapse/`, `cortex/`, etc.) find the project's
/// `.flow.json` instead of failing because the current dir lacks one.
pub fn run_impl(project_root: &Path, plugin_root: &Path) -> Result<Value, String> {
    let init_data = match read_flow_json(project_root) {
        Some(v) => v,
        None => {
            return Ok(json!({
                "status": "error",
                "message": "FLOW not initialized. Run /flow:flow-prime first.",
            }));
        }
    };

    let plugin_json_path = plugin_root.join(".claude-plugin").join("plugin.json");
    let plugin_content = fs::read_to_string(&plugin_json_path)
        .map_err(|e| format!("Could not read {}: {}", plugin_json_path.display(), e))?;
    let plugin_data: Value = serde_json::from_str(&plugin_content)
        .map_err(|e| format!("Could not parse plugin.json: {}", e))?;
    let plugin_version = plugin_data
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "plugin.json missing version".to_string())?
        .to_string();

    let stored_flow_version = init_data
        .get("flow_version")
        .and_then(as_nonempty_str)
        .map(String::from);

    if stored_flow_version.as_deref() != Some(plugin_version.as_str()) {
        let stored_display = stored_flow_version.clone().unwrap_or_default();
        let stored_config = init_data.get("config_hash").and_then(as_nonempty_str);
        let stored_setup = init_data.get("setup_hash").and_then(as_nonempty_str);

        let plugin_config_hash = compute_config_hash();
        let plugin_setup_hash = compute_setup_hash(plugin_root)?;

        let config_match = stored_config
            .map(|s| s == plugin_config_hash)
            .unwrap_or(false);
        let setup_match = stored_setup
            .map(|s| s == plugin_setup_hash)
            .unwrap_or(false);

        if config_match && setup_match {
            let old_version = stored_display.clone();
            let mut updated = init_data.clone();
            updated["flow_version"] = json!(plugin_version);
            // `updated` is an in-memory Value we just cloned and mutated.
            // serde_json::to_string on a Value cannot fail for any shape
            // we construct here (no float NaN, no non-UTF-8 strings), so
            // a serialization error would be a programmer bug.
            let serialized =
                serde_json::to_string(&updated).expect("in-memory Value always serializes");
            fs::write(project_root.join(".flow.json"), format!("{}\n", serialized))
                .map_err(|e| format!("Could not write .flow.json: {}", e))?;

            return Ok(json!({
                "status": "ok",
                "auto_upgraded": true,
                "old_version": old_version,
                "new_version": plugin_version,
            }));
        }

        return Ok(json!({
            "status": "error",
            "message": format!(
                "FLOW version mismatch: initialized for v{}, plugin is v{}. \
        Run /flow:flow-prime --reprime to upgrade (keeps current config), or /flow:flow-prime to reconfigure.",
                stored_display, plugin_version
            ),
        }));
    }

    Ok(json!({
        "status": "ok",
    }))
}

/// Main-arm dispatch: accepts a resolved `project_root` and
/// `plugin_root` Option directly. Returns `(value, exit_code)` for
/// the caller to print and exit.
///
/// Callers must pass the project root (where `.flow.json` lives), not
/// the user's current working directory. From a mono-repo subdirectory,
/// the two are different and only the project root finds the marker.
pub fn run_impl_main(project_root: &Path, plugin_root: Option<PathBuf>) -> (Value, i32) {
    let root = match plugin_root {
        Some(p) => p,
        None => {
            return (
                json!({
                    "status": "error",
                    "message": "Plugin root not found",
                }),
                1,
            );
        }
    };
    match run_impl(project_root, &root) {
        Ok(value) => (value, 0),
        Err(msg) => (
            json!({
                "status": "error",
                "message": msg,
            }),
            1,
        ),
    }
}
