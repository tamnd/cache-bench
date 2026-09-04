//! `cache-bench run`, which measures one cell once and writes one file.
//!
//! This is the direct analogue of the original's `bench` command, and it is the only place in the project that produces a number nobody else measured. Everything downstream, the selection, the combining, the charts and the documents, is arithmetic and formatting over files this wrote.
//!
//! The sequence is the original's, in the original's order, and it does not vary by engine:
//!
//! 1. Take the results directory, so that a second run cannot be started against the same one.
//! 2. Check that nothing is already answering on the socket.
//! 3. Ask the binary what version it is, because the answer goes in the file.
//! 4. Start the server, pinned to the cache half of the cores, with persistence off, a memory limit nothing will reach, and a unix socket.
//! 5. Wait for it to answer over that socket.
//! 6. Run a warmup SET pass and throw the numbers away.
//! 7. Attach perf to the server, if this cell is a perf cell.
//! 8. Run the measured SET pass and the measured GET pass, both pinned to the other half of the cores.
//! 9. Stop perf and read what it counted.
//! 10. Stop the server, and confirm its whole process group is gone.
//! 11. Write the run file and get it onto the disk.
//!
//! Steps 1, 2 and the flush in 11 are ours. The rest is the original's sequence, in the original's order, including the flat tenth of a second it sleeps after a server comes up.
//!
//! A run that fails anywhere in there writes nothing. A partial run is worse than a missing one, because the next stage cannot tell them apart and a sweep is restartable by exactly that: a cell's file is either there and complete, or it is not there and gets measured again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_cache::Server;
use cb_core::{
    Arch, CacheKind, Compat, Config, Endpoint, Info, Launch, Perf, Profile, Profiles, Run, RunName,
    Slot,
};
use cb_memtier::{Invocation, Pass};

use crate::lock::Lock;
use crate::results;

/// How long a server gets to answer after being started.
///
/// A minute. Every one of these answers in well under a second on a machine that is not swapping, so this is not a budget either, it is the point at which waiting longer is not going to help.
const READY: Duration = Duration::from_secs(60);

/// How long one memtier pass may take before the run is given up on.
///
/// Half an hour. A reference pass is tens of seconds, and the slowest cell in the sweep, one I/O thread at pipeline one, is minutes. Anything past this is memtier waiting on a server that stopped answering, and the cost of finding that out late is every run behind it in a sweep that takes days.
const PASS: Duration = Duration::from_mins(30);

/// How long a server gets to stop before it is killed.
const STOP: Duration = Duration::from_secs(10);

/// The original's flat sleep between a server coming up and the first pass.
const SETTLE: Duration = Duration::from_millis(100);

/// Which cell to measure, and where to put it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Which cache server.
    #[arg(value_name = "CACHE")]
    cache: CacheKind,
    /// How many I/O threads to give it, which is the x axis of every chart.
    #[arg(long, value_name = "N")]
    threads: u32,
    /// memtier's pipeline depth.
    #[arg(long, default_value_t = 1, value_name = "N")]
    pipeline: u32,
    // Spelled the way the filename spells it, rather than as a flag, so that the word in the command is the word in the name of the file it produces.
    /// Attach perf to the server for the measured passes.
    #[arg(
        long,
        default_value = "no",
        value_parser = yes_or_no,
        value_name = "yes|no",
        action = clap::ArgAction::Set,
    )]
    perf: bool,
    /// Which run of the cell this is, numbered from one as the filenames are.
    #[arg(long, default_value_t = 1, value_name = "N")]
    run: u32,
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
    /// The unix socket the server listens on.
    #[arg(long, default_value = "/tmp/cachebench.sock", value_name = "PATH")]
    socket: PathBuf,
    /// Corrected, or the original's behaviour with its defects.
    #[arg(long, default_value_t = Compat::Corrected, value_name = "MODE")]
    compat: Compat,
    /// Which perf to use, when this cell is a perf cell.
    #[arg(long, default_value = "perf", value_name = "PATH")]
    perf_binary: PathBuf,
    /// Measure the cell again even though its file is already there.
    #[arg(long)]
    force: bool,
}

/// Everything a run needs that is the same for every cell in a sweep.
///
/// Split out from the arguments because `sweep` measures ten thousand cells against one of these and one lock, and a sweep that re-read the config between cells could measure the first half of the matrix against one build of a server and the second half against another.
pub(crate) struct Setup<'a> {
    /// Where the binaries are.
    pub(crate) config: &'a Config,
    /// The machine shape being measured.
    pub(crate) profile: &'a Profile,
    /// Its name, which goes in every result file.
    pub(crate) profile_name: &'a str,
    /// The results directory.
    pub(crate) dir: &'a Path,
    /// The socket every server binds in turn.
    pub(crate) socket: &'a Path,
    /// Corrected, or the original's behaviour with its defects.
    pub(crate) compat: Compat,
    /// Which perf to use for the cells that want counters.
    pub(crate) perf_binary: &'a Path,
}

