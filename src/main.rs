use clap::{Parser, Subcommand};
use serde_json::json;
use std::process;

use flow_rs::add_finding;
use flow_rs::add_issue;
use flow_rs::add_notification;
use flow_rs::analyze_issues;
use flow_rs::append_note;
use flow_rs::auto_close_parent;
use flow_rs::bump_version;
use flow_rs::check_freshness;
use flow_rs::check_phase;
use flow_rs::ci;
use flow_rs::cleanup;
use flow_rs::close_issue;
use flow_rs::close_issues;
use flow_rs::commands;
use flow_rs::complete_fast;
use flow_rs::complete_finalize;
use flow_rs::complete_merge;
use flow_rs::complete_post_merge;
use flow_rs::complete_preflight;
use flow_rs::create_milestone;
use flow_rs::create_sub_issue;
use flow_rs::extract_release_notes;
use flow_rs::finalize_commit;
use flow_rs::format_complete_summary;
use flow_rs::format_issues_summary;
use flow_rs::format_pr_timings;
use flow_rs::format_status;
use flow_rs::git::project_root;
use flow_rs::hooks;
use flow_rs::issue;
use flow_rs::label_issues;
use flow_rs::link_blocked_by;
use flow_rs::notify_slack;
use flow_rs::orchestrate_report;
use flow_rs::orchestrate_state;
use flow_rs::output::json_error;
use flow_rs::phase_enter;
use flow_rs::phase_finalize;
use flow_rs::phase_transition;
use flow_rs::plan_check;
use flow_rs::plan_extract;
use flow_rs::prime_check;
use flow_rs::prime_setup;
use flow_rs::promote_permissions;
use flow_rs::qa_mode;
use flow_rs::qa_reset;
use flow_rs::qa_verify;
use flow_rs::render_pr_body;
use flow_rs::scaffold_qa;
use flow_rs::start_finalize;
use flow_rs::start_gate;
use flow_rs::start_init;
use flow_rs::start_workspace;
use flow_rs::tombstone_audit;
use flow_rs::tui_data;
use flow_rs::update_deps;
use flow_rs::update_pr_body;
use flow_rs::upgrade_check;
use flow_rs::write_rule;

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Bump the FLOW plugin version across all files.
    #[command(name = "bump-version")]
    BumpVersion(bump_version::Args),

    /// Pre-merge freshness check: fetch main, verify branch is up-to-date.
    #[command(name = "check-freshness")]
    CheckFreshness(check_freshness::Args),

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

    /// Run bin/ci with dirty-check optimization, retry logic, and CI sentinel management.
    /// Use --format/--lint/--build/--test to run a single phase, or --force to bypass the sentinel skip.
    Ci(ci::Args),

    /// Run bin/dependencies with a configurable timeout and report git status changes.
    #[command(name = "update-deps")]
    UpdateDeps,

    /// Analyze open GitHub issues for the flow-issues skill.
    #[command(name = "analyze-issues")]
    AnalyzeIssues(analyze_issues::Args),

    /// Append a note to FLOW state
    AppendNote(append_note::Args),
    /// Record a triage finding in FLOW state
    AddFinding(add_finding::Args),
    /// Record a filed issue in FLOW state
    AddIssue(add_issue::Args),
    /// Record a Slack notification in FLOW state
    AddNotification(add_notification::Args),

    /// FLOW cleanup orchestrator (worktree, branches, state files).
    Cleanup(cleanup::Args),

    /// Create a GitHub issue via gh CLI with body-file.
    Issue(issue::Args),
    /// Close a single GitHub issue via gh CLI.
    #[command(name = "close-issue")]
    CloseIssue(close_issue::Args),
    /// Close issues referenced in the FLOW start prompt.
    #[command(name = "close-issues")]
    CloseIssues(close_issues::Args),

    /// Create a GitHub sub-issue relationship.
    #[command(name = "create-sub-issue")]
    CreateSubIssue(create_sub_issue::Args),
    /// Create a GitHub blocked-by dependency.
    #[command(name = "link-blocked-by")]
    LinkBlockedBy(link_blocked_by::Args),
    /// Create a GitHub milestone.
    #[command(name = "create-milestone")]
    CreateMilestone(create_milestone::Args),

    /// Extract release notes for a specific version from RELEASE-NOTES.md.
    #[command(name = "extract-release-notes")]
    ExtractReleaseNotes(extract_release_notes::Args),

    /// Verify /flow:flow-prime has been run with a matching version.
    #[command(name = "prime-check")]
    PrimeCheck(prime_check::Args),

    /// Consolidated prime setup: permissions, version marker, hooks, launcher.
    #[command(name = "prime-setup")]
    PrimeSetup(prime_setup::Args),

    /// Promote permissions from settings.local.json into settings.json.
    #[command(name = "promote-permissions")]
    PromotePermissions(promote_permissions::Args),

    /// Auto-close parent issue and milestone when all children are done.
    #[command(name = "auto-close-parent")]
    AutoCloseParent(auto_close_parent::Args),

    /// FLOW Complete phase fast path (gate + preflight + CI + merge in one call).
    #[command(name = "complete-fast")]
    CompleteFast(complete_fast::Args),

    /// FLOW Complete phase preflight (state detection, PR check, merge main).
    #[command(name = "complete-preflight")]
    CompletePreflight(complete_preflight::Args),

    /// FLOW Complete phase merge (freshness check + squash merge).
    #[command(name = "complete-merge")]
    CompleteMerge(complete_merge::Args),

    /// FLOW Complete phase finalize (post-merge + cleanup in one call).
    #[command(name = "complete-finalize")]
    CompleteFinalize(complete_finalize::Args),

    /// FLOW Complete phase post-merge operations.
    #[command(name = "complete-post-merge")]
    CompletePostMerge(complete_post_merge::Args),

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
        /// Canonical branch name (from start-init). Skips branch derivation.
        #[arg(long)]
        branch: Option<String>,
        /// Relative path inside the project root captured at flow-start
        /// time. Empty string means worktree root. Persisted to the state
        /// file so subsequent commands can route the agent back to the
        /// same subdirectory after the worktree is created.
        #[arg(long = "relative-cwd", default_value = "")]
        relative_cwd: String,
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

    /// Complete Start phase and send notifications
    #[command(name = "start-finalize")]
    StartFinalize(start_finalize::Args),

    /// Consolidated CI and dependency gate for start phase
    #[command(name = "start-gate")]
    StartGate(start_gate::Args),

    /// Consolidated start initialization (lock + prime + upgrade + init-state + labels)
    #[command(name = "start-init")]
    StartInit(start_init::Args),

    /// Create worktree, PR, backfill state, release lock
    #[command(name = "start-workspace")]
    StartWorkspace(start_workspace::Args),

    /// Format the FLOW status panel for display.
    #[command(name = "format-status")]
    FormatStatus {
        /// Override branch for state file lookup
        #[arg(long)]
        branch: Option<String>,
    },

    /// Build SessionStart hook context from state files.
    #[command(name = "session-context")]
    SessionContext,

    /// Add or remove Flow In-Progress label on issues
    LabelIssues(label_issues::Args),
    /// Format issues summary for Complete phase
    FormatIssuesSummary(format_issues_summary::Args),
    /// Format the Complete phase Done banner
    #[command(name = "format-complete-summary")]
    FormatCompleteSummary(format_complete_summary::Args),
    /// Format phase timings as a markdown table for PR body
    #[command(name = "format-pr-timings")]
    FormatPrTimings(format_pr_timings::Args),

    /// Finalize a commit: commit from message file, cleanup, pull, push.
    #[command(name = "finalize-commit")]
    FinalizeCommit(finalize_commit::Args),
    /// Post a message to Slack via webhook.
    #[command(name = "notify-slack")]
    NotifySlack(notify_slack::Args),
    /// Write content to a target file path.
    #[command(name = "write-rule")]
    WriteRule(write_rule::Args),

    /// Generic phase entry: gate + enter + step counters + return state data.
    #[command(name = "phase-enter")]
    PhaseEnter(phase_enter::Args),

    /// Generic phase exit: complete + Slack + notification.
    #[command(name = "phase-finalize")]
    PhaseFinalize(phase_finalize::Args),

    /// Scan the current plan file for unenumerated universal-coverage prose.
    #[command(name = "plan-check")]
    PlanCheck(plan_check::Args),

    /// Extract pre-decomposed plan or prepare state for model-driven planning.
    #[command(name = "plan-extract")]
    PlanExtract(plan_extract::Args),

    /// Render complete PR body from state
    #[command(name = "render-pr-body")]
    RenderPrBody(render_pr_body::Args),

    /// Update PR body with artifacts
    #[command(name = "update-pr-body")]
    UpdatePrBody(update_pr_body::Args),

    /// Generate orchestration morning report
    #[command(name = "orchestrate-report")]
    OrchestrateReport(orchestrate_report::Args),

    /// Manage orchestration queue state
    #[command(name = "orchestrate-state")]
    OrchestrateState(orchestrate_state::Args),

    /// Audit tombstone tests for staleness by checking PR merge dates.
    #[command(name = "tombstone-audit")]
    TombstoneAudit(tombstone_audit::Args),

    /// Interactive TUI for viewing and managing active FLOW features.
    #[command(name = "tui")]
    Tui,

    /// TUI data layer: load flows, orchestration, account metrics as JSON.
    #[command(name = "tui-data")]
    TuiData {
        /// Load all flow summaries from .flow-states/*.json
        #[arg(long)]
        load_all_flows: bool,
        /// Load orchestration state from .flow-states/orchestrate.json
        #[arg(long)]
        load_orchestration: bool,
        /// Load account metrics (monthly cost, rate limits)
        #[arg(long)]
        load_account_metrics: bool,
    },

    /// Check GitHub for newer FLOW releases.
    #[command(name = "upgrade-check")]
    UpgradeCheck(upgrade_check::Args),

    /// Manage dev-mode plugin_root redirection in .flow.json.
    #[command(name = "qa-mode")]
    QaMode(qa_mode::Args),

    /// Reset a QA repo to seed state.
    #[command(name = "qa-reset")]
    QaReset(qa_reset::Args),

    /// Verify QA assertions after a completed flow.
    #[command(name = "qa-verify")]
    QaVerify(qa_verify::Args),

    /// Create a QA repo from a named template directory under qa/templates/.
    #[command(name = "scaffold-qa")]
    ScaffoldQa(scaffold_qa::Args),

    /// Run a Claude Code hook handler.
    Hook {
        #[command(subcommand)]
        hook: HookCommands,
    },

    #[command(external_subcommand)]
    #[allow(dead_code)]
    External(Vec<String>),
}

