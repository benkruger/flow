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
use std::process;

use clap::Args as ClapArgs;
use serde::Serialize;
use serde_json::ser::Formatter;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::utils::plugin_root;

/// Universal allow list — canonical source for all permission merging.
/// Shared with `prime_setup.rs` via pub import.
pub const UNIVERSAL_ALLOW: &[&str] = &[
    "Bash(git add *)",
    "Bash(git blame *)",
    "Bash(git branch *)",
    "Bash(git config *)",
    "Bash(git -C *)",
    "Bash(git diff *)",
    "Bash(git fetch *)",
    "Bash(git log *)",
    "Bash(git merge *)",
    "Bash(git pull *)",
    "Bash(git push)",
    "Bash(git push *)",
    "Bash(git remote *)",
    "Bash(git reset *)",
    "Bash(git restore *)",
    "Bash(git rev-list *)",
    "Bash(git rev-parse *)",
    "Bash(git show *)",
    "Bash(git status)",
    "Bash(git symbolic-ref *)",
    "Bash(git worktree *)",
    "Bash(cd *)",
    "Bash(pwd)",
    "Bash(chmod +x *)",
    "Bash(gh pr create *)",
    "Bash(gh pr edit *)",
    "Bash(gh pr close *)",
    "Bash(gh pr list *)",
    "Bash(gh pr view *)",
    "Bash(gh pr checks *)",
    "Bash(gh pr merge *)",
    "Bash(gh issue *)",
    "Bash(gh label *)",
    "Bash(gh -C *)",
    "Bash(*bin/flow *)",
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
    "Agent(flow:ci-fixer)",
    "Skill(decompose:decompose)",
];

