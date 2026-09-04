//! Reading `perf stat -x,` output.
//!
//! The original parses the human readable table with two string helpers that take the text between a newline and a key, and between a `#` and a key. That works until a number is wide enough to change the column alignment, or the locale puts a thousands separator in, or a counter is multiplexed and perf appends a scaling percentage where the helper expected nothing.
//!
//! The machine readable form has none of those problems. Fields are separated by commas, in a fixed order, and a counter the hardware could not measure arrives as the literal text `<not supported>` in the value column rather than as a missing line.

use cb_core::{Counter, CpuCounter, EventCounter, Perf};

/// The events asked for, in the order they are passed to `perf stat`.
///
/// Same six the original asks for. `task-clock` is the last one because it is not a hardware counter and is what `cpu_utilized` is computed from.
pub const EVENTS: [&str; 6] = [
    "cycles",
    "instructions",
    "branches",
    "branch-misses",
    "page-faults",
    "task-clock",
];

/// The events as one argument to `-e`.
#[must_use]
pub fn event_list() -> String {
    EVENTS.join(",")
}

/// One line of `perf stat -x,` output.
///
/// The documented field order is value, unit, event, run time, percentage, metric value, metric unit. Only the first three are read here, because the rest describe multiplexing and perf's own derived metrics, and both of those are things this works out for itself.
#[derive(Debug, PartialEq, Eq)]
struct Row<'a> {
    /// The value column, which is a number or a reason there is no number.
    value: &'a str,
    /// Which event it is.
    event: &'a str,
}

/// Split one line, or `None` for a line that is not a counter.
///
/// perf writes a comment line or two before the counters on some versions, and an empty line at the end, and neither is an error.
fn row(line: &str) -> Option<Row<'_>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split(',');
    let value = fields.next()?.trim();
    let _unit = fields.next()?;
    let event = fields.next()?.trim();
    if event.is_empty() {
        return None;
    }
    Some(Row { value, event })
}

/// Read a whole `perf stat -x,` capture.
///
/// `wall` is how long the measured passes actually took, in seconds, which is what `cpu_utilized` is computed against. The original scrapes that number out of perf's comment column, where it is a value perf formatted for a human to read. Computing it here from `task-clock` and a duration this process measured gives the same number without depending on how perf chose to print it. Recorded as D21.
///
/// # Errors
///
/// If the text holds no counter lines at all, which means perf did not run rather than that it counted nothing.
pub fn read(text: &str, wall: f64) -> Result<Perf, BadPerf> {
    let mut out = Perf::default();
    let mut seen = 0_usize;
    let mut clock = None;

    for line in text.lines() {
        let Some(row) = row(line) else { continue };
        seen += 1;
        // A counter the machine could not measure keeps the words perf used. `cb_core::Counter` carries the distinction all the way to the chart layer, which then leaves the bar off rather than drawing it at zero.
        let event = EventCounter::Text(row.value.to_owned());
        match row.event {
            "cycles" => out.cycles = Some(event),
            "instructions" => out.instructions = Some(event),
            "branches" => out.branches = Some(event),
            "branch-misses" => out.branch_misses = Some(event),
            "page-faults" => out.page_faults = Some(event),
            "task-clock" => clock = row.value.parse::<f64>().ok(),
            _ => seen -= 1,
        }
    }

    if seen == 0 {
        return Err(BadPerf::Empty);
    }
    // task-clock is milliseconds of CPU time. Over the wall time of the passes it covered, that is how many CPUs were busy, which is the number perf prints as CPUs utilized.
    if let Some(clock) = clock
        && wall > 0.0
    {
        out.cpu_utilized = Some(CpuCounter::Number(clock / 1000.0 / wall));
    }
    Ok(out)
}

/// Whether every hardware counter in a capture came back with a real number.
///
/// A capture where they did not is not an error. It is a machine with no PMU, and the run is still worth keeping for its throughput and latency.
#[must_use]
pub fn counted(perf: &Perf) -> bool {
    [
        perf.cycles.as_ref(),
        perf.instructions.as_ref(),
        perf.branches.as_ref(),
        perf.branch_misses.as_ref(),
    ]
    .into_iter()
    .flatten()
    .all(Counter::is_measured)
}

