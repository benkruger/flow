use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "flow-rs", version, about = "FLOW CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(_) => {}
        None => {
            println!("flow-rs: no command specified. Use --help for usage.");
        }
    }
}
