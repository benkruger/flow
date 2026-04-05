use std::fs;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use chrono::Utc;
use chrono_tz::America::Los_Angeles;
use serde_json::{json, Value};

use crate::git::{current_branch, project_root};
use crate::github::detect_repo;
use crate::lock::mutate_state;
use crate::utils::{derive_feature, detect_tty, now, write_tab_sequences};

/// Detect orchestrate.json state. Returns context block string.
/// Handles completed (morning report + cleanup), all-processed (empty), and in-progress (resume).
fn detect_orchestrate(state_dir: &Path) -> String {
    let orch_path = state_dir.join("orchestrate.json");
    let content = match fs::read_to_string(&orch_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let orch: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    // Completed: inject morning report, then clean up
    if !orch.get("completed_at").map_or(true, |v| v.is_null()) {
        let summary = state_dir
            .join("orchestrate-summary.md")
            .exists()
            .then(|| fs::read_to_string(state_dir.join("orchestrate-summary.md")).unwrap_or_default())
            .unwrap_or_default();

        let block = format!(
            "<flow-orchestrate-report>\n\
             FLOW orchestration completed. Present this report to the user:\n\n\
             {}\n\
             </flow-orchestrate-report>\n",
            summary
        );

        // Clean up orchestrator files
        for name in [
            "orchestrate.json",
            "orchestrate-summary.md",
            "orchestrate.log",
            "orchestrate-queue.json",
        ] {
            let p = state_dir.join(name);
            let _ = fs::remove_file(&p);
        }

        return block;
    }

    // All items processed — orchestrator finishing, no resume needed
    let queue = orch.get("queue").and_then(|q| q.as_array());
    if let Some(items) = queue {
        if !items.is_empty()
            && items
                .iter()
                .all(|item| !item.get("outcome").map_or(true, |v| v.is_null()))
        {
            return String::new();
        }
    }

    // In-progress: inject resume context with queue position
    let queue_items = queue.cloned().unwrap_or_default();
    let current_index = orch.get("current_index").and_then(|v| v.as_i64());
    let current_issue = if let Some(idx) = current_index {
        let idx = idx as usize;
        if idx < queue_items.len() {
            let item = &queue_items[idx];
            let num = item
                .get("issue_number")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string());
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("#{} ({})", num, title)
        } else {
            "(unknown)".to_string()
        }
    } else {
        "(unknown)".to_string()
    };

    let completed_count = queue_items
        .iter()
        .filter(|item| {
            item.get("outcome")
                .and_then(|v| v.as_str())
                .map_or(false, |s| s == "completed")
        })
        .count();
    let total = queue_items.len();

    format!(
        "<flow-orchestrate-context>\n\
         FLOW orchestration in progress. Processing issue {}.\n\
         Progress: {}/{} completed.\n\
         Resume the orchestrator by invoking flow:flow-orchestrate --continue-step.\n\
         </flow-orchestrate-context>\n",
        current_issue, completed_count, total
    )
}