/// Which cell, and which run of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cell {
    /// Which server.
    pub(crate) cache: CacheKind,
    /// How many I/O threads it gets.
    pub(crate) threads: u32,
    /// memtier's pipeline depth.
    pub(crate) pipeline: u32,
    /// Whether counters are attached.
    pub(crate) perf: bool,
    /// Which run of the cell, numbered from one as the filenames are.
    pub(crate) run: u32,
}

impl Cell {
    /// The filename this run will be written under, which is also how it is named everywhere else.
    pub(crate) const fn name(self) -> RunName {
        RunName {
            cache: self.cache,
            threads: self.threads,
            pipeline: self.pipeline,
            perf: self.perf,
            slot: Slot::Run(self.run),
        }
    }
}

/// Measure the cell and write the file.
///
/// # Errors
///
/// On anything that would make the number wrong rather than late. A missing binary, a server that will not answer, a pass that did not complete the operations it was asked for, a server that outlived its own run, or a perf cell on a machine with no counters.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let config = read(&args.config)?;
    let config = Config::parse(&config, Arch::host()).map_err(|e| why(&args.config, &e))?;
    let profiles = read(&args.profiles)?;
    let profiles = Profiles::parse(&profiles).map_err(|e| why(&args.profiles, &e))?;
    let profile = profiles.get(&args.profile).map_err(|e| e.to_string())?;
    profile.check().map_err(|e| why(&args.profiles, &e))?;
    check_machine(profile, cpus())?;

    let cell = Cell {
        cache: args.cache,
        threads: args.threads,
        pipeline: args.pipeline,
        perf: args.perf,
        run: args.run,
    };
    let name = cell.name();
    let path = results::runs_dir(&args.dir).join(name.to_string());
    // A sweep is restartable by file existence and nothing else, so a run that is already on disk is left alone rather than measured again.
    if path.exists() && !args.force {
        println!("{name} is already measured, so nothing was run");
        return Ok(());
    }

    // Held for the whole run and given back on the way out, whether the run worked or not.
    let _held = Lock::take(&args.dir)?;
    once(
        &Setup {
            config: &config,
            profile,
            profile_name: &args.profile,
            dir: &args.dir,
            socket: &args.socket,
            compat: args.compat,
            perf_binary: &args.perf_binary,
        },
        cell,
    )
}

