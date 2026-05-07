//! Pure capture function reading the three account-window inputs:
//! the rate-limits JSON in `~/.claude`, the session transcript JSONL,
//! and the per-session cost file under `.claude/cost/<YYYY-MM>/`.
//!
//! The helper is invoked by every state-mutating transition in
//! `phase_enter`, `phase_finalize`, `phase_transition`, `set_timestamp`
//! (when the mutated field names a step counter), `start_init`, and
//! `complete_finalize` — so it MUST never panic and MUST never block
//! on input that does not exist. Each input source is read with a
//! fail-open guard: a missing file leaves the corresponding fields
//! as `None` but the snapshot is still produced. `captured_at` is
//! always populated because it comes from the caller-supplied
//! `now_fn` closure.
//!
//! The capture function is pure given its inputs (paths + closure
//! values) — every effectful read is funnelled through `home`,
//! `transcript_path`, and `cost_path` so tests can supply tempdir
//! fixtures that drive every branch without mocking the filesystem.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::Value;

use crate::state::{ModelTokens, StepSnapshot, WindowSnapshot};
use crate::utils::{now, tolerant_i64_opt};

/// Capture an account-window snapshot.
///
/// `home` — directory holding `.claude/rate-limits.json`. Pass the
/// real `$HOME` in production; tests pass a tempdir.
///
/// `transcript_path` — optional path to the session transcript
/// JSONL. `None` skips the read entirely; missing or malformed
/// JSONL contributes nothing rather than failing.
///
/// `cost_path` — optional path to the per-session cost file (a
/// single floating-point number on a single line). `None` or
/// missing file leaves `session_cost_usd` as `None`.
///
/// `session_id` — the active session UUID copied through to the
/// snapshot for downstream multi-session delta math.
///
/// `now_fn` — wall-clock closure. Production passes
/// `crate::utils::now`; tests pass a fixed string so assertions
/// are deterministic.
pub fn capture(
    home: &Path,
    transcript_path: Option<&Path>,
    cost_path: Option<&Path>,
    session_id: Option<&str>,
    now_fn: impl FnOnce() -> String,
) -> WindowSnapshot {
    let captured_at = now_fn();

    let (five_hour_pct, seven_day_pct) = read_rate_limits(home);
    let cost = cost_path.and_then(read_cost);
    let agg = transcript_path.map(read_transcript).unwrap_or_default();

    let context_window_pct = agg.context_at_last_turn.and_then(|tokens| {
        agg.last_model
            .as_deref()
            .and_then(context_window_size)
            .map(|window| (tokens as f64) * 100.0 / (window as f64))
    });

    WindowSnapshot {
        captured_at,
        session_id: session_id.map(|s| s.to_string()),
        model: agg.last_model,
        five_hour_pct,
        seven_day_pct,
        session_input_tokens: agg.totals_present.then_some(agg.input_tokens),
        session_output_tokens: agg.totals_present.then_some(agg.output_tokens),
        session_cache_creation_tokens: agg.totals_present.then_some(agg.cache_creation_tokens),
        session_cache_read_tokens: agg.totals_present.then_some(agg.cache_read_tokens),
        session_cost_usd: cost,
        by_model: agg.by_model,
        turn_count: agg.totals_present.then_some(agg.turn_count),
        tool_call_count: agg.totals_present.then_some(agg.tool_call_count),
        context_at_last_turn_tokens: agg.context_at_last_turn,
        context_window_pct,
    }
}

