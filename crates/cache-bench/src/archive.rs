//! `cache-bench archive`, which packs what a sweep measured into one file that can be attached to a release.
//!
//! A published results directory holds the charts, the combined `output.json` and the host description, and those are small enough to live in git forever.
//! What is underneath them is not. A reference sweep is 23808 run files and about five times that many load generator and server logs, and tens of thousands of small files in a repository is a tax on every clone anybody ever makes of it.
//! They are still the evidence, though, and evidence that only exists on the machine that produced it is evidence for as long as that machine lasts, which on a desktop box somebody else also uses is not long.
//!
//! So they ship, as one file on a release rather than as thousands in the tree. This makes that file, and reads it back.
//!
//! The archive is a gzipped tar of `runs`, `logs` and the small files beside them, with a `MANIFEST` in it holding the SHA-256 and the byte count of every member.
//! `--check` walks an archive and recomputes every one of those digests, which is a different question from whether the download finished: a checksum over the whole file says the bytes arrived, and this says the archive holds the number of runs it claims and that each of them is the file it says it is.
//! Given `--dir` as well, it also asks whether every chosen file named in that directory's `output.json` is in the archive, which is the link between the asset and the charts that were drawn from it.
//!
//! Nothing is held in memory except one file at a time. A sweep's logs run to gigabytes and the point of this is to move them somewhere, not to require a machine that can hold them.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use cb_core::Output;

use crate::chart::digest;
use crate::results;

/// The file inside the archive that lists what the archive holds.
const MANIFEST: &str = "MANIFEST";

/// The directories under a results directory that hold what was measured.
///
/// `runs` is the reduced numbers, one file per measurement, and `logs` is what the load generator and the server actually printed while that measurement was taken. A run file is derived from the pair of load generator files beside it, so `logs` is the layer underneath and it is the layer somebody arguing with a number ends up in.
const UNDER: [&str; 2] = ["runs", "logs"];

/// The small files that sit beside those directories and are worth carrying with them.
///
/// `output.json` is in the list even though it is also committed, so that an archive can be checked by somebody who has only the archive.
const BESIDE: [&str; 3] = ["host.json", "failures.json", "output.json"];

/// What to pack, or what to read back.
#[derive(Debug, clap::Args)]
#[command(group(
    clap::ArgGroup::new("what").required(true).args(["out", "check"])
))]
pub(crate) struct Args {
    /// The results directory, which is the one holding `runs`.
    #[arg(long, value_name = "PATH")]
    dir: Option<PathBuf>,
    /// Write the archive here, as a gzipped tar.
    #[arg(long, value_name = "PATH", conflicts_with = "check")]
    out: Option<PathBuf>,
    /// Read an archive back and check every file in it against the manifest inside it.
    #[arg(long, value_name = "PATH")]
    check: Option<PathBuf>,
}

/// Pack a directory, or check an archive.
///
/// # Errors
///
/// If the directory cannot be read, if the archive cannot be written or read, or if a check finds a file that is missing, extra or altered.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    if let Some(path) = &args.check {
        return check(path, args.dir.as_deref());
    }
    let (Some(dir), Some(out)) = (&args.dir, &args.out) else {
        return Err("packing needs both --dir and --out".to_owned());
    };
    pack(dir, out)
}

/// One member of the archive, after it has been written and before the manifest is.
#[derive(Debug)]
struct Listed {
    /// Its name inside the archive, which is relative to the top directory.
    name: String,
    /// Its SHA-256 as lower case hex.
    hash: String,
    /// How many bytes it is.
    len: u64,
}

