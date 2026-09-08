//! `cache-bench spread`, which says which cells of a sweep are too noisy to quote.
//!
//! Every chosen file already carries the coefficient of variation over the runs that went into it, and until now nothing read it back.
//! Reviewing it per cell before publishing is a line on a checklist, and a line on a checklist is a thing somebody does by eye at the end of a week long sweep, which is the worst moment to be asking a person to be careful.
//!
//! So this reads it back. It says, per engine, how bad the worst cell was, how many are over the line and which thread counts they are at, and with `--check` it fails instead of only saying so.
//! The default threshold is the figure the published notes are already written in, so a cell this refuses is a cell the review would have refused.
//!
//! What it does not do is throw anything away. A noisy cell is still a measurement of something, and what to do about one is a decision with a machine and a deadline in it.
//! This names them and stops.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use cb_core::{Output, Spread};

/// The coefficient of variation above which a cell is not worth quoting.
///
/// A tenth of a percent is a quiet box and a few percent is a box with something else on it.
/// Five percent is well outside anything a machine that is being measured properly produces, and it is the figure the `wsl32coarse` notes were hand written in, so a cell that fails here is a cell that failed the review that produced them.
const OVER: f64 = 0.05;

/// Which directory to look at and how strict to be about it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory, the one holding `output.json`.
    #[arg(long, value_name = "PATH")]
    dir: PathBuf,
    /// The coefficient of variation a cell has to stay under.
    #[arg(long, value_name = "CV", default_value_t = OVER)]
    over: f64,
    /// Fail if any cell is over, rather than reporting and stopping.
    #[arg(long)]
    check: bool,
}

/// Report it.
///
/// # Errors
///
/// If the results directory will not read, if it carries no cell with a spread on it, or if `--check` was asked for and a cell is over the threshold.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let cells = read(args)?;
    let engines = tally(&cells, args.over);

    println!(
        "spread    {} cells in {}, against a coefficient of variation of {:.2}",
        cells.len(),
        args.dir.display(),
        args.over
    );
    println!();
    println!("{:<12}  {:>5}  {:>4}  where", "engine", "worst", "over");
    for (name, engine) in &engines {
        // Trimmed, because the last column is empty on exactly the rows a reader is hoping to see, and a line of trailing spaces is the kind of thing that shows up in a diff later.
        let row = format!(
            "{name:<12}  {:>5.2}  {:>4}  {}",
            engine.worst,
            engine.over,
            places(&engine.at)
        );
        println!("{}", row.trim_end());
    }

    let over: usize = engines.values().map(|e| e.over).sum();
    println!();
    if over == 0 {
        println!("every cell is under {:.2}", args.over);
        return Ok(());
    }
    let what = format!(
        "{over} of {} cells are above {:.2}, so a number from one of them is a picture of which run won rather than of how fast the engine is",
        cells.len(),
        args.over
    );
    if args.check {
        return Err(what);
    }
    println!("{what}");
    Ok(())
}

/// One cell, reduced to the thing this command is about.
#[derive(Debug)]
struct Cell {
    /// Which engine it measured.
    cache: String,
    /// How many server threads it ran against.
    threads: u32,
    /// The worse of its two passes.
    cv: f64,
}

/// What one engine's cells came to.
#[derive(Debug, Default)]
struct Engine {
    /// The worst coefficient of variation over all of its cells.
    worst: f64,
    /// How many of its cells were over the line.
    over: usize,
    /// The thread counts those cells were at, deduplicated and in order.
    ///
    /// Deduplicated because an engine runs several cells at one thread count, one per pipeline depth, and a reader looking at this column wants to know which end of the thread axis went bad rather than to count to eight.
    at: Vec<u32>,
}

/// The coefficient of variation a cell is judged on, which is the worse of its two passes.
///
/// The worse rather than the two of them separately, because a cell is quotable only if both halves of it are, and a GET chart drawn from a cell whose SET pass was a coin toss is still drawn from a disturbed cell.
fn worse(spread: &Spread) -> f64 {
    spread.sets.opsec_cv.0.max(spread.gets.opsec_cv.0)
}

