//! `cache-bench mem`, which measures what an engine costs to hold a known number of keys.
//!
//! The other half of the comparison. `run` and `sweep` measure how fast an engine is; nothing measured how much it costs, which means half of what a reader wants to compare two cache servers on has had no metric here at all.
//!
//! The sequence, per engine, is the same shape as a run and for the same reasons:
//!
//! 1. Check nothing is already answering on the socket, because a leftover server would be measured instead of this one.
//! 2. Ask the binary its version, before anything is started, so a missing binary fails now.
//! 3. Start it pinned to the cache half of the cores, with a memory limit nothing will reach.
//! 4. Read what it holds before a single key goes in. That is the baseline, and it is reported rather than subtracted.
//! 5. Write exactly one key per client slot per operation, so the number of distinct keys left behind is known rather than estimated. That arithmetic is [`cb_mem::Plan`].
//! 6. Let it settle, then read the largest resident set it ever had.
//! 7. Stop it, and confirm the group is gone.
//! 8. Put the row in `memory.json`, replacing this engine's previous row if there was one.
//!
//! A memory limit that the working set would reach turns this into a measurement of an eviction policy, so the limit is checked against the payload before anything starts rather than discovered afterwards as a suspiciously good number.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_cache::Server;
use cb_core::{Arch, CacheKind, Compat, Config, Endpoint, Launch, Profile, Profiles};
use cb_mem::{Plan, Report, Row};
use cb_memtier::{Invocation, Pass};

use crate::lock::Lock;
use crate::run::{check_machine, cpus, pin};

/// How long a server gets to answer after being started.
const READY: Duration = Duration::from_secs(60);

/// How long the filling pass may take before the measurement is given up on.
const PASS: Duration = Duration::from_mins(60);

/// How long a server gets to stop before it is killed.
const STOP: Duration = Duration::from_secs(10);

/// How long an engine is left alone between the last key going in and the peak being read.
///
/// Several of these do work after the writes stop: a background rehash finishing, a defragmenter passing, an allocator returning pages. Reading the moment memtier exits measures the middle of that rather than the end of it. Five seconds is long enough for the ones that do it and short enough that measuring eight engines is minutes rather than an afternoon.
const SETTLE: Duration = Duration::from_secs(5);

/// The name of the file this writes, next to the results it belongs with.
const FILE: &str = "memory.json";

/// Which engines to measure, and how many keys to give them.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Measure one engine rather than all of them.
    #[arg(long, value_name = "CACHE")]
    cache: Option<CacheKind>,
    /// How many distinct keys to leave in each server.
    ///
    /// It has to divide by the number of clients the profile runs, which is what makes the count known rather than estimated. A number given here that does not divide is refused rather than adjusted, because somebody who typed a count meant that count.
    ///
    /// Left out, it is about ten million, rounded down to something the profile's clients divide. A fixed default cannot be right for every profile: ten million does not divide by the 256 clients the reference profile runs, so the default has to be a function of the profile or it is a default that always fails.
    #[arg(long, value_name = "N")]
    entries: Option<u64>,
    /// How many I/O threads to give each server.
    ///
    /// It does not change what an engine holds, so this is here to make the pass finish rather than to shape the result.
    #[arg(long, default_value_t = 8, value_name = "N")]
    threads: u32,
    /// memtier's pipeline depth for the filling pass, which only affects how long it takes.
    #[arg(long, default_value_t = 25, value_name = "N")]
    pipeline: u32,
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
}