/// Write the archive.
///
/// The manifest goes in last rather than first, because writing it first would mean holding every file's digest before the first byte of the archive is written, and that means holding every file.
/// A tar is read from the front to the back either way, and `--check` gathers what it finds before it looks for the manifest, so the order inside the file is not something a reader has to care about.
fn pack(dir: &Path, out: &Path) -> Result<(), String> {
    let files = paths(dir)?;
    let runs = under(&files, "runs");
    if runs == 0 {
        return Err(format!(
            "{} holds no run files, so there is nothing here worth keeping out of git",
            results::runs_dir(dir).display()
        ));
    }

    let top = top_name(dir);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);

    let mut listed = Vec::with_capacity(files.len());
    for (name, path) in &files {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = u64::try_from(bytes.len())
            .map_err(|e| format!("{name} is too large to record: {e}"))?;
        listed.push(Listed {
            name: name.clone(),
            hash: digest(&bytes),
            len,
        });
        add(&mut builder, &format!("{top}/{name}"), &bytes)?;
    }
    let manifest = render(&listed, &top);
    add(
        &mut builder,
        &format!("{top}/{MANIFEST}"),
        manifest.as_bytes(),
    )?;

    let gz = builder
        .into_inner()
        .map_err(|e| format!("{}: {e}", out.display()))?;
    gz.finish().map_err(|e| format!("{}: {e}", out.display()))?;

    let bytes = fs::read(out).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "archive   {} files into {}, {runs} runs and {} logs",
        listed.len() + 1,
        out.display(),
        under(&files, "logs")
    );
    println!("{}  {}", digest(&bytes), name_of(out));
    Ok(())
}

/// Read an archive back and check it against the manifest inside it.
///
/// The digests are recomputed as the archive is walked rather than after extracting it, so a check costs one pass, one file of memory and no disk.
fn check(path: &Path, dir: Option<&Path>) -> Result<(), String> {
    let mut found: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut manifest: Option<String> = None;

    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("{}: {e}", path.display()))?;
        let name = entry
            .path()
            .map_err(|e| format!("{}: {e}", path.display()))?
            .to_string_lossy()
            .into_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // A directory arrives as a member of no length, and a tar written by something other than this may or may not carry them.
        if name.ends_with('/') {
            continue;
        }
        let Some((_, inner)) = name.split_once('/') else {
            return Err(format!(
                "{} holds {name} at the top rather than under one directory, so it is not an archive this wrote",
                path.display()
            ));
        };
        if inner == MANIFEST {
            manifest = Some(String::from_utf8_lossy(&bytes).into_owned());
            continue;
        }
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        found.insert(inner.to_owned(), (digest(&bytes), len));
    }

    let manifest = manifest.ok_or_else(|| {
        format!(
            "{} has no {MANIFEST} in it, so there is nothing to check it against",
            path.display()
        )
    })?;
    let wanted = parse(&manifest)?;
    let wrong = compare(&wanted, &found);
    if wrong > 0 {
        return Err(format!(
            "{wrong} files in {} do not match its own {MANIFEST}",
            path.display()
        ));
    }
    println!(
        "archive   {} files in {} match its {MANIFEST}",
        wanted.len(),
        path.display()
    );

    match dir {
        Some(dir) => against(dir, &found),
        None => Ok(()),
    }
}

/// Say what the archive holds that the manifest does not, and the other way round.
fn compare(
    wanted: &BTreeMap<String, (String, u64)>,
    found: &BTreeMap<String, (String, u64)>,
) -> usize {
    let mut wrong = 0_usize;
    for (name, (hash, len)) in wanted {
        match found.get(name) {
            Some((got, _)) if got == hash => {}
            Some((_, got)) => {
                println!("differs   {name}, {got} bytes against {len}");
                wrong += 1;
            }
            None => {
                println!("missing   {name}");
                wrong += 1;
            }
        }
    }
    for name in found.keys() {
        if !wanted.contains_key(name) {
            println!("extra     {name}");
            wrong += 1;
        }
    }
    wrong
}

