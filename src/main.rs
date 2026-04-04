use clap::{Parser, Subcommand};
use serde_json::json;
use std::process;

use flow_rs::add_issue;
use flow_rs::add_notification;
use flow_rs::append_note;
use flow_rs::check_phase::check_phase;
use flow_rs::close_issue;
use flow_rs::close_issues;
use flow_rs::commands;
use flow_rs::format_status;
use flow_rs::git::{project_root, resolve_branch};
use flow_rs::issue;
use flow_rs::lock::mutate_state;
use flow_rs::output::json_error;
use flow_rs::phase_config::{find_state_files, load_phase_config, PHASE_ORDER};
use flow_rs::phase_transition::{phase_complete, phase_enter};
use flow_rs::start_setup;
use flow_rs::utils::{detect_dev_mode, read_version};

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Verify prerequisite phase is complete before entry.
    #[command(name = "check-phase")]
    CheckPhase {
        /// Phase name being entered
        #[arg(long)]
        required: String,
        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
    },

    /// Phase entry and completion state transitions.
    #[command(name = "phase-transition")]
    PhaseTransition {
        /// Phase name (e.g. flow-start, flow-plan, flow-code)
        #[arg(long)]
        phase: String,
        /// Action: enter or complete
        #[arg(long)]
        action: String,
        /// Override next phase name (default: next in order)
        #[arg(long, name = "next-phase")]
        next_phase: Option<String>,
        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
        /// Optional reason for backward transitions
        #[arg(long)]
        reason: Option<String>,
    },

    /// Append a note to FLOW state
    AppendNote(append_note::Args),
    /// Record a filed issue in FLOW state
    AddIssue(add_issue::Args),
    /// Record a Slack notification in FLOW state
    AddNotification(add_notification::Args),

    /// Create a GitHub issue via gh CLI with body-file.
    Issue(issue::Args),
    /// Close a single GitHub issue via gh CLI.
    #[command(name = "close-issue")]
    CloseIssue(close_issue::Args),
    /// Close issues referenced in the FLOW start prompt.
    #[command(name = "close-issues")]
    CloseIssues(close_issues::Args),

    /// Set timestamp and value fields in the FLOW state file.
    #[command(name = "set-timestamp")]
    SetTimestamp {
        /// path=value pairs (use NOW for current timestamp)
        #[arg(long = "set", required = true)]
        set_args: Vec<String>,

        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
    },

    /// Set _blocked flag in the state file (PermissionRequest hook).
    #[command(name = "set-blocked")]
    SetBlocked,

    /// Clear _blocked flag from the state file (PostToolUse hook).
    #[command(name = "clear-blocked")]
    ClearBlocked,

    /// Create the initial FLOW state file with null PR fields.
    #[command(name = "init-state")]
    InitState {
        /// Feature name words
        feature_name: String,
        /// Path to file containing start prompt (file is deleted after reading)
        #[arg(long = "prompt-file")]
        prompt_file: Option<String>,
        /// Override all skills to fully autonomous preset
        #[arg(long)]
        auto: bool,
        /// Initial start_step value for TUI progress
        #[arg(long = "start-step")]
        start_step: Option<i64>,
        /// Total start steps for TUI progress
        #[arg(long = "start-steps-total")]
        start_steps_total: Option<i64>,
    },

    /// Append a timestamped log entry to .flow-states/<branch>.log
    Log {
        /// Branch name (determines log file name)
        branch: String,
        /// Message to append
        message: String,
    },
    /// Generate an 8-character hex session ID
    #[command(name = "generate-id")]
    GenerateId,

    /// Serialize flow-start with a queue directory.
    #[command(name = "start-lock")]
    StartLock {
        /// Acquire the lock
        #[arg(long)]
        acquire: bool,
        /// Release the lock
        #[arg(long)]
        release: bool,
        /// Check lock status
        #[arg(long)]
        check: bool,
        /// Feature name (required for --acquire and --release)
        #[arg(long)]
        feature: Option<String>,
        /// Wait for lock to be released
        #[arg(long)]
        wait: bool,
        /// Max seconds to wait (default 90)
        #[arg(long, default_value = "90")]
        timeout: u64,
        /// Seconds between retry attempts (default 10)
        #[arg(long, default_value = "10")]
        interval: u64,
    },

    /// Update Start phase step counter, optionally wrapping a subcommand.
    #[command(name = "start-step")]
    StartStep {
        /// Step number to set
        #[arg(long)]
        step: i64,
        /// Branch name for state file lookup
        #[arg(long)]
        branch: String,
        /// Subcommand to exec after updating step (everything after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        subcommand: Vec<String>,
    },

    /// FLOW Start phase setup (worktree, PR, state file)
    #[command(name = "start-setup")]
    StartSetup(start_setup::Args),

    /// Format the FLOW status panel for display.
    #[command(name = "format-status")]
    FormatStatus {
        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
    },

    /// Build continue-context JSON for session resumption.
    #[command(name = "continue-context")]
    ContinueContext {
        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
    },

    #[command(external_subcommand)]
    #[allow(dead_code)]
    External(Vec<String>),
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            eprintln!("flow-rs: no command specified. Use --help for usage.");
            process::exit(1);
        }
        Some(Commands::CheckPhase { required, branch }) => {
            run_check_phase(&required, branch.as_deref());
        }
        Some(Commands::PhaseTransition {
            phase,
            action,
            next_phase,
            branch,
            reason,
        }) => {
            run_phase_transition(
                &phase,
                &action,
                next_phase.as_deref(),
                branch.as_deref(),
                reason.as_deref(),
            );
        }
        Some(Commands::AppendNote(args)) => append_note::run(args),
        Some(Commands::AddIssue(args)) => add_issue::run(args),
        Some(Commands::AddNotification(args)) => add_notification::run(args),
        Some(Commands::Issue(args)) => issue::run(args),
        Some(Commands::CloseIssue(args)) => close_issue::run(args),
        Some(Commands::CloseIssues(args)) => close_issues::run(args),
        Some(Commands::SetTimestamp { set_args, branch }) => {
            commands::set_timestamp::run(set_args, branch);
        }
        Some(Commands::SetBlocked) => {
            commands::set_blocked::run();
        }
        Some(Commands::ClearBlocked) => {
            commands::clear_blocked::run();
        }
        Some(Commands::InitState {
            feature_name,
            prompt_file,
            auto,
            start_step,
            start_steps_total,
        }) => {
            if feature_name.is_empty() {
                json_error(
                    "Feature name required. Usage: bin/flow init-state \"<feature name>\"",
                    &[("step", json!("args"))],
                );
                process::exit(1);
            }
            commands::init_state::run(
                &feature_name,
                prompt_file.as_deref(),
                auto,
                start_step,
                start_steps_total,
            );
        }
        Some(Commands::Log { branch, message }) => {
            commands::log::run(&branch, &message);
        }
        Some(Commands::GenerateId) => {
            commands::generate_id::run();
        }
        Some(Commands::StartLock {
            acquire,
            release,
            check,
            feature,
            wait,
            timeout,
            interval,
        }) => {
            commands::start_lock::run(acquire, release, check, feature, wait, timeout, interval);
        }
        Some(Commands::StartStep {
            step,
            branch,
            subcommand,
        }) => {
            // Strip leading "--" if present (clap trailing_var_arg includes it)
            let subcommand: Vec<String> = if subcommand.first().map(|s| s.as_str()) == Some("--") {
                subcommand.into_iter().skip(1).collect()
            } else {
                subcommand
            };
            commands::start_step::run(step, &branch, subcommand);
        }
        Some(Commands::StartSetup(args)) => {
            start_setup::run(args);
        }
        Some(Commands::FormatStatus { branch }) => {
            run_format_status(branch.as_deref());
        }
        Some(Commands::ContinueContext { branch }) => {
            commands::continue_context::run(branch.as_deref());
        }
        Some(Commands::External(_)) => {
            process::exit(127);
        }
    }
}

