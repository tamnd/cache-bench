//! `cache-bench chart`, which draws the 154.
//!
//! Two sources. A results directory, which is what a sweep produces and what a reader wants, and the golden series committed here, which needs no measurements and is what CI draws. The second one exists because the claim that two machines produce identical bytes has to be checkable on a fresh checkout by anybody, not only by whoever has 20160 run files on disk.
//!
//! The manifest is a SHA-256 per chart. `--manifest` writes one and `--check` reads one back, which together are the determinism proof: draw on Linux, draw on Windows, and diff two text files rather than 154 pictures.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cb_chart::render::{Stamp, draw};
use cb_chart::{Axis, Chart, Corpus, Scale, Spec};
use cb_core::{Compat, Output, Profiles};

/// All 154 charts as the original drew them, which is what `--golden` draws from.
const SERIES: &str = include_str!("../../../testdata/golden/series.json");

/// What to draw and where to put it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory, which is what `combine` wrote an `output.json` into.
    #[arg(long, value_name = "PATH", conflicts_with = "golden")]
    dir: Option<PathBuf>,
    /// Draw the golden series committed here instead, which needs no measurements.
    #[arg(long)]
    golden: bool,
    /// Where the PNGs go. Defaults to `graphs` inside the results directory.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
    /// Write a SHA-256 manifest here.
    #[arg(long, value_name = "PATH")]
    manifest: Option<PathBuf>,
    /// Check what was drawn against a manifest written earlier, and draw nothing to disk.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["out", "manifest"])]
    check: Option<PathBuf>,
    /// Which hardware profile produced the numbers, for the stamp along the bottom.
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
    /// Where to read the profiles from.
    #[arg(long, value_name = "PATH", default_value = "profiles.toml")]
    profiles: PathBuf,
    /// Reproduce the original's zeroes rather than leaving an unmeasured bar off.
    #[arg(long, default_value_t = Compat::Corrected, value_name = "MODE")]
    compat: Compat,
}

