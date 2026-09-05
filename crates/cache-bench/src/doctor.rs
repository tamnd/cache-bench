//! `cache-bench doctor`, which is the thing you run before starting a sweep that takes days.
//!
//! Two halves. The first reads files: that the config names every binary the sweep runs, that every profile would measure what it says it measures, and that every host names a profile that exists. `--files-only` stops there, which is what CI runs, because CI has no PMU and no memtier and is not going to sweep anything.
//!
//! The second half asks the machine. Cores, the split between the cache half and the load generator half, memory against what the profile is going to ask for, the PMU, the load average, and the load generator's own version. Where a check fails it refuses rather than warning, because a warning printed before a sweep that runs for eight days is a warning nobody is sitting there to read, and the failure it was warning about produces numbers rather than an error.
//!
//! `--write` puts what it found in `host.json` next to the results, which is the record of what a results directory was measured on. `--deep` starts each of the seven servers in turn and stops them again, which is the check that the machine can actually run the thing.
//!
//! Everything here is a check somebody would otherwise make by starting a sweep on a machine that is not this one and coming back in two days.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_core::{
    Arch, CacheKind, Config, Endpoint, Hosts, Launch, Machine, Pmu, Profile, Profiles, Tool,
};

use crate::host::Host;

/// What to read, and how much of it to check.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Where the compiled binaries are.
    #[arg(long, default_value = "config.jsonc", value_name = "PATH")]
    config: PathBuf,
    /// The machine shapes and the sweep that fits each one.
    #[arg(long, default_value = "profiles.toml", value_name = "PATH")]
    profiles: PathBuf,
    /// Where sweeps run. Absent means here, which is the normal case.
    #[arg(long, default_value = "hosts.toml", value_name = "PATH")]
    hosts: PathBuf,
    /// Print one profile in full rather than a line for each.
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
    /// Read the files and stop, without touching the machine.
    #[arg(long)]
    files_only: bool,
    /// Start each server in turn and stop it again, which is the only check that says the machine can run them.
    #[arg(long)]
    deep: bool,
    /// Write what the machine says about itself to `host.json` in this results directory.
    #[arg(long, value_name = "DIR")]
    write: Option<PathBuf>,
    /// The unix socket `--deep` starts each server on.
    #[arg(long, default_value = "/tmp/cachebench.sock", value_name = "PATH")]
    socket: PathBuf,
}

/// How long a server gets to answer during `--deep`.
const READY: Duration = Duration::from_secs(60);

/// How long it gets to stop again.
const STOP: Duration = Duration::from_secs(10);

/// The load average above which a machine is busy with something that is not this.
///
/// One, which on any of these machines is a single core doing something. It sounds strict for a box with 32 of them, and it is meant to: the cores this sweep pins to are named in the profile, and a process on one of those is a competitor for the exact core the server is on rather than background load spread over the machine.
const BUSY: f64 = 1.0;

