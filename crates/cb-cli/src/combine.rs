//! `cache-bench combine`, which gathers every chosen file into the one file the charts read.
//!
//! This is the last step before anything is drawn, and it is the file that gets published next to the charts so that anybody can redraw them without trusting us.
//! There is no computation in it. Every number in `output.json` was already decided by `choose`, and all this does is collect the four files per cell, in the order a directory listing gives them, and paste them into an array with the filename kept next to each one.
//!
//! The layout is the original's, down to an indentation scheme no formatter would choose, which is what lets the original's chart tool read a file we wrote.

use std::path::PathBuf;

use cb_core::{Entry, Output};

use crate::results;

/// Which directory to combine, and where to put the result.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// The results directory, which is the one holding `runs`.
    #[arg(long, default_value = "results", value_name = "PATH")]
    dir: PathBuf,
    /// Write somewhere other than `output.json` inside that directory.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

/// Collect the chosen files and write the combined one.
///
/// # Errors
///
/// If the directory cannot be read, if a chosen file will not parse, or if there is nothing in it to combine.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let found = results::chosen(&args.dir)?;
    if found.is_empty() {
        return Err(format!(
            "{} has no chosen files in it, so run choose first",
            results::runs_dir(&args.dir).display()
        ));
    }

    // Four per cell, and a count that is not a multiple of four means a cell was reduced partway.
    let leftover = found.len() % 4;
    if leftover != 0 {
        println!("{leftover} chosen files do not belong to a complete set of four");
    }

    let output = Output {
        entries: found
            .into_iter()
            .map(|(file, data)| Entry { file, data })
            .collect(),
    };
    let text = output.emit();
    let path = args
        .out
        .clone()
        .unwrap_or_else(|| args.dir.join("output.json"));
    results::write(&path, &text)?;
    println!(
        "wrote {} with {} entries, {} cells",
        path.display(),
        output.entries.len(),
        output.entries.len() / 4
    );
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use cb_core::Output;

    use super::{Args, run};
    use crate::results::{runs_dir, write};

    const CHOSEN: &str = include_str!(
        "../../../testdata/golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"
    );

    fn sample(tag: &str, names: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cache-bench-combine-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        for name in names {
            write(&runs_dir(&dir).join(name), CHOSEN).unwrap();
        }
        dir
    }

    fn args(dir: PathBuf) -> Args {
        Args { dir, out: None }
    }

    // The order of the entries is the order of a sorted directory listing, because that is how the original builds the file and the two have to agree.
    #[test]
    fn entries_come_out_in_filename_order() {
        let dir = sample(
            "order",
            &[
                "bench_redis-threads_2-pipeline_1-perf_no-run_median.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_worst.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_average.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_best.json",
            ],
        );
        run(&args(dir.clone())).unwrap();
        let text = std::fs::read_to_string(dir.join("output.json")).unwrap();
        let out = Output::parse(&text).unwrap();
        let files: Vec<&str> = out.entries.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(
            files,
            [
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_average.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_best.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_worst.json",
                "bench_redis-threads_2-pipeline_1-perf_no-run_median.json",
            ]
        );
        assert_eq!(out.emit(), text);
    }

    // The 31 measured runs are not entries. Only the four a cell was reduced to are.
    #[test]
    fn measured_runs_are_left_out() {
        let dir = sample(
            "runs-left-out",
            &[
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_31.json",
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json",
            ],
        );
        run(&args(dir.clone())).unwrap();
        let text = std::fs::read_to_string(dir.join("output.json")).unwrap();
        assert_eq!(Output::parse(&text).unwrap().entries.len(), 1);
    }

    #[test]
    fn a_directory_with_nothing_chosen_says_to_choose_first() {
        let dir = sample(
            "nothing-chosen",
            &["bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1.json"],
        );
        assert!(run(&args(dir)).unwrap_err().contains("run choose first"));
    }

    #[test]
    fn an_explicit_output_path_is_used() {
        let dir = sample(
            "explicit-out",
            &["bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"],
        );
        let out = dir.join("elsewhere/combined.json");
        let mut args = args(dir.clone());
        args.out = Some(out.clone());
        run(&args).unwrap();
        assert!(out.exists());
        assert!(!dir.join("output.json").exists());
    }
}
