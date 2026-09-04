//! `cache-bench sweep`, which is `run` ten thousand times in the right order.
//!
//! The order is the original's: engine, then thread count, then pipeline depth, then whether counters are attached, then the run number. It matters for two reasons. A partial results directory from this harness lines up with a partial one from the original, so the two can be compared before either finishes. And all 31 runs of a cell happen next to each other in time, so a window where somebody else was using the machine comes out as one cell that is visibly wrong rather than as a slight tilt spread across every cell in the sweep.
//!
//! Nothing in here measures anything. It decides which cells to measure and in what order, skips the ones already on disk, and hands each of the rest to the same code path `run` uses, so a cell measured by a sweep and a cell measured by hand are the same cell measured the same way.
//!
//! The restart rule is file existence and nothing else, and a file that will not parse does not count as existence. A sweep that ran for six days and lost power holds a directory of result files plus, possibly, one file that was created and never finished. Trusting that file because its name is right is how a truncated run ends up in a median.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cb_core::{
    Arch, CacheKind, Compat, Config, Failures, Outcome, PerfMode, Profile, Profiles, Step,
};

use crate::lock::Lock;
use crate::results;
use crate::run::{Cell, Setup};

/// How many recent runs the estimate averages over.
///
/// The cells are not the same size as each other, so the last twenty are a better guide to the next one than the last thousand are.
const WINDOW: usize = 20;

/// How many runs there have to be before an estimate is printed at all.
const ENOUGH: usize = 3;

/// How many of one engine's cells may fail in a row before the rest of them are left.
const GIVE_UP: u32 = 3;

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
/// If the config or the profile will not do, if the results directory is held by something else, or if anything was missing at the end. A cell that fails does not stop the sweep, because the other ten thousand are still worth measuring, but a sweep that did not measure everything says so on the way out rather than reporting success.
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

    let journal = args.dir.join("logs").join("sweep.jsonl");
    let record = args.dir.join("failures.json");
    let mut failures = failures(&record)?;
    // Every engine gets another chance at the start of a session, because the usual reason one was given up on is something on the machine that somebody has since fixed.
    failures.reconsider();

    // The whole matrix is checked against the disk before anything is measured, so the count in the progress line is the work left rather than the work there was, and so a directory full of half written files says so at the start instead of eight days in.
    let total = cells.len();
    let mut todo = Vec::new();
    for cell in cells {
        let name = cell.name().to_string();
        if done(&results::runs_dir(&args.dir).join(&name)) {
            failures.measured(&name);
            continue;
        }
        todo.push(cell);
    }
    let skipped = total - todo.len();
    keep(&record, &failures);

    let started = Instant::now();
    let tally = measure_all(&setup, &todo, &journal, &record, &mut failures);

    let mut said = vec![
        format!("{} measured here", tally.measured),
        format!("{skipped} already on disk"),
    ];
    if tally.failed > 0 {
        said.push(format!("{} failed", tally.failed));
    }
    if tally.left > 0 {
        said.push(format!(
            "{} left alone because their engine was given up on",
            tally.left
        ));
    }
    println!(
        "swept {total} cells in {}: {}",
        spell(started.elapsed()),
        said.join(", ")
    );

    if failures.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} cells have no file and are named with a reason in {}",
        failures.failures.len(),
        record.display()
    ))
}

/// How a session went.
struct Tally {
    /// Cells measured here.
    measured: usize,
    /// Cells attempted here that produced no file.
    failed: usize,
    /// Cells not attempted, because their engine had been given up on.
    left: usize,
}

/// The loop, over the cells that are not already on disk.
///
/// Every attempt goes in the journal, whether it worked or not, and the failure file is rewritten after each one, because the thing this is built for is being killed partway through.
fn measure_all(
    setup: &Setup<'_>,
    todo: &[Cell],
    journal: &std::path::Path,
    record: &std::path::Path,
    failures: &mut Failures,
) -> Tally {
    let mut seconds: Vec<f64> = Vec::new();
    let mut tally = Tally {
        measured: 0,
        failed: 0,
        left: 0,
    };
    let mut given_up: Vec<CacheKind> = Vec::new();
    let mut in_a_row = 0_u32;
    let mut last: Option<CacheKind> = None;

    for (at, cell) in todo.iter().enumerate() {
        if given_up.contains(&cell.cache) {
            tally.left += 1;
            continue;
        }
        let name = cell.name().to_string();
        let when = cb_core::now();
        // Before the run rather than after it, because the question this answers is whether the machine was already busy, and a run is itself load.
        let load = crate::host::load_average();
        match eta(todo.len() - at, &seconds) {
            Some(rest) => println!("[{}/{}] {name}, about {rest} left", at + 1, todo.len()),
            None => println!("[{}/{}] {name}", at + 1, todo.len()),
        }

        let began = Instant::now();
        let outcome = crate::run::once(setup, *cell);
        let took = began.elapsed().as_secs_f64();

        let why = match outcome {
            Ok(()) => {
                tally.measured += 1;
                seconds.push(took);
                failures.measured(&name);
                in_a_row = 0;
                None
            }
            Err(e) => {
                tally.failed += 1;
                failures.failed(&name, &when, &e);
                eprintln!("{name} failed: {e}");
                in_a_row = if last == Some(cell.cache) {
                    in_a_row.saturating_add(1)
                } else {
                    1
                };
                last = Some(cell.cache);
                // An engine whose every cell fails is a thousand cells that each take their own time to fail, and this is day three of eight. The rest of the matrix is still worth measuring, so this one is put down and named in the failure file.
                if in_a_row >= GIVE_UP {
                    given_up.push(cell.cache);
                    failures.abandon(cell.cache.name(), &when, in_a_row, &e);
                    eprintln!(
                        "{} has failed {in_a_row} times in a row, so the rest of its cells are being left rather than failing one at a time for the next day",
                        cell.cache
                    );
                }
                Some(e)
            }
        };
        note(
            journal,
            &Step {
                cell: name,
                started: when,
                seconds: took,
                load,
                outcome: if why.is_none() {
                    Outcome::Measured
                } else {
                    Outcome::Failed
                },
                why,
            },
        );
        keep(record, failures);
    }
    tally
}