/// Run the checks and print what they found.
///
/// # Errors
///
/// On the first thing that would stop a sweep. The message says which file and what about it, because the alternative is a sweep that fails on a machine you are not sitting at.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let config = read(&args.config)?;
    let config = Config::parse(&config, Arch::host()).map_err(|e| why(&args.config, &e))?;
    let mut missing = Vec::new();
    if config.memtier().is_err() {
        missing.push("memtier".to_owned());
    }
    for kind in CacheKind::ALL {
        if config.binary(kind).is_err() {
            missing.push(kind.name().to_owned());
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "{} has no path for {}, and everything the sweep runs has to be named in it",
            args.config.display(),
            missing.join(", ")
        ));
    }
    let paths = config.names().count();
    println!("config    {paths} paths, every server and the load generator");

    let profiles = read(&args.profiles)?;
    let profiles = Profiles::parse(&profiles).map_err(|e| why(&args.profiles, &e))?;
    let count = profiles.profiles.len();
    println!("profiles  {count} usable");

    // Absent is not a failure. It is the normal case, and it means this machine.
    let mut named = Vec::new();
    if args.hosts.exists() {
        let hosts = read(&args.hosts)?;
        let hosts = Hosts::parse(&hosts).map_err(|e| why(&args.hosts, &e))?;
        hosts
            .check_against(&profiles)
            .map_err(|e| why(&args.hosts, &e))?;
        let local = hosts.hosts.values().filter(|h| h.is_local()).count();
        println!(
            "hosts     {} named, {local} of them this machine",
            hosts.hosts.len()
        );
        // Kept for the anonymity check on anything about to be written into a results directory.
        for (name, host) in &hosts.hosts {
            named.push(name.clone());
            if let Some(ssh) = &host.ssh {
                named.push(ssh.clone());
            }
        }
    } else {
        println!("hosts     no hosts file, so a sweep runs here");
    }

    println!();
    match &args.profile {
        Some(name) => {
            let profile = profiles.get(name).map_err(|e| e.to_string())?;
            print_one(name, profile);
        }
        None => {
            for (name, profile) in &profiles.profiles {
                print_line(name, profile);
            }
        }
    }
    if args.files_only {
        return Ok(());
    }

    // The machine half. Everything below here is about the box this is running on rather than about the files.
    let host = crate::host::probe();
    let pmu = cb_perf::probe();
    let memtier = memtier(&config);
    println!();
    describe(&host, &pmu, memtier.as_deref());

    let Some(name) = &args.profile else {
        println!();
        println!(
            "checks    nothing checked against a profile, because none was named. Pass --profile to have this machine measured against the one it is going to sweep with."
        );
        return Ok(());
    };
    let profile = profiles.get(name).map_err(|e| e.to_string())?;
    check(profile, &host, &pmu, memtier.as_deref())?;
    println!();
    println!("checks    this machine can run {name}");

    if args.deep {
        deep(&config, profile, &args.socket)?;
    }
    if let Some(dir) = &args.write {
        let written = record(name, &host, &pmu, memtier.as_deref())?;
        written
            .check_anonymous(&named.iter().map(String::as_str).collect::<Vec<_>>())
            .map_err(|e| e.to_string())?;
        let path = dir.join("host.json");
        crate::results::write(&path, &written.emit())?;
        println!("host.json {}", path.display());
    }
    Ok(())
}

/// The load generator's own version line, which is the one version that is not in any result file.
fn memtier(config: &Config) -> Option<String> {
    let binary = config.memtier().ok()?;
    let capture =
        std::env::temp_dir().join(format!("cache-bench-memtier-{}.txt", std::process::id()));
    let said = cb_cache::version(binary, &capture).ok();
    let _ = std::fs::remove_file(&capture);
    said
}

/// What the machine said about itself, printed the way the file section above is printed.
fn describe(host: &Host, pmu: &cb_perf::Probe, memtier: Option<&str>) {
    let unknown = "not published by this machine";
    println!("kernel    {}", host.kernel.as_deref().unwrap_or(unknown));
    println!("distro    {}", host.distro.as_deref().unwrap_or(unknown));
    println!(
        "cpu       {}, {} logical",
        host.cpu_model.as_deref().unwrap_or(unknown),
        host.cpus
            .map_or_else(|| "an unknown number of".to_owned(), |n| n.to_string())
    );
    println!(
        "memory    {} total, {} available",
        show(host.memory),
        show(host.available)
    );
    println!("governor  {}", host.governor.as_deref().unwrap_or(unknown));
    println!(
        "mitigate  {}",
        host.mitigations.as_deref().unwrap_or(unknown)
    );
    println!(
        "pmu       {}",
        if pmu.counted {
            "yes, a live counter answered".to_owned()
        } else {
            format!("no, {}", pmu.reason)
        }
    );
    println!(
        "memtier   {}",
        memtier.unwrap_or("did not answer --version")
    );
    println!(
        "load      {}",
        host.load
            .map_or_else(|| unknown.to_owned(), |load| format!("{load:.2}"))
    );
}

/// A byte count, or a note that the machine does not publish it.
fn show(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "unknown".to_owned(), |n| cb_core::Bytes(n).short())
}

