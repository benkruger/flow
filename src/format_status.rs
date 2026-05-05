use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use crate::flow_paths::FlowPaths;
use crate::git::resolve_branch;
use crate::phase_config::{self, find_state_files, load_phase_config, PhaseConfig, PHASE_ORDER};
use crate::state::FlowState;
use crate::utils::{derive_feature, detect_dev_mode, elapsed_since, format_time, read_version};
use crate::window_deltas::flow_total;

/// Column width for phase name alignment.
const NAME_WIDTH: usize = 12;

/// Format an integer token count as a compact string: `1.2K`, `3.4M`,
/// or the raw integer when below 1000. Stable formatting so test
/// assertions can pin specific values.
fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Render the Tokens line from `state.window_at_*` snapshots and the
/// per-phase snapshots via `window_deltas::flow_total`. Returns `None`
/// when the state has no token activity (no tokens, no cost, no reset
/// observed) so the caller omits the line entirely rather than rendering
/// a placeholder. The reset marker (`↻`) is appended when any span
/// observed a rate-limit window reset.
fn tokens_line(state: &Value) -> Option<String> {
    let flow_state: FlowState = serde_json::from_value(state.clone()).ok()?;
    let report = flow_total(&flow_state);
    let total = report
        .input_tokens_delta
        .saturating_add(report.output_tokens_delta)
        .saturating_add(report.cache_creation_tokens_delta)
        .saturating_add(report.cache_read_tokens_delta);
    if total == 0
        && report.cost_delta_usd.abs() < f64::EPSILON
        && !report.window_reset_observed
    {
        return None;
    }
    let marker = if report.window_reset_observed {
        "  ↻"
    } else {
        ""
    };
    Some(format!(
        "  Tokens  : {}  (${:.3}){}",
        format_tokens(total),
        report.cost_delta_usd,
        marker
    ))
}

