use clap::{Parser, Subcommand};
use std::process;

use flow_rs::add_issue;
use flow_rs::add_notification;
use flow_rs::append_note;
use flow_rs::commands;

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Append a note to FLOW state
    AppendNote(append_note::Args),
    /// Record a filed issue in FLOW state
    AddIssue(add_issue::Args),
    /// Record a Slack notification in FLOW state
    AddNotification(add_notification::Args),

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
        Some(Commands::AppendNote(args)) => append_note::run(args),
        Some(Commands::AddIssue(args)) => add_issue::run(args),
        Some(Commands::AddNotification(args)) => add_notification::run(args),
        Some(Commands::SetTimestamp { set_args, branch }) => {
            commands::set_timestamp::run(set_args, branch);
        }
        Some(Commands::SetBlocked) => {
            commands::set_blocked::run();
        }
        Some(Commands::ClearBlocked) => {
            commands::clear_blocked::run();
        }
        Some(Commands::Log { branch, message }) => {
            commands::log::run(&branch, &message);
        }
        Some(Commands::GenerateId) => {
            commands::generate_id::run();
        }
        Some(Commands::External(_)) => {
            // Unknown subcommand — exit 127 for hybrid dispatcher fallback
            process::exit(127);
        }
    }
}