#[derive(Subcommand)]
enum HookCommands {
    /// Validate Bash/Agent command input against blocklist and allowlist.
    #[command(name = "validate-pretool")]
    ValidatePretool,
    /// Block Edit/Write on .claude/rules, .claude/skills, CLAUDE.md during FLOW phases.
    #[command(name = "validate-claude-paths")]
    ValidateClaudePaths,
    /// Block file tool calls targeting the main repo from inside a worktree.
    #[command(name = "validate-worktree-paths")]
    ValidateWorktreePaths,
    /// Enforce auto-continue for AskUserQuestion prompts.
    #[command(name = "validate-ask-user")]
    ValidateAskUser,
    /// Stop hook: continuation gating, blocked-flag management, tab color.
    #[command(name = "stop-continue")]
    StopContinue,
    /// StopFailure hook: capture API error context into state file.
    #[command(name = "stop-failure")]
    StopFailure,
    /// PostCompact hook: capture compaction summary into state file.
    #[command(name = "post-compact")]
    PostCompact,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            eprintln!("flow-rs: no command specified. Use --help for usage.");
            process::exit(1);
        }
        Some(Commands::BumpVersion(args)) => bump_version::run(args),
        Some(Commands::CheckFreshness(args)) => check_freshness::run(args),
        Some(Commands::CheckPhase { required, branch }) => {
            let root = project_root();
            let (out, code) = check_phase::run_impl_main(&required, branch.as_deref(), &root);
            flow_rs::dispatch::dispatch_text(&out, code);
        }
        Some(Commands::PhaseTransition {
            phase,
            action,
            next_phase,
            branch,
            reason,
        }) => {
            let root = project_root();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (out, code) = phase_transition::run_impl_main(
                &phase,
                &action,
                next_phase.as_deref(),
                branch.as_deref(),
                reason.as_deref(),
                &root,
                &cwd,
            );
            flow_rs::dispatch::dispatch_json(out, code);
        }
        Some(Commands::Ci(args)) => ci::run(args),
        Some(Commands::UpdateDeps) => update_deps::run(),
        Some(Commands::AnalyzeIssues(args)) => analyze_issues::run(args),
        Some(Commands::AppendNote(args)) => append_note::run(args),
        Some(Commands::Cleanup(args)) => cleanup::run(args),
        Some(Commands::AddFinding(args)) => add_finding::run(args),
        Some(Commands::AddIssue(args)) => add_issue::run(args),
        Some(Commands::AddNotification(args)) => add_notification::run(args),
        Some(Commands::Issue(args)) => issue::run(args),
        Some(Commands::CloseIssue(args)) => close_issue::run(args),
        Some(Commands::CloseIssues(args)) => close_issues::run(args),
        Some(Commands::CreateSubIssue(args)) => create_sub_issue::run(args),
        Some(Commands::LinkBlockedBy(args)) => link_blocked_by::run(args),
        Some(Commands::CreateMilestone(args)) => create_milestone::run(args),
        Some(Commands::ExtractReleaseNotes(args)) => extract_release_notes::run(args),
        Some(Commands::PrimeCheck(args)) => prime_check::run(args),
        Some(Commands::PrimeSetup(args)) => prime_setup::run(args),
        Some(Commands::PromotePermissions(args)) => promote_permissions::run(args),
        Some(Commands::AutoCloseParent(args)) => auto_close_parent::run(args),
        Some(Commands::CompleteFast(args)) => complete_fast::run(args),
        Some(Commands::CompletePreflight(args)) => complete_preflight::run(args),
        Some(Commands::CompleteMerge(args)) => complete_merge::run(args),
        Some(Commands::CompleteFinalize(args)) => complete_finalize::run(args),
        Some(Commands::CompletePostMerge(args)) => complete_post_merge::run(args),
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
            branch,
            relative_cwd,
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
                branch.as_deref(),
                &relative_cwd,
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
            commands::start_step::run(step, &branch, subcommand);
        }
        Some(Commands::StartFinalize(args)) => {
            let root = project_root();
            let (v, code) = start_finalize::run_impl_main(&args, &root);
            flow_rs::dispatch::dispatch_json(v, code);
        }
        Some(Commands::StartGate(args)) => {
            let root = project_root();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (v, code) = start_gate::run_impl_main(&args, &root, &cwd);
            flow_rs::dispatch::dispatch_json(v, code);
        }
        Some(Commands::StartInit(args)) => {
            let root = project_root();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (v, code) = start_init::run_impl_main(&args, &root, &cwd);
            flow_rs::dispatch::dispatch_json(v, code);
        }
        Some(Commands::StartWorkspace(args)) => {
            let root = project_root();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (v, code) = start_workspace::run_impl_main(&args, &root, &cwd);
            flow_rs::dispatch::dispatch_json(v, code);
        }
        Some(Commands::FormatStatus { branch }) => {
            let root = project_root();
            match format_status::run_impl_main(branch.as_deref(), &root) {
                Ok((text, code)) => flow_rs::dispatch::dispatch_text(&text, code),
                Err((msg, code)) => {
                    eprintln!("{}", msg);
                    process::exit(code);
                }
            }
        }
        Some(Commands::SessionContext) => {
            commands::session_context::run();
        }
        Some(Commands::LabelIssues(args)) => {
            label_issues::run(args);
        }
        Some(Commands::FormatIssuesSummary(args)) => {
            let (value, code) = format_issues_summary::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::FormatCompleteSummary(args)) => {
            let (value, code) = format_complete_summary::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::FormatPrTimings(args)) => {
            let (value, code) = format_pr_timings::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::FinalizeCommit(args)) => {
            finalize_commit::run(args);
        }
        Some(Commands::NotifySlack(args)) => {
            notify_slack::run(args);
        }
        Some(Commands::WriteRule(args)) => {
            write_rule::run(args);
        }
        Some(Commands::PhaseEnter(args)) => {
            phase_enter::run(args);
        }
        Some(Commands::PhaseFinalize(args)) => {
            phase_finalize::run(args);
        }
        Some(Commands::PlanCheck(args)) => {
            plan_check::run(args);
        }
        Some(Commands::PlanExtract(args)) => {
            plan_extract::run(args);
        }
        Some(Commands::RenderPrBody(args)) => {
            render_pr_body::run(args);
        }
        Some(Commands::UpdatePrBody(args)) => {
            update_pr_body::run(args);
        }
        Some(Commands::OrchestrateReport(args)) => orchestrate_report::run(args),
        Some(Commands::OrchestrateState(args)) => orchestrate_state::run(args),
        Some(Commands::TombstoneAudit(args)) => tombstone_audit::run(args),
        Some(Commands::Tui) => {
            let root = project_root();
            flow_rs::tui_terminal::run_tui_arm(&root);
        }
        Some(Commands::TuiData {
            load_all_flows,
            load_orchestration,
            load_account_metrics,
        }) => {
            let root = project_root();
            match tui_data::run_impl_main(
                load_all_flows,
                load_orchestration,
                load_account_metrics,
                &root,
            ) {
                Ok((value, code)) => flow_rs::dispatch::dispatch_json(value, code),
                Err((msg, code)) => {
                    eprintln!("{}", msg);
                    process::exit(code);
                }
            }
        }
        Some(Commands::UpgradeCheck(args)) => upgrade_check::run(args),
        Some(Commands::QaMode(args)) => qa_mode::run(args),
        Some(Commands::QaReset(args)) => qa_reset::run(args),
        Some(Commands::QaVerify(args)) => qa_verify::run(args),
        Some(Commands::ScaffoldQa(args)) => scaffold_qa::run(args),
        Some(Commands::Hook { hook }) => match hook {
            HookCommands::ValidatePretool => hooks::validate_pretool::run(),
            HookCommands::ValidateClaudePaths => hooks::validate_claude_paths::run(),
            HookCommands::ValidateWorktreePaths => hooks::validate_worktree_paths::run(),
            HookCommands::ValidateAskUser => hooks::validate_ask_user::run(),
            HookCommands::StopContinue => hooks::stop_continue::run(),
            HookCommands::StopFailure => hooks::stop_failure::run(),
            HookCommands::PostCompact => hooks::post_compact::run(),
        },
        Some(Commands::External(_)) => {
            process::exit(127);
        }
    }
}
