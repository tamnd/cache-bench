//! `cache-bench sweep`, which is `run` ten thousand times in the right order.
//!
//! The order is the original's: engine, then thread count, then pipeline depth, then whether counters are attached, then the run number. It matters for two reasons. A partial results directory from this harness lines up with a partial one from the original, so the two can be compared before either finishes. And all 31 runs of a cell happen next to each other in time, so a window where somebody else was using the machine comes out as one cell that is visibly wrong rather than as a slight tilt spread across every cell in the sweep.
//!
//! Nothing in here measures anything. It decides which cells to measure and in what order, skips the ones already on disk, and hands each of the rest to the same code path `run` uses, so a cell measured by a sweep and a cell measured by hand are the same cell measured the same way.
//!
//! The restart rule is file existence, and a file that will not parse does not count as existence. A sweep that ran for six days and lost power holds a directory of result files plus, possibly, one file that was created and never finished. Trusting that file because its name is right is how a truncated run ends up in a median.
//!
//! The one thing besides a file that stops a cell being measured is the count of how many times it already has been. Four attempts and it is left alone, because a cell that has failed four times is one this machine cannot measure and the fifth costs a full run to find that out again. `--retry-failed` throws those counts away.

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

/// How many times a cell may be attempted, across every sweep of a directory, before it is left alone.
///
/// Four, which is the number the count in `failures.json` was written down for. A cell that failed once is a cell to try again, because the usual reason is something that was on the machine at the time and has since gone. A cell that has failed four times is a cell this machine cannot measure, and the fifth attempt costs a full run to learn that again.
///
/// Leaving them is what lets a sweep finish rather than circle. The failures of one shape land next to each other in the order the matrix runs in, because a shape is five consecutive runs, so three of them in a row put the engine down under [`GIVE_UP`] and every later cell is left with it. On a box where one shape cannot be measured that is the difference between a sweep that gets through the rest of the matrix and a sweep that is restarted forever and gets no further.
///
/// `--retry-failed` throws the counts away, which is what somebody says when the machine has changed.
const TRIES: u32 = 4;

/// How much of the machine somebody else may be using before this waits rather than measures, in cores.
///
/// Half a core. It sounds strict for a box with 32 of them and it is meant to: the cores this sweep pins to are named in the profile, so a competitor is a competitor for one of the four or sixteen cores the server under test is on rather than background spread over the machine.
const CROWDED: f64 = 0.5;

/// How long to let the last run's teardown drain out before sampling what everybody else is doing.
const SETTLE: Duration = Duration::from_secs(2);

/// How long each sample takes.
///
/// Long enough that a core switching between two processes averages out and short enough that four seconds a run against three minutes a run is nothing.
const SAMPLE: Duration = Duration::from_secs(2);

/// How long to wait before looking again, once the machine has been found busy.
const ASK_AGAIN: Duration = Duration::from_secs(60);

/// How many times to look before giving up on the machine rather than on the cell.
///
/// Sixty of them, so an hour. Anything on the box for less than that is a build or a test run and waiting for it costs one cell's worth of time out of a sweep that runs for days. Anything on the box for longer than that is another job rather than a blip, and every remaining cell would measure the two of them fighting.
const PATIENCE: u32 = 60;

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
    /// Attempt the cells that have already been attempted their four times, rather than leaving them alone.
    #[arg(long)]
    retry_failed: bool,
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
    // A cell gets another chance only when somebody asks for it, because the count of how many times it has failed is the one thing here that is about the cell rather than about the session.
    if args.retry_failed {
        failures.try_again();
    }

    let total = cells.len();
    let Left {
        todo,
        skipped,
        spent,
    } = whats_left(cells, &results::runs_dir(&args.dir), &mut failures);
    if spent > 0 {
        println!(
            "{spent} cells have been attempted {TRIES} times each and are being left alone, so that this sweep can reach the end of the matrix. Their reasons are in {}, and --retry-failed attempts them again.",
            record.display()
        );
    }
    keep(&record, &failures);

    let started = Instant::now();
    let tally = measure_all(&setup, &todo, &journal, &record, &mut failures);

    println!(
        "swept {total} cells in {}: {}",
        spell(started.elapsed()),
        summary(&tally, skipped, spent)
    );

    // Ahead of the missing cells, because a sweep that stopped early is missing cells by definition and the count of them is not the thing to read.
    if let Some(why) = tally.stopped {
        return Err(why);
    }
    if failures.is_empty() {
        return Ok(());
    }
    Err(missing(&failures, &record))
}

