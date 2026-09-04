//! `cache-bench sweep`, which is `run` ten thousand times in the right order.
//!
//! The order is the original's: engine, then thread count, then pipeline depth, then whether counters are attached, then the run number. It matters for two reasons. A partial results directory from this harness lines up with a partial one from the original, so the two can be compared before either finishes. And all 31 runs of a cell happen next to each other in time, so a window where somebody else was using the machine comes out as one cell that is visibly wrong rather than as a slight tilt spread across every cell in the sweep.
//!
//! Nothing in here measures anything. It decides which cells to measure and in what order, skips the ones already on disk, and hands each of the rest to the same code path `run` uses, so a cell measured by a sweep and a cell measured by hand are the same cell measured the same way.
//!
//! The restart rule is file existence and nothing else, and a file that will not parse does not count as existence. A sweep that ran for six days and lost power holds a directory of result files plus, possibly, one file that was created and never finished. Trusting that file because its name is right is how a truncated run ends up in a median.

use std::path::PathBuf;
use std::time::Instant;

use cb_core::{Arch, CacheKind, Compat, Config, PerfMode, Profile, Profiles};

use crate::lock::Lock;
use crate::results;
use crate::run::{Cell, Setup};

/// Which matrix to sweep, and where to put it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Where the compiled binaries are.
    #[arg(long, default_value = "config.jsonc", value_name = "PATH")]
    config: PathBuf,
    /// The machine shapes and the sweep that fits each one.
    #[arg(long, default_value = "profiles.toml", value_name = "PATH")]
    profiles: PathBuf,
    /// Which profile this machine is.
    #[arg(long, value_name = "NAME")]
    profile: String,
    /// The results directory to write into.
    #[arg(long, default_value = "results", value_name = "PATH")]
    dir: PathBuf,
    /// The unix socket every server listens on in turn.
    #[arg(long, default_value = "/tmp/cachebench.sock", value_name = "PATH")]
    socket: PathBuf,
    /// Corrected, or the original's behaviour with its defects.
    #[arg(long, default_value_t = Compat::Corrected, value_name = "MODE")]
    compat: Compat,
    /// Which perf to use, for the cells that want counters.
    #[arg(long, default_value = "perf", value_name = "PATH")]
    perf_binary: PathBuf,
    /// Sweep only this engine, repeatable. The default is all seven.
    #[arg(long, value_name = "CACHE")]
    cache: Vec<CacheKind>,
    /// Print the cells this would measure, in order, and measure none of them.
    #[arg(long)]
    dry_run: bool,
}

/// Sweep the matrix.
///
/// # Errors
///
/// If the config or the profile will not do, if the results directory is held by something else, or if any cell fails. A cell that fails stops the sweep rather than being skipped, because the run files already written are the whole record of what happened and there is nowhere yet for the reason to go.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let config = read(&args.config)?;
    let config = Config::parse(&config, Arch::host()).map_err(|e| why(&args.config, &e))?;
    let profiles = read(&args.profiles)?;
    let profiles = Profiles::parse(&profiles).map_err(|e| why(&args.profiles, &e))?;
    let profile = profiles.get(&args.profile).map_err(|e| e.to_string())?;
    profile.check().map_err(|e| why(&args.profiles, &e))?;
    crate::run::check_machine(profile, crate::run::cpus())?;

    let caches = caches(&args.cache);
    // Asked here rather than when the sweep reaches that engine, because a config that never named Garnet is a config that fails on day three of eight, having measured everything before it.
    for cache in &caches {
        config.binary(*cache).map_err(|e| e.to_string())?;
    }
    config.memtier().map_err(|e| e.to_string())?;

    let cells = plan(&caches, profile);
    let named: Vec<&str> = caches.iter().map(|kind| kind.name()).collect();
    println!(
        "{} cells over {}, at {} thread counts and {} pipeline depths, {} runs each",
        cells.len(),
        named.join(", "),
        profile.threads.len(),
        profile.pipelines.len(),
        profile.runs
    );

    if args.dry_run {
        for cell in &cells {
            println!("{}", cell.name());
        }
        return Ok(());
    }

    // Taken once for the whole sweep rather than once per cell, because a gap between two cells is a gap somebody else can start a second sweep in.
    let _held = Lock::take(&args.dir)?;
    let setup = Setup {
        config: &config,
        profile,
        profile_name: &args.profile,
        dir: &args.dir,
        socket: &args.socket,
        compat: args.compat,
        perf_binary: &args.perf_binary,
    };

    let started = Instant::now();
    let total = cells.len();
    let mut measured = 0_usize;
    let mut skipped = 0_usize;
    for (at, cell) in cells.iter().enumerate() {
        let name = cell.name();
        let path = results::runs_dir(&args.dir).join(name.to_string());
        if done(&path) {
            skipped += 1;
            continue;
        }
        println!("[{}/{total}] {name}", at + 1);
        crate::run::once(&setup, *cell).map_err(|e| {
            format!(
                "{name} failed after {measured} runs measured in this session, which are on disk and will not be repeated when this is started again: {e}"
            )
        })?;
        measured += 1;
    }
    println!(
        "swept {total} cells in {:?}: {measured} measured here, {skipped} already on disk",
        started.elapsed()
    );
    Ok(())
}

