//! `cache-bench choose`, which turns the runs of every cell into the four files a chart reads.
//!
//! A sweep writes 31 files per cell and nothing plots 31 of anything. This is the step that reduces each cell to a median, a best, a worst and an average, and it is the step where the original has four defects that all change published numbers.
//!
//! Both behaviours are here. Corrected is the default and `--compat=upstream` reproduces the original exactly, which is how its published files stay regenerable and how the size of each defect can be measured rather than asserted.
//! Nothing else about the two modes differs, so running both over the same directory and diffing the output is a fair comparison of the statistics and of nothing else.

use std::path::PathBuf;

use cb_core::{Compat, Run};
use cb_stats::{Kind, correct, upstream};

use crate::results::{self, Cell};

/// Which directory to reduce, and how.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// The results directory, which is the one holding `runs`.
    #[arg(long, default_value = "results", value_name = "PATH")]
    dir: PathBuf,
    /// Corrected, or the original's behaviour with its four defects.
    #[arg(long, default_value_t = Compat::Corrected, value_name = "MODE")]
    compat: Compat,
    /// Reduce one cell rather than every cell, named as the run files are without the `-run_N.json`.
    #[arg(long, value_name = "NAME")]
    cell: Option<String>,
    /// Write somewhere else, leaving the results directory alone.
    ///
    /// This is what makes two modes comparable without either of them overwriting the other.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
    /// Say what would be written without writing it.
    #[arg(long)]
    dry_run: bool,
}

/// Reduce every cell and write the four files each one comes to.
///
/// # Errors
///
/// If the directory cannot be read, if a run file will not parse, or if a cell cannot be reduced.
/// A cell that cannot be reduced stops the run rather than being skipped, because a chart drawn from a directory that is missing some of its cells looks exactly like a chart drawn from a complete one.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let out = args.out.as_ref().unwrap_or(&args.dir);
    let mut cells = results::cells(&args.dir)?;
    if let Some(want) = &args.cell {
        cells.retain(|cell| &cell.name == want);
        if cells.is_empty() {
            return Err(format!(
                "{} has no cell called {want}",
                results::runs_dir(&args.dir).display()
            ));
        }
    }
    if cells.is_empty() {
        return Err(format!(
            "{} has no run files in it",
            results::runs_dir(&args.dir).display()
        ));
    }

    let mut written = 0_usize;
    let mut smallest = usize::MAX;
    let mut gaps = 0_usize;
    for cell in &cells {
        smallest = smallest.min(cell.runs.len());
        if cell.after_gap > 0 {
            gaps += 1;
            println!(
                "{}: {} runs, and {} more past a missing one, which will not be used",
                cell.name,
                cell.runs.len(),
                cell.after_gap
            );
        }
        for (kind, run) in Kind::ALL.into_iter().zip(reduce(cell, args.compat)?) {
            let path = cell.chosen_path(out, kind);
            if !args.dry_run {
                results::write(&path, &run.emit())?;
            }
            written += 1;
        }
    }

    let what = if args.dry_run { "would write" } else { "wrote" };
    println!(
        "{what} {written} files for {} cells, {} mode, {smallest} runs in the smallest cell",
        cells.len(),
        args.compat
    );
    if gaps > 0 {
        println!("{gaps} cells have a gap in their runs, so a sweep did not finish");
    }
    Ok(())
}