/// The one line a sweep says about itself on the way out.
fn summary(tally: &Tally, skipped: usize, spent: usize) -> String {
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
    if spent > 0 {
        said.push(format!("{spent} left alone after {TRIES} tries each"));
    }
    said.join(", ")
}

/// What the sweep fails with when the matrix has holes in it.
///
/// The two counts are said apart because they mean different things to whoever restarts this. A cell that is short of its tries is worth another sweep. A cell that has had them is not, and a script that keeps starting a sweep to get it will start one forever.
fn missing(failures: &Failures, record: &std::path::Path) -> String {
    let all = failures.failures.len();
    let done_for = failures
        .failures
        .iter()
        .filter(|f| f.attempts >= TRIES)
        .count();
    if done_for == 0 {
        return format!(
            "{all} cells have no file and are named with a reason in {}",
            record.display()
        );
    }
    format!(
        "{all} cells have no file and are named with a reason in {}, and {done_for} of them have been attempted {TRIES} times each and will not be attempted again without --retry-failed",
        record.display()
    )
}

/// Whether a cell is worth a run, given what the failure file remembers about it.
fn worth_trying(failures: &Failures, cell: &str) -> bool {
    failures.attempts(cell) < TRIES
}

/// What a matrix comes to once the disk and the failure file have had their say.
struct Left {
    /// The cells to measure, in the order they were planned in.
    todo: Vec<Cell>,
    /// Cells with a file already, which is what a restarted sweep is mostly made of.
    skipped: usize,
    /// Cells that have had their tries and are being left alone.
    spent: usize,
}

/// Take the cells that already have a file and the ones that have had their tries out of the matrix.
///
/// The whole matrix is checked against the disk before anything is measured, so the count in the progress line is the work left rather than the work there was, and so a directory full of half written files says so at the start instead of eight days in.
fn whats_left(cells: Vec<Cell>, runs: &std::path::Path, failures: &mut Failures) -> Left {
    let total = cells.len();
    let mut todo = Vec::new();
    let mut spent = 0;
    for cell in cells {
        let name = cell.name().to_string();
        if done(&runs.join(&name)) {
            failures.measured(&name);
            continue;
        }
        if !worth_trying(failures, &name) {
            spent += 1;
            continue;
        }
        todo.push(cell);
    }
    Left {
        skipped: total - todo.len() - spent,
        todo,
        spent,
    }
}

