use clap::{Parser, Subcommand};
use serde_json::json;
use std::process;

use flow_rs::add_finding;
use flow_rs::add_issue;
use flow_rs::add_notification;
use flow_rs::analyze_issues;
use flow_rs::append_note;
use flow_rs::auto_close_parent;
use flow_rs::base_branch_cmd;
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
use flow_rs::render_pr_body;
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
    BumpVersion {
        /// New version (semver: X.Y.Z)
        version: Option<String>,
    },

    /// Pre-merge freshness check: fetch main, verify branch is up-to-date.
    #[command(name = "check-freshness")]
    CheckFreshness {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        raw_args: Vec<String>,
    },

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

    /// Print the integration branch this flow coordinates against.
    #[command(name = "base-branch")]
    BaseBranch {
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
        Some(Commands::BumpVersion { version }) => {
            let (msg, code) =
                bump_version::run_impl_main(version.as_deref(), flow_rs::utils::plugin_root());
            flow_rs::dispatch::dispatch_text(&msg, code);
        }
        Some(Commands::CheckFreshness { raw_args }) => {
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (value, code) = check_freshness::run_impl_main(&raw_args, &cwd);
            flow_rs::dispatch::dispatch_json(value, code);
        }
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
        Some(Commands::Ci(args)) => {
            let flow_ci_running = std::env::var("FLOW_CI_RUNNING").is_ok();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let root = flow_rs::git::project_root();
            let (value, code) = ci::run_impl(&args, &cwd, &root, flow_ci_running);
            // Pretty-print: this arm is run interactively by humans far more
            // than every other arm, and its JSON carries the phase-timing
            // phases[] array which is dense to scan compact. Other arms keep
            // compact JSON so existing stdout-contains substring tests stay
            // green.
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            std::process::exit(code);
        }
        Some(Commands::UpdateDeps) => {
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let env_timeout = std::env::var("FLOW_UPDATE_DEPS_TIMEOUT").ok();
            let (value, code) = update_deps::run_impl(&cwd, env_timeout.as_deref());
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AnalyzeIssues(args)) => {
            let (value, code) = analyze_issues::run_impl_main(args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AppendNote(args)) => {
            let root = project_root();
            let (value, code) = append_note::run_impl_main(args, &root);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::Cleanup(args)) => {
            let (value, code) = cleanup::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AddFinding(args)) => {
            let root = flow_rs::git::project_root();
            let (value, code) =
                add_finding::run_impl_main_with_cwd_result(args, &root, std::env::current_dir());
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AddIssue(args)) => {
            let root = project_root();
            let (value, code) = add_issue::run_impl_main(args, &root);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AddNotification(args)) => {
            let root = project_root();
            let (value, code) = add_notification::run_impl_main(args, &root);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::Issue(args)) => {
            let root = flow_rs::git::project_root();
            let root_for_state = root.clone();
            let root_for_repo = root.clone();
            let state_reader = move || -> Option<String> {
                flow_rs::git::resolve_branch(None, &root_for_state).and_then(|branch| {
                    let state_path =
                        flow_rs::flow_paths::FlowPaths::new(&root_for_state, &branch).state_file();
                    std::fs::read_to_string(&state_path).ok()
                })
            };
            let repo_resolver =
                move || -> Option<String> { flow_rs::github::detect_repo(Some(&root_for_repo)) };
            let (value, code) = issue::run_impl_main(args, &root, &state_reader, &repo_resolver);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CloseIssue(args)) => {
            let (value, code) =
                close_issue::run_impl_main(args, &|| flow_rs::github::detect_repo(None));
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CloseIssues(args)) => {
            let (value, code) = close_issues::run_impl_main(args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CreateSubIssue(args)) => {
            let (value, code) = create_sub_issue::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::LinkBlockedBy(args)) => {
            let (value, code) = link_blocked_by::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CreateMilestone(args)) => {
            let (value, code) = create_milestone::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::ExtractReleaseNotes(args)) => {
            let (msg, code) =
                extract_release_notes::run_impl_main(&args, flow_rs::utils::plugin_root());
            flow_rs::dispatch::dispatch_text(&msg, code);
        }
        Some(Commands::PrimeCheck(_args)) => {
            // `.flow.json` lives at the project root, not at the user's
            // current directory. Resolve project_root via git so that
            // `bin/flow prime-check` works from a mono-repo subdirectory.
            let project_root = flow_rs::git::project_root();
            let (value, code) =
                prime_check::run_impl_main(&project_root, flow_rs::utils::plugin_root());
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::PrimeSetup(args)) => {
            let (value, code) = prime_setup::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::PromotePermissions(args)) => {
            let (value, code) = promote_permissions::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::AutoCloseParent(args)) => {
            let (value, code) = auto_close_parent::run_with_current_dir_from(
                args,
                std::env::current_dir,
                &auto_close_parent::run_api,
            );
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CompleteFast(args)) => {
            flow_rs::dispatch::dispatch_result_json(complete_fast::run_impl(&args));
        }
        Some(Commands::CompletePreflight(args)) => {
            let (value, code) = complete_preflight::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CompleteMerge(args)) => {
            let (value, code) = complete_merge::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::CompleteFinalize(args)) => {
            flow_rs::dispatch::dispatch_json(complete_finalize::run_impl(&args), 0);
        }
        Some(Commands::CompletePostMerge(args)) => {
            let (value, code) = complete_post_merge::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::SetTimestamp { set_args, branch }) => {
            let root = project_root();
            let cwd = std::env::current_dir().unwrap_or(std::path::PathBuf::from("."));
            let (value, code) =
                commands::set_timestamp::run_impl_main(&set_args, branch.as_deref(), &root, &cwd);
            flow_rs::dispatch::dispatch_json(value, code);
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
            let root = project_root();
            let (value, code) = commands::start_lock::run_impl_main(
                acquire, release, check, feature, wait, timeout, interval, &root,
            );
            flow_rs::dispatch::dispatch_json(value, code);
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
        Some(Commands::BaseBranch { branch }) => {
            let root = project_root();
            match base_branch_cmd::run_impl_main(branch.as_deref(), &root) {
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
            let (value, code) = label_issues::run_impl_main(args);
            flow_rs::dispatch::dispatch_json(value, code);
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
            let (value, code) = finalize_commit::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::NotifySlack(args)) => {
            // Delegate to notify_slack::notify so the subprocess test
            // surface exercises notify and its internal binders, not
            // a closure-injected seam. notify() is called cross-module
            // from phase_finalize and start_finalize.
            let value = notify_slack::notify(&args);
            flow_rs::dispatch::dispatch_json(value, 0);
        }
        Some(Commands::WriteRule(args)) => {
            let (value, code) = write_rule::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::PhaseEnter(args)) => {
            flow_rs::dispatch::dispatch_ok_result_json(phase_enter::run_impl(&args));
        }
        Some(Commands::PhaseFinalize(args)) => {
            flow_rs::dispatch::dispatch_ok_result_json(phase_finalize::run_impl_main(&args));
        }
        Some(Commands::PlanCheck(args)) => {
            flow_rs::dispatch::dispatch_ok_result_json(plan_check::run_impl(&args));
        }
        Some(Commands::PlanExtract(args)) => {
            flow_rs::dispatch::dispatch_ok_result_json(plan_extract::run_impl(&args));
        }
        Some(Commands::RenderPrBody(args)) => {
            let (value, code) = render_pr_body::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::UpdatePrBody(args)) => {
            let (value, code) = update_pr_body::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::OrchestrateReport(args)) => {
            flow_rs::dispatch::dispatch_json(orchestrate_report::run_impl(&args), 0);
        }
        Some(Commands::OrchestrateState(args)) => {
            let (value, code) = orchestrate_state::run_impl_main(&args);
            flow_rs::dispatch::dispatch_json(value, code);
        }
        Some(Commands::TombstoneAudit(args)) => {
            flow_rs::dispatch::dispatch_ok_result_json(tombstone_audit::run_impl(&args));
        }
        Some(Commands::Tui) => {
            let root = project_root();
            match flow_rs::tui_terminal::run_tui_arm_impl(&root) {
                Ok(()) => process::exit(0),
                Err((msg, code)) => {
                    eprintln!("{}", msg);
                    process::exit(code);
                }
            }
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