/// Ask whether the archive holds the files a published directory's `output.json` was built out of.
///
/// By name and not by content. The entries in `output.json` are the chosen files re-emitted with the whole file's indentation, so comparing them byte for byte would be comparing two renderings rather than two measurements, and the digests above have already said the archived file is intact.
/// What this answers is the question somebody downloading the asset actually has, which is whether it belongs to the charts they are looking at.
fn against(dir: &Path, found: &BTreeMap<String, (String, u64)>) -> Result<(), String> {
    let path = dir.join("output.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let output = Output::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut absent = 0_usize;
    for entry in &output.entries {
        if !found.contains_key(&format!("runs/{}", entry.file)) {
            println!("not in the archive   {}", entry.file);
            absent += 1;
        }
    }
    if absent > 0 {
        return Err(format!(
            "{absent} of the {} chosen files {} was built from are not in the archive",
            output.entries.len(),
            path.display()
        ));
    }
    println!(
        "against   all {} chosen files behind {} are in it",
        output.entries.len(),
        path.display()
    );
    Ok(())
}

/// Everything worth packing, as a name inside the archive and a path on disk, in name order.
///
/// Names and not contents, because a sweep's logs are larger than the memory of the machine that took them.
fn paths(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut found = Vec::new();
    for name in BESIDE {
        let path = dir.join(name);
        if path.is_file() {
            found.push((name.to_owned(), path));
        }
    }

    for under in UNDER {
        let at = dir.join(under);
        // `logs` is not there on a directory somebody has tidied, and `runs` not being there is caught by the caller with a better sentence than this could give.
        if !at.is_dir() {
            continue;
        }
        let listing = fs::read_dir(&at).map_err(|e| format!("{}: {e}", at.display()))?;
        for entry in listing {
            let path = entry.map_err(|e| format!("{}: {e}", at.display()))?.path();
            if path.is_file() {
                found.push((format!("{under}/{}", name_of(&path)), path));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

/// How many of the files are under one of the two directories.
fn under(files: &[(String, PathBuf)], name: &str) -> usize {
    let prefix = format!("{name}/");
    files.iter().filter(|f| f.0.starts_with(&prefix)).count()
}

/// Put one member in, with a header that says nothing about the machine it was packed on.
///
/// No owner, no group, no mode beyond readable, and a modified time of zero. A tar header carries a uid and a username by default, and the point of `host.json` being anonymous is undone by a release asset with somebody's login in every one of a hundred thousand headers.
fn add<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let size =
        u64::try_from(bytes.len()).map_err(|e| format!("{name} is too large to record: {e}"))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    builder
        .append_data(&mut header, name, bytes)
        .map_err(|e| format!("{name}: {e}"))
}

/// The manifest, which is what `sha256sum -c` reads with the byte count added.
fn render(listed: &[Listed], top: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str(
        "What one sweep measured: the run files, the logs under them and the small files beside them.\n",
    );
    let _ = writeln!(out, "Directory {top}, {} files.\n", listed.len());
    out.push_str("Each line below is the SHA-256, the byte count and the name, and `cache-bench archive --check` is what reads them back.\n\n");
    for one in listed {
        let _ = writeln!(out, "{}  {}  {}", one.hash, one.len, one.name);
    }
    out
}

/// Read a manifest back, ignoring everything that is not a line of it.
fn parse(text: &str) -> Result<BTreeMap<String, (String, u64)>, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split("  ");
        let (Some(hash), Some(len), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        // A line that does not start with a digest is the prose at the top, whatever else is true of it.
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let len = len
            .parse::<u64>()
            .map_err(|e| format!("{name} has a byte count that is not a number: {e}"))?;
        out.insert(name.trim().to_owned(), (hash.to_owned(), len));
    }
    if out.is_empty() {
        return Err(format!("the {MANIFEST} lists no files"));
    }
    Ok(out)
}

/// The name of the directory inside the archive, which is the name of the results directory.
///
/// Everything unpacks under one directory rather than into the working one, because an asset that scatters a hundred thousand files over whatever the reader happened to be standing in is an asset people learn not to open.
fn top_name(dir: &Path) -> String {
    let name = name_of(dir);
    if name.is_empty() || name == "." || name == ".." {
        "results".to_owned()
    } else {
        name
    }
}

/// The last component of a path, as text.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::{Args, parse, paths, render, run, top_name};
    use crate::results::{runs_dir, write};

    /// A results directory with a few runs, a chosen file, a log and a host description in it.
    fn sample(tag: &str) -> PathBuf {
        use cb_core::golden::{CHOSEN, RUN_PERF};

        let dir = std::env::temp_dir().join(format!("cache-bench-archive-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        for at in 1..=3 {
            let name = format!("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_{at}.json");
            write(&runs_dir(&dir).join(name), RUN_PERF).unwrap();
        }
        write(
            &runs_dir(&dir).join("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"),
            CHOSEN,
        )
        .unwrap();
        write(
            &dir.join("logs")
                .join("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1-server.log"),
            "dragonfly up\n",
        )
        .unwrap();
        write(&dir.join("host.json"), "{}\n").unwrap();
        dir
    }

    fn packing(dir: &Path, out: &Path) -> Args {
        Args {
            dir: Some(dir.to_owned()),
            out: Some(out.to_owned()),
            check: None,
        }
    }

    fn checking(path: &Path, dir: Option<&Path>) -> Args {
        Args {
            dir: dir.map(Path::to_owned),
            out: None,
            check: Some(path.to_owned()),
        }
    }

    #[test]
    fn an_archive_reads_back_against_its_own_manifest() {
        let dir = sample("round-trip");
        let out = dir.join("runs.tar.gz");
        run(&packing(&dir, &out)).unwrap();
        run(&checking(&out, None)).unwrap();
    }

    // The logs are the layer under the run files, and an asset that leaves them behind cannot answer the question somebody arguing with a number has.
    #[test]
    fn the_logs_go_in_with_the_runs() {
        let dir = sample("logs");
        let names: Vec<String> = paths(&dir).unwrap().into_iter().map(|f| f.0).collect();
        assert!(names.iter().any(|n| n.starts_with("logs/")), "{names:?}");
        assert_eq!(names.iter().filter(|n| n.starts_with("runs/")).count(), 4);
        assert!(names.contains(&"host.json".to_owned()), "{names:?}");
    }

    // The check has to be a check. A byte that changed, a file that went and a file that arrived all have to be found, or the manifest is decoration.
    #[test]
    fn a_changed_a_missing_and_an_extra_file_are_all_found() {
        let one = ("a".repeat(64), 4_u64);
        let two = ("b".repeat(64), 8_u64);
        let wanted: BTreeMap<String, (String, u64)> = [
            ("runs/one.json".to_owned(), one.clone()),
            ("runs/two.json".to_owned(), two),
        ]
        .into();

        let mut found = wanted.clone();
        assert_eq!(super::compare(&wanted, &found), 0);

        found.insert("runs/two.json".to_owned(), ("c".repeat(64), 8));
        assert_eq!(super::compare(&wanted, &found), 1);

        found.remove("runs/two.json");
        assert_eq!(super::compare(&wanted, &found), 1);

        found.insert("runs/three.json".to_owned(), one);
        assert_eq!(super::compare(&wanted, &found), 2);
    }

    #[test]
    fn a_directory_with_no_runs_says_so() {
        let dir = std::env::temp_dir().join("cache-bench-archive-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(runs_dir(&dir)).unwrap();
        let err = run(&packing(&dir, &dir.join("runs.tar.gz"))).unwrap_err();
        assert!(err.contains("no run files"), "{err}");
    }

    // The link between the asset and the charts. A directory whose `output.json` names a file the archive does not hold is the failure this exists to catch.
    #[test]
    fn a_chosen_file_that_is_not_in_the_archive_is_named() {
        let dir = sample("cross-check");
        let out = dir.join("runs.tar.gz");
        run(&packing(&dir, &out)).unwrap();

        // Combining after packing is the wrong order on purpose, and it is the order somebody in a hurry uses.
        let output = cb_core::Output {
            entries: vec![cb_core::Entry {
                file: "bench_redis-threads_1-pipeline_1-perf_no-run_median.json".to_owned(),
                data: cb_core::Run::parse(cb_core::golden::CHOSEN).unwrap(),
            }],
        };
        write(&dir.join("output.json"), &output.emit()).unwrap();

        let err = run(&checking(&out, Some(&dir))).unwrap_err();
        assert!(err.contains("are not in the archive"), "{err}");
    }

    #[test]
    fn the_prose_at_the_top_of_a_manifest_is_not_read_as_a_file() {
        let dir = sample("prose");
        let listed: Vec<super::Listed> = paths(&dir)
            .unwrap()
            .into_iter()
            .map(|(name, _)| super::Listed {
                name,
                hash: "0".repeat(64),
                len: 1,
            })
            .collect();
        let read = parse(&render(&listed, "wsl32coarse")).unwrap();
        assert_eq!(read.len(), listed.len());
        assert!(read.contains_key("host.json"));
    }

    #[test]
    fn the_top_directory_is_named_after_the_results_directory() {
        assert_eq!(top_name(Path::new("results/wsl32coarse")), "wsl32coarse");
        assert_eq!(top_name(Path::new("/")), "results");
    }
}