/// Production binder around `capture` for the six producer call
/// sites. Reads `session_id` and `transcript_path` from the
/// in-memory state JSON, derives the per-session cost-file path
/// under `<project_root>/.claude/cost/<YYYY-MM>/<session_id>.txt`,
/// and invokes `capture` with `home` plus those paths.
///
/// Producers call this from inside `mutate_state` closures (the
/// state JSON is already in memory) and write the returned
/// snapshot into the appropriate state field. `home` is supplied
/// by the producer (typically `$HOME`) so this helper takes no
/// process-env dependency.
pub fn capture_for_active_state(home: &Path, state: &Value, project_root: &Path) -> WindowSnapshot {
    // session_id and transcript_path are both state-derived strings.
    // A corrupted or hand-edited `.flow-states/<branch>/state.json`
    // can populate either field with attacker-controlled values, so
    // we validate before constructing filesystem paths. session_id
    // must look like a UUID-shaped token (no path separators, no
    // traversal segments). transcript_path is rejected when it is
    // not absolute or escapes the user's `~/.claude/projects/`
    // directory — the only place flow's session transcripts live in
    // production.
    let session_id = state
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| is_safe_session_id(s))
        .map(|s| s.to_string());
    let transcript_path = state
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| is_safe_transcript_path(p, home));
    let cost_path = session_id
        .as_ref()
        .map(|sid| cost_file_path(project_root, sid));
    capture(
        home,
        transcript_path.as_deref(),
        cost_path.as_deref(),
        session_id.as_deref(),
        now,
    )
}

