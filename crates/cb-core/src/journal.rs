//! What a sweep did, kept next to what it measured.
//!
//! Two files. `logs/sweep.jsonl` is one line per run attempted, in the order they were attempted, and it is append only. `failures.json` is the current list of cells that were tried and did not produce a file, and it is rewritten as the sweep goes.
//!
//! They answer different questions. The journal answers what was happening at three in the morning on day six, which is the question somebody asks a week later when one cell in one chart looks wrong, and the load average recorded before each run is usually the answer. The failure file answers what is missing from this results directory and why, and it exists because the alternative to naming a missing cell is a chart that draws a zero, and a zero is a claim about an engine while an absence is not.
//!
//! Neither file is an input to anything. Nothing downstream reads them, no chart is drawn from them, and a results directory with both of them deleted still produces the same charts. They are there for the person reading the numbers afterwards.

use serde::{Deserialize, Serialize};

/// One run attempted, as it goes into `sweep.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// The run file this cell would produce, which is what names a cell everywhere in this harness.
    pub cell: String,
    /// When the attempt started, RFC 3339 in UTC.
    pub started: String,
    /// How long it took, whether it worked or not.
    pub seconds: f64,
    /// The one minute load average taken just before the attempt, where the machine publishes one.
    ///
    /// Before rather than after, because what matters is whether the machine was already busy when this run started. A run that is itself the load is not the thing being looked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load: Option<f64>,
    /// Whether it produced a file.
    pub outcome: Outcome,
    /// Why it did not, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// How an attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// It wrote its run file.
    Measured,
    /// It did not, and `why` says what happened.
    Failed,
}

impl Step {
    /// One line of the journal, newline included.
    ///
    /// Not pretty printed, because this is a file that gets thousands of lines and is read with `grep` and `jq` rather than by eye.
    #[must_use]
    pub fn emit(&self) -> String {
        // Falls back to something that will not parse rather than panicking, on a shape that cannot occur.
        let mut line = serde_json::to_string(self).unwrap_or_else(|_| "null".to_owned());
        line.push('\n');
        line
    }

    /// Read one line back.
    ///
    /// # Errors
    ///
    /// If the line is not JSON of this shape.
    pub fn parse(line: &str) -> Result<Self, BadJournal> {
        serde_json::from_str(line).map_err(|e| BadJournal::Shape(e.to_string()))
    }
}

/// One cell that was attempted and did not produce a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// The run file it would have produced.
    pub cell: String,
    /// When it last failed, RFC 3339 in UTC.
    pub when: String,
    /// How many times it has been attempted across all sweeps of this directory.
    ///
    /// A cell that failed once in a fortnight of measuring is a cell to try again. A cell that has failed four times is a cell where something is actually wrong, and the difference is worth keeping across restarts rather than losing every time the sweep is started again.
    pub attempts: u32,
    /// What went wrong the last time, verbatim.
    pub why: String,
}

/// One engine given up on for the rest of a sweep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abandoned {
    /// Which engine.
    pub cache: String,
    /// When it was given up on, RFC 3339 in UTC.
    pub when: String,
    /// How many of its cells failed in a row first.
    pub after: u32,
    /// What the last of those failures said.
    pub why: String,
}

/// What is missing from a results directory, and why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failures {
    /// Every cell that has been attempted and has no file, in name order.
    #[serde(default)]
    pub failures: Vec<Failure>,
    /// Every engine that was given up on partway through, in name order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abandoned: Vec<Abandoned>,
}

impl Failures {
    /// Read a `failures.json`.
    ///
    /// # Errors
    ///
    /// If the file is not JSON of this shape.
    pub fn parse(text: &str) -> Result<Self, BadJournal> {
        serde_json::from_str(text).map_err(|e| BadJournal::Shape(e.to_string()))
    }

    /// Write one back, pretty printed with a trailing newline, because a person reads this one.
    #[must_use]
    pub fn emit(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "null".to_owned());
        text.push('\n');
        text
    }

    /// Note that a cell failed, keeping the count of how many times it has now.
    pub fn failed(&mut self, cell: &str, when: &str, why: &str) {
        if let Some(found) = self.failures.iter_mut().find(|f| f.cell == cell) {
            found.attempts = found.attempts.saturating_add(1);
            when.clone_into(&mut found.when);
            why.clone_into(&mut found.why);
            return;
        }
        self.failures.push(Failure {
            cell: cell.to_owned(),
            when: when.to_owned(),
            attempts: 1,
            why: why.to_owned(),
        });
        self.failures.sort_by(|a, b| a.cell.cmp(&b.cell));
    }

    /// Note that a cell that used to fail has now been measured.
    ///
    /// A failure file that still names a cell whose file is on the disk is a failure file that will be read as a claim about that cell, so the entry goes when the measurement arrives.
    pub fn measured(&mut self, cell: &str) {
        self.failures.retain(|f| f.cell != cell);
    }

    /// Note that an engine has been given up on for the rest of this sweep.
    pub fn abandon(&mut self, cache: &str, when: &str, after: u32, why: &str) {
        self.abandoned.retain(|a| a.cache != cache);
        self.abandoned.push(Abandoned {
            cache: cache.to_owned(),
            when: when.to_owned(),
            after,
            why: why.to_owned(),
        });
        self.abandoned.sort_by(|a, b| a.cache.cmp(&b.cache));
    }

    /// Forget every engine given up on, which is what a new sweep does before it starts.
    ///
    /// Giving up is a decision about one session. The next sweep tries that engine again, because the usual reason an engine failed is that something on the machine was wrong and somebody has since fixed it.
    pub fn reconsider(&mut self) {
        self.abandoned.clear();
    }

    /// Whether anything is missing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.failures.is_empty() && self.abandoned.is_empty()
    }
}

