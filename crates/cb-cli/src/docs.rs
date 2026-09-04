//! `cache-bench docs`, which writes the documents that go with a results directory.
//!
//! So far that is the two chart indexes. The rest of the milestone adds the results README, the methodology and the divergences table, and they all come out of this one command so that a results directory is either regenerated whole or not at all.
//!
//! What goes in a document is decided by what is on disk. The generator is handed the list of charts actually sitting in `graphs`, so a sweep that ran on a machine with no hardware counters produces an index that says the cycles charts are missing instead of one with eight broken images in it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cb_chart::{Scale, Spec};
use cb_docs::Index;

/// What to generate and where to put it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory, the one holding `graphs`.
    #[arg(long, value_name = "PATH", conflicts_with = "golden")]
    dir: Option<PathBuf>,
    /// Generate as if every chart the spec names had been drawn, which needs no measurements.
    #[arg(long)]
    golden: bool,
    /// Where the documents go. Defaults to the results directory.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
    /// Write nothing and fail if what is on disk is not what would be written.
    #[arg(long)]
    check: bool,
    /// A sentence explaining why a chart might be missing on this host, shown under any section that is short of one.
    #[arg(long, value_name = "TEXT", default_value = "")]
    absent: String,
}

/// Generate them.
///
/// # Errors
///
/// If the graphs directory will not list, if the documents will not write, or if `--check` was asked for and a document on disk is not the one that would be written.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let have = present(args)?;
    let out = destination(args)?;

    let mut stale = Vec::new();
    for scale in Scale::ALL {
        let index = Index {
            scale,
            have: &have,
            absent: &args.absent,
        };
        let path = out.join(index.file());
        let text = index.render();
        if args.check {
            if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                stale.push(path);
            }
            continue;
        }
        fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("docs      {} written", path.display());
    }

    if !stale.is_empty() {
        for path in &stale {
            println!("stale     {}", path.display());
        }
        return Err(format!(
            "{} documents are not what would be generated, rerun cache-bench docs",
            stale.len()
        ));
    }
    if args.check {
        println!("docs      up to date in {}", out.display());
    }
    Ok(())
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