/// FLOW deny list — canonical source for deny permissions.
/// Shared with `prime_setup.rs` via pub import.
pub const FLOW_DENY: &[&str] = &[
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
pub const EXCLUDE_ENTRIES: &[&str] = &[
    ".flow-states/",
    ".worktrees/",
    ".flow.json",
    ".claude/cost/",
    ".claude/scheduled_tasks.lock",
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
pub fn compute_config_hash() -> Result<String, String> {
    let canonical = canonical_config();
    let mut buf: Vec<u8> = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonDefaultFormatter);
    canonical
        .serialize(&mut ser)
        .map_err(|e| format!("Failed to serialize canonical config: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    Ok(hex_prefix(&digest, 12))
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

/// Read and parse `.flow.json` from the given directory. Returns
/// `None` on any I/O or parse error so the caller decides whether
/// the missing or malformed file is fatal — most callers treat it
/// as "FLOW not initialized in this project".
fn read_flow_json(cwd: &Path) -> Option<Value> {
    let content = fs::read_to_string(cwd.join(".flow.json")).ok()?;
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
pub fn run_impl(cwd: &Path, plugin_root: &Path) -> Result<Value, String> {
    let init_data = match read_flow_json(cwd) {
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

        let plugin_config_hash = compute_config_hash()?;
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
            let serialized = serde_json::to_string(&updated)
                .map_err(|e| format!("Could not serialize .flow.json: {}", e))?;
            fs::write(cwd.join(".flow.json"), format!("{}\n", serialized))
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

pub fn run(_args: Args) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = match plugin_root() {
        Some(p) => p,
        None => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "message": "Plugin root not found",
                })
            );
            process::exit(1);
        }
    };
    match run_impl(&cwd, &root) {
        Ok(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
        }
        Err(msg) => {
            println!(
                "{}",
                json!({
                    "status": "error",
                    "message": msg,
                })
            );
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Use the real plugin root so compute_setup_hash and plugin.json
    /// lookups succeed. The fixture cwd is a tempdir under our control.
    fn real_plugin_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn write_flow_json(cwd: &Path, content: &str) {
        fs::write(cwd.join(".flow.json"), content).unwrap();
    }

    #[test]
    fn no_flow_json_returns_not_initialized_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("FLOW not initialized"));
    }

    #[test]
    fn matching_version_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        // Read the actual plugin version so we're testing against truth.
        let plugin_content =
            fs::read_to_string(root.join(".claude-plugin").join("plugin.json")).unwrap();
        let plugin_data: Value = serde_json::from_str(&plugin_content).unwrap();
        let version = plugin_data["version"].as_str().unwrap();

        write_flow_json(dir.path(), &format!(r#"{{"flow_version": "{}"}}"#, version));

        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "ok");
    }

    #[test]
    fn version_mismatch_with_no_hashes_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        write_flow_json(dir.path(), r#"{"flow_version": "0.0.1-ancient"}"#);

        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("FLOW version mismatch"));
    }

    #[test]
    fn version_mismatch_with_matching_hashes_auto_upgrades() {
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        // Use the current hashes so the auto-upgrade path triggers.
        let config_hash = compute_config_hash().unwrap();
        let setup_hash = compute_setup_hash(&root).unwrap();
        write_flow_json(
            dir.path(),
            &format!(
                r#"{{"flow_version": "0.0.1-prior", "config_hash": "{}", "setup_hash": "{}"}}"#,
                config_hash, setup_hash
            ),
        );

        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["auto_upgraded"], true);
        assert_eq!(result["old_version"], "0.0.1-prior");

        // The on-disk .flow.json should have the new version written.
        let updated: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(".flow.json")).unwrap())
                .unwrap();
        // Read plugin version to verify it was written.
        let plugin_content =
            fs::read_to_string(root.join(".claude-plugin").join("plugin.json")).unwrap();
        let plugin_data: Value = serde_json::from_str(&plugin_content).unwrap();
        let expected_version = plugin_data["version"].as_str().unwrap();
        assert_eq!(updated["flow_version"], expected_version);
    }

    #[test]
    fn version_mismatch_with_stale_config_hash_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        let setup_hash = compute_setup_hash(&root).unwrap();
        // config_hash is bogus — auto-upgrade should not trigger.
        write_flow_json(
            dir.path(),
            &format!(
                r#"{{"flow_version": "0.0.1-prior", "config_hash": "deadbeef0000", "setup_hash": "{}"}}"#,
                setup_hash
            ),
        );

        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("version mismatch"));
    }

    #[test]
    fn as_nonempty_str_handles_empty_and_present() {
        let with_value = json!("hello");
        let empty = json!("");
        let null = json!(null);
        let num = json!(42);
        assert_eq!(as_nonempty_str(&with_value), Some("hello"));
        assert_eq!(as_nonempty_str(&empty), None);
        assert_eq!(as_nonempty_str(&null), None);
        assert_eq!(as_nonempty_str(&num), None);
    }

    #[test]
    fn read_flow_json_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_flow_json(dir.path()).is_none());
    }

    #[test]
    fn read_flow_json_malformed_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        write_flow_json(dir.path(), "not json");
        assert!(read_flow_json(dir.path()).is_none());
    }

    #[test]
    fn read_flow_json_valid_returns_parsed() {
        let dir = tempfile::tempdir().unwrap();
        write_flow_json(dir.path(), r#"{"a": 1}"#);
        let v = read_flow_json(dir.path()).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn compute_config_hash_deterministic() {
        let h1 = compute_config_hash().unwrap();
        let h2 = compute_config_hash().unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 12);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compute_setup_hash_deterministic() {
        let root = real_plugin_root();
        let h1 = compute_setup_hash(&root).unwrap();
        let h2 = compute_setup_hash(&root).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 12);
    }

    #[test]
    fn compute_setup_hash_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = compute_setup_hash(dir.path()).unwrap_err();
        assert!(err.contains("Could not read"));
    }

    #[test]
    fn version_mismatch_with_empty_stored_version() {
        // An empty flow_version string is treated as absent by
        // the `as_nonempty_str` helper (defined above in this module),
        // so stored_display defaults to "" and the mismatch message
        // fires with an empty version prefix.
        let dir = tempfile::tempdir().unwrap();
        let root = real_plugin_root();
        write_flow_json(dir.path(), r#"{"flow_version": ""}"#);

        let result = run_impl(dir.path(), &root).unwrap();
        assert_eq!(result["status"], "error");
        let msg = result["message"].as_str().unwrap();
        assert!(
            msg.contains("version mismatch"),
            "expected mismatch, got: {}",
            msg
        );
        // The stored version display should be empty (initialized for v)
        assert!(
            msg.contains("initialized for v,"),
            "expected empty version prefix, got: {}",
            msg
        );
    }
}
