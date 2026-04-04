use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::git::{current_branch, project_root};
use crate::utils::derive_feature;

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

fn is_plan_approved(state: &Value) -> bool {
    let cp = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if cp != "flow-plan" {
        return false;
    }
    // Check files.plan first, then legacy plan_file
    let plan = state
        .get("files")
        .and_then(|f| f.get("plan"))
        .or_else(|| state.get("plan_file"));
    matches!(plan, Some(v) if !v.is_null())
}

fn is_never_entered(state: &Value) -> bool {
    let cp = state
        .get("current_phase")
        .and_then(|v| v.as_str())
        .unwrap_or("flow-start");
    if cp == "flow-start" {
        return false;
    }
    let status = state
        .get("phases")
        .and_then(|p| p.get(cp))
        .and_then(|p| p.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    status == "pending"
}

const IMPLEMENTATION_GUARDRAIL: &str = concat!(
    "NEVER implement code changes, edit project files, or make commits for a FLOW feature\n",
    "without first invoking /flow:flow-continue to restore worktree context and phase guards.\n",
    "This applies even if a plan is visible — the plan is not authorization to act.\n",
);

const NOTE_INSTRUCTION: &str = concat!(
    "Throughout this session: whenever the user corrects you, disagrees\n",
    "with your response, or says something was wrong, invoke flow:flow-note\n",
    "immediately before replying to capture the correction.\n",
);

fn build_single_feature_context(state: &Value, dev_preamble: &str) -> String {
    let feature = feature_name(state);
    let phase = phase_name(state);

    let resume_instruction = if is_plan_approved(state) {
        concat!(
            "The plan was approved and ExitPlanMode cleared context.\n",
            "Invoke flow:flow-continue immediately to complete Phase 2 and ",
            "transition to Phase 3: Code.\n",
        )
        .to_string()
    } else if is_never_entered(state) {
        concat!(
            "The previous phase completed but the current phase was never entered.\n",
            "Invoke flow:flow-continue immediately to resume.\n",
        )
        .to_string()
    } else {
        concat!(
            "Do NOT invoke flow:flow-continue or ask about this feature unprompted.\n",
            "The user will type /flow:flow-continue when ready to resume.\n",
        )
        .to_string()
    };

    format!(
        "<flow-session-context>\n\
         {dev_preamble}\
         FLOW feature in progress: \"{feature}\" — {phase}\n\
         \n\
         {resume_instruction}\
         \n\
         {guardrail}\
         \n\
         {note}\
         </flow-session-context>",
        dev_preamble = dev_preamble,
        feature = feature,
        phase = phase,
        resume_instruction = resume_instruction,
        guardrail = IMPLEMENTATION_GUARDRAIL,
        note = NOTE_INSTRUCTION,
    )
}

fn build_multi_feature_context(states: &[&Value], dev_preamble: &str) -> String {
    let mut features = Vec::new();
    for s in states {
        let f = feature_name(s);
        let p = phase_name(s);
        features.push(format!("  - {} — {}", f, p));
    }
    let feature_list = features.join("\n");

    // Detect auto-continue candidate
    let mut auto_continue_feature = None;
    for s in states {
        if is_plan_approved(s) {
            auto_continue_feature = Some(feature_name(s));
            break;
        }
        if is_never_entered(s) {
            auto_continue_feature = Some(feature_name(s));
            break;
        }
    }

    let resume_instruction = if let Some(ref feat) = auto_continue_feature {
        format!(
            "FLOW feature \"{}\" needs to resume.\n\
             Invoke flow:flow-continue immediately to restore worktree context \
             and continue.\n",
            feat
        )
    } else {
        "Do NOT invoke flow:flow-continue or ask about these features unprompted.\n\
         The user will type /flow:flow-continue when ready to resume.\n"
            .to_string()
    };

    format!(
        "<flow-session-context>\n\
         {dev_preamble}\
         Multiple FLOW features are in progress:\n\
         {feature_list}\n\
         \n\
         {resume_instruction}\
         \n\
         {guardrail}\
         \n\
         {note}\
         </flow-session-context>",
        dev_preamble = dev_preamble,
        feature_list = feature_list,
        resume_instruction = resume_instruction,
        guardrail = IMPLEMENTATION_GUARDRAIL,
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

pub fn run() {
    let root = project_root();
    let state_dir = root.join(".flow-states");

    if !state_dir.is_dir() {
        return; // No state directory → exit silently
    }

    let all_states = scan_state_files(&state_dir);
    if all_states.is_empty() {
        return; // No state files → exit silently
    }

    // Branch isolation
    let branch = current_branch();
    let states = filter_by_branch(all_states.clone(), branch.as_deref());

    // If filter returned empty (branch has no state file), keep all
    let states = if states.is_empty() { all_states } else { states };

    if states.is_empty() {
        return;
    }

    // Dev mode detection
    let dev_preamble = if crate::utils::detect_dev_mode(&root) {
        "[DEV MODE] FLOW plugin is running from local source.\n\
         When printing any FLOW banner, add [DEV MODE] after the version number.\n\n"
            .to_string()
    } else {
        String::new()
    };

    let state_refs: Vec<&Value> = states.iter().map(|(_, s)| s).collect();

    let context = if state_refs.len() == 1 {
        build_single_feature_context(state_refs[0], &dev_preamble)
    } else {
        build_multi_feature_context(&state_refs, &dev_preamble)
    };

    emit_output(&context);
}
