//! `cache-bench docs`, which writes the documents that go with a results directory.
//!
//! Three of them. The two chart indexes, and the README that carries the methodology, the hardware, the versions and the caveat. They come out of one command so that a results directory is either regenerated whole or not at all, and `--check` is what CI runs to fail a build where one of them was edited by hand.
//!
//! What goes in a document is decided by what is on disk. The generator is handed the list of charts actually sitting in `graphs`, so a sweep that ran on a machine with no hardware counters produces documents that say the cycles charts are missing instead of ones with eight broken images in them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cb_chart::{Scale, Spec};
use cb_core::{Compat, Machine, Output, Profile, Profiles};
use cb_docs::{Index, Readme};

/// What to generate and where to put it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory, the one holding `graphs`, `output.json` and `host.json`.
    #[arg(long, value_name = "PATH", conflicts_with = "golden")]
    dir: Option<PathBuf>,
    /// Generate the chart indexes as if every chart the spec names had been drawn, which needs no measurements. There is no README in this mode, because there is no host to describe.
    #[arg(long)]
    golden: bool,
    /// Where the documents go. Defaults to the results directory.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
    /// Write nothing and fail if what is on disk is not what would be written.
    #[arg(long)]
    check: bool,
    /// Where to read the profiles from.
    #[arg(long, value_name = "PATH", default_value = "profiles.toml")]
    profiles: PathBuf,
    /// A sentence explaining why a chart might be missing on this host, shown under any index section that is short of one.
    #[arg(long, value_name = "TEXT", default_value = "")]
    absent: String,
    /// Which statistics produced the numbers, which the README says out loud.
    #[arg(long, default_value_t = Compat::Corrected, value_name = "MODE")]
    compat: Compat,
}

/// Generate them.
///
/// # Errors
///
/// If the results directory will not read, if the documents will not write, or if `--check` was asked for and a document on disk is not the one that would be written.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let have = present(args)?;
    let out = destination(args)?;

    let mut wanted: Vec<(PathBuf, String)> = Vec::new();
    for scale in Scale::ALL {
        let index = Index {
            scale,
            have: &have,
            absent: &args.absent,
        };
        wanted.push((out.join(index.file()), index.render()));
    }
    if let Some(dir) = &args.dir {
        let (machine, profile, versions) = about(dir, &args.profiles)?;
        publishable(&profile, &machine.profile)?;
        let memory = memory(dir)?;
        let readme = Readme {
            machine: &machine,
            profile: &profile,
            versions: &versions,
            have: &have,
            compat: args.compat,
            memory: &memory.rows,
        };
        wanted.push((out.join(readme.file()), readme.render()));
    }

    if args.check {
        return compare(&out, &wanted);
    }
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (path, text) in &wanted {
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("docs      {} written", path.display());
    }
    Ok(())
}

/// Fail on any document that is not what would be generated.
fn compare(out: &Path, wanted: &[(PathBuf, String)]) -> Result<(), String> {
    let mut stale = Vec::new();
    for (path, text) in wanted {
        if fs::read_to_string(path).ok().as_deref() != Some(text.as_str()) {
            println!("stale     {}", path.display());
            stale.push(path);
        }
    }
    if !stale.is_empty() {
        return Err(format!(
            "{} of {} documents are not what would be generated, rerun cache-bench docs",
            stale.len(),
            wanted.len()
        ));
    }
    println!(
        "docs      {} documents up to date in {}",
        wanted.len(),
        out.display()
    );
    Ok(())
}

/// What the results directory says about itself: the host, the profile it ran, and the version of every engine in it.
fn about(
    dir: &Path,
    profiles: &Path,
) -> Result<(Machine, Profile, BTreeMap<String, String>), String> {
    let path = dir.join("host.json");
    let text = fs::read_to_string(&path).map_err(|e| {
        format!(
            "{}: {e}, which is what doctor writes before a sweep",
            path.display()
        )
    })?;
    let machine = Machine::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let text = fs::read_to_string(profiles).map_err(|e| format!("{}: {e}", profiles.display()))?;
    let profile = Profiles::parse(&text)
        .map_err(|e| format!("{}: {e}", profiles.display()))?
        .get(&machine.profile)
        .map_err(|e| format!("{}: {e}", profiles.display()))?
        .clone();

    Ok((machine, profile, versions(dir)?))
}