fn run_check_phase(phase: &str, branch_override: Option<&str>) {
    // First phase has no prerequisites
    if phase == PHASE_ORDER[0] {
        process::exit(0);
    }

    let root = project_root();
    let (branch, candidates) = resolve_branch(branch_override, &root);

    if branch.is_none() && !candidates.is_empty() {
        println!("BLOCKED: Multiple active features. Pass --branch.");
        for c in &candidates {
            println!("  - {}", c);
        }
        process::exit(1);
    }

    let branch = match branch {
        Some(b) => b,
        None => {
            println!("BLOCKED: Could not determine current git branch.");
            process::exit(1);
        }
    };

    let state_file = root.join(".flow-states").join(format!("{}.json", branch));
    if !state_file.exists() {
        println!(
            "BLOCKED: No FLOW feature in progress on branch \"{}\".",
            branch
        );
        println!("Run /flow:flow-start to begin a new feature.");
        process::exit(1);
    }

    let content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(e) => {
            println!("BLOCKED: Could not read state file: {}", e);
            process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            println!("BLOCKED: Could not read state file: {}", e);
            process::exit(1);
        }
    };

    // Load frozen phase config if available
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", branch));
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    match check_phase(&state, phase, frozen_config.as_ref()) {
        Ok((allowed, output)) => {
            if !output.is_empty() {
                println!("{}", output);
            }
            process::exit(if allowed { 0 } else { 1 });
        }
        Err(msg) => {
            let _ = json_error(&msg, &[]);
            process::exit(1);
        }
    }
}