/// Validate a state-derived `session_id` against the shape Claude
/// Code populates: alphanumeric plus `-` and `_`, no path separators
/// or traversal segments. Rejects `..`, `.`, `/`, `\`, NUL, and any
/// other character that could escape the per-session cost-file path.
fn is_safe_session_id(s: &str) -> bool {
    if s.is_empty() || s == "." || s == ".." {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validate a state-derived `transcript_path` against the canonical
/// location where Claude Code writes session transcripts:
/// `<home>/.claude/projects/`. Rejects relative paths, paths
/// outside that prefix, and paths containing a NUL byte. Production
/// transcripts always live under that directory; values pointing
/// elsewhere are corrupted state and read attempts could leak
/// arbitrary file contents into snapshot fields.
///
/// Cross-module consumer: `src/hooks/transcript_walker.rs` validates
/// the same `transcript_path` string before reading the JSONL
/// session log to gate user-only skill invocations and the
/// validate-ask-user carve-out. Per
/// `.claude/rules/external-input-path-construction.md`, the same
/// validator runs at every state-derived path-construction site so
/// the prefix-containment contract is enforced once.
pub fn is_safe_transcript_path(path: &Path, home: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    if path.to_string_lossy().contains('\0') {
        return false;
    }
    if !path.is_absolute() {
        return false;
    }
    let expected_prefix = home.join(".claude").join("projects");
    path.starts_with(&expected_prefix)
}

/// Write a `WindowSnapshot` into the named top-level field of a
/// state JSON value. No-op when `state` is not an object — the
/// guard mirrors the project-wide convention from
/// `.claude/rules/rust-patterns.md` "State Mutation Object Guards"
/// for `mutate_state` closures. Producers call this from inside a
/// `mutate_state` closure so the field write is atomic with the
/// state-file lock.
pub fn write_snapshot_into_state(state: &mut Value, field: &str, snapshot: &WindowSnapshot) {
    if let Some(obj) = state.as_object_mut() {
        // `WindowSnapshot` is a derived-`Serialize` struct over
        // primitive and `Option<primitive>` fields plus an `IndexMap`
        // — serialization is infallible in practice. Match the
        // `.expect()` contract used at the four per-phase callsites
        // (phase_enter, phase_finalize, phase_transition,
        // set_timestamp) so a future schema change that breaks
        // serialization fails loudly here instead of silently
        // writing `null` and corrupting consumer queries.
        let value = serde_json::to_value(snapshot).expect("WindowSnapshot must serialize");
        obj.insert(field.to_string(), value);
    }
}

/// Append a `StepSnapshot` to `state.phases.<phase>.step_snapshots[]`.
///
/// Wraps the supplied `WindowSnapshot` in a `StepSnapshot` carrying
/// the counter value and the field name (one of the five named step
/// counters per `commands::set_timestamp::is_step_counter_field`),
/// then appends to the array. The array is auto-initialized to
/// `[]` when the slot is missing or holds a non-array value (legacy
/// state files). No-op when `state` itself is not an object.
pub fn append_step_snapshot(
    state: &mut Value,
    phase: &str,
    step: i64,
    field: &str,
    snapshot: WindowSnapshot,
) {
    if !state.is_object() {
        return;
    }
    // Per-level object guards per `.claude/rules/rust-patterns.md`
    // "State Mutation Object Guards" + `.claude/rules/state-files.md`
    // "Corruption Resilience": auto-heal `state["phases"]` and the
    // per-phase entry to objects when a hand-edited state file holds
    // wrong types (number / string / array). Without these guards,
    // the IndexMut chain below panics with `cannot access key X in
    // JSON <type>` and crashes every step-counter increment.
    if !state["phases"].is_object() {
        state["phases"] = serde_json::json!({});
    }
    if !state["phases"][phase].is_object() {
        state["phases"][phase] = serde_json::json!({});
    }
    let step_snap = StepSnapshot {
        step,
        field: field.to_string(),
        snapshot,
    };
    let value = serde_json::to_value(&step_snap).expect("StepSnapshot must serialize");
    if !state["phases"][phase]["step_snapshots"].is_array() {
        state["phases"][phase]["step_snapshots"] = serde_json::json!([]);
    }
    state["phases"][phase]["step_snapshots"]
        .as_array_mut()
        .expect("step_snapshots normalized to array above")
        .push(value);
}

/// Read `$HOME` as a `PathBuf`, falling back to an empty path
/// when the env var is unset. Producers call this once per
/// transition to thread the home dir into `capture_for_active_state`.
/// Empty home is harmless — `capture` reads
/// `<home>/.claude/rate-limits.json` and the open fails gracefully.
pub fn home_dir_or_empty() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Resolve the per-session cost-file path
/// `<project_root>/.claude/cost/<YYYY-MM>/<session_id>.txt`. The
/// month folder mirrors `tui_data::load_account_metrics` so the
/// snapshot reads the same file that account-monthly aggregation
/// already reads.
fn cost_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    let now_local = chrono::Local::now();
    let year_month = now_local.format("%Y-%m").to_string();
    project_root
        .join(".claude")
        .join("cost")
        .join(year_month)
        .join(format!("{}.txt", session_id))
}

/// Read `~/.claude/rate-limits.json` and extract the two pct fields.
/// Missing file or malformed JSON returns `(None, None)`. Reject
/// empty/relative `home` outright — joining with a relative path
/// would resolve `.claude/rate-limits.json` against the worktree's
/// cwd and read a committed `.claude/rate-limits.json` from a
/// hostile repo as if it were the user's rate-limits data.
fn read_rate_limits(home: &Path) -> (Option<i64>, Option<i64>) {
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return (None, None);
    }
    let path = home.join(".claude").join("rate-limits.json");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let five = value.get("five_hour_pct").and_then(tolerant_i64_opt);
    let seven = value.get("seven_day_pct").and_then(tolerant_i64_opt);
    (five, seven)
}

/// Read a per-session cost file (a single floating-point number).
/// Missing file, malformed content, or non-finite parse returns `None`.
fn read_cost(path: &Path) -> Option<f64> {
    let content = fs::read_to_string(path).ok()?;
    let parsed: f64 = content.trim().parse().ok()?;
    if parsed.is_finite() {
        Some(parsed)
    } else {
        None
    }
}

/// Aggregate state derived from a single transcript scan.
#[derive(Default)]
struct TranscriptAgg {
    /// Whether at least one assistant message contributed counters.
    /// When false the snapshot leaves token / turn / tool counts as
    /// `None` rather than reporting structurally-zero values that
    /// could be confused with "session ran but used no tokens".
    totals_present: bool,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    turn_count: i64,
    tool_call_count: i64,
    last_model: Option<String>,
    context_at_last_turn: Option<i64>,
    by_model: IndexMap<String, ModelTokens>,
}

