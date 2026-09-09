//! `cache-bench recheck`, which applies today's checks to a sweep that was measured under yesterday's.
//!
//! Every check in `cb-memtier` is a function of the bytes memtier wrote, and those bytes are kept: the load generator's JSON for all three passes of a run sits in `logs/` next to the run file it produced. So when a check is added, the sweeps that were measured before it existed can be judged by it without measuring anything again, which is the difference between an hour and a week on a box.
//!
//! That is what this is for. It reads each run file's three logs back through the same parser a live run goes through, and a run whose logs no longer pass is moved out of `runs/` and recorded in `failures.json` exactly as it would have been had the check existed on the day.
//!
//! Two things about it are deliberate.
//!
//! The runs that survive are renumbered. A cell is its runs one through n and it stops at the first number that is missing, so leaving a hole where run two was would throw away runs three, four and five as well, and those are measurements that passed. Renumbering keeps them. The run number is a slot in a cell rather than anything about the measurement, and every run file carries its own `run_started`, so nothing that says when a number was taken is lost.
//!
//! The refused runs are moved rather than deleted, into `runs/refused/`, which the cell reader does not descend into. A run that a check refuses is still the evidence for why it was refused, and a directory that has been through this should be able to show its working.
//!
//! What this does not do is make a cell whole again. A sweep that met this check on the day would have retried the cell and either got its runs or given the engine up, and this cannot do either. A cell that comes out of here with two runs where the profile asked for five is under sampled, and it says so rather than pretending otherwise.

use std::fs;
use std::path::{Path, PathBuf};

use cb_core::Failures;
use cb_memtier::Pass;

use crate::results::{self, Cell};

/// Which directory to re-judge, and whether to change it.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A results directory, the one holding `runs` and `logs`.
    #[arg(long, value_name = "PATH")]
    dir: PathBuf,
    /// Move the runs that no longer pass, rather than only saying which they are.
    #[arg(long)]
    apply: bool,
    /// Fail if any run no longer passes, rather than reporting and stopping.
    #[arg(long)]
    check: bool,
}

/// One run, and what reading its logs back said about it.
#[derive(Debug)]
struct Verdict {
    /// The run file's stem, which is the cell name and the run number.
    name: String,
    /// Which cell it belongs to.
    cell: String,
    /// What refused it, or `None` if all three passes still read.
    why: Option<String>,
}

/// Re-judge a results directory.
///
/// # Errors
///
/// If the directory will not read, if a log a run file needs is not beside it, or if `--check` was asked for and a run no longer passes.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let logs = args.dir.join("logs");
    if !logs.is_dir() {
        return Err(format!(
            "{} has no logs directory, and the load generator's own output is what a check is applied to, so there is nothing here to re-judge",
            args.dir.display()
        ));
    }
    let cells = results::cells(&args.dir)?;
    let verdicts = judge(&cells, &logs)?;
    let refused: Vec<&Verdict> = verdicts.iter().filter(|v| v.why.is_some()).collect();

    println!(
        "recheck   {} runs in {}, against the checks this build makes",
        verdicts.len(),
        args.dir.display()
    );
    report(&cells, &refused);

    if refused.is_empty() {
        println!();
        println!(
            "every run still passes, so nothing here was measured under a check that has since changed"
        );
        return Ok(());
    }
    let what = format!(
        "{} of {} runs no longer pass",
        refused.len(),
        verdicts.len()
    );
    if args.check {
        return Err(what);
    }
    println!();
    if !args.apply {
        println!("{what}, and nothing was moved, because --apply was not asked for");
        return Ok(());
    }
    apply(&args.dir, &cells, &refused)?;
    println!("{what}, and they are under runs/refused with failures.json naming them");
    println!("rerun choose, combine, chart and docs to rebuild what is published from this");
    Ok(())
}

/// Read every run's logs back through the parser.
fn judge(cells: &[Cell], logs: &Path) -> Result<Vec<Verdict>, String> {
    let mut out = Vec::new();
    for cell in cells {
        for (at, run) in cell.runs.iter().enumerate() {
            let number = at + 1;
            let name = format!("{}-run_{number}", cell.name);
            out.push(Verdict {
                why: refusal(logs, &name, run.info.operations)?,
                cell: cell.name.clone(),
                name,
            });
        }
    }
    Ok(out)
}

/// What refuses this run, reading all three of its passes back.
///
/// The warmup is read along with the other two, the way a live run reads it, because a warmup that did not hold up is a measured SET pass that is partly a measurement of hash table growth.
fn refusal(logs: &Path, name: &str, operations: u64) -> Result<Option<String>, String> {
    for pass in [Pass::Warmup, Pass::Sets, Pass::Gets] {
        let path = logs.join(format!("{name}-{}.json", pass.label()));
        let text = fs::read_to_string(&path).map_err(|why| {
            format!(
                "{} is the {pass} log of a run that is in this directory, and it will not read: {why}",
                path.display()
            )
        })?;
        if let Err(why) = cb_memtier::read(&text, pass, operations) {
            return Ok(Some(why.to_string()));
        }
    }
    Ok(None)
}

