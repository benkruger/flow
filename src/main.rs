use clap::{Parser, Subcommand};
use std::process;

use flow_rs::commands;

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
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