/// Hard cap on transcript bytes read per snapshot. Capture runs at
/// every state-mutating transition (six producer call sites,
/// including `set_timestamp` for every step counter advance). A
/// long autonomous flow accumulates a transcript that grows without
/// bound; reading the full file dozens of times within a single
/// session is O(tasks × transcript_size) and risks OOM on
/// memory-constrained machines. 50 MB covers a multi-thousand-turn
/// session (typical compacted transcripts are < 10 MB) while
/// bounding worst-case I/O. When a transcript exceeds the cap the
/// counters are derived from a prefix of the file rather than the
/// whole tail — counts and percentages may under-report but the
/// process stays bounded.
const TRANSCRIPT_BYTE_CAP: u64 = 50 * 1024 * 1024;

/// Line-stream the transcript JSONL accumulating assistant-message
/// usage. Lines that fail to parse as JSON are skipped silently —
/// transcripts can include partial writes at the tail when a
/// session is in flight. Reads at most `TRANSCRIPT_BYTE_CAP` bytes
/// to bound I/O across long autonomous flows.
fn read_transcript(path: &Path) -> TranscriptAgg {
    let mut agg = TranscriptAgg::default();
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return agg,
    };
    let reader = BufReader::new(file.take(TRANSCRIPT_BYTE_CAP));
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };

        agg.totals_present = true;
        agg.turn_count = agg.turn_count.saturating_add(1);

        let model = message.get("model").and_then(|m| m.as_str());
        if let Some(m) = model {
            agg.last_model = Some(m.to_string());
        }

        let usage = message.get("usage");
        let input = usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(tolerant_i64_opt)
            .unwrap_or(0);
        let output = usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(tolerant_i64_opt)
            .unwrap_or(0);
        let cache_create = usage
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(tolerant_i64_opt)
            .unwrap_or(0);
        let cache_read = usage
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(tolerant_i64_opt)
            .unwrap_or(0);

        agg.input_tokens = agg.input_tokens.saturating_add(input);
        agg.output_tokens = agg.output_tokens.saturating_add(output);
        agg.cache_creation_tokens = agg.cache_creation_tokens.saturating_add(cache_create);
        agg.cache_read_tokens = agg.cache_read_tokens.saturating_add(cache_read);

        // Context window utilization measures tokens sent INTO the
        // model for this turn. Per Anthropic API: `input_tokens`,
        // `cache_creation_input_tokens`, and `cache_read_input_tokens`
        // are three distinct buckets that together total the input
        // context. `output_tokens` is generated by the model, not
        // part of the context window for this turn — including it
        // overcounts and produces context_window_pct values above
        // 100% on real flows.
        agg.context_at_last_turn = Some(
            input
                .saturating_add(cache_create)
                .saturating_add(cache_read),
        );

        if let Some(m) = model {
            let entry = agg.by_model.entry(m.to_string()).or_default();
            entry.input = entry.input.saturating_add(input);
            entry.output = entry.output.saturating_add(output);
            entry.cache_create = entry.cache_create.saturating_add(cache_create);
            entry.cache_read = entry.cache_read.saturating_add(cache_read);
        }

        if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    agg.tool_call_count = agg.tool_call_count.saturating_add(1);
                }
            }
        }
    }
    agg
}

/// Lookup table for known Claude model context-window sizes.
///
/// Returns `Some(n)` when the model name matches a known family;
/// `None` for unknown models so `context_window_pct` defaults to
/// `None` rather than presenting a misleading percentage based on a
/// guessed window size. The `[1m]` suffix marks the 1M-context
/// variant of Opus 4.7; standard Claude models fall back to 200K.
fn context_window_size(model: &str) -> Option<i64> {
    if model.contains("[1m]") {
        return Some(1_000_000);
    }
    if model.starts_with("claude-") {
        return Some(200_000);
    }
    None
}