/// Say which cells lost runs and how many they have left.
fn report(cells: &[Cell], refused: &[&Verdict]) {
    if refused.is_empty() {
        return;
    }
    println!();
    println!("{:<52}  {:>4}  {:>4}  why", "cell", "kept", "lost");
    for cell in cells {
        let lost = refused.iter().filter(|v| v.cell == cell.name).count();
        if lost == 0 {
            continue;
        }
        let why = refused
            .iter()
            .find(|v| v.cell == cell.name)
            .and_then(|v| v.why.as_deref())
            .unwrap_or("");
        println!(
            "{:<52}  {:>4}  {:>4}  {}",
            cell.name,
            cell.runs.len() - lost,
            lost,
            first_sentence(why)
        );
    }
}

/// The head of a refusal, because the table is one line a cell and the whole message is a paragraph.
fn first_sentence(why: &str) -> String {
    match why.find(", and ") {
        Some(at) => why[..at].to_owned(),
        None => why.to_owned(),
    }
}

/// Move the refused runs aside, renumber what is left and record the failures.
fn apply(dir: &Path, cells: &[Cell], refused: &[&Verdict]) -> Result<(), String> {
    let runs = results::runs_dir(dir);
    let aside = runs.join("refused");
    fs::create_dir_all(&aside)
        .map_err(|why| format!("{} will not be made: {why}", aside.display()))?;

    let record = dir.join("failures.json");
    let mut failures = match fs::read_to_string(&record) {
        Ok(text) => Failures::parse(&text).map_err(|why| format!("{}: {why}", record.display()))?,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Failures::default(),
        Err(why) => return Err(format!("{} will not read: {why}", record.display())),
    };
    let when = when();

    for verdict in refused {
        let from = runs.join(format!("{}.json", verdict.name));
        let to = aside.join(format!("{}.json", verdict.name));
        rename(&from, &to)?;
        let why = verdict.why.as_deref().unwrap_or("");
        failures.failed(&verdict.name, &when, why);
    }

    for cell in cells {
        renumber(&runs, cell, refused)?;
    }

    // The four aggregates were reduced from runs that are no longer all there, so they are output that no longer follows from its input.
    for cell in cells {
        if !refused.iter().any(|v| v.cell == cell.name) {
            continue;
        }
        for kind in cb_stats::Kind::ALL {
            let path = cell.chosen_path(dir, kind);
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|why| format!("{} will not go: {why}", path.display()))?;
            }
        }
    }

    results::write(&record, &failures.emit())
        .map_err(|why| format!("{} will not be written: {why}", record.display()))
}

/// Close the gaps a cell's surviving runs were left with.
///
/// Downwards and one at a time, so that a run never lands on a number another run is still holding.
fn renumber(runs: &Path, cell: &Cell, refused: &[&Verdict]) -> Result<(), String> {
    let gone: Vec<usize> = (1..=cell.runs.len())
        .filter(|at| {
            let name = format!("{}-run_{at}", cell.name);
            refused.iter().any(|v| v.name == name)
        })
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    let mut to = 1;
    for from in 1..=cell.runs.len() {
        if gone.contains(&from) {
            continue;
        }
        if from != to {
            rename(
                &runs.join(format!("{}-run_{from}.json", cell.name)),
                &runs.join(format!("{}-run_{to}.json", cell.name)),
            )?;
        }
        to += 1;
    }
    Ok(())
}

/// Move a file, saying which one if it will not go.
fn rename(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|why| {
        format!(
            "{} will not move to {}: {why}",
            from.display(),
            to.display()
        )
    })
}

