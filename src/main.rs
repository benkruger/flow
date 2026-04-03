use clap::{Parser, Subcommand};
use std::process;

use flow_rs::add_issue;
use flow_rs::add_notification;
use flow_rs::append_note;

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
        Some(Commands::External(_)) => {
            // Unknown subcommand — exit 127 for hybrid dispatcher fallback
            process::exit(127);
        }
    }
}
