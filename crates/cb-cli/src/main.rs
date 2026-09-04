//! The `cache-bench` command line tool.
//!
//! One subcommand per stage, because a sweep takes days and the stages after it have to be rerunnable without repeating it.
//! Most of them are not written yet. The milestones say which: <https://github.com/tamnd/cache-bench/milestones>

mod doctor;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Reproduce tidwall/cache-benchmarks, in Rust.
#[derive(Debug, Parser)]
#[command(name = "cache-bench", version, about, long_about = None)]
struct Cli {
    /// Which stage to run.
    #[command(subcommand)]
    command: Command,
}

/// The stages.
#[derive(Debug, Subcommand)]
enum Command {
    /// Check everything a sweep needs before starting one.
    Doctor(doctor::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Doctor(args) => doctor::run(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("cache-bench: {why}");
            ExitCode::FAILURE
        }
    }
}