/// Measure every engine asked for and write the file.
///
/// # Errors
///
/// On anything that would make the number wrong rather than late: a missing binary, a server that will not answer, a fill pass that did not complete, a memory limit the working set would reach, or a machine with no `/proc` to read a resident set out of.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let config = std::fs::read_to_string(&args.config)
        .map_err(|e| format!("cannot read {}: {e}", args.config.display()))?;
    let config = Config::parse(&config, Arch::host())
        .map_err(|e| format!("{}: {e}", args.config.display()))?;
    let profiles = std::fs::read_to_string(&args.profiles)
        .map_err(|e| format!("cannot read {}: {e}", args.profiles.display()))?;
    let profiles =
        Profiles::parse(&profiles).map_err(|e| format!("{}: {e}", args.profiles.display()))?;
    let profile = profiles.get(&args.profile).map_err(|e| e.to_string())?;
    profile.check().map_err(|e| e.to_string())?;
    check_machine(profile, cpus())?;

    let plan = Plan::new(
        profile,
        args.entries.unwrap_or_else(|| about_ten_million(profile)),
    )
    .map_err(|e| e.to_string())?;
    let payload = plan.payload(profile);
    check_headroom(payload, profile)?;

    let wanted: Vec<CacheKind> = match args.cache {
        Some(one) => vec![one],
        None => CacheKind::ALL.to_vec(),
    };

    std::fs::create_dir_all(&args.dir)
        .map_err(|e| format!("cannot make {}: {e}", args.dir.display()))?;
    let _held = Lock::take(&args.dir)?;

    let path = args.dir.join(FILE);
    let mut report = match std::fs::read_to_string(&path) {
        Ok(text) => Report::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(_) => Report::default(),
    };

    println!(
        "{} entries, {} of keys and values, {} clients writing {} each",
        plan.entries,
        human(payload),
        plan.clients,
        plan.per_client
    );

    for cache in wanted {
        let row = one(args, &config, profile, plan, cache)?;
        println!(
            "{cache} peaked at {} holding {} entries, which is {:.1} bytes each and {:.1} over the payload",
            human(row.peak_rss),
            row.entries,
            row.total_per_entry(),
            row.overhead_per_entry()
        );
        report.put(row);
        // Written after each engine rather than at the end, because measuring eight of these takes an hour and a failure on the last one should not throw away the seven before it.
        crate::results::write(&path, &report.emit())?;
    }
    println!("wrote {}", path.display());
    Ok(())
}

/// One engine, started, filled, measured and stopped.
fn one(
    args: &Args,
    config: &Config,
    profile: &Profile,
    plan: Plan,
    cache: CacheKind,
) -> Result<Row, String> {
    // The stray check. A server left over from an earlier engine would answer this whole measurement, and the peak read at the end would be that engine's.
    if cb_cache::anybody_there(Endpoint::Unix(&args.socket)) {
        return Err(format!(
            "something is already answering on {}, so a server from an earlier measurement is still up and this one would have measured that instead",
            args.socket.display()
        ));
    }

    let binary = config.binary(cache).map_err(|e| e.to_string())?;
    let memtier = config.memtier().map_err(|e| e.to_string())?;
    let logs = args.dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("cannot make {}: {e}", logs.display()))?;
    let at = |what: &str| logs.join(format!("mem_{}-{what}", cache.name()));

    // Asked before anything is started, so that a binary that is not there fails before a socket is bound.
    let version = cb_cache::version(binary, &at("version.txt")).map_err(|e| e.to_string())?;

    let launch = Launch {
        binary,
        threads: args.threads,
        maxmemory: profile.maxmemory,
        endpoint: Endpoint::Unix(&args.socket),
        compat: Compat::Corrected,
        as_root: cb_cache::as_root(),
    };
    let mut server = Server::start(
        cache,
        &launch,
        pin(&profile.cache_pin),
        &at("server.log"),
        READY,
    )
    .map_err(|e| e.to_string())?;
    let pid = server.pid();
    println!("{cache} up in {:?} as pid {pid}", server.ready());

    let measured = fill_and_read(args, profile, plan, memtier, cache, pid, &at);
    // Asked before the stop, because after it the answer is no for the ordinary reason.
    let alive = server.alive().map_err(|e| e.to_string());

    // Stopped whether the fill worked or not, and a group that outlived it is a failure in its own right.
    let stopped = server.stop(STOP).map_err(|e| e.to_string());
    let (baseline, peak, processes) = measured?;
    // A server that died partway through the fill is holding a fraction of the keys, and a fraction divided by the whole count is a very good bytes-per-entry number.
    if !alive? {
        return Err(format!(
            "{cache} exited during its own measurement, so this peak belongs to a server holding some unknown part of {} entries",
            plan.entries
        ));
    }
    stopped?;

    Ok(Row {
        cache: cache.name().to_owned(),
        version,
        entries: plan.entries,
        peak_rss: peak,
        baseline_rss: baseline,
        payload_bytes: plan.payload(profile),
        processes,
        note: note(cache).to_owned(),
    })
}

