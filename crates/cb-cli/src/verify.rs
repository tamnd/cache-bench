//! `cache-bench verify`, which is the claim this port makes about itself, run as a command.
//!
//! The claim is that this is a port rather than a rewrite that resembles one, and the evidence is that the original's own files go through it and come back unchanged.
//! Two levels of it. On its own it checks the golden files committed here, which is a few kilobytes and runs anywhere. Pointed at a checkout of the original with `--against` it checks all 20160 run files, all 2304 chosen files and the whole published `output.json`.
//!
//! It also prints what the corrections cost, because both modes read the same directory and differ in nothing but the statistics, so the difference between them is the size of the four defects rather than an opinion about them.

use std::path::{Path, PathBuf};

use cb_core::{Entry, Output, Run};
use cb_stats::{Kind, correct, upstream};

use crate::results;

/// What to check against.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory to check against, which is `results` inside a checkout of the original.
    ///
    /// Without it, only the golden files committed here are checked, which needs nothing and proves less.
    #[arg(long, value_name = "PATH")]
    against: Option<PathBuf>,
    /// Skip the comparison between the two modes, which is the slow half.
    #[arg(long)]
    no_compare: bool,
}

/// The run file with counters attached, where every counter is a JSON string and one of them is not a number at all.
const RUN_PERF: &str = include_str!(
    "../../../testdata/golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1.json"
);
/// The run file without counters, where `perf` has to come back as an empty object rather than as null.
const RUN_PLAIN: &str = include_str!(
    "../../../testdata/golden/bench_dragonfly-threads_1-pipeline_1-perf_no-run_1.json"
);
/// A chosen file, where the same counters are JSON numbers and `kind` has been appended last.
const CHOSEN: &str = include_str!(
    "../../../testdata/golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"
);
/// Three entries of the published combined file, which pins the layout and its odd indentation.
const COMBINED: &str = include_str!("../../../testdata/golden/output-three-cells.json");
/// One whole cell with counters, its 31 runs and the four files the original reduced them to.
const CELL_PERF: &str =
    include_str!("../../../testdata/golden/cells/dragonfly-threads_1-pipeline_1-perf_yes.json");
/// The same cell measured without counters, which escapes one of the four defects by accident.
const CELL_PLAIN: &str =
    include_str!("../../../testdata/golden/cells/dragonfly-threads_1-pipeline_1-perf_no.json");

/// A cell as the golden files hold it, which is the runs and the answers side by side.
#[derive(serde::Deserialize)]
struct Cell {
    /// What the cell was named in the original's results.
    #[serde(rename = "cell")]
    name: String,
    /// The 31 runs.
    runs: Vec<Run>,
    /// The four files the original reduced them to, by kind.
    upstream: std::collections::BTreeMap<String, Run>,
}

/// Run every check and say what each one found.
///
/// # Errors
///
/// If anything fails to come back the way it went in. The message says how many things and the lines above it say which.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let mut failed = 0_usize;
    failed += format_round_trips();
    failed += golden_cells()?;
    if let Some(dir) = &args.against {
        failed += corpus(dir, !args.no_compare)?;
    } else {
        println!("corpus    not checked, pass --against a checkout of the original");
    }
    if failed > 0 {
        return Err(format!("{failed} checks did not come back unchanged"));
    }
    println!("ok");
    Ok(())
}

/// The format half. Every committed file parses and writes back out as the bytes it came in as.
fn format_round_trips() -> usize {
    let mut failed = 0;
    for (what, text) in [
        ("a run with counters", RUN_PERF),
        ("a run without counters", RUN_PLAIN),
        ("a chosen file", CHOSEN),
    ] {
        match Run::parse(text) {
            Ok(run) if run.emit() == text => {}
            Ok(_) => {
                println!("format    {what} came back different");
                failed += 1;
            }
            Err(e) => {
                println!("format    {what} did not parse: {e}");
                failed += 1;
            }
        }
    }
    match Output::parse(COMBINED) {
        Ok(out) if out.emit() == COMBINED => {}
        Ok(_) => {
            println!("format    the combined file came back different");
            failed += 1;
        }
        Err(e) => {
            println!("format    the combined file did not parse: {e}");
            failed += 1;
        }
    }
    if failed == 0 {
        println!("format    4 golden files round trip byte for byte");
    }
    failed
}

/// The statistics half, against the two cells committed here.
///
/// One engine at one thread count and one pipeline depth, so this catches a mistake but cannot show the absence of one. That is what `--against` is for.
fn golden_cells() -> Result<usize, String> {
    let mut failed = 0;
    let mut checked = 0;
    for text in [CELL_PERF, CELL_PLAIN] {
        let cell: Cell =
            serde_json::from_str(text).map_err(|e| format!("a golden cell will not parse: {e}"))?;
        let ours = upstream::choose_all(&cell.runs).map_err(|e| format!("{}: {e}", cell.name))?;
        for (kind, got) in Kind::ALL.into_iter().zip(ours) {
            let Some(want) = cell.upstream.get(kind.name()) else {
                println!("cells     {} has no {kind} in the golden file", cell.name);
                failed += 1;
                continue;
            };
            if got.emit() == want.emit() {
                checked += 1;
            } else {
                println!("cells     {} {kind} came back different", cell.name);
                failed += 1;
            }
        }
    }
    if failed == 0 {
        println!("cells     2 cells, {checked} chosen files reproduced byte for byte");
    }
    Ok(failed)
}