/// Everything that would make this machine the wrong one to run this profile on.
///
/// Each of these is refused rather than warned about. A warning printed at the start of a job that runs for eight days is read by nobody, and every one of these failures produces numbers rather than an error.
fn check(
    profile: &Profile,
    host: &Host,
    pmu: &cb_perf::Probe,
    memtier: Option<&str>,
) -> Result<(), String> {
    crate::run::check_machine(profile, host.cpus)?;
    if memtier.is_none() {
        return Err(
            "the load generator did not answer --version, so the sweep would fail on its first run rather than at the end of this check".to_owned(),
        );
    }
    if profile.perf.contains(&cb_core::PerfMode::Yes) && !pmu.counted {
        return Err(format!(
            "this profile sweeps the cycles half of the matrix and {}, so use a profile whose perf list is no only, or fix the machine",
            pmu.reason
        ));
    }
    // The working set is what memtier actually puts in the store, and it is what has to fit in memory. maxmemory is the ceiling above it, and a ceiling higher than the machine has is fine, because nothing ever reaches it.
    if let Some(available) = host.available {
        let needed = profile.working_set().bytes().saturating_mul(2);
        if available < needed {
            return Err(format!(
                "this profile's working set is about {}, which wants {} of headroom, and this machine has {} available, so the warmup would push the server into swap and the measured pass would be a measurement of the disk",
                cb_core::Bytes(profile.working_set().bytes()).short(),
                cb_core::Bytes(needed).short(),
                cb_core::Bytes(available).short()
            ));
        }
    }
    if let Some(load) = host.load
        && load > BUSY
    {
        return Err(format!(
            "the load average is {load:.2} and this wants it at or below {BUSY:.2}, because something else on a pinned core is a competitor for that core rather than background noise. Wait for the machine to go quiet and ask again."
        ));
    }
    Ok(())
}

/// Start each of the seven in turn, wait for it to answer, stop it, and check nothing survived.
///
/// This is the check that cannot be made from a file. A path in the config that points at a binary of the wrong architecture, a server built without unix socket support, a Garnet whose .NET runtime is not installed: all of them read as a correct config and all of them fail on the first run of a sweep.
fn deep(config: &Config, profile: &Profile, socket: &Path) -> Result<(), String> {
    println!();
    let logs = std::env::temp_dir().join(format!("cache-bench-deep-{}", std::process::id()));
    std::fs::create_dir_all(&logs).map_err(|e| format!("cannot make {}: {e}", logs.display()))?;
    for kind in CacheKind::ALL {
        let binary = config.binary(kind).map_err(|e| e.to_string())?;
        let launch = Launch {
            binary,
            threads: 1,
            maxmemory: profile.maxmemory,
            endpoint: Endpoint::Unix(socket),
            compat: cb_core::Compat::Corrected,
            as_root: cb_cache::as_root(),
        };
        let log = logs.join(format!("{}.log", kind.name()));
        let server = cb_cache::Server::start(kind, &launch, None, &log, READY).map_err(|e| {
            format!(
                "{kind} did not come up, and its output is in {}: {e}",
                log.display()
            )
        })?;
        let ready = server.ready();
        server
            .stop(STOP)
            .map_err(|e| format!("{kind} came up and would not go away again: {e}"))?;
        println!(
            "{:10} up in {ready:?}, answered, and stopped clean",
            kind.name()
        );
    }
    let _ = std::fs::remove_dir_all(&logs);
    Ok(())
}

/// What goes in `host.json`.
///
/// A fact this machine does not publish is refused rather than written as `unknown`. This file is the whole of what a published results directory says about where its numbers came from, and a reader cannot tell a field that was never asked from a machine that would not answer.
fn record(
    name: &str,
    host: &Host,
    pmu: &cb_perf::Probe,
    memtier: Option<&str>,
) -> Result<Machine, String> {
    let need = |what: &str, value: Option<String>| {
        value.ok_or_else(|| {
            format!(
                "this machine does not publish {what}, and a results directory that cannot say what measured it is the thing host.json exists to fix"
            )
        })
    };
    Ok(Machine {
        profile: name.to_owned(),
        kernel: need("its kernel version", host.kernel.clone())?,
        distro: need("which distribution it is", host.distro.clone())?,
        cpu_model: need("what its CPU is", host.cpu_model.clone())?,
        cpus: host
            .cpus
            .ok_or("this machine does not say how many CPUs it has")?,
        memory_bytes: host
            .memory
            .ok_or("this machine does not say how much memory it has")?,
        pmu: if pmu.counted {
            Pmu::Present
        } else {
            Pmu::Absent
        },
        governor: need("its frequency governor", host.governor.clone())?,
        mitigations: need("its CPU mitigations", host.mitigations.clone())?,
        memtier: need("a load generator version", memtier.map(ToOwned::to_owned))?,
        cache_bench: Tool {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            git: commit(),
        },
        rustc: rustc(),
        // The sweep overwrites this with its own start, and doctor writes it so that a host.json is complete on its own.
        started: cb_core::now(),
        finished: None,
    })
}

