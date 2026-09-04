//! The `cache-bench` command line tool.
//!
//! One subcommand per stage, because a sweep takes days and the stages after it have to be rerunnable without repeating it.
//! Most of them are not written yet. The milestones say which: <https://github.com/tamnd/cache-bench/milestones>

mod chart;
mod choose;
mod combine;
mod docs;
mod doctor;
mod lock;
mod results;
mod run;
mod verify;

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
    /// Measure one cell once and write one run file.
    Run(run::Args),
    /// Reduce each cell's runs to a median, a best, a worst and an average.
    Choose(choose::Args),
    /// Gather every chosen file into the one file the charts read.
    Combine(combine::Args),
    /// Draw the charts.
    Chart(chart::Args),
    /// Write the documents that go with a results directory.
    Docs(docs::Args),
    /// Check this port against the original's own files.
    Verify(verify::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Doctor(args) => doctor::run(args),
        Command::Run(args) => run::run(args),
        Command::Choose(args) => choose::run(args),
        Command::Combine(args) => combine::run(args),
        Command::Chart(args) => chart::run(args),
        Command::Docs(args) => docs::run(args),
        Command::Verify(args) => verify::run(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("cache-bench: {why}");
            ExitCode::FAILURE
        }
    }
}
