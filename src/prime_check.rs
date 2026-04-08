//! Version gate — verify `/flow:flow-prime` has been run with a
//! matching version. Port of lib/prime-check.py.
//!
//! Usage: `bin/flow prime-check`
//!
//! Output (JSON to stdout):
//!   Success: `{"status": "ok", "framework": "rails|python|ios|go|rust"}`
//!   Auto-upgrade: `{"status": "ok", "framework": "...", "auto_upgraded": true, "old_version": "...", "new_version": "..."}`
//!   Failure: `{"status": "error", "message": "..."}`
//!
//! # Constants
//!
//! `UNIVERSAL_ALLOW`, `FLOW_DENY`, and `EXCLUDE_ENTRIES` are the
//! canonical source for permission and exclude lists. They are shared
//! with `src/prime_setup.rs` which imports them via `pub use`.
//!
//! # Hash byte-parity with Python
//!
//! `compute_config_hash` must produce byte-identical SHA-256 input
//! bytes to Python's `json.dumps(canonical, sort_keys=True)`. Python's
//! default separators are `(", ", ": ")`; Rust's
//! `serde_json::to_string` default is `(",", ":")`. Without a fix the
//! resulting digests differ, breaking round-trip with Python-written
//! `.flow.json` files. `PythonDefaultFormatter` below implements the
//! three `serde_json::ser::Formatter` methods needed to emit the
//! Python separators.

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

use crate::utils::{frameworks_dir, plugin_root};

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
    "Bash(*bin/*)",
    "Bash(rm .flow-*)",
    "Bash(rm tests/test_adversarial_*)",
    "Bash(claude plugin list)",
    "Bash(claude plugin marketplace add *)",
    "Bash(claude plugin install *)",
    "Bash(curl *)",
    "Read(~/.claude/rules/*)",
    "Read(~/.claude/projects/**/tool-results/*)",
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
/// to match Python's `json.dumps` default. Required for byte-parity
/// with Python's hash input. Only the three separator methods are
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

/// Load framework-specific permissions from frameworks/<name>/permissions.json.
/// Returns an empty vec if the file is missing (Python parity).
pub fn load_framework_permissions(framework: &str, fw_dir: &Path) -> Vec<String> {
    let path = fw_dir.join(framework).join("permissions.json");
    if !path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let data: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    data.get("allow")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the canonical config map for a framework.
/// Top-level keys are in a BTreeMap so serialization is alphabetically
/// sorted, matching Python's `json.dumps(sort_keys=True)`.
fn canonical_config(framework: &str, fw_dir: &Path) -> BTreeMap<String, Value> {
    let mut allow: Vec<String> = UNIVERSAL_ALLOW.iter().map(|s| s.to_string()).collect();
    allow.extend(load_framework_permissions(framework, fw_dir));
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
/// Must produce output byte-identical to Python's
/// `hashlib.sha256(json.dumps(canonical, sort_keys=True).encode()).hexdigest()[:12]`.
pub fn compute_config_hash(framework: &str, fw_dir: &Path) -> Result<String, String> {
    let canonical = canonical_config(framework, fw_dir);
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
/// Changed from lib/prime-setup.py in PR #894 (Rust port). Existing
/// users with Python-era hashes will be forced to re-prime, which is
/// correct for this major infrastructure change.
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
    let take = (n + 1) / 2;
    let mut s = String::with_capacity(take * 2);
    for b in bytes.iter().take(take) {
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s.truncate(n);
    s
}

/// Read and parse `.flow.json` from the given directory. Returns None
/// on any I/O or parse error — matches `flow_utils.read_flow_json`
/// Python semantics where the caller decides error policy.
fn read_flow_json(cwd: &Path) -> Option<Value> {
    let content = fs::read_to_string(cwd.join(".flow.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Filter `Some("")` as falsy. Matches Python's `if x:` semantics for
/// string dict values: both missing keys and empty strings are falsy.
/// See rust-port-parity.md "Empty-String vs Missing-Key Falsy Equivalence".
fn as_nonempty_str(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty())
}

#[derive(ClapArgs)]
pub struct Args {}

/// Build the prime-check result as a JSON value.
///
/// Returns `Ok` for `status: ok` results (happy path, auto-upgrade) and
/// for `status: error` results that Python prints with `sys.exit(0)`.
/// Returns `Err` only for infrastructure failures (plugin root not
/// found, plugin.json unreadable) that should exit 1.
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
        let framework = init_data
            .get("framework")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let fw_dir =
            frameworks_dir().ok_or_else(|| "Frameworks directory not found".to_string())?;
        let plugin_config_hash = compute_config_hash(framework, &fw_dir)?;
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
            if !(updated.is_object() || updated.is_null()) {
                return Ok(json!({
                    "status": "error",
                    "message": "FLOW not initialized. Run /flow:flow-prime first.",
                }));
            }
            updated["flow_version"] = json!(plugin_version);
            let serialized = serde_json::to_string(&updated)
                .map_err(|e| format!("Could not serialize .flow.json: {}", e))?;
            fs::write(cwd.join(".flow.json"), format!("{}\n", serialized))
                .map_err(|e| format!("Could not write .flow.json: {}", e))?;

            return Ok(json!({
                "status": "ok",
                "framework": framework,
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

    let framework = init_data
        .get("framework")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !matches!(framework, "rails" | "python" | "ios" | "go" | "rust") {
        return Ok(json!({
            "status": "error",
            "message": "Missing framework in .flow.json. Run /flow:flow-prime to configure.",
        }));
    }

    Ok(json!({
        "status": "ok",
        "framework": framework,
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