/// Draw them.
///
/// # Errors
///
/// If the results will not load, if a chart will not draw, if the output directory will not take the files, or if a manifest was given and what came out does not match it.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let charts = source(args)?;
    let stamp = stamp(args)?;

    let mut manifest = BTreeMap::new();
    let mut drawn = 0_usize;
    let mut skipped = Vec::new();

    let out = destination(args)?;
    if let Some(dir) = &out {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    for chart in &charts {
        let scale = scale_of(&chart.file);
        let axis = match Axis::new(scale, chart) {
            Ok(axis) => axis,
            Err(why) => {
                skipped.push(format!("{}: {why}", chart.file));
                continue;
            }
        };
        let canvas =
            draw(chart, &axis, scale, &stamp).map_err(|e| format!("{}: {e}", chart.file))?;

        let mut png = Vec::new();
        canvas
            .write_png(&mut png)
            .map_err(|e| format!("{}: {e}", chart.file))?;
        manifest.insert(chart.file.clone(), digest(&png));

        if let Some(dir) = &out {
            let path = dir.join(&chart.file);
            fs::write(&path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        drawn += 1;
    }

    for why in &skipped {
        println!("skipped   {why}");
    }
    match &out {
        Some(dir) => println!("charts    {drawn} drawn into {}", dir.display()),
        None => println!("charts    {drawn} drawn"),
    }

    if let Some(path) = &args.manifest {
        let text = render_manifest(&manifest);
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!(
            "manifest  {} hashes written to {}",
            manifest.len(),
            path.display()
        );
    }
    // The golden series is the whole corpus, so anything short of all of them is a bug here rather than a gap in the measurements.
    if args.golden && drawn != expected() {
        return Err(format!(
            "{drawn} charts drawn from the golden series, {} expected",
            expected()
        ));
    }
    if let Some(path) = &args.check {
        return compare(path, &manifest);
    }
    Ok(())
}

/// The charts to draw.
fn source(args: &Args) -> Result<Vec<Chart>, String> {
    if args.golden {
        let mut charts: Vec<Chart> = serde_json::from_str(SERIES)
            .map_err(|e| format!("the golden series will not parse: {e}"))?;
        charts.sort_by(|a, b| a.file.cmp(&b.file));
        return Ok(charts);
    }
    let dir = args
        .dir
        .as_ref()
        .ok_or("pass --dir a results directory, or --golden to draw the committed series")?;
    let path = dir.join("output.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let output = Output::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let corpus = Corpus::new(&output, args.compat).map_err(|e| format!("nothing to chart: {e}"))?;
    let mut charts = corpus.charts();
    charts.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(charts)
}

/// Where the PNGs go, or nothing if only the hashes were asked for.
fn destination(args: &Args) -> Result<Option<PathBuf>, String> {
    if args.check.is_some() {
        return Ok(None);
    }
    if let Some(dir) = &args.out {
        return Ok(Some(dir.clone()));
    }
    match &args.dir {
        Some(dir) => Ok(Some(dir.join("graphs"))),
        None if args.manifest.is_some() => Ok(None),
        None => Err("pass --out somewhere to put the charts".to_owned()),
    }
}

/// The stamp that goes along the bottom of every chart.
///
/// Nothing without `--profile`, which is what keeps a chart drawn from the golden series byte identical everywhere. A chart drawn from real measurements should always carry one, and `doctor` is where the sweep is told to.
fn stamp(args: &Args) -> Result<Stamp, String> {
    let Some(name) = &args.profile else {
        return Ok(Stamp::default());
    };
    let text = fs::read_to_string(&args.profiles)
        .map_err(|e| format!("{}: {e}", args.profiles.display()))?;
    let profiles =
        Profiles::parse(&text).map_err(|e| format!("{}: {e}", args.profiles.display()))?;
    let profile = profiles
        .profiles
        .get(name)
        .ok_or_else(|| format!("{} has no profile called {name}", args.profiles.display()))?;
    Ok(Stamp {
        profile: name.clone(),
        machine: profile.description.clone(),
        note: format!("{} cores", profile.cores),
    })
}

/// Which scale a chart is drawn on, which its filename says.
fn scale_of(file: &str) -> Scale {
    if file.contains("scale_logarithmic") {
        Scale::Logarithmic
    } else {
        Scale::Linear
    }
}

/// The manifest as text, one chart per line.
///
/// The hash first so that the file is what `sha256sum -c` would take, and sorted by name so that two runs on two machines diff cleanly.
fn render_manifest(hashes: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (file, hash) in hashes {
        out.push_str(hash);
        out.push_str("  ");
        out.push_str(file);
        out.push('\n');
    }
    out
}

/// Compare what was just drawn against a manifest written earlier.
fn compare(path: &Path, ours: &BTreeMap<String, String>) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let theirs = parse_manifest(&text)?;

    let mut wrong = 0_usize;
    for (file, hash) in &theirs {
        match ours.get(file) {
            Some(ours) if ours == hash => {}
            Some(_) => {
                println!("differs   {file}");
                wrong += 1;
            }
            None => {
                println!("missing   {file}");
                wrong += 1;
            }
        }
    }
    for file in ours.keys() {
        if !theirs.contains_key(file) {
            println!("extra     {file}");
            wrong += 1;
        }
    }
    if wrong > 0 {
        return Err(format!("{wrong} charts do not match {}", path.display()));
    }
    println!("manifest  {} charts match {}", theirs.len(), path.display());
    Ok(())
}

/// Read a manifest back.
fn parse_manifest(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (at, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (hash, file) = line
            .split_once("  ")
            .ok_or_else(|| format!("line {} is not a hash and a filename", at + 1))?;
        out.insert(file.trim().to_owned(), hash.trim().to_owned());
    }
    Ok(out)
}

/// The SHA-256 of a file, as lower case hex.
fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let out = sha2::Sha256::digest(bytes);
    out.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The 154 chart names, which is what a complete draw should produce.
#[must_use]
pub(crate) fn expected() -> usize {
    Spec::all().len()
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "a failed fixture is a failed test")]
mod tests {
    use super::{digest, expected, parse_manifest, render_manifest, scale_of};
    use cb_chart::Scale;
    use std::collections::BTreeMap;

    #[test]
    fn the_scale_comes_off_the_filename() {
        assert_eq!(
            scale_of("graph_cpucycles-pipeline_1-kind_median-scale_logarithmic.png"),
            Scale::Logarithmic
        );
        assert_eq!(
            scale_of("graph_cpucycles-pipeline_1-kind_median-scale_linear.png"),
            Scale::Linear
        );
    }

    #[test]
    fn there_are_a_hundred_and_fifty_four_to_draw() {
        assert_eq!(expected(), 154);
    }

    // The known answer, so that a change of hash library is caught here rather than as 154 charts that all moved at once.
    #[test]
    fn the_digest_is_sha_256() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_manifest_reads_back_as_what_was_written() {
        let mut hashes = BTreeMap::new();
        hashes.insert("b.png".to_owned(), digest(b"b"));
        hashes.insert("a.png".to_owned(), digest(b"a"));
        let text = render_manifest(&hashes);
        // Sorted by name, so that two machines produce two files that diff cleanly.
        assert!(text.starts_with(&digest(b"a")));
        assert_eq!(parse_manifest(&text).expect("it parses"), hashes);
    }

    #[test]
    fn a_manifest_line_that_is_not_one_says_which_line() {
        let why = parse_manifest("not a manifest line\n").expect_err("it should not parse");
        assert!(why.contains("line 1"), "{why}");
    }
}