/// Anything that stops a perf capture being readable.
#[derive(Debug, thiserror::Error)]
pub enum BadPerf {
    /// Nothing in the capture looks like a counter.
    #[error(
        "perf produced no counter lines at all, so check that it ran and that it was given -x,"
    )]
    Empty,
    /// perf would not start, which is nearly always that it is not installed.
    #[error("perf would not start: {0}")]
    NotStarted(String),
    /// perf started and could not be stopped or waited on.
    #[error("perf would not stop: {0}")]
    NotStopped(String),
    /// perf ran and its output could not be read back.
    #[error("cannot read what perf wrote to {0}: {1}")]
    NoCapture(String, String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::Counter;

    use super::{BadPerf, counted, event_list, read};

    /// What `perf stat -x, -e cycles,instructions,branches,branch-misses,page-faults,task-clock -p PID` writes on a machine with counters.
    const REAL: &str = "\
642245372237,,cycles,60002144000,100.00,,
1204451991884,,instructions,60002144000,100.00,1.88,insn per cycle
231998113452,,branches,60002144000,100.00,3866.50,M/sec
1856471712,,branch-misses,60002144000,100.00,0.80,of all branches
41522,,page-faults,60002144000,100.00,692.01,/sec
59998.63,msec,task-clock,59998634000,100.00,3.000,CPUs utilized
";

    /// The same thing inside a virtual machine, where the hardware events are not there to be counted.
    const VIRTUAL: &str = "\
<not supported>,,cycles,0,0.00,,
<not supported>,,instructions,0,0.00,,
<not supported>,,branches,0,0.00,,
<not supported>,,branch-misses,0,0.00,,
41522,,page-faults,60002144000,100.00,692.01,/sec
59998.63,msec,task-clock,59998634000,100.00,3.000,CPUs utilized
";

    #[test]
    fn every_counter_comes_back_as_the_text_perf_printed() {
        let perf = read(REAL, 20.0).unwrap();
        // Compared as text, because that is what a counter holds. Turning it into a float is what the chart layer does with it, not what this reads.
        assert_eq!(perf.cycles.unwrap().as_f64().to_string(), "642245372237");
        assert_eq!(
            perf.branch_misses.unwrap().as_f64().to_string(),
            "1856471712"
        );
        assert_eq!(perf.page_faults.unwrap().as_f64().to_string(), "41522");
    }

    // The original takes this out of perf's own comment column, which is a number formatted for a human.
    #[test]
    fn cpus_utilized_is_computed_rather_than_scraped() {
        let perf = read(REAL, 20.0).unwrap();
        // 59998.63 ms of CPU time over 20 s of wall time is three CPUs busy, which is what perf printed in the comment column.
        let busy = format!("{:.3}", perf.cpu_utilized.unwrap().as_f64());
        assert_eq!(busy, "3.000");
    }

    // The whole reason the text form is kept rather than parsed to a float on the way in.
    #[test]
    fn an_unsupported_counter_is_not_a_zero() {
        let perf = read(VIRTUAL, 20.0).unwrap();
        let cycles = perf.cycles.unwrap();
        assert!(!cycles.is_measured());
        assert_eq!(cycles, Counter::Text("<not supported>".to_owned()));
        assert!(!counted(&read(VIRTUAL, 20.0).unwrap()));
        assert!(counted(&read(REAL, 20.0).unwrap()));
    }

    // A host with no PMU still produces a usable run. Its page faults and its task clock are software events and they counted fine.
    #[test]
    fn a_capture_with_no_hardware_counters_still_reads() {
        let perf = read(VIRTUAL, 20.0).unwrap();
        assert!(perf.page_faults.unwrap().is_measured());
        assert!(perf.cpu_utilized.is_some());
    }

    #[test]
    fn comment_lines_and_blank_lines_are_not_counters() {
        let text = format!("# started on Thu Sep  4 09:12:03 2026\n\n{REAL}\n");
        assert_eq!(read(&text, 20.0).unwrap(), read(REAL, 20.0).unwrap());
    }

    // perf that did not run at all writes nothing, and reading that as a run with no counters would lose the reason.
    #[test]
    fn an_empty_capture_is_an_error() {
        let why = read("", 20.0).unwrap_err();
        assert!(matches!(why, BadPerf::Empty));
        assert!(read("# started on Thu\n", 20.0).is_err());
    }

    // A wall time of zero would be a division by zero, and it means the passes were not timed rather than that they were instant.
    #[test]
    fn an_untimed_run_reports_no_utilisation_rather_than_an_infinity() {
        assert!(read(REAL, 0.0).unwrap().cpu_utilized.is_none());
    }

    #[test]
    fn the_event_list_is_the_one_the_original_asks_for() {
        assert_eq!(
            event_list(),
            "cycles,instructions,branches,branch-misses,page-faults,task-clock"
        );
    }
}