/// The baseline, the fill, the settle and the peak.
///
/// Split out from [`one`] so that a failure in here still stops the server, rather than leaving one holding the cores because the measurement gave up early.
fn fill_and_read(
    args: &Args,
    profile: &Profile,
    plan: Plan,
    memtier: &Path,
    cache: CacheKind,
    pid: u32,
    at: &dyn Fn(&str) -> PathBuf,
) -> Result<(u64, u64, u32), String> {
    // Read before a single key goes in, so a reader can see which engines start large. An engine that reserves four gigabytes at startup has a peak that says very little about the keys in it.
    let before = cb_mem::group(pid).map_err(|e| e.to_string())?;

    // One operation per key and a key range the clients divide evenly, so what is left behind is `plan.entries` and not a number nobody knows.
    let mut shaped = profile.clone();
    shaped.operations = plan.per_client;
    shaped.key_maximum = plan.entries;

    let invocation = Invocation {
        profile: &shaped,
        pipeline: args.pipeline,
        pass: Pass::Sets,
        protocol: cache.protocol(),
        socket: &args.socket,
        json_out: &at("fill.json"),
    };
    let load = cb_memtier::run(
        memtier,
        &invocation,
        pin(&profile.bench_pin),
        &at("fill.log"),
        PASS,
    )
    .map_err(|e| e.to_string())?;
    println!("{cache} filled in {:?}", load.took);

    // Several of these do work after the writes stop, so reading now would measure the middle of a rehash rather than the end of one.
    std::thread::sleep(SETTLE);

    let after = cb_mem::group(pid).map_err(|e| e.to_string())?;
    if after.peak <= before.peak {
        return Err(format!(
            "{cache} peaked at {} holding no keys and {} holding {}, so either the fill did not reach it or its memory is not in its own process",
            human(before.peak),
            human(after.peak),
            plan.entries
        ));
    }
    Ok((before.peak, after.peak, after.processes))
}

/// The default entry count for a profile: about ten million, and something its clients divide.
///
/// Ten million is enough that the per-entry figure is about the data structure rather than about whatever an engine allocated at startup, and small enough that eight engines each fill in a few minutes.
///
/// Rounded down rather than up so the answer stays under the headroom check for a profile that was sized against ten million, and never below one client's worth, which is the smallest thing that can be divided at all.
fn about_ten_million(profile: &Profile) -> u64 {
    let clients = u64::from(profile.connections_per_thread) * u64::from(profile.bench_threads);
    if clients == 0 {
        // Plan::new refuses this with a better sentence than anything that could be said here.
        return 10_000_000;
    }
    (10_000_000 / clients).max(1) * clients
}

/// A limit the working set would reach turns this into an eviction benchmark.
///
/// Checked against the payload rather than against a guess at the overhead, and given a wide margin, because the thing being caught is a limit that is the wrong order of magnitude rather than one that is close.
fn check_headroom(payload: u64, profile: &Profile) -> Result<(), String> {
    let limit = profile.maxmemory.bytes();
    if limit == 0 {
        return Ok(());
    }
    // Twice the payload, because an engine with a hundred bytes of payload per key and fifty of overhead is ordinary and one that needs more than double is not an engine this measurement can distinguish from an engine that started evicting.
    if payload.saturating_mul(2) > limit {
        return Err(format!(
            "the keys and values alone are {}, and this profile limits each server to {}, so an engine would start evicting partway through and the count of what it holds would be wrong. Lower --entries or use a profile with a larger maxmemory.",
            human(payload),
            human(limit)
        ));
    }
    Ok(())
}