/// Reset interrupted session timing. Accumulates elapsed seconds, clears _blocked.
/// Operates on a mutable Value reference (for use inside mutate_state closure).
fn reset_interrupted(state: &mut Value) {
    // Clear _blocked
    if let Some(obj) = state.as_object_mut() {
        obj.remove("_blocked");
    }

    let cp = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("flow-start")
        .to_string();

    let session_started = state
        .get("phases")
        .and_then(|p| p.get(&cp))
        .and_then(|p| p.get("session_started_at"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(started_str) = session_started {
        // Parse the timestamp and compute elapsed
        match DateTime::parse_from_rfc3339(&started_str)
            .or_else(|_| DateTime::parse_from_str(&started_str, "%Y-%m-%dT%H:%M:%S%:z"))
        {
            Ok(started_dt) => {
                let now_dt = Utc::now().with_timezone(&Los_Angeles);
                let elapsed = (now_dt - started_dt.with_timezone(&Los_Angeles))
                    .num_seconds()
                    .max(0);
                let existing = state
                    .get("phases")
                    .and_then(|p| p.get(&cp))
                    .and_then(|p| p.get("cumulative_seconds"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0); // null → 0
                state["phases"][&cp]["cumulative_seconds"] = json!(existing + elapsed);
                state["phases"][&cp]["session_started_at"] = json!(now());
            }
            Err(_) => {
                state["phases"][&cp]["session_started_at"] = Value::Null;
            }
        }
    }
}

/// Extract and clear _last_failure from state. Returns the failure value if present.
fn consume_last_failure(state: &mut Value) -> Option<Value> {
    state
        .as_object_mut()
        .and_then(|obj| obj.remove("_last_failure"))
        .filter(|v| !v.is_null())
}

/// Extract and clear compact_summary and compact_cwd from state.
fn consume_compact_data(state: &mut Value) -> (Option<String>, Option<String>) {
    let summary = state
        .as_object_mut()
        .and_then(|obj| obj.remove("compact_summary"))
        .and_then(|v| v.as_str().map(|s| s.to_string()));
    let cwd = state
        .as_object_mut()
        .and_then(|obj| obj.remove("compact_cwd"))
        .and_then(|v| v.as_str().map(|s| s.to_string()));
    (summary, cwd)
}

/// Update session_tty for the matching branch state file.
fn update_tty(state: &mut Value) {
    if let Some(tty) = detect_tty() {
        let current = state
            .get("session_tty")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if current != tty {
            state["session_tty"] = json!(tty);
        }
    }
}

/// Build failure context block from StopFailure data.
fn build_failure_block(failure: &Option<Value>) -> String {
    let failure = match failure {
        Some(f) => f,
        None => return String::new(),
    };
    let f_type = failure
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let f_ts = failure
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let f_msg = failure
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if f_msg.is_empty() {
        format!("Previous session ended due to {} at {}\n\n", f_type, f_ts)
    } else {
        format!(
            "Previous session ended due to {} at {}: {}\n\n",
            f_type, f_ts, f_msg
        )
    }
}

/// Build compact context block from PostCompact data.
fn build_compact_block(summary: &Option<String>, cwd: &Option<String>, worktree: &str) -> String {
    let mut block = String::new();
    if let Some(s) = summary {
        block.push_str(&format!(
            "<compact-summary>\n\
             The conversation was just compacted. \
             Here is what was happening before compaction:\n\
             {}\n\
             </compact-summary>\n\n",
            s
        ));
    }
    if let Some(c) = cwd {
        if c != worktree {
            block.push_str(&format!(
                "WARNING: CWD at compaction was {} but the active \
                 worktree is {}. cd into the worktree before editing.\n\n",
                c, worktree
            ));
        }
    }
    block
}

/// Transient data consumed from a state file during processing.
struct ConsumedData {
    failure: Option<Value>,
    compact_summary: Option<String>,
    compact_cwd: Option<String>,
}

/// Process a single state file: reset timing, update TTY, consume transient data.
/// All mutations happen in a single atomic mutate_state call.
/// Returns the mutated state and consumed transient data.
fn process_state_file(
    path: &Path,
    is_matching_branch: bool,
) -> Option<(Value, ConsumedData)> {
    use std::cell::RefCell;

    let consumed = RefCell::new(ConsumedData {
        failure: None,
        compact_summary: None,
        compact_cwd: None,
    });

    let result = mutate_state(path, |state| {
        // 1. Reset interrupted timing
        reset_interrupted(state);

        // 2. TTY detection only for matching branch
        if is_matching_branch {
            update_tty(state);
        }

        // 3. Consume transient fields (removed from state, captured for context)
        let mut c = consumed.borrow_mut();
        c.failure = consume_last_failure(state);
        let (summary, cwd) = consume_compact_data(state);
        c.compact_summary = summary;
        c.compact_cwd = cwd;
    });

    match result {
        Ok(state) => Some((state, consumed.into_inner())),
        Err(_) => None,
    }
}

/// Scan .flow-states/ for JSON state files, excluding *-phases.json and orchestrate.json.
fn scan_state_files(state_dir: &Path) -> Vec<(PathBuf, Value)> {
    let entries = match fs::read_dir(state_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only .json files
        if !name.ends_with(".json") {
            continue;
        }
        // Exclude *-phases.json copies
        if name.ends_with("-phases.json") {
            continue;
        }
        // Exclude orchestrate.json (handled separately)
        if name == "orchestrate.json" {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let state: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue, // Skip corrupt files
        };

        results.push((path, state));
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

/// Filter state files by current branch. Fail-open: if branch detection fails, return all.
fn filter_by_branch(states: Vec<(PathBuf, Value)>, branch: Option<&str>) -> Vec<(PathBuf, Value)> {
    let branch = match branch {
        Some(b) if !b.is_empty() => b,
        _ => return states, // Fail-open: detached HEAD or error
    };

    let filtered: Vec<(PathBuf, Value)> = states
        .into_iter()
        .filter(|(path, _)| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s == branch)
                .unwrap_or(false)
        })
        .collect();

    if filtered.is_empty() {
        // On main (or branch without state file) — keep all files
        // Re-scan since we consumed the iterator
        return Vec::new(); // Caller should handle this
    }

    filtered
}

const STEP_NAMES: &[&str] = &["Simplify", "Review", "Security", "Code Review Plugin"];

fn step_suffix(state: &Value) -> String {
    let cp = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("flow-start");
    if cp != "flow-code-review" {
        return String::new();
    }
    let step = match state.get("code_review_step") {
        Some(v) => v,
        None => return String::new(),
    };
    let step_int = if let Some(n) = step.as_i64() {
        n as usize
    } else if let Some(s) = step.as_str() {
        match s.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return String::new(),
        }
    } else {
        return String::new();
    };
    if step_int > 0 && step_int < 4 {
        format!(
            " (Step {}/4 done — resume at Step {}: {})",
            step_int,
            step_int + 1,
            STEP_NAMES[step_int]
        )
    } else {
        String::new()
    }
}

fn feature_name(state: &Value) -> String {
    let branch = state
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    derive_feature(branch)
}

fn worktree_path(state: &Value) -> String {
    let branch = state
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    format!(".worktrees/{}", branch)
}

fn phase_name(state: &Value) -> String {
    let cp = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("flow-start");
    let name = state
        .get("phases")
        .and_then(|p| p.get(cp))
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    format!("{}{}", name, step_suffix(state))
}

const NOTE_INSTRUCTION: &str = concat!(
    "Throughout this session: whenever the user corrects you, disagrees\n",
    "with your response, or says something was wrong, invoke flow:flow-note\n",
    "immediately before replying to capture the correction.\n",
);

const SINGLE_FEATURE_RESUME_INSTRUCTION: &str = concat!(
    "Do NOT invoke any FLOW skill or ask about this feature unprompted.\n",
    "The user will type the phase command when ready to resume.\n",
);

const MULTI_FEATURE_RESUME_INSTRUCTION: &str = concat!(
    "Do NOT invoke any FLOW skill or ask about these features unprompted.\n",
    "The user will type the phase command when ready to resume.\n",
);

fn build_single_feature_context(
    state: &Value,
    consumed: &ConsumedData,
    dev_preamble: &str,
) -> String {
    let feature = feature_name(state);
    let phase = phase_name(state);
    let wt = worktree_path(state);

    let failure_block = build_failure_block(&consumed.failure);
    let compact_block =
        build_compact_block(&consumed.compact_summary, &consumed.compact_cwd, &wt);

    format!(
        "<flow-session-context>\n\
         {dev_preamble}\
         FLOW feature in progress: \"{feature}\" — {phase}\n\
         \n\
         {failure_block}\
         {compact_block}\
         {resume_instruction}\
         \n\
         {note}\
         </flow-session-context>",
        dev_preamble = dev_preamble,
        feature = feature,
        phase = phase,
        failure_block = failure_block,
        compact_block = compact_block,
        resume_instruction = SINGLE_FEATURE_RESUME_INSTRUCTION,
        note = NOTE_INSTRUCTION,
    )
}

fn build_multi_feature_context(
    states: &[&Value],
    consumed_list: &[&ConsumedData],
    dev_preamble: &str,
) -> String {
    let mut features = Vec::new();
    for s in states {
        let f = feature_name(s);
        let p = phase_name(s);
        features.push(format!("  - {} — {}", f, p));
    }
    let feature_list = features.join("\n");

    // Per-feature failure and compact blocks
    let mut failure_blocks = String::new();
    let mut compact_blocks = String::new();
    for (i, s) in states.iter().enumerate() {
        if let Some(c) = consumed_list.get(i) {
            let f_block = build_failure_block(&c.failure);
            if !f_block.is_empty() {
                failure_blocks.push_str(&format!("[{}] {}", feature_name(s), f_block));
            }
            let wt = worktree_path(s);
            let c_block = build_compact_block(&c.compact_summary, &c.compact_cwd, &wt);
            if !c_block.is_empty() {
                compact_blocks.push_str(&format!("[{}] {}", feature_name(s), c_block));
            }
        }
    }

    format!(
        "<flow-session-context>\n\
         {dev_preamble}\
         Multiple FLOW features are in progress:\n\
         {feature_list}\n\
         \n\
         {failure_blocks}\
         {compact_blocks}\
         {resume_instruction}\
         \n\
         {note}\
         </flow-session-context>",
        dev_preamble = dev_preamble,
        feature_list = feature_list,
        failure_blocks = failure_blocks,
        compact_blocks = compact_blocks,
        resume_instruction = MULTI_FEATURE_RESUME_INSTRUCTION,
        note = NOTE_INSTRUCTION,
    )
}

fn emit_output(context: &str) {
    let output = json!({
        "additional_context": context,
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": context,
        },
    });
    println!("{}", serde_json::to_string(&output).unwrap());
}

/// Write tab colors best-effort — errors are silently ignored.
fn write_tab_colors(repo: Option<&str>, root: &Path) {
    let _ = write_tab_sequences(repo, Some(root));
}

pub fn run() {
    let root = project_root();
    let state_dir = root.join(".flow-states");

    if !state_dir.is_dir() {
        return; // No state directory → exit silently
    }

    // Orchestrate detection (before feature state processing)
    let orchestrate_block = detect_orchestrate(&state_dir);

    // Scan for state file paths (lightweight — just reads and parses JSON)
    let all_states = scan_state_files(&state_dir);

    // Branch isolation
    let branch = current_branch();
    let filtered = filter_by_branch(all_states.clone(), branch.as_deref());
    let file_list = if filtered.is_empty() { all_states } else { filtered };

    if file_list.is_empty() && orchestrate_block.is_empty() {
        // Early exit — write tab colors from detected repo before returning
        let detected = detect_repo(Some(&root));
        write_tab_colors(detected.as_deref(), &root);
        return;
    }

    // Process each state file: reset timing, update TTY, consume transients
    let mut processed: Vec<(Value, ConsumedData)> = Vec::new();
    for (path, _) in &file_list {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let is_matching = branch.as_deref().map_or(false, |b| b == stem);
        if let Some(result) = process_state_file(path, is_matching) {
            processed.push(result);
        }
    }

    if processed.is_empty() && orchestrate_block.is_empty() {
        // All states failed to parse — fall back to detect_repo
        let detected = detect_repo(Some(&root));
        write_tab_colors(detected.as_deref(), &root);
        return;
    }

    // Write tab colors: use first state's repo when states exist, else detect_repo
    let first_repo = processed
        .first()
        .and_then(|(s, _)| s.get("repo"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if first_repo.is_some() {
        write_tab_colors(first_repo.as_deref(), &root);
    } else {
        let detected = detect_repo(Some(&root));
        write_tab_colors(detected.as_deref(), &root);
    }

    // Dev mode detection
    let dev_preamble = if crate::utils::detect_dev_mode(&root) {
        "[DEV MODE] FLOW plugin is running from local source.\n\
         When printing any FLOW banner, add [DEV MODE] after the version number.\n\n"
            .to_string()
    } else {
        String::new()
    };

    // Build feature context
    let state_refs: Vec<&Value> = processed.iter().map(|(s, _)| s).collect();
    let consumed_refs: Vec<&ConsumedData> = processed.iter().map(|(_, c)| c).collect();

    let feature_context = if state_refs.is_empty() {
        String::new()
    } else if state_refs.len() == 1 {
        build_single_feature_context(state_refs[0], &consumed_refs[0], &dev_preamble)
    } else {
        build_multi_feature_context(&state_refs, &consumed_refs, &dev_preamble)
    };

    // Combine orchestrate and feature context (matching Python logic)
    let context = if !orchestrate_block.is_empty() && feature_context.is_empty() {
        orchestrate_block
    } else if !orchestrate_block.is_empty() {
        format!("{}\n{}", orchestrate_block, feature_context)
    } else {
        feature_context
    };

    emit_output(&context);
}
