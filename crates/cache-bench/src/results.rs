//! A results directory, which is all the state this harness keeps.
//!
//! There is no index and no database. A directory holds `runs/`, which is a few thousand files whose names say what they are, and `output.json`, which is every chosen run pasted into one file.
//! Both the selection step and the chart step work by listing the directory and reading the names back, so everything here is name handling and file reading and nothing else.
//!
//! A cell is one engine at one thread count and one pipeline depth, with or without counters attached, measured however many times the profile says.
//! The run files of a cell are numbered from one, and the four files it reduces to sit next to them under the same prefix.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cb_core::Run;
use cb_stats::Kind;

/// One cell and the runs that were found for it.
#[derive(Debug)]
pub(crate) struct Cell {
    /// The filename with the `-run_N.json` taken off, which is what names a cell everywhere in this harness.
    pub(crate) name: String,
    /// Runs one through n in order, stopping at the first number that is missing.
    pub(crate) runs: Vec<Run>,
    /// How many run files sit past that first gap.
    ///
    /// A sweep writes runs in order and skips the ones already on disk, so a gap means a run failed or a file was deleted, and the files above it are measurements that will not be used.
    /// Worth saying out loud rather than reducing 30 runs and calling it 31.
    pub(crate) after_gap: usize,
}

impl Cell {
    /// Where the file for one of the four aggregates goes.
    pub(crate) fn chosen_path(&self, dir: &Path, kind: Kind) -> PathBuf {
        runs_dir(dir).join(format!("{}-run_{}.json", self.name, kind.name()))
    }
}

/// The directory the run files live in.
pub(crate) fn runs_dir(dir: &Path) -> PathBuf {
    dir.join("runs")
}

/// Every cell in a results directory, in name order.
///
/// Files that are not run files are ignored rather than refused, because a results directory is somewhere people put things.
///
/// # Errors
///
/// If the directory cannot be listed, or if a run file cannot be read or does not parse.
pub(crate) fn cells(dir: &Path) -> Result<Vec<Cell>, String> {
    let mut found: BTreeMap<String, BTreeMap<u32, PathBuf>> = BTreeMap::new();
    for (name, path) in list(&runs_dir(dir))? {
        let Some((cell, slot)) = name.rsplit_once("-run_") else {
            continue;
        };
        // A slot that is not a number is one of the four aggregates, which is output rather than input.
        if let Ok(at) = slot.parse::<u32>() {
            found.entry(cell.to_owned()).or_default().insert(at, path);
        }
    }

    let mut cells = Vec::with_capacity(found.len());
    for (name, files) in found {
        let mut runs = Vec::with_capacity(files.len());
        let mut at = 1;
        while let Some(path) = files.get(&at) {
            runs.push(read(path)?);
            at += 1;
        }
        let after_gap = files.len() - runs.len();
        cells.push(Cell {
            name,
            runs,
            after_gap,
        });
    }
    Ok(cells)
}

/// When the first and last run in a results directory started, or `None` if there are no runs in it yet.
///
/// The aggregates are left out, because a chosen file copies its timestamp from one of the runs it was reduced from and would count that run twice.
///
/// The strings are compared rather than parsed, which works because they are RFC 3339 in UTC to the second and so sort as text in the order they happened. That is a property of the format rather than a lucky accident, and it is the reason the format was picked.
///
/// # Errors
///
/// If the directory is there and cannot be listed, or if a run file cannot be read or does not parse. A directory with no `runs` in it at all is not an error: that is what a results directory looks like before a sweep has run in it, which is the order `doctor --write` is meant to be used in.
pub(crate) fn span(dir: &Path) -> Result<Option<(String, String)>, String> {
    let runs = runs_dir(dir);
    if !runs.is_dir() {
        return Ok(None);
    }
    let mut first: Option<String> = None;
    let mut last: Option<String> = None;
    for (name, path) in list(&runs)? {
        let Some((_, slot)) = name.rsplit_once("-run_") else {
            continue;
        };
        if slot.parse::<u32>().is_err() {
            continue;
        }
        let Some(at) = read(&path)?.info.run_started else {
            continue;
        };
        if first.as_ref().is_none_or(|held| at < *held) {
            first = Some(at.clone());
        }
        if last.as_ref().is_none_or(|held| at > *held) {
            last = Some(at);
        }
    }
    Ok(first.zip(last))
}