/// One cell, measured once, with the caller holding the results directory.
///
/// The lock and the check for a file that is already there belong to the caller, because `sweep` takes the directory once and then measures ten thousand cells inside it rather than taking and giving it back between each one.
///
/// # Errors
///
/// On anything that would make the number wrong rather than late.
pub(crate) fn once(setup: &Setup<'_>, cell: Cell) -> Result<(), String> {
    check_threads(cell.threads, setup.profile)?;

    // The stray check. A server left over from an earlier cell would answer this whole run, and every number in it would be a real number belonging to a different engine.
    if cb_cache::anybody_there(Endpoint::Unix(setup.socket)) {
        return Err(format!(
            "something is already answering on {}, so a server from an earlier run is still up and this run would have measured that one instead",
            setup.socket.display()
        ));
    }

    let binary = setup.config.binary(cell.cache).map_err(|e| e.to_string())?;
    let memtier = setup.config.memtier().map_err(|e| e.to_string())?;
    let logs = setup.dir.join("logs");
    make(&logs)?;
    let name = cell.name();
    let stem = name.to_string().trim_end_matches(".json").to_owned();
    let at = |what: &str| logs.join(format!("{stem}-{what}"));

    // Asked before anything is started, so that a binary that is not there fails the run before a server is up and a socket is bound.
    let version = cb_cache::version(binary, &at("version.txt")).map_err(|e| e.to_string())?;

    // A perf cell on a machine with no usable counters would write a file full of missing counters that looks exactly like a machine where one counter is unsupported, and the chart layer cannot tell those apart.
    if cell.perf {
        let probe = cb_perf::probe();
        if !probe.counted {
            return Err(format!(
                "this cell asks for counters and {}, so measure it without --perf or fix the machine",
                probe.reason
            ));
        }
    }

    let launch = Launch {
        binary,
        threads: cell.threads,
        maxmemory: setup.profile.maxmemory,
        endpoint: Endpoint::Unix(setup.socket),
        compat: setup.compat,
        as_root: cb_cache::as_root(),
    };
    let started = cb_core::now();
    let mut server = Server::start(
        cell.cache,
        &launch,
        pin(&setup.profile.cache_pin),
        &at("server.log"),
        READY,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{} up in {:?} as pid {}",
        cell.cache,
        server.ready(),
        server.pid()
    );
    // The original's flat sleep after startup, kept. It is not needed here, because the wait above is a round trip rather than a guess, but it is a tenth of a second in a run that lasts minutes and dropping it would be a change to the sequence for no measurable gain.
    std::thread::sleep(SETTLE);

    let measured = measure(setup, cell, memtier, &at, &mut server);

    // The server is stopped whether the passes worked or not, and a group that outlived its run is a failure in its own right, because the next run would be measured against it.
    let stopped = server.stop(STOP).map_err(|e| e.to_string());
    let (sets, gets, counters) = measured?;
    stopped?;

    let run = Run {
        info: Info {
            cache: cell.cache.name().to_owned(),
            version,
            threads: cell.threads,
            bench_threads: setup.profile.bench_threads,
            connections: setup.profile.connections(),
            operations: setup.profile.total_operations(),
            sizerange: setup.profile.size_range.to_string(),
            pipeline: cell.pipeline,
            // Both of these are ours, and upstream mode writes the original's file exactly.
            profile: ours(setup.compat, || setup.profile_name.to_owned()),
            run_started: ours(setup.compat, || started.clone()),
            kind: None,
        },
        sets,
        gets,
        perf: counters,
        spread: None,
    };
    let path = results::runs_dir(setup.dir).join(name.to_string());
    results::write(&path, &run.emit())?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The three passes, with perf attached across the two that are kept.
///
/// Split out from [`once`] so that a failure in here still stops the server, rather than leaving one holding the cores because the run gave up early.
fn measure(
    setup: &Setup<'_>,
    cell: Cell,
    memtier: &Path,
    at: &dyn Fn(&str) -> PathBuf,
    server: &mut Server,
) -> Result<(cb_core::Op, cb_core::Op, Perf), String> {
    let pass = |pass: Pass, json_out: &Path| -> Result<cb_memtier::Load, String> {
        let invocation = Invocation {
            profile: setup.profile,
            pipeline: cell.pipeline,
            pass,
            protocol: cell.cache.protocol(),
            socket: setup.socket,
            json_out,
        };
        cb_memtier::run(
            memtier,
            &invocation,
            pin(&setup.profile.bench_pin),
            &at(&format!("{}.log", pass.label())),
            PASS,
        )
        .map_err(|e| e.to_string())
    };

    // Thrown away, and checked anyway. A warmup that did not run means the measured SET pass is partly a measurement of hash table growth.
    let warmup = pass(Pass::Warmup, &at("warmup.json"))?;
    println!("warmup done in {:?}", warmup.took);

    // Attached here rather than before the warmup, which is where the original attaches it, so the counters cover the two measured passes and the short gap between them and nothing else.
    let session = if cell.perf {
        Some(
            cb_perf::Session::attach(setup.perf_binary, server.pid(), &at("perf.csv"))
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };

    let sets = pass(Pass::Sets, &at("sets.json"))?;
    let gets = pass(Pass::Gets, &at("gets.json"))?;

    let counters = match session {
        Some(session) => session.finish().map_err(|e| e.to_string())?,
        None => Perf::default(),
    };

    // A server that died during the passes produced numbers for part of a run, and memtier will happily report a rate over whatever it managed before the connections went.
    if !server.alive().map_err(|e| e.to_string())? {
        return Err(format!(
            "{} exited during its own run, so these numbers are a fraction of a run rather than a measurement",
            cell.cache
        ));
    }

    println!(
        "sets {:.0} ops/sec in {:?}, gets {:.0} ops/sec in {:?}",
        sets.op.opsec.0, sets.took, gets.op.opsec.0, gets.took
    );
    Ok((sets.op, gets.op, counters))
}

/// A thread count that would measure something other than what it says it measures.
fn check_threads(threads: u32, profile: &Profile) -> Result<(), String> {
    if threads == 0 {
        return Err("a server with no I/O threads is not a cell".to_owned());
    }
    let cores = profile.cache_pin.len();
    if threads as usize > cores {
        return Err(format!(
            "{threads} I/O threads on {cores} pinned cores measures oversubscription rather than the server, so lower --threads or use a profile with more cores in cache_pin"
        ));
    }
    Ok(())
}

/// A profile that describes a bigger machine than this one.
///
/// The pin is the reason this matters. Asking the kernel for a core that is not there is refused, so the reference profile on a laptop fails somewhere in the middle of a run with `Invalid argument` and nothing saying which argument. Worse, a mask that names some cores this machine has and some it does not is accepted and quietly narrowed, so a server that was meant to have sixteen cores gets four and the run produces numbers.
///
/// A machine that will not say how many cores it has is left alone. That is a question this harness can do without an answer to, and refusing a run over it would be worse than the failure it prevents.
///
/// The count is passed in rather than asked for here, because `doctor` makes the same check as part of a set of them and both want one implementation of it.
pub(crate) fn check_machine(profile: &Profile, cpus: Option<u32>) -> Result<(), String> {
    let Some(here) = cpus else {
        return Ok(());
    };
    if profile.cores > here {
        return Err(format!(
            "this profile is written for {} cores and this machine has {here}, so the pins in it name cores that are not here, and a run pinned to a core that does not exist is either refused by the kernel or quietly given a smaller one",
            profile.cores
        ));
    }
    Ok(())
}

/// How many logical CPUs this machine has, where it says.
///
/// A machine with more cores than a u32 can hold does not exist, and if one turns up it is not the machine anything here refuses.
pub(crate) fn cpus() -> Option<u32> {
    std::thread::available_parallelism()
        .ok()
        .map(|here| u32::try_from(here.get()).unwrap_or(u32::MAX))
}

/// The pin, where pinning is possible.
///
/// Everything here runs on Linux. Elsewhere the pin is dropped rather than refused, because the parts of this that can be exercised on a laptop are worth being able to exercise, and a run without a pin says so in its own output rather than pretending to be comparable.
#[cfg(target_os = "linux")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the option is the other platform's answer, and the two have to have the same signature"
)]
fn pin(cpus: &cb_core::CpuSet) -> Option<&cb_core::CpuSet> {
    Some(cpus)
}