/// The commit this binary was built from.
fn commit() -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok();
    out.filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "not built from a checkout".to_owned())
}

/// The compiler, for the one engine that is built from source here.
fn rustc() -> Option<String> {
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// One line for each profile, which is enough to see which one this machine is.
///
/// A profile that cannot be published from says so here rather than only at the `docs` that refuses it, since this listing is where somebody picks which one to give a machine to.
fn print_line(name: &str, profile: &cb_core::Profile) {
    println!(
        "{name:10} {} cores, cache {}, bench {}, {} runs{}",
        profile.cores,
        profile.cache_pin,
        profile.bench_pin,
        profile.total_runs(),
        if profile.publishable {
            ""
        } else {
            ", not for publishing"
        }
    );
}

/// The whole profile, for the one somebody is about to sweep with.
fn print_one(name: &str, profile: &cb_core::Profile) {
    let threads = profile
        .threads
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let pipelines = profile
        .pipelines
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let perf = profile
        .perf
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    println!("profile      {name}");
    println!("description  {}", profile.description);
    println!("cores        {}", profile.cores);
    println!("cache pin    {}", profile.cache_pin);
    println!("bench pin    {}", profile.bench_pin);
    println!("threads      {threads}");
    println!("pipelines    {pipelines}");
    println!("perf         {perf}");
    println!(
        "load         {} threads, {} connections",
        profile.bench_threads,
        profile.connections()
    );
    // The working set is a key count times a mean value size, so it is almost never a whole number of anything and prints as a byte count.
    // Whole mebibytes is the reading somebody wants next to a limit written as 32gb.
    println!(
        "memory       {} for a working set of about {}mb",
        profile.maxmemory,
        profile.working_set().mib()
    );
    println!(
        "sweep        {} runs, {} per cell",
        profile.total_runs(),
        profile.runs
    );
}

/// Read a file, saying which one when it is not there.
fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{} cannot be read: {e}", path.display()))
}