/// Every cell in the directory, once each.
///
/// A results directory carries four files per cell, one per aggregate, and all four report the same spread because the spread describes the cell rather than the aggregate.
/// Reading one of the four rather than deduplicating afterwards keeps the count in the output equal to the number of cells that were measured.
fn read(args: &Args) -> Result<Vec<Cell>, String> {
    let path = args.dir.join("output.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let output = Output::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut cells = Vec::new();
    for entry in &output.entries {
        if entry.data.info.kind.as_deref() != Some("median") {
            continue;
        }
        let Some(spread) = &entry.data.spread else {
            continue;
        };
        cells.push(Cell {
            cache: entry.data.info.cache.clone(),
            threads: entry.data.info.threads,
            cv: worse(spread),
        });
    }
    if cells.is_empty() {
        return Err(format!(
            "{} carries no cell with a spread on it, which is what a directory combined in upstream compatibility mode looks like, since the spread object is ours and that mode leaves it out",
            path.display()
        ));
    }
    Ok(cells)
}

/// Gather the cells by engine.
fn tally(cells: &[Cell], over: f64) -> BTreeMap<String, Engine> {
    let mut out: BTreeMap<String, Engine> = BTreeMap::new();
    for cell in cells {
        let engine = out.entry(cell.cache.clone()).or_default();
        if cell.cv > engine.worst {
            engine.worst = cell.cv;
        }
        if cell.cv > over {
            engine.over += 1;
            if !engine.at.contains(&cell.threads) {
                engine.at.push(cell.threads);
            }
        }
    }
    for engine in out.values_mut() {
        engine.at.sort_unstable();
    }
    out
}

/// The thread counts an engine was noisy at, as a phrase rather than a list.
///
/// Thread counts rather than whole cell names, because an engine that is noisy is usually noisy at the top of the thread axis and nowhere else, and that shape is the thing a reader is looking for.
/// The cell names are in `output.json` for anybody who wants them.
fn places(threads: &[u32]) -> String {
    match threads {
        [] => String::new(),
        [one] => format!("threads {one}"),
        [rest @ .., last] => {
            let front: Vec<String> = rest.iter().map(u32::to_string).collect();
            format!("threads {} and {last}", front.join(", "))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::{Dispersion, Fixed3, Spread};

    use super::{Cell, places, tally, worse};

    fn cell(cache: &str, threads: u32, cv: f64) -> Cell {
        Cell {
            cache: cache.to_owned(),
            threads,
            cv,
        }
    }

    fn dispersion(cv: f64) -> Dispersion {
        Dispersion {
            opsec_p25: Fixed3(0.0),
            opsec_p75: Fixed3(0.0),
            opsec_sd: Fixed3(0.0),
            opsec_cv: Fixed3(cv),
        }
    }

    // The count is over cells and the column beside it is over thread counts, and an engine measured at several pipeline depths per thread count is what tells the two of them apart.
    #[test]
    fn the_count_is_cells_and_the_column_beside_it_is_thread_counts() {
        let cells = [
            cell("yo", 8, 0.4),
            cell("yo", 8, 0.6),
            cell("yo", 16, 0.7),
            cell("yo", 1, 0.01),
        ];
        let out = tally(&cells, 0.05);
        let yo = &out["yo"];
        assert_eq!(yo.over, 3);
        assert_eq!(yo.at, [8, 16]);
        assert!((yo.worst - 0.7).abs() < 1e-9, "{}", yo.worst);
    }

    // A steady engine has to come out with an empty list rather than with no row at all, because a reader comparing engines wants the one that was steady sitting next to the one that was not.
    #[test]
    fn an_engine_that_was_never_noisy_still_has_a_row() {
        let out = tally(&[cell("redis", 1, 0.01), cell("redis", 16, 0.02)], 0.05);
        let redis = &out["redis"];
        assert_eq!(redis.over, 0);
        assert!(redis.at.is_empty());
        assert!((redis.worst - 0.02).abs() < 1e-9, "{}", redis.worst);
    }

    // Exactly on the line is under it. A threshold a cell can sit on is a threshold that answers differently depending on rounding somewhere else.
    #[test]
    fn a_cell_exactly_on_the_line_is_not_over_it() {
        let out = tally(&[cell("valkey", 4, 0.05)], 0.05);
        assert_eq!(out["valkey"].over, 0);
    }

    #[test]
    fn the_worse_of_the_two_passes_is_the_one_that_counts() {
        let mut spread = Spread {
            n: 5,
            trim: 0,
            sets: dispersion(0.02),
            gets: dispersion(0.31),
            perf: None,
        };
        assert!((worse(&spread) - 0.31).abs() < 1e-9);
        std::mem::swap(&mut spread.sets, &mut spread.gets);
        assert!((worse(&spread) - 0.31).abs() < 1e-9);
    }

    #[test]
    fn the_thread_counts_read_as_a_phrase() {
        assert_eq!(places(&[]), "");
        assert_eq!(places(&[16]), "threads 16");
        assert_eq!(places(&[8, 16]), "threads 8 and 16");
        assert_eq!(places(&[1, 8, 16]), "threads 1, 8 and 16");
    }
}