/// No affinity call here, so nothing is pinned.
#[cfg(not(target_os = "linux"))]
const fn pin(_cpus: &cb_core::CpuSet) -> Option<&cb_core::CpuSet> {
    None
}

/// `yes` or `no`, which is how the filename says it.
fn yes_or_no(text: &str) -> Result<bool, String> {
    match text {
        "yes" => Ok(true),
        "no" => Ok(false),
        other => Err(format!("expected yes or no, and got {other}")),
    }
}

/// A field that is ours rather than the original's.
fn ours<T>(compat: Compat, value: impl FnOnce() -> T) -> Option<T> {
    match compat {
        Compat::Corrected => Some(value()),
        Compat::Upstream => None,
    }
}

/// Read a file, saying which one when it is not there.
fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|why| format!("cannot read {}: {why}", path.display()))
}

/// Make a directory, saying which one when it cannot be made.
fn make(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|why| format!("cannot make {}: {why}", path.display()))
}

/// A parse failure, with the file that failed to parse in front of it.
fn why(path: &Path, error: &dyn std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use cb_core::{Compat, Profiles};

    use super::{check_machine, check_threads, ours};

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

    // The failure that produces a chart rather than an error. Sixteen threads on sixteen cores is the sweep, and seventeen is a measurement of the scheduler.
    #[test]
    fn a_thread_count_past_the_pinned_cores_is_refused() {
        let profile = profile();
        assert!(check_threads(16, &profile).is_ok());
        let why = check_threads(17, &profile).unwrap_err();
        assert!(why.contains("oversubscription"), "{why}");
        assert!(check_threads(0, &profile).is_err());
    }

    // The pins in a profile name cores by number, so a profile written for a bigger machine names cores that are not there.
    #[test]
    fn a_profile_written_for_a_bigger_machine_is_refused() {
        let mut profile = profile();
        profile.cores = 32;
        let why = check_machine(&profile, Some(8)).unwrap_err();
        assert!(why.contains("has 8"), "{why}");
        assert!(check_machine(&profile, Some(32)).is_ok());
        assert!(check_machine(&profile, Some(64)).is_ok());
        // A machine that will not say is left alone rather than refused.
        assert!(check_machine(&profile, None).is_ok());
    }

    // The two fields this project added are the two that have to disappear in upstream mode, because a file with an extra key in it is not the original's file.
    #[test]
    fn our_own_fields_are_left_out_in_upstream_mode() {
        assert_eq!(ours(Compat::Corrected, || "reference"), Some("reference"));
        assert_eq!(ours(Compat::Upstream, || "reference"), None);
    }
}