/// What a reader comparing the rows has to know about this engine.
///
/// Empty for the ones where a peak resident set is a consequence of the keys in it, which is most of them. The two that reserve memory up front get a sentence, because a number for those without one is a number that reads as waste and is a configuration.
const fn note(cache: CacheKind) -> &'static str {
    match cache {
        CacheKind::Garnet => {
            "Garnet sizes its index at startup, so part of this peak is a configuration rather than a consequence of the keys in it."
        }
        CacheKind::Dragonfly => {
            "Dragonfly preallocates per proactor, so part of this peak follows the thread count rather than the keys."
        }
        _ => "",
    }
}

/// Bytes, as something to read in a log line.
#[allow(
    clippy::cast_precision_loss,
    reason = "byte counts here are gigabytes at most, which a double holds exactly"
)]
fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::{CacheKind, Profiles};

    use super::{about_ten_million, check_headroom, human, note};

    fn profile() -> cb_core::Profile {
        let text = std::fs::read_to_string("../../profiles.toml").unwrap();
        Profiles::parse(&text)
            .unwrap()
            .get("wsl32")
            .unwrap()
            .clone()
    }

    // A fixed default cannot divide by every profile's client count, and a default that always fails is not a default.
    #[test]
    fn the_default_count_divides_by_the_clients_of_every_profile_there_is() {
        let text = std::fs::read_to_string("../../profiles.toml").unwrap();
        let all = Profiles::parse(&text).unwrap();
        for (name, profile) in &all.profiles {
            let entries = about_ten_million(profile);
            assert!(
                cb_mem::Plan::new(profile, entries).is_ok(),
                "{name} would refuse its own default of {entries}"
            );
            // Near enough ten million to be about the data structure rather than about whatever the engine allocated at startup.
            assert!(entries > 9_000_000, "{name} defaults to only {entries}");
            assert!(
                entries <= 10_000_000,
                "{name} defaults to {entries}, above the target it rounds down from"
            );
        }
    }

    #[test]
    fn a_working_set_that_fits_is_allowed() {
        let p = profile();
        assert!(check_headroom(1_000_000_000, &p).is_ok());
    }

    // The failure this catches produces a very good bytes-per-entry number rather than an error, because an engine that evicted is an engine holding fewer keys than the count it is divided by.
    #[test]
    fn a_working_set_that_would_be_evicted_says_what_to_change() {
        let p = profile();
        let err = check_headroom(p.maxmemory.bytes(), &p).unwrap_err();
        assert!(err.contains("--entries"), "{err}");
        assert!(err.contains("evicting"), "{err}");
    }

    // The two engines that reserve memory up front are the two where a peak resident set is partly a configuration, and a row for those without a sentence saying so reads as waste.
    #[test]
    fn the_engines_that_preallocate_carry_a_note_and_the_others_do_not() {
        assert!(!note(CacheKind::Garnet).is_empty());
        assert!(!note(CacheKind::Dragonfly).is_empty());
        for plain in [
            CacheKind::Redis,
            CacheKind::Valkey,
            CacheKind::Memcache,
            CacheKind::Pogocache,
            CacheKind::Yo,
            CacheKind::Rugo,
        ] {
            assert!(note(plain).is_empty(), "{plain} should have no note");
        }
    }

    #[test]
    fn bytes_read_as_something_a_person_can_read() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1024), "1.0 KB");
        assert_eq!(human(1_610_612_736), "1.5 GB");
    }
}