/// Build the status panel string from state dict and version.
pub fn format_panel(
    state: &Value,
    version: &str,
    now: Option<DateTime<FixedOffset>>,
    dev_mode: bool,
    phase_config: Option<&PhaseConfig>,
) -> String {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names = phase_config::phase_names();
    let default_numbers = phase_config::phase_numbers();
    let default_commands = phase_config::commands();

    let order = phase_config.map(|c| &c.order).unwrap_or(&default_order);
    let names = phase_config.map(|c| &c.names).unwrap_or(&default_names);
    let numbers = phase_config.map(|c| &c.numbers).unwrap_or(&default_numbers);
    let commands = phase_config
        .map(|c| &c.commands)
        .unwrap_or(&default_commands);

    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => return String::new(),
    };

    // Check if all phases are complete
    let all_complete = order.iter().all(|key| {
        phases
            .get(key.as_str())
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            == Some("complete")
    });

    if all_complete {
        return format_all_complete(state, version, dev_mode, phase_config);
    }

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(format!("  FLOW v{} — Current Status{}", version, dev_label));
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(String::new());

    let branch = state.get("branch").and_then(|b| b.as_str()).unwrap_or("");
    lines.push(format!("  Feature : {}", derive_feature(branch)));
    lines.push(format!("  Branch  : {}", branch));
    // Subdirectory scope (only shown when non-empty). When the user
    // started the flow inside a mono-repo subdir, relative_cwd records
    // the path so the agent (and the panel reader) can see which
    // subdirectory the flow operates in.
    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !relative_cwd.is_empty() {
        lines.push(format!("  Subdir  : {}", relative_cwd));
    }
    lines.push(format!(
        "  PR      : {}",
        state
            .get("pr_url")
            .and_then(|u| u.as_str())
            .unwrap_or("N/A")
    ));

    // Elapsed time
    let started_at = state.get("started_at").and_then(|s| s.as_str());
    let elapsed = elapsed_since(started_at, now);
    lines.push(format!("  Elapsed : {}", format_time(elapsed)));

    // Notes count (omit if zero)
    let notes = state
        .get("notes")
        .and_then(|n| n.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if notes > 0 {
        lines.push(format!("  Notes   : {}", notes));
    }

    lines.push(String::new());
    lines.push("  Phases".to_string());
    lines.push("  ------".to_string());

    let mut current_phase_data: Option<&Value> = None;

    for key in order {
        let phase = phases.get(key.as_str());
        let status = phase
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let name = names
            .get(key.as_str())
            .map(|s| s.as_str())
            .unwrap_or(key.as_str());
        let num = numbers.get(key.as_str()).copied().unwrap_or(0);

        if status == "complete" {
            let seconds = phase
                .and_then(|p| p.get("cumulative_seconds"))
                .and_then(|s| s.as_i64())
                .unwrap_or(0);
            let time_str = format_time(seconds);
            let padded_name = format!("{:<width$}", name, width = NAME_WIDTH);
            lines.push(format!(
                "  [x] Phase {}:  {} ({})",
                num, padded_name, time_str
            ));
        } else if status == "in_progress" {
            let padded_name = format!("{:<width$}", name, width = NAME_WIDTH);
            lines.push(format!(
                "  [>] Phase {}:  {} <-- YOU ARE HERE",
                num, padded_name
            ));
            current_phase_data = phase;
        } else {
            lines.push(format!("  [ ] Phase {}:  {}", num, name));
        }
    }

    lines.push(String::new());

    if let Some(cpd) = current_phase_data {
        let mut seconds = cpd
            .get("cumulative_seconds")
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        let session_started = cpd.get("session_started_at").and_then(|s| s.as_str());
        if let Some(ss) = session_started {
            if !ss.is_empty() {
                seconds += elapsed_since(Some(ss), now);
            }
        }
        let visits = cpd.get("visit_count").and_then(|v| v.as_i64()).unwrap_or(0);
        lines.push(format!(
            "  Time in current phase : {}",
            format_time(seconds)
        ));
        lines.push(format!("  Times visited         : {}", visits));
        lines.push(String::new());
    }

    // Continue (in_progress) vs Next (pending)
    let current = state
        .get("current_phase")
        .and_then(|c| c.as_str())
        .unwrap_or("flow-start");
    let current_status = phases
        .get(current)
        .and_then(|p| p.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("pending");
    let default_cmd = format!("/flow:{}", current);
    if current_status == "in_progress" {
        let cmd = commands
            .get(current)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  Continue: {}", cmd));
    } else {
        let cmd = commands
            .get(current)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  Next: {}", cmd));
    }

    // Tokens line (omitted when no snapshot data exists).
    if let Some(line) = tokens_line(state) {
        lines.push(line);
    }

    lines.push(String::new());
    lines.push("────────────────────────────────────────────".to_string());

    lines.join("\n")
}

/// Build the enriched all-complete panel.
pub fn format_all_complete(
    state: &Value,
    version: &str,
    dev_mode: bool,
    phase_config: Option<&PhaseConfig>,
) -> String {
    let default_order: Vec<String> = PHASE_ORDER.iter().map(|&s| s.to_string()).collect();
    let default_names = phase_config::phase_names();
    let default_numbers = phase_config::phase_numbers();

    let order = phase_config.map(|c| &c.order).unwrap_or(&default_order);
    let names = phase_config.map(|c| &c.names).unwrap_or(&default_names);
    let numbers = phase_config.map(|c| &c.numbers).unwrap_or(&default_numbers);

    let phases = state.get("phases").and_then(|p| p.as_object());
    let phases = match phases {
        Some(p) => p,
        None => return String::new(),
    };

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    lines.push(format!(
        "  FLOW v{} — All Phases Complete!{}",
        version, dev_label
    ));
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    lines.push(String::new());

    let branch = state.get("branch").and_then(|b| b.as_str()).unwrap_or("");
    lines.push(format!("  Feature : {}", derive_feature(branch)));
    // Subdirectory scope (only shown when non-empty). Mirrors the
    // in-progress panel in format_panel: when a flow was started
    // inside a mono-repo subdirectory, the user needs to see which
    // one even after the flow is complete.
    let relative_cwd = state
        .get("relative_cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !relative_cwd.is_empty() {
        lines.push(format!("  Subdir  : {}", relative_cwd));
    }
    lines.push(format!(
        "  PR      : {}",
        state
            .get("pr_url")
            .and_then(|u| u.as_str())
            .unwrap_or("N/A")
    ));

    // Total elapsed from phase timings
    let total: i64 = order
        .iter()
        .map(|key| {
            phases
                .get(key.as_str())
                .and_then(|p| p.get("cumulative_seconds"))
                .and_then(|s| s.as_i64())
                .unwrap_or(0)
        })
        .sum();
    lines.push(format!("  Elapsed : {}", format_time(total)));

    lines.push(String::new());
    lines.push("  Phases".to_string());
    lines.push("  ------".to_string());

    for key in order {
        let phase = phases.get(key.as_str());
        let padded_name = format!(
            "{:<width$}",
            names
                .get(key.as_str())
                .map(|s| s.as_str())
                .unwrap_or(key.as_str()),
            width = NAME_WIDTH
        );
        let seconds = phase
            .and_then(|p| p.get("cumulative_seconds"))
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        let time_str = format_time(seconds);
        let num = numbers.get(key.as_str()).copied().unwrap_or(0);
        lines.push(format!(
            "  [x] Phase {}:  {} ({})",
            num, padded_name, time_str
        ));
    }

    // Tokens line (omitted when no snapshot data exists).
    if let Some(line) = tokens_line(state) {
        lines.push(String::new());
        lines.push(line);
    }

    lines.push(String::new());
    lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

    lines.join("\n")
}

/// Build a summary panel listing multiple active features.
pub fn format_multi_panel(
    results: &[(std::path::PathBuf, Value, String)],
    version: &str,
    dev_mode: bool,
) -> String {
    let names = phase_config::phase_names();
    let numbers = phase_config::phase_numbers();
    let cmds = phase_config::commands();

    let dev_label = if dev_mode { " [DEV MODE]" } else { "" };
    let mut lines = Vec::new();
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(format!(
        "  FLOW v{} — Multiple Features Active{}",
        version, dev_label
    ));
    lines.push("────────────────────────────────────────────".to_string());
    lines.push(String::new());

    for (i, (_path, state, matched_branch)) in results.iter().enumerate() {
        let phase_key = state
            .get("current_phase")
            .and_then(|c| c.as_str())
            .unwrap_or("flow-start");
        let phase_name = names
            .get(phase_key)
            .map(|s| s.as_str())
            .unwrap_or(phase_key);
        let phase_num: String = numbers
            .get(phase_key)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let phase_status = state
            .get("phases")
            .and_then(|p| p.get(phase_key))
            .and_then(|p| p.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        let default_cmd = format!("/flow:{}", phase_key);
        let cmd = cmds
            .get(phase_key)
            .map(|s| s.as_str())
            .unwrap_or(&default_cmd);
        lines.push(format!("  {}. {}", i + 1, derive_feature(matched_branch)));
        lines.push(format!("     Branch : {}", matched_branch));
        lines.push(format!(
            "     Phase  : {} — {} ({})",
            phase_num, phase_name, phase_status
        ));
        lines.push(format!("     Next   : {}", cmd));
        lines.push(String::new());
    }

    lines.push("────────────────────────────────────────────".to_string());
    lines.join("\n")
}

/// Driver for the `bin/flow format-status` subcommand.
///
/// Returns `Result<(stdout_text, code), (stderr_text, code)>`:
///
/// - `Ok((panel, 0))` — a single-flow panel or a multi-flow panel
///   was rendered and should be written to stdout with exit 0.
/// - `Ok(("", 1))` — no state files exist for any branch. The caller
///   exits 1 silently (historical contract: no stdout or stderr).
/// - `Err(("Could not determine current branch", 2))` — branch
///   resolution failed. The caller writes the message to stderr
///   and exits 2.
///
/// Tests supply `root` as a fixture TempDir and `branch_override`
/// explicitly so the helper does not shell out to `git rev-parse`
/// against the host worktree.
pub fn run_impl_main(
    branch_override: Option<&str>,
    root: &Path,
) -> Result<(String, i32), (String, i32)> {
    let branch = match resolve_branch(branch_override, root) {
        Some(b) => b,
        None => {
            return Err(("Could not determine current branch".to_string(), 2));
        }
    };

    let results = find_state_files(root, &branch);
    let results = if results.is_empty() {
        let all = find_state_files(root, "");
        if all.is_empty() {
            return Ok((String::new(), 1));
        }
        all
    } else {
        results
    };

    let version = read_version();
    let dev_mode = detect_dev_mode(root);

    if results.len() > 1 {
        return Ok((format_multi_panel(&results, &version, dev_mode), 0));
    }

    let (_state_path, state, matched_branch) = &results[0];
    let frozen_path = FlowPaths::new(root, matched_branch).frozen_phases();
    let phase_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    Ok((
        format_panel(state, &version, None, dev_mode, phase_config.as_ref()),
        0,
    ))
}