/// Every chosen file in a results directory, paired with the name it is stored under, in the order a directory listing gives them.
///
/// That order is the order the entries appear in `output.json`, and it is the original's order because the original builds the file straight out of a sorted directory listing.
///
/// # Errors
///
/// If the directory cannot be listed, or if a chosen file cannot be read or does not parse.
pub(crate) fn chosen(dir: &Path) -> Result<Vec<(String, Run)>, String> {
    let mut found = Vec::new();
    for (name, path) in list(&runs_dir(dir))? {
        let Some((_, slot)) = name.rsplit_once("-run_") else {
            continue;
        };
        if slot.parse::<Kind>().is_ok() {
            found.push((format!("{name}.json"), path));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
        .into_iter()
        .map(|(name, path)| read(&path).map(|run| (name, run)))
        .collect()
}

/// Every `.json` file in a directory, as a stem and a path.
///
/// The order is whatever the filesystem gives, so anything that cares about order sorts it.
fn list(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("{} cannot be listed: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| format!("{} cannot be listed: {e}", dir.display()))?
            .path();
        if path.extension().is_some_and(|e| e == "json")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            out.push((stem.to_owned(), path));
        }
    }
    Ok(out)
}

/// Read one run file, saying which file when it will not parse.
pub(crate) fn read(path: &Path) -> Result<Run, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
    Run::parse(&text).map_err(|e| format!("{} is not a run file: {e}", path.display()))
}