/// Every cell to measure, in the order the original measures them.
///
/// Engine, thread count, pipeline depth, counters, run. The loops are written in that order and nothing sorts the result afterwards, because the order is the point.
fn plan(caches: &[CacheKind], profile: &Profile) -> Vec<Cell> {
    let mut cells = Vec::new();
    for cache in caches {
        for threads in &profile.threads {
            for pipeline in &profile.pipelines {
                for perf in &profile.perf {
                    for run in 1..=profile.runs {
                        cells.push(Cell {
                            cache: *cache,
                            threads: *threads,
                            pipeline: *pipeline,
                            perf: matches!(perf, PerfMode::Yes),
                            run,
                        });
                    }
                }
            }
        }
    }
    cells
}

/// Which engines this sweep covers, in the original's order whatever order they were asked for in.
fn caches(asked: &[CacheKind]) -> Vec<CacheKind> {
    if asked.is_empty() {
        return CacheKind::ALL.to_vec();
    }
    CacheKind::ALL
        .into_iter()
        .filter(|kind| asked.contains(kind))
        .collect()
}

/// Whether this cell is already measured.
///
/// A file that is there and will not parse is not a measurement, it is the shape of one, and it is what a sweep that was killed partway through a write leaves behind. It gets measured again and says so, because a truncated file that is trusted becomes a run in a median and there is nothing downstream that can tell.
fn done(path: &std::path::Path) -> bool {
    if !path.exists() {
        return false;
    }
    match results::read(path) {
        Ok(_) => true,
        Err(e) => {
            println!("{e}, so it is being measured again");
            false
        }
    }
}

/// Read a file, saying which one when it is not there.
fn read(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|why| format!("cannot read {}: {why}", path.display()))
}

/// A parse failure, with the file that failed to parse in front of it.
fn why(path: &std::path::Path, error: &dyn std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeSet;

    use cb_core::{CacheKind, Profiles};

    use super::{caches, done, plan};

    /// The profile the reference numbers were measured with.
    fn profile() -> cb_core::Profile {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../profiles.toml"))
                .expect("reads profiles.toml");
        Profiles::parse(&text)
            .expect("parses")
            .get("reference")
            .expect("has the reference profile")
            .clone()
    }

    // The order is the whole reason this function exists. Run number varies fastest, then counters, then pipeline depth, then threads, then the engine.
    #[test]
    fn the_sweep_is_in_the_originals_order() {
        let profile = profile();
        let cells = plan(&CacheKind::ALL, &profile);
        let names: Vec<String> = cells.iter().take(4).map(|c| c.name().to_string()).collect();
        assert_eq!(
            names,
            vec![
                "bench_memcache-threads_1-pipeline_1-perf_no-run_1.json",
                "bench_memcache-threads_1-pipeline_1-perf_no-run_2.json",
                "bench_memcache-threads_1-pipeline_1-perf_no-run_3.json",
                "bench_memcache-threads_1-pipeline_1-perf_no-run_4.json",
            ]
        );
        // The runs of a cell are together in time, which is what makes a noisy window one bad cell rather than a tilt across the whole sweep.
        let runs = profile.runs as usize;
        assert!(cells[..runs].iter().all(|c| c.run <= profile.runs));
        assert_eq!(cells[runs].run, 1);
        assert!(cells[runs].perf);
    }

    // A cell measured twice is an hour of the machine's time thrown away, and a cell measured none is a hole in a chart.
    #[test]
    fn every_cell_is_planned_exactly_once() {
        let profile = profile();
        let cells = plan(&CacheKind::ALL, &profile);
        let names: BTreeSet<String> = cells.iter().map(|c| c.name().to_string()).collect();
        assert_eq!(names.len(), cells.len());
        assert_eq!(cells.len() as u64, profile.total_runs());
    }

    #[test]
    fn asking_for_one_engine_sweeps_one_engine() {
        let profile = profile();
        let cells = plan(&caches(&[CacheKind::Yo]), &profile);
        assert!(cells.iter().all(|c| c.cache == CacheKind::Yo));
        assert_eq!(cells.len() as u64, profile.total_runs() / 7);
    }

    // Asked for in any order, swept in the original's, because the order is a property of the sweep and not of the command line.
    #[test]
    fn the_engines_are_swept_in_the_originals_order() {
        assert_eq!(
            caches(&[CacheKind::Yo, CacheKind::Memcache]),
            vec![CacheKind::Memcache, CacheKind::Yo]
        );
        assert_eq!(caches(&[]), CacheKind::ALL.to_vec());
    }

    // The failure this rule prevents is a file that was created and never finished being counted as a measurement.
    #[test]
    fn a_file_that_will_not_parse_is_not_a_measured_cell() {
        const RUN: &str = include_str!(
            "../../../testdata/golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1.json"
        );
        let dir = std::env::temp_dir().join("cache-bench-sweep-done");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let missing = dir.join("missing.json");
        assert!(!done(&missing));

        let whole = dir.join("whole.json");
        crate::results::write(&whole, RUN).unwrap();
        assert!(done(&whole));

        let cut = dir.join("cut.json");
        crate::results::write(&cut, &RUN[..RUN.len() / 2]).unwrap();
        assert!(!done(&cut));
    }
}