/// Now, RFC 3339 in UTC to the second, which is the format everything else in a results directory is stamped in.
fn when() -> String {
    cb_core::now()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Args, run};

    /// A results directory with one cell of three runs in it, whose second run's GET log has threads that finished far apart.
    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-recheck-{}-{label}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("runs")).unwrap();
        fs::create_dir_all(dir.join("logs")).unwrap();
        for at in 1..=3 {
            let name = format!("bench_yo-threads_8-pipeline_25-perf_no-run_{at}");
            fs::write(dir.join(format!("runs/{name}.json")), run_file(at)).unwrap();
            // Run two is the one that did not hold up, and only on its GET pass, which is how a real one looks.
            for pass in ["warmup", "sets", "gets"] {
                let ragged = at == 2 && pass == "gets";
                fs::write(
                    dir.join(format!("logs/{name}-{pass}.json")),
                    log(ragged, pass),
                )
                .unwrap();
            }
        }
        dir
    }

    /// A run file with the fields anything here reads.
    fn run_file(at: u32) -> String {
        format!(
            r#"{{"info": {{"cache": "yo", "version": "yo 0.3.28", "threads": 8, "bench_threads": 16, "connections": 256, "operations": 1000, "sizerange": "1-1024", "pipeline": 25, "profile": "test", "run_started": "2026-09-0{at}T00:00:00Z"}}, "sets": {{"opsec": 1.0, "mbsec": 1.0, "latency": {{"min": 0.1, "max": 0.2, "avg": 0.15, "p50_00": 0.1, "p90_00": 0.1, "p99_00": 0.1, "p99_90": 0.1, "p99_99": 0.1}}}}, "gets": {{"opsec": 1.0, "mbsec": 1.0, "latency": {{"min": 0.1, "max": 0.2, "avg": 0.15, "p50_00": 0.1, "p90_00": 0.1, "p99_00": 0.1, "p99_90": 0.1, "p99_99": 0.1}}}}, "perf": {{}}}}"#
        )
    }

    /// A memtier log, with its threads either finishing together or a long way apart.
    fn log(ragged: bool, pass: &str) -> String {
        let wall = if ragged { 17.682 } else { 4.011 };
        let which = if pass == "gets" { "Gets" } else { "Sets" };
        format!(
            r#"{{"ALL STATS": {{"CPU": {{"threads_counted": 2, "Per Thread": {{"Thread 0": {{"wall_seconds": 4.005}}, "Thread 1": {{"wall_seconds": {wall}}}}}}}, "{which}": {{"Ops/sec": 1234.5, "KB/sec": 2048.0, "Count": 1000, "Min Latency": 0.1, "Max Latency": 9.9, "Average Latency": 0.5, "Percentile Latencies": {{"p50.00": 0.4, "p90.00": 0.8, "p99.00": 1.5, "p99.90": 3.0, "p99.99": 7.0}}}}}}}}"#
        )
    }

    fn args(dir: &Path, apply: bool, check: bool) -> Args {
        Args {
            dir: dir.to_owned(),
            apply,
            check,
        }
    }

    // Saying so and changing nothing is the default, because a results directory is a week of somebody's machine.
    #[test]
    fn reporting_moves_nothing() {
        let dir = fixture("report");
        run(&args(&dir, false, false)).unwrap();
        assert!(
            dir.join("runs/bench_yo-threads_8-pipeline_25-perf_no-run_2.json")
                .exists()
        );
        assert!(!dir.join("runs/refused").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn check_fails_when_a_run_no_longer_passes() {
        let dir = fixture("check");
        let why = run(&args(&dir, false, true)).unwrap_err();
        assert!(why.contains("1 of 3 runs"), "{why}");
        fs::remove_dir_all(&dir).ok();
    }

    // The point of renumbering. Leaving a hole at run two would take run three with it, and run three is a measurement that passed.
    #[test]
    fn the_runs_above_a_refused_one_are_kept_rather_than_orphaned() {
        let dir = fixture("apply");
        run(&args(&dir, true, false)).unwrap();

        let runs = dir.join("runs");
        let cell = "bench_yo-threads_8-pipeline_25-perf_no-run";
        assert!(runs.join(format!("{cell}_1.json")).exists());
        assert!(runs.join(format!("{cell}_2.json")).exists());
        assert!(!runs.join(format!("{cell}_3.json")).exists());

        // What is now run two is the one that used to be run three, which is what says nothing was thrown away.
        let text = fs::read_to_string(runs.join(format!("{cell}_2.json"))).unwrap();
        assert!(text.contains("2026-09-03"), "{text}");

        // The refused one is kept, because it is the evidence for its own refusal.
        assert!(
            runs.join(format!("refused/{cell}_2.json")).exists(),
            "the refused run was deleted rather than moved aside"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_failure_file_names_the_run_and_says_what_refused_it() {
        let dir = fixture("failures");
        run(&args(&dir, true, false)).unwrap();
        let text = fs::read_to_string(dir.join("failures.json")).unwrap();
        assert!(
            text.contains("bench_yo-threads_8-pipeline_25-perf_no-run_2"),
            "{text}"
        );
        assert!(text.contains("load generator threads"), "{text}");
        fs::remove_dir_all(&dir).ok();
    }

    // A directory with no logs beside it cannot be re-judged at all, and saying that is better than reporting that every run passed.
    #[test]
    fn a_directory_with_no_logs_says_so_rather_than_passing_everything() {
        let dir = fixture("nologs");
        fs::remove_dir_all(dir.join("logs")).unwrap();
        let why = run(&args(&dir, false, false)).unwrap_err();
        assert!(why.contains("no logs directory"), "{why}");
        fs::remove_dir_all(&dir).ok();
    }
}