/// Anything that stops one of these files being read.
#[derive(Debug, thiserror::Error)]
pub enum BadJournal {
    /// Not JSON of the right shape.
    #[error("not a sweep record: {0}")]
    Shape(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Failures, Outcome, Step};

    fn step() -> Step {
        Step {
            cell: "bench_redis-threads_1-pipeline_1-perf_no-run_1.json".to_owned(),
            started: "2026-09-04T03:14:15Z".to_owned(),
            seconds: 42.5,
            load: Some(0.4),
            outcome: Outcome::Measured,
            why: None,
        }
    }

    // One line per run, because this file gets thousands of them and is read with jq.
    #[test]
    fn a_step_is_one_line_and_comes_back_the_same() {
        let line = step().emit();
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        assert_eq!(Step::parse(&line).unwrap(), step());
    }

    // A run that worked has nothing to say about why, and a key holding null is noise in a file that is read by a person.
    #[test]
    fn a_run_that_worked_carries_no_reason() {
        let line = step().emit();
        assert!(!line.contains("why"), "{line}");
        assert!(line.contains("\"outcome\":\"measured\""), "{line}");
    }

    #[test]
    fn a_run_that_failed_says_what_happened() {
        let mut failed = step();
        failed.outcome = Outcome::Failed;
        failed.why = Some("dragonfly exited during its own run".to_owned());
        let line = failed.emit();
        assert!(line.contains("\"outcome\":\"failed\""), "{line}");
        assert_eq!(Step::parse(&line).unwrap(), failed);
    }

    // The count is the difference between a cell that failed once in a fortnight and a cell where something is actually wrong.
    #[test]
    fn a_cell_that_keeps_failing_is_counted_rather_than_repeated() {
        let mut failures = Failures::default();
        failures.failed(
            "a.json",
            "2026-09-04T00:00:00Z",
            "the server did not answer",
        );
        failures.failed(
            "a.json",
            "2026-09-04T01:00:00Z",
            "the server did not answer",
        );
        assert_eq!(failures.failures.len(), 1);
        assert_eq!(failures.failures[0].attempts, 2);
        assert_eq!(failures.failures[0].when, "2026-09-04T01:00:00Z");
    }

    // A failure file that names a cell whose file is on the disk is read as a claim about that cell.
    #[test]
    fn a_cell_that_is_measured_later_stops_being_a_failure() {
        let mut failures = Failures::default();
        failures.failed("a.json", "2026-09-04T00:00:00Z", "no");
        failures.failed("b.json", "2026-09-04T00:00:00Z", "no");
        failures.measured("a.json");
        assert_eq!(failures.failures.len(), 1);
        assert_eq!(failures.failures[0].cell, "b.json");
        assert!(!failures.is_empty());
        failures.measured("b.json");
        assert!(failures.is_empty());
    }

    // The next sweep tries an abandoned engine again, because the usual reason one was abandoned is something somebody has since fixed.
    #[test]
    fn an_abandoned_engine_is_tried_again_by_the_next_sweep() {
        let mut failures = Failures::default();
        failures.abandon("garnet", "2026-09-04T00:00:00Z", 3, "no runtime installed");
        assert_eq!(failures.abandoned.len(), 1);
        assert!(!failures.is_empty());
        failures.reconsider();
        assert!(failures.is_empty());
    }

    #[test]
    fn a_failure_file_comes_back_the_same() {
        let mut failures = Failures::default();
        failures.failed("b.json", "2026-09-04T00:00:00Z", "no");
        failures.failed("a.json", "2026-09-04T00:00:00Z", "no");
        failures.abandon("garnet", "2026-09-04T00:00:00Z", 3, "no runtime installed");
        let text = failures.emit();
        assert_eq!(Failures::parse(&text).unwrap(), failures);
        // Name order, so that two sweeps of the same directory produce a file that diffs cleanly.
        assert_eq!(failures.failures[0].cell, "a.json");
    }

    // An empty failure file is the normal outcome, and it has to say that rather than being absent.
    #[test]
    fn nothing_missing_is_still_a_file() {
        let text = Failures::default().emit();
        assert!(text.contains("\"failures\": []"), "{text}");
        assert!(!text.contains("abandoned"), "{text}");
        assert!(Failures::parse(&text).unwrap().is_empty());
    }
}