/// Put the filename in front of a parse failure, since the message on its own does not say which file it came from.
fn why(path: &Path, e: &dyn std::error::Error) -> String {
    format!("{}: {e}", path.display())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Args, Host, check, record, run};

    /// The repository root, which is where the three real files are.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn args() -> Args {
        Args {
            config: root().join("config.jsonc"),
            profiles: root().join("profiles.toml"),
            hosts: root().join("hosts.example.toml"),
            profile: None,
            files_only: true,
            deep: false,
            write: None,
            socket: PathBuf::from("/tmp/cache-bench-doctor.sock"),
        }
    }

    /// A machine with enough of everything, which each test then takes one thing away from.
    fn host() -> Host {
        Host {
            kernel: Some("Linux 6.8.0-45-generic x86_64".to_owned()),
            distro: Some("Ubuntu 24.04.3 LTS".to_owned()),
            cpu_model: Some("Neoverse-V2".to_owned()),
            cpus: Some(32),
            memory: Some(64 * 1024 * 1024 * 1024),
            available: Some(60 * 1024 * 1024 * 1024),
            governor: Some("performance".to_owned()),
            mitigations: Some("20 known, all of them mitigated".to_owned()),
            load: Some(0.04),
        }
    }

    /// A machine that can count cycles, and one that cannot.
    fn pmu(counted: bool) -> cb_perf::Probe {
        cb_perf::Probe {
            counted,
            reason: "this machine has no counters".to_owned(),
        }
    }

    fn profile(name: &str) -> cb_core::Profile {
        let text = std::fs::read_to_string(root().join("profiles.toml")).unwrap();
        cb_core::Profiles::parse(&text)
            .unwrap()
            .get(name)
            .unwrap()
            .clone()
    }

    // The files this repository ships have to pass its own doctor, which is the same check CI runs.
    #[test]
    fn our_own_files_pass() {
        run(&args()).unwrap();
    }

    #[test]
    fn a_machine_with_enough_of_everything_passes() {
        check(
            &profile("reference"),
            &host(),
            &pmu(true),
            Some("memtier 2.1.4"),
        )
        .unwrap();
    }

    // The failure this catches writes a result file full of missing counters, which reads the same as a machine where one counter happens to be unsupported.
    #[test]
    fn a_profile_that_sweeps_cycles_needs_a_machine_that_counts_them() {
        let why = check(
            &profile("reference"),
            &host(),
            &pmu(false),
            Some("memtier 2.1.4"),
        )
        .unwrap_err();
        assert!(why.contains("no counters"), "{why}");
        // The same machine is fine for a profile that does not ask for them, which is what wsl32 is.
        check(
            &profile("wsl32"),
            &host(),
            &pmu(false),
            Some("memtier 2.1.4"),
        )
        .unwrap();
    }

    // A working set that does not fit is a sweep that measures the disk, and it produces a full set of numbers while it does it.
    #[test]
    fn a_machine_that_would_swap_is_refused() {
        let mut host = host();
        host.available = Some(1024 * 1024 * 1024);
        let why = check(
            &profile("reference"),
            &host,
            &pmu(true),
            Some("memtier 2.1.4"),
        )
        .unwrap_err();
        assert!(why.contains("working set"), "{why}");
    }

    #[test]
    fn a_busy_machine_is_refused_rather_than_warned_about() {
        let mut host = host();
        host.load = Some(3.5);
        let why = check(
            &profile("reference"),
            &host,
            &pmu(true),
            Some("memtier 2.1.4"),
        )
        .unwrap_err();
        assert!(why.contains("load average is 3.50"), "{why}");
    }

    #[test]
    fn a_load_generator_that_does_not_answer_is_refused() {
        let why = check(&profile("reference"), &host(), &pmu(true), None).unwrap_err();
        assert!(why.contains("load generator"), "{why}");
    }

    // host.json is the whole of what a published results directory says about where its numbers came from, and a reader cannot tell a field nobody asked for from a machine that would not answer.
    #[test]
    fn a_host_record_is_not_written_with_facts_the_machine_did_not_give() {
        let mut quiet = host();
        quiet.governor = None;
        let why = record("reference", &quiet, &pmu(true), Some("memtier 2.1.4")).unwrap_err();
        assert!(why.contains("governor"), "{why}");
        let written = record("reference", &host(), &pmu(true), Some("memtier 2.1.4")).unwrap();
        assert_eq!(written.cpus, 32);
        assert_eq!(written.pmu, cb_core::Pmu::Present);
        // Nothing in it names the machine, which is checked here and again before it is written.
        written.check_anonymous(&["server1", "gamingpc"]).unwrap();
    }

    #[test]
    fn a_profile_by_name_prints_and_a_name_that_is_not_there_does_not() {
        let mut args = args();
        args.profile = Some("reference".to_owned());
        run(&args).unwrap();
        args.profile = Some("laptop".to_owned());
        assert!(run(&args).unwrap_err().contains("laptop"));
    }

    // A server added to CacheKind and not to the config is the mistake this catches, and the message has to name the server rather than say the file is wrong.
    #[test]
    fn a_config_missing_a_server_says_which() {
        let text = std::fs::read_to_string(root().join("config.jsonc")).unwrap();
        let (before, after) = text.split_once("\"garnet\"").unwrap();
        let path = std::env::temp_dir().join("cache-bench-doctor-no-garnet.jsonc");
        std::fs::write(&path, format!("{before}\"gornet\"{after}")).unwrap();
        let mut args = args();
        args.config = path;
        let err = run(&args).unwrap_err();
        assert!(err.contains("garnet"), "{err}");
    }

    // Absent is the normal case and has to be a pass rather than a missing file error.
    #[test]
    fn no_hosts_file_is_fine() {
        let mut args = args();
        args.hosts = root().join("there-is-no-such-file.toml");
        run(&args).unwrap();
    }

    #[test]
    fn a_file_that_is_not_there_says_which_one() {
        let mut args = args();
        args.profiles = root().join("nope.toml");
        assert!(run(&args).unwrap_err().contains("nope.toml"));
    }
}