/// Read the failure file, or start a new one.
///
/// A file that will not parse stops the sweep here rather than being written over, because it is the only record of what an earlier sweep of this directory could not measure.
fn failures(path: &std::path::Path) -> Result<Failures, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Failures::parse(&text).map_err(|e| why(path, &e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Failures::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Write the failure file, saying so and carrying on if it cannot be written.
///
/// A sweep that has been running for six days does not stop because a note about it could not be saved. Whatever is wrong with the disk will stop the next run file too, and that one does stop it.
fn keep(path: &std::path::Path, failures: &Failures) {
    if let Err(e) = results::write(path, &failures.emit()) {
        eprintln!("the failure file could not be written: {e}");
    }
}

/// Append one line to the journal, saying so and carrying on if it cannot be appended.
fn note(path: &std::path::Path, step: &Step) {
    if let Err(e) = append(path, &step.emit()) {
        eprintln!("the sweep log could not be written: {e}");
    }
}

/// Append to a file, making the directory above it first.
fn append(path: &std::path::Path, line: &str) -> Result<(), String> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} cannot be created: {e}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{} cannot be opened: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("{} cannot be written: {e}", path.display()))
}

/// How long the rest of the sweep will take, once there is enough measured here to say.
///
/// The average of the last few runs rather than of all of them, because a sweep walks from one thread up to sixteen and from pipeline one up to fifty, and the cells are not the same size as each other. The recent ones are the better guide to the next one, and nothing here pretends to more than that.
///
/// Nothing is printed until there are a few, because an estimate from one run is a number with no information in it and people believe printed numbers.
fn eta(remaining: usize, seconds: &[f64]) -> Option<String> {
    if seconds.len() < ENOUGH {
        return None;
    }
    let recent = &seconds[seconds.len().saturating_sub(WINDOW)..];
    let mean = recent.iter().sum::<f64>() / f64::from(u32::try_from(recent.len()).ok()?);
    let rest = mean * f64::from(u32::try_from(remaining).ok()?);
    // A sweep long enough to overflow this is not a sweep.
    Some(spell(Duration::from_secs_f64(rest.max(0.0))))
}

/// A duration, said the way a person would say it.
fn spell(took: Duration) -> String {
    let seconds = took.as_secs();
    let (days, hours, minutes) = (
        seconds / 86400,
        (seconds % 86400) / 3600,
        (seconds % 3600) / 60,
    );
    if days > 0 {
        return format!("{days}d {hours}h");
    }
    if hours > 0 {
        return format!("{hours}h {minutes}m");
    }
    if minutes > 0 {
        return format!("{minutes}m {}s", seconds % 60);
    }
    format!("{seconds}s")
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

    use super::{caches, done, eta, plan, spell};

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

    // An estimate from one run is a number with no information in it, and people believe printed numbers.
    #[test]
    fn nothing_is_estimated_until_there_is_something_to_estimate_from() {
        assert_eq!(eta(100, &[]), None);
        assert_eq!(eta(100, &[60.0, 60.0]), None);
        assert_eq!(eta(100, &[60.0, 60.0, 60.0]).unwrap(), "1h 40m");
    }

    // The recent runs, because a sweep walks from one thread to sixteen and from pipeline one to fifty, and the cells are not the same size as each other.
    #[test]
    fn the_estimate_follows_the_runs_it_just_did() {
        let mut seconds = vec![600.0; 30];
        seconds.extend([60.0; 20]);
        assert_eq!(eta(60, &seconds).unwrap(), "1h 0m");
    }

    #[test]
    fn a_duration_is_said_the_way_a_person_says_it() {
        use std::time::Duration;

        assert_eq!(spell(Duration::from_secs(9)), "9s");
        assert_eq!(spell(Duration::from_secs(90)), "1m 30s");
        assert_eq!(spell(Duration::from_secs(3700)), "1h 1m");
        assert_eq!(spell(Duration::from_secs(200_000)), "2d 7h");
    }

    // The failure this rule prevents is a file that was created and never finished being counted as a measurement.
    #[test]
    fn a_file_that_will_not_parse_is_not_a_measured_cell() {
        use cb_core::golden::RUN_PERF as RUN;
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
