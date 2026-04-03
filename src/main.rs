use clap::{Parser, Subcommand};
use std::process;

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    // Ported commands will be added here as enum variants.
    // The external subcommand catch-all routes unrecognized
    // commands to exit 127, signaling bin/flow to try Python.
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
        Some(Commands::External(_)) => {
            // Unknown subcommand — exit 127 for hybrid dispatcher fallback
            process::exit(127);
        }
    }
}