/// Refuse a results README from a profile whose numbers were never meant to leave the machine.
///
/// A README is where a measurement stops being a note to oneself and becomes a claim: it names the hardware, states the method, and reads as the answer to how these engines compare. A profile marked `publishable = false` describes a box too small or too shared to answer that, so the sweep is worth running often and the document is worth refusing every time.
///
/// The refusal is the whole command rather than the README alone. The chart indexes would generate happily, and a directory holding indexes and charts but no README is a directory that looks like somebody deleted the caveat.
fn publishable(profile: &Profile, name: &str) -> Result<(), String> {
    if profile.publishable {
        return Ok(());
    }
    Err(format!(
        "the {name} profile is marked publishable = false, so a sweep run under it has no results README. Its numbers answer whether a change helped on the machine that ran it, which is not the same question as how these engines compare, and this is the command where the difference stops being visible. Sweep {name} as often as is useful and publish from a profile that describes a machine the numbers are meant to come from."
    ))
}

/// One version line per engine, read out of the results rather than out of a list somebody keeps.
///
/// An engine that reports two different versions inside one results directory means the sweep was rerun against a rebuilt server, which makes the two halves of a chart incomparable, so it is refused rather than reported as whichever one was read last.
fn versions(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = dir.join("output.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let output = Output::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for entry in &output.entries {
        let info = &entry.data.info;
        if let Some(seen) = out.get(&info.cache) {
            if seen != &info.version {
                return Err(format!(
                    "{} has {} at two versions, {seen:?} and {:?}",
                    path.display(),
                    info.cache,
                    info.version
                ));
            }
            continue;
        }
        out.insert(info.cache.clone(), info.version.clone());
    }
    Ok(out)
}

/// The charts that are there to be linked.
fn present(args: &Args) -> Result<BTreeSet<String>, String> {
    if args.golden {
        return Ok(Spec::all().into_iter().map(Spec::file).collect());
    }
    let dir = args
        .dir
        .as_ref()
        .ok_or("pass --dir a results directory, or --golden to generate as if it were complete")?;
    charts(&dir.join("graphs"))
}

/// Every PNG in a graphs directory, by name.
fn charts(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let listing = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in listing {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            out.insert(name);
        }
    }
    Ok(out)
}

/// Where the documents go.
fn destination(args: &Args) -> Result<PathBuf, String> {
    if let Some(dir) = &args.out {
        return Ok(dir.clone());
    }
    args.dir
        .clone()
        .ok_or_else(|| "pass --out somewhere to put the documents".to_owned())
}

/// What `cache-bench mem` left in this directory, if it was ever run against it.
///
/// A directory with no `memory.json` is every directory produced before that command existed, so it is an empty measurement rather than an error, and the README leaves the section out. A file that is there and will not parse is an error, because that is a measurement somebody made and this would otherwise publish a results directory silently missing it.
fn memory(dir: &Path) -> Result<cb_mem::Report, String> {
    let path = dir.join("memory.json");
    match fs::read_to_string(&path) {
        Ok(text) => cb_mem::Report::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(cb_mem::Report::default()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::Profiles;

    use super::publishable;

    fn profiles() -> Profiles {
        let text = std::fs::read_to_string("../../profiles.toml").unwrap();
        Profiles::parse(&text).unwrap()
    }

    #[test]
    fn a_profile_that_describes_a_bench_host_generates_a_readme() {
        let all = profiles();
        for name in ["epyc8", "wsl32", "reference"] {
            assert_eq!(publishable(all.get(name).unwrap(), name), Ok(()), "{name}");
        }
    }

    // The point of the flag. A fast loop exists to answer whether a change helped, and the box it runs on is too small to answer how these engines compare, so the document that reads as the second answer is refused.
    #[test]
    fn a_fast_loop_profile_is_refused_and_says_what_to_sweep_instead() {
        let all = profiles();
        let err = publishable(all.get("smoke").unwrap(), "smoke").unwrap_err();
        assert!(err.contains("publishable = false"), "{err}");
        assert!(err.contains("smoke"), "{err}");
    }

    // Every profile written before the field existed describes a machine whose numbers were meant to be published, so the absent case has to be the permissive one or those directories stop regenerating.
    #[test]
    fn a_profile_that_does_not_mention_it_is_publishable() {
        let text = "\
[profiles.quiet]
description = \"a profile written before the flag existed\"
cores = 4
cache_pin = \"0-1\"
bench_pin = \"2-3\"
threads = [1]
bench_threads = 2
connections_per_thread = 16
operations = 10000
size_range = \"1-1024\"
key_maximum = 250000
maxmemory = \"1gb\"
pipelines = [1]
runs = 3
perf = [\"no\"]
";
        let all = Profiles::parse(text).unwrap();
        assert_eq!(publishable(all.get("quiet").unwrap(), "quiet"), Ok(()));
    }
}