/// The four aggregates of one cell, in the order the original writes them.
///
/// Upstream mode reduces all four in one call rather than four, because its run count is a global that the first call leaves smaller for the second. Doing them one at a time would silently give four copies of the first answer.
fn reduce(cell: &Cell, compat: Compat) -> Result<[Run; 4], String> {
    let why = |e| format!("{}: {e}", cell.name);
    if compat.is_upstream() {
        return upstream::choose_all(&cell.runs).map_err(why);
    }
    let mut out = Vec::with_capacity(4);
    for kind in Kind::ALL {
        out.push(correct::choose(&cell.runs, kind).map_err(why)?);
    }
    out.try_into()
        .map_err(|_| "four aggregates did not come to four files".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::{Path, PathBuf};

    use cb_core::{Compat, Run};
    use cb_stats::Kind;

    use super::{Args, run};
    use crate::results::{runs_dir, write};

    const CELL: &str = "bench_dragonfly-threads_1-pipeline_1-perf_yes";

    /// Where one of a cell's four files lands.
    fn path_for(dir: &Path, cell: &str, kind: Kind) -> PathBuf {
        runs_dir(dir).join(format!("{cell}-run_{}.json", kind.name()))
    }

    /// The golden cell, written out as the 31 run files a sweep would have left behind.
    fn sample(tag: &str) -> PathBuf {
        #[derive(serde::Deserialize)]
        struct Cell {
            runs: Vec<Run>,
        }
        use cb_core::golden::CELL_PERF as GOLDEN;
        let cell: Cell = serde_json::from_str(GOLDEN).unwrap();
        let dir = std::env::temp_dir().join(format!("cache-bench-choose-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        for (at, one) in cell.runs.iter().enumerate() {
            let name = format!("{CELL}-run_{}.json", at + 1);
            write(&runs_dir(&dir).join(name), &one.emit()).unwrap();
        }
        dir
    }

    fn args(dir: PathBuf) -> Args {
        Args {
            dir,
            compat: Compat::Corrected,
            cell: None,
            out: None,
            dry_run: false,
        }
    }

    // The original's own chosen files, regenerated from the original's own run files, through the command line rather than through the library.
    #[test]
    fn upstream_mode_writes_the_originals_files_byte_for_byte() {
        #[derive(serde::Deserialize)]
        struct Cell {
            upstream: std::collections::BTreeMap<String, Run>,
        }
        use cb_core::golden::CELL_PERF as GOLDEN;
        let want: Cell = serde_json::from_str(GOLDEN).unwrap();

        let dir = sample("upstream");
        let mut args = args(dir.clone());
        args.compat = Compat::Upstream;
        run(&args).unwrap();
        for kind in Kind::ALL {
            let got = std::fs::read_to_string(path_for(&dir, CELL, kind)).unwrap();
            assert_eq!(got, want.upstream[kind.name()].emit(), "{kind}");
        }
    }

    // The two modes have to disagree, or one of them is not doing its job.
    #[test]
    fn corrected_mode_writes_a_different_median() {
        let dir = sample("corrected");
        run(&args(dir.clone())).unwrap();
        let corrected = std::fs::read_to_string(path_for(&dir, CELL, Kind::Median)).unwrap();

        let mut args = args(dir.clone());
        args.compat = Compat::Upstream;
        args.out = Some(dir.join("upstream"));
        run(&args).unwrap();
        let upstream =
            std::fs::read_to_string(path_for(&dir.join("upstream"), CELL, Kind::Median)).unwrap();
        assert_ne!(corrected, upstream);
    }

    // Corrected mode carries the spread and upstream mode does not, because a file with a fifth key in it is not the original's file.
    #[test]
    fn only_corrected_mode_adds_the_spread() {
        let dir = sample("spread");
        run(&args(dir.clone())).unwrap();
        let picked =
            Run::parse(&std::fs::read_to_string(path_for(&dir, CELL, Kind::Average)).unwrap())
                .unwrap();
        assert!(picked.spread.is_some());
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        let dir = sample("dry");
        let mut args = args(dir.clone());
        args.dry_run = true;
        run(&args).unwrap();
        assert!(!path_for(&dir, CELL, Kind::Median).exists());
    }

    #[test]
    fn one_cell_by_name_and_a_name_that_is_not_there() {
        let dir = sample("one-cell");
        let mut args = args(dir.clone());
        args.cell = Some(CELL.to_owned());
        run(&args).unwrap();
        assert!(path_for(&dir, CELL, Kind::Best).exists());

        args.cell = Some("bench_redis-threads_1-pipeline_1-perf_no".to_owned());
        assert!(run(&args).unwrap_err().contains("no cell called"));
    }

    // A cell too small for the original's index arithmetic stops the command and names itself, where the original crashes.
    // Corrected mode reduces the same cell without complaint, because a median of one run is one run.
    #[test]
    fn a_cell_that_cannot_be_reduced_names_itself() {
        let dir = sample("too-small");
        for at in 2..=31 {
            let name = format!("{CELL}-run_{at}.json");
            std::fs::remove_file(runs_dir(&dir).join(name)).unwrap();
        }
        let mut args = args(dir);
        args.compat = Compat::Upstream;
        let err = run(&args).unwrap_err();
        assert!(err.contains(CELL), "{err}");
    }

    #[test]
    fn an_empty_directory_says_so() {
        let dir = std::env::temp_dir().join("cache-bench-choose-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(runs_dir(&dir)).unwrap();
        assert!(run(&args(dir)).unwrap_err().contains("no run files"));
    }
}