/// How a session went.
struct Tally {
    /// Cells measured here.
    measured: usize,
    /// Cells attempted here that produced no file.
    failed: usize,
    /// Cells not attempted, because their engine had been given up on.
    left: usize,
    /// Why the sweep stopped before it reached the end, when it did.
    stopped: Option<String>,
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
        stopped: None,
    };
    let mut given_up: Vec<CacheKind> = Vec::new();
    let mut in_a_row = 0_u32;
    let mut last: Option<CacheKind> = None;
    // A machine that will not say how many cores it has cannot have a share of them worked out, so on that machine there is nothing to wait for and the sweep runs as it always did.
    let cpus = crate::run::cpus();
    let mut sample = || {
        // The window has to start after the last run's teardown, or the thing being counted is this sweep putting its own server away.
        std::thread::sleep(SETTLE);
        crate::host::busy_cores(SAMPLE, cpus?)
    };

    for (at, cell) in todo.iter().enumerate() {
        if given_up.contains(&cell.cache) {
            tally.left += 1;
            continue;
        }
        let name = cell.name().to_string();
        // Before the run rather than after it, because the question both of these answer is whether the machine was already busy, and a run is itself load.
        let busy = match wait_for_quiet(&mut sample, &mut std::thread::sleep) {
            Ok(busy) => busy,
            Err(e) => {
                eprintln!("{e}");
                tally.stopped = Some(e);
                break;
            }
        };
        let when = cb_core::now();
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
                busy,
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

/// Wait until nothing else is using this machine, and answer what it was doing when it went quiet.
///
/// This is the check the results directory on the eight core host needed and did not have. `doctor` asks whether the machine is busy once, before anything starts, and a sweep runs for days. That box turned out to be a build runner as well, so the sweep started on an idle machine and spent the next several hours measuring against eight compilers, which is not a slower number, it is not a number.
///
/// Waiting rather than failing, because a cell that fails is a hole in a chart and the usual reason a machine is busy is something that will be finished in twenty minutes. Stopping the whole sweep rather than waiting forever, because nothing is lost by stopping: the cells already on disk stay where they are and a sweep started again picks up from them.
///
/// The two arguments are how it looks and how it waits, so that this can be tested without an hour going by.
///
/// # Errors
///
/// If the machine was still busy after [`PATIENCE`] looks.
fn wait_for_quiet(
    look: &mut dyn FnMut() -> Option<f64>,
    nap: &mut dyn FnMut(Duration),
) -> Result<Option<f64>, String> {
    let mut worst = 0.0_f64;
    for asked in 0..=PATIENCE {
        // A machine that does not publish the counters is a machine with nothing to wait for, which is every machine that is not Linux.
        let Some(busy) = look() else {
            return Ok(None);
        };
        if busy <= CROWDED {
            if asked > 0 {
                println!("the machine is quiet again at {busy:.2} cores in use, carrying on");
            }
            return Ok(Some(busy));
        }
        worst = worst.max(busy);
        if asked == 0 {
            println!(
                "something else is using {busy:.2} cores of this machine and this measures at {CROWDED:.2} or under, so it is waiting rather than measuring a race"
            );
        }
        if asked < PATIENCE {
            nap(ASK_AGAIN);
        }
    }
    Err(format!(
        "something else has been using up to {worst:.2} cores of this machine for {} and has not stopped, so every cell from here on would measure the two of them fighting. The sweep is stopping instead. Nothing measured is lost: start it again on the same directory when the machine is free and it picks up where this left off.",
        spell(ASK_AGAIN * PATIENCE)
    ))
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

    use cb_core::{CacheKind, Failures, Profiles};

    use super::{
        PATIENCE, TRIES, caches, done, eta, missing, plan, spell, wait_for_quiet, whats_left,
        worth_trying,
    };

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
        assert_eq!(
            cells.len() as u64,
            profile.total_runs() / CacheKind::ALL.len() as u64
        );
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

    // The common case, which is a machine with nothing on it but this sweep.
    #[test]
    fn a_quiet_machine_is_measured_on_without_waiting() {
        let mut naps = 0;
        let busy = wait_for_quiet(&mut || Some(0.03), &mut |_| naps += 1).unwrap();
        assert_eq!(busy, Some(0.03));
        assert_eq!(naps, 0);
    }

    // A build that finishes is the usual reason a machine is busy, and waiting for it costs one cell out of a sweep that runs for days.
    #[test]
    fn a_machine_that_goes_quiet_is_waited_for() {
        let mut looks = 0;
        let mut naps = 0;
        let busy = wait_for_quiet(
            &mut || {
                looks += 1;
                Some(if looks > 3 { 0.10 } else { 7.5 })
            },
            &mut |_| naps += 1,
        )
        .unwrap();
        assert_eq!(busy, Some(0.10));
        assert_eq!(naps, 3);
    }

    // The eight core host is a build runner as well, and a sweep that kept going there spent hours producing numbers of two workloads fighting. Stopping loses nothing, because the cells on disk stay and the sweep picks up from them.
    #[test]
    fn a_machine_that_stays_busy_stops_the_sweep_rather_than_the_cell() {
        let mut naps = 0;
        let why = wait_for_quiet(&mut || Some(14.5), &mut |_| naps += 1).unwrap_err();
        assert!(why.contains("14.50 cores"), "{why}");
        assert!(why.contains("1h 0m"), "{why}");
        assert!(why.contains("picks up where this left off"), "{why}");
        assert_eq!(naps, PATIENCE as usize);
    }

    // Every machine that is not Linux publishes nothing to work this out from, and a sweep on one of those runs exactly as it did before there was a check here.
    #[test]
    fn a_machine_that_publishes_nothing_is_not_waited_for() {
        let mut naps = 0;
        let busy = wait_for_quiet(&mut || None, &mut |_| naps += 1).unwrap();
        assert_eq!(busy, None);
        assert_eq!(naps, 0);
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

    // A cell that failed once is a cell to try again. A cell that has had its four tries is one this machine cannot measure, and the sweep has to get past it rather than spend a run learning that again.
    #[test]
    fn a_cell_is_worth_another_run_until_it_has_had_its_tries() {
        let mut failures = Failures::default();
        assert!(worth_trying(&failures, "a.json"), "never attempted");
        for _ in 1..TRIES {
            failures.failed("a.json", "2026-09-04T00:00:00Z", "the box was busy");
            assert!(worth_trying(&failures, "a.json"), "short of its tries");
        }
        failures.failed("a.json", "2026-09-04T00:00:00Z", "the box was busy");
        assert!(!worth_trying(&failures, "a.json"), "has had its tries");
        // And another cell in the same file is its own question.
        assert!(worth_trying(&failures, "b.json"));
    }

    // The three counts have to add up to the matrix, or the progress line and the summary are both about a different sweep than the one that ran.
    #[test]
    fn the_matrix_comes_apart_into_measured_spent_and_left_to_do() {
        use cb_core::golden::RUN_PERF as RUN;
        let dir = std::env::temp_dir().join("cache-bench-sweep-left");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let cells = plan(&[CacheKind::Yo], &profile());
        let total = cells.len();
        assert!(total > 2, "the profile has a matrix worth splitting");
        let on_disk = cells[0].name().to_string();
        let had_its_tries = cells[1].name().to_string();
        crate::results::write(&dir.join(&on_disk), RUN).unwrap();

        let mut failures = Failures::default();
        // The one on the disk is named in the failure file as well, which is what a cell that failed and was then measured looks like.
        failures.failed(&on_disk, "2026-09-04T00:00:00Z", "the box was busy");
        for _ in 0..TRIES {
            failures.failed(&had_its_tries, "2026-09-04T00:00:00Z", "the box was busy");
        }

        let left = whats_left(cells, &dir, &mut failures);
        assert_eq!(left.skipped, 1, "the one with a file");
        assert_eq!(left.spent, 1, "the one that has had its tries");
        assert_eq!(left.todo.len(), total - 2);
        assert_eq!(
            failures.attempts(&on_disk),
            0,
            "a cell with a file is not a failure any more"
        );
    }

    // A script restarting the sweep reads this line to decide whether another sweep would get anywhere.
    #[test]
    fn the_way_out_says_whether_another_sweep_would_help() {
        let record = std::path::Path::new("results/failures.json");
        let mut failures = Failures::default();
        failures.failed("a.json", "2026-09-04T00:00:00Z", "the box was busy");
        let said = missing(&failures, record);
        assert!(said.contains("1 cells have no file"), "{said}");
        assert!(!said.contains("--retry-failed"), "{said}");

        for _ in 1..TRIES {
            failures.failed("a.json", "2026-09-04T00:00:00Z", "the box was busy");
        }
        let said = missing(&failures, record);
        assert!(said.contains("--retry-failed"), "{said}");
        assert!(said.contains("1 of them"), "{said}");
    }

    // The counts are what make the sweep circle rather than finish, so throwing them away has to be enough to make it try again.
    #[test]
    fn retrying_a_spent_cell_takes_asking_for_it() {
        let mut failures = Failures::default();
        for _ in 0..TRIES {
            failures.failed("a.json", "2026-09-04T00:00:00Z", "the box was busy");
        }
        assert!(!worth_trying(&failures, "a.json"));
        failures.try_again();
        assert!(worth_trying(&failures, "a.json"));
    }
}
