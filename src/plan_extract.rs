//! Plan extraction command — accelerates the Plan phase for pre-decomposed issues.
//!
//! Consolidates the Plan phase ceremony (gate check, phase enter, issue fetch,
//! DAG/plan file creation, state mutations, logging, PR render, phase complete)
//! into a single process. For decomposed issues with an Implementation Plan
//! section, this eliminates ~12-19 model round trips.
//!
//! Three response paths:
//! - `extracted`: decomposed issue with Implementation Plan — phase completed in one call
//! - `standard`: not decomposed or no plan section — model takes over for decompose/explore/write
//! - `resumed`: plan already exists in state — phase completed in one call
//!
//! Error responses use `Ok(json!({"status": "error", ...}))` so the caller
//! receives structured JSON on stdout. Infrastructure errors (file I/O, lock
//! failures) return `Err(String)` for `run()` to wrap in `json_error`.

use clap::Parser;
use serde_json::{json, Value};

use crate::output::{json_error, json_ok};

/// Extract and fast-track pre-decomposed plans, or prepare state for model-driven planning.
#[derive(Parser, Debug)]
#[command(name = "plan-extract")]
pub struct Args {
    /// Override branch for state file lookup
    #[arg(long)]
    pub branch: Option<String>,

    /// PR number (read from state file if omitted)
    #[arg(long)]
    pub pr: Option<i64>,
}

pub fn run(args: Args) {
    match run_impl(&args) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            json_error(&e, &[]);
            std::process::exit(1);
        }
    }
}

/// Fallible entry point for plan extraction.
///
/// Returns structured JSON as `Ok(Value)` for all business responses
/// (including status-error responses like gate failures). Returns
/// `Err(String)` only for infrastructure failures (file I/O, lock errors).
pub fn run_impl(args: &Args) -> Result<Value, String> {
    // TODO: Tasks 2-5 implement the full logic here
    Ok(json!({
        "status": "error",
        "message": "not implemented"
    }))
}