fn run_phase_transition(
    phase: &str,
    action: &str,
    next_phase: Option<&str>,
    branch_override: Option<&str>,
    reason: Option<&str>,
) {
    if !PHASE_ORDER.contains(&phase) {
        json_error(
            &format!(
                "Invalid phase: {}. Must be one of: {}",
                phase,
                PHASE_ORDER.join(", ")
            ),
            &[],
        );
        process::exit(1);
    }

    if action != "enter" && action != "complete" {
        json_error(
            &format!("Invalid action: {}. Must be 'enter' or 'complete'", action),
            &[],
        );
        process::exit(1);
    }

    let root = project_root();
    let (branch, candidates) = resolve_branch(branch_override, &root);

    if branch.is_none() {
        if !candidates.is_empty() {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "status": "error",
                    "message": "Multiple active features. Pass --branch.",
                    "candidates": candidates,
                }))
                .unwrap()
            );
        } else {
            json_error("Could not determine current branch", &[]);
        }
        process::exit(1);
    }

    let branch = branch.unwrap();
    let state_path = root.join(".flow-states").join(format!("{}.json", branch));

    if !state_path.exists() {
        json_error(
            &format!("No state file found: {}", state_path.display()),
            &[],
        );
        process::exit(1);
    }

    // Read state to validate phase key exists
    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(e) => {
            json_error(&format!("Could not read state file: {}", e), &[]);
            process::exit(1);
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            json_error(&format!("Could not read state file: {}", e), &[]);
            process::exit(1);
        }
    };

    // Validate phase key exists in state
    if state.get("phases").is_none() || state["phases"].get(phase).is_none() {
        json_error(
            &format!("Phase {} not found in state file", phase),
            &[],
        );
        process::exit(1);
    }

    // Load frozen phase config if available
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", branch));
    let frozen_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    let frozen_order: Option<Vec<String>> = frozen_config.as_ref().map(|c| c.order.clone());
    let frozen_commands = frozen_config.as_ref().map(|c| c.commands.clone());

    // Use mutate_state for atomic read-lock-transform-write
    let result_holder = std::cell::RefCell::new(serde_json::Value::Null);

    let mutate_result = mutate_state(&state_path, |state| {
        let result = if action == "enter" {
            phase_enter(state, phase, reason)
        } else {
            phase_complete(
                state,
                phase,
                next_phase,
                frozen_order.as_deref(),
                frozen_commands.as_ref(),
            )
        };
        *result_holder.borrow_mut() = result;
    });

    match mutate_result {
        Ok(_) => {
            let result = result_holder.into_inner();
            println!("{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            json_error(&format!("State mutation failed: {}", e), &[]);
            process::exit(1);
        }
    }
}

fn run_format_status(branch_override: Option<&str>) {
    let root = project_root();
    let (branch, candidates) = resolve_branch(branch_override, &root);

    if branch.is_none() && !candidates.is_empty() {
        // Ambiguous — show all candidates via find_state_files
        let results = find_state_files(&root, "");
        if results.is_empty() {
            process::exit(1);
        }
        let version = read_version();
        let dev_mode = detect_dev_mode(&root);
        let panel = format_status::format_multi_panel(&results, &version, dev_mode);
        println!("{}", panel);
        process::exit(0);
    }

    let branch = match branch {
        Some(b) => b,
        None => {
            eprintln!("Could not determine current branch");
            process::exit(2);
        }
    };

    let results = find_state_files(&root, &branch);
    if results.is_empty() {
        process::exit(1);
    }

    let version = read_version();
    let dev_mode = detect_dev_mode(&root);

    if results.len() > 1 {
        let panel = format_status::format_multi_panel(&results, &version, dev_mode);
        println!("{}", panel);
        process::exit(0);
    }

    let (_state_path, state, matched_branch) = &results[0];

    // Load frozen phase config if available
    let frozen_path = root
        .join(".flow-states")
        .join(format!("{}-phases.json", matched_branch));
    let phase_config = if frozen_path.exists() {
        load_phase_config(&frozen_path).ok()
    } else {
        None
    };

    let panel = format_status::format_panel(
        state,
        &version,
        None,
        dev_mode,
        phase_config.as_ref(),
    );
    println!("{}", panel);
}

