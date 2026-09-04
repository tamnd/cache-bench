//! `cache-bench doctor`, which is the thing you run before starting a sweep that takes days.
//!
//! This is the half of it that reads files. It checks that the config names every binary the sweep runs, that every profile in the file would measure what it says it measures, and that every host names a profile that exists.
//! The other half probes the machine, and that lands with the runner. Until then `--files-only` and the default do the same work, and the flag exists so that the line in CI does not have to change on the day the other half arrives.
//!
//! Everything here is a check somebody would otherwise make by starting a sweep on a machine that is not this one and coming back in two days.

use std::path::{Path, PathBuf};

use cb_core::{Arch, CacheKind, Config, Hosts, Profiles};

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
}

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
    } else {
        println!("hosts     no hosts file, so a sweep runs here");
    }

    if !args.files_only {
        println!("host      not probed, the machine half of doctor lands with the runner");
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
    Ok(())
}

/// One line for each profile, which is enough to see which one this machine is.
fn print_line(name: &str, profile: &cb_core::Profile) {
    println!(
        "{name:10} {} cores, cache {}, bench {}, {} runs",
        profile.cores,
        profile.cache_pin,
        profile.bench_pin,
        profile.total_runs()
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

    use super::{Args, run};

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
        }
    }

    // The files this repository ships have to pass its own doctor, which is the same check CI runs.
    #[test]
    fn our_own_files_pass() {
        run(&args()).unwrap();
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