/// The whole thing, against a real results directory.
///
/// Reads every run file, reduces every cell in upstream mode, compares each chosen file against the one already on disk, and then compares the combined file it would write against the one already there.
fn corpus(dir: &Path, compare: bool) -> Result<usize, String> {
    let cells = results::cells(dir)?;
    if cells.is_empty() {
        return Err(format!(
            "{} has no run files in it",
            results::runs_dir(dir).display()
        ));
    }
    let runs: usize = cells.iter().map(|c| c.runs.len()).sum();
    println!("runs      {runs} run files read, {} cells", cells.len());

    let mut failed = 0;
    let mut checked = 0;
    let mut entries = Vec::with_capacity(cells.len() * 4);
    let mut moved = Moved::default();
    for cell in &cells {
        let ours = upstream::choose_all(&cell.runs).map_err(|e| format!("{}: {e}", cell.name))?;
        for (kind, got) in Kind::ALL.into_iter().zip(&ours) {
            let path = cell.chosen_path(dir, kind);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
            if got.emit() == text {
                checked += 1;
            } else {
                println!("upstream  {} came back different", path.display());
                failed += 1;
            }
            entries.push(Entry {
                file: format!("{}-run_{}.json", cell.name, kind.name()),
                data: got.clone(),
            });
        }
        if compare {
            for (kind, was) in Kind::ALL.into_iter().zip(&ours) {
                let now =
                    correct::choose(&cell.runs, kind).map_err(|e| format!("{}: {e}", cell.name))?;
                moved.add(kind, was, &now);
            }
        }
    }
    if failed == 0 {
        println!("upstream  {checked} chosen files reproduced byte for byte");
    }

    entries.sort_by(|a, b| a.file.cmp(&b.file));
    let count = entries.len();
    let ours = Output { entries }.emit();
    let path = dir.join("output.json");
    match std::fs::read_to_string(&path) {
        Ok(text) if text == ours => println!(
            "combined  output.json reproduced byte for byte, {count} entries, {} kb",
            ours.len() / 1024
        ),
        Ok(_) => {
            println!("combined  {} came back different", path.display());
            failed += 1;
        }
        Err(e) => {
            println!("combined  {} cannot be read: {e}", path.display());
            failed += 1;
        }
    }

    if compare {
        moved.report();
    }
    Ok(failed)
}

/// How far the corrected numbers sit from the original's, over every cell.
///
/// Kept as the relative difference of each throughput series per aggregate, plus which way it went, because the size of the four defects and the direction of them are two separate claims and the direction is the one that survives averaging.
#[derive(Default)]
struct Moved {
    /// One entry per kind, in the order of `Kind::ALL`.
    rows: [Row; 4],
}

/// One aggregate's worth of differences.
#[derive(Default)]
struct Row {
    /// Relative difference in GET throughput, as a percentage, one per cell.
    gets: Vec<f64>,
    /// The same for SET.
    sets: Vec<f64>,
    /// Cells where the original's GET number is the higher of the two.
    gets_higher: usize,
}

impl Moved {
    /// Record one cell reduced both ways.
    fn add(&mut self, kind: Kind, was: &Run, now: &Run) {
        let at = match kind {
            Kind::Median => 0,
            Kind::Best => 1,
            Kind::Worst => 2,
            Kind::Average => 3,
        };
        let row = &mut self.rows[at];
        row.gets.push(apart(was.gets.opsec.0, now.gets.opsec.0));
        row.sets.push(apart(was.sets.opsec.0, now.sets.opsec.0));
        if was.gets.opsec.0 > now.gets.opsec.0 {
            row.gets_higher += 1;
        }
    }

    /// Print the middle and the tail of each series, and the direction of the median.
    fn report(&self) {
        for (kind, row) in Kind::ALL.into_iter().zip(&self.rows) {
            let (Some(gets), Some(sets)) = (summary(&row.gets), summary(&row.sets)) else {
                continue;
            };
            println!("moved     {kind:8} get {gets}, set {sets}");
        }
        let row = &self.rows[0];
        println!(
            "moved     the original's median get is the higher of the two in {} of {} cells",
            row.gets_higher,
            row.gets.len()
        );
    }
}

/// How far apart two numbers are, as a percentage of the second.
///
/// Zero when the second is zero, since a cell that measured nothing is not a cell either mode disagrees about.
fn apart(was: f64, now: f64) -> f64 {
    if now <= 0.0 {
        0.0
    } else {
        (was - now).abs() / now * 100.0
    }
}

/// The middle and the largest of a series, as one line.
fn summary(values: &[f64]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted[sorted.len() / 2];
    let worst = sorted[sorted.len() - 1];
    Some(format!(
        "{middle:.2} percent typical and {worst:.2} at worst"
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Args, apart, format_round_trips, golden_cells, run, summary};

    // The whole point of the command, and it needs nothing to run, which is why it can be in CI.
    #[test]
    fn the_committed_files_all_check_out() {
        assert_eq!(format_round_trips(), 0);
        assert_eq!(golden_cells().unwrap(), 0);
        run(&Args {
            against: None,
            no_compare: false,
        })
        .unwrap();
    }

    #[test]
    fn a_directory_that_is_not_there_says_so() {
        let err = run(&Args {
            against: Some("/there/is/no/such/results/dir".into()),
            no_compare: false,
        })
        .unwrap_err();
        assert!(err.contains("cannot be listed"), "{err}");
    }

    #[test]
    fn a_difference_is_a_percentage_of_the_corrected_number() {
        assert!((apart(110.0, 100.0) - 10.0).abs() < 1e-9);
        assert!((apart(90.0, 100.0) - 10.0).abs() < 1e-9);
        assert!(apart(1.0, 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_summary_of_nothing_is_nothing() {
        assert!(summary(&[]).is_none());
        let one = summary(&[1.0, 2.0, 9.0]).unwrap();
        assert!(one.contains("2.00 percent typical"), "{one}");
        assert!(one.contains("and 9.00 at worst"), "{one}");
    }
}