/// Write a file, making the directory above it first, and get it onto the disk before saying so.
///
/// The flush matters here in a way it does not in most programs. A sweep runs for days and is restartable by which run files exist, so a machine that loses power holds a directory whose contents are the harness's whole memory of what it has done. A file that was written but not flushed comes back as a file of the right name holding nothing, and the run it stands for is never measured again.
pub(crate) fn write(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} cannot be created: {e}", parent.display()))?;
    }
    let mut file = std::fs::File::create(path)
        .map_err(|e| format!("{} cannot be written: {e}", path.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{} cannot be written: {e}", path.display()))?;
    // The name is a separate thing from the contents, and the contents being on the disk does not put the name there. Windows has no directory handle to flush and does not need one.
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{cells, chosen, runs_dir, span, write};

    /// A results directory holding one cell of `n` runs, plus whatever extra files the caller names.
    fn sample(tag: &str, n: u32, extra: &[&str]) -> PathBuf {
        use cb_core::golden::RUN_PERF as RUN;
        let dir = std::env::temp_dir().join(format!("cache-bench-results-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        for at in 1..=n {
            let name = format!("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_{at}.json");
            write(&runs_dir(&dir).join(name), RUN).unwrap();
        }
        for name in extra {
            write(&runs_dir(&dir).join(name), RUN).unwrap();
        }
        dir
    }

    #[test]
    fn a_cell_is_its_runs_in_order() {
        let dir = sample("in-order", 4, &[]);
        let found = cells(&dir).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].name,
            "bench_dragonfly-threads_1-pipeline_1-perf_yes"
        );
        assert_eq!(found[0].runs.len(), 4);
        assert_eq!(found[0].after_gap, 0);
    }

    // Run 10 has to come after run 9 rather than after run 1, which is what sorting the names as text would give.
    #[test]
    fn runs_are_ordered_by_number_and_not_by_name() {
        let dir = sample("by-number", 12, &[]);
        let found = cells(&dir).unwrap();
        assert_eq!(found[0].runs.len(), 12);
    }

    // A gap means a run failed, and the files above it are measurements nothing will use.
    #[test]
    fn a_gap_stops_the_cell_and_gets_counted() {
        let dir = sample("with-gap", 6, &[]);
        std::fs::remove_file(
            runs_dir(&dir).join("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_4.json"),
        )
        .unwrap();
        let found = cells(&dir).unwrap();
        assert_eq!(found[0].runs.len(), 3);
        assert_eq!(found[0].after_gap, 2);
    }

    // The four aggregates live next to the runs under the same prefix, and reading them back as runs would reduce a cell against its own output.
    #[test]
    fn chosen_files_are_not_runs() {
        let dir = sample(
            "not-runs",
            3,
            &["bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"],
        );
        assert_eq!(cells(&dir).unwrap()[0].runs.len(), 3);
        let picked = chosen(&dir).unwrap();
        assert_eq!(picked.len(), 1);
        assert_eq!(
            picked[0].0,
            "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"
        );
    }

    #[test]
    fn a_directory_that_is_not_there_says_so() {
        let err = cells(Path::new("/there/is/no/such/results/dir")).unwrap_err();
        assert!(err.contains("cannot be listed"), "{err}");
    }

    /// A results directory of runs that started at the times named, in a deliberately unhelpful order.
    fn stamped(tag: &str, times: &[&str]) -> PathBuf {
        use cb_core::Run;
        use cb_core::golden::RUN_PERF;

        let dir = std::env::temp_dir().join(format!("cache-bench-span-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        for (at, time) in times.iter().enumerate() {
            let mut run = Run::parse(RUN_PERF).unwrap();
            run.info.run_started = Some((*time).to_owned());
            let name = format!(
                "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_{}.json",
                at + 1
            );
            write(&runs_dir(&dir).join(name), &run.emit()).unwrap();
        }
        dir
    }

    // The span is the whole point of the timestamps, and it is read off the files rather than off a clock, so the order they were written in must not decide the answer.
    #[test]
    fn the_span_is_the_earliest_and_latest_run_whatever_order_they_are_in() {
        let dir = stamped(
            "unordered",
            &[
                "2026-09-08T04:00:00Z",
                "2026-09-07T22:15:00Z",
                "2026-09-08T01:30:00Z",
            ],
        );
        let (first, last) = span(&dir).unwrap().unwrap();
        assert_eq!(first, "2026-09-07T22:15:00Z");
        assert_eq!(last, "2026-09-08T04:00:00Z");
    }

    // An aggregate copies its timestamp from one of the runs it was reduced from, so counting it would put a run in the span twice, and a directory reduced halfway would answer differently from the same directory reduced all the way.
    #[test]
    fn the_span_does_not_count_the_aggregates() {
        use cb_core::Run;
        use cb_core::golden::CHOSEN;

        let dir = stamped("aggregates", &["2026-09-08T01:00:00Z"]);
        let mut median = Run::parse(CHOSEN).unwrap();
        median.info.run_started = Some("2031-01-01T00:00:00Z".to_owned());
        write(
            &runs_dir(&dir).join("bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json"),
            &median.emit(),
        )
        .unwrap();
        let (first, last) = span(&dir).unwrap().unwrap();
        assert_eq!(first, "2026-09-08T01:00:00Z");
        assert_eq!(last, "2026-09-08T01:00:00Z");
    }

    // Writing host.json before the sweep is the intended order, and there is nothing to read then. That is not a failure, and a missing runs directory is the shape it takes.
    #[test]
    fn a_directory_with_no_runs_in_it_yet_has_no_span() {
        let dir = std::env::temp_dir().join("cache-bench-span-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(span(&dir).unwrap(), None);
        // And so does one where the runs directory exists but holds nothing.
        std::fs::create_dir_all(runs_dir(&dir)).unwrap();
        assert_eq!(span(&dir).unwrap(), None);
    }

    #[test]
    fn a_file_that_is_not_a_run_file_says_which_one() {
        let dir = sample("bad-file", 1, &[]);
        let path = runs_dir(&dir).join("bench_redis-threads_1-pipeline_1-perf_no-run_1.json");
        write(&path, "not json").unwrap();
        let err = cells(&dir).unwrap_err();
        assert!(err.contains("is not a run file"), "{err}");
    }
}
