//! The file a memory measurement leaves behind.
//!
//! One row per engine, in a bare array, which is the shape `output.json` already has and the shape the scoreboard in tamnd/rugo reads. It is written whole each time rather than appended to, so a rerun of one engine replaces that engine's row and leaves the rest.
//!
//! It reports and does not judge. Garnet preallocates its index, Dragonfly preallocates per proactor, and an engine that reserves its memory up front has a peak resident set that is a configuration rather than a consequence of the keys in it. A number here that did not say so would be a number that misleads, so `note` travels in the row and into the generated results README beside it.

use serde::{Deserialize, Serialize};

/// What one engine cost, and what it was holding when it cost that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// Which server, by the name every other file here calls it.
    pub cache: String,
    /// The version string it reported, recorded the way a run file records it.
    pub version: String,
    /// How many distinct keys it was holding. Known rather than asked for, which is what [`crate::Plan`] is about.
    pub entries: u64,
    /// The largest resident set it ever had, in bytes. `VmHWM`, not `VmRSS`, because the question is what the machine had to have.
    pub peak_rss: u64,
    /// What it was holding before a single key went in, in bytes.
    ///
    /// Not subtracted from anything. It is here so a reader can see which engines start large, since an engine that reserves four gigabytes at startup and then holds ten million keys in it has a peak that says nothing about the keys.
    pub baseline_rss: u64,
    /// The keys and the values themselves, in bytes, from the plan rather than from the server.
    pub payload_bytes: u64,
    /// How many processes were in the group when the peak was read.
    ///
    /// One for every engine measured so far. More than one means the peak is a sum across processes, and a sum across processes counts a shared page once per process that has it, so the figure is an upper bound rather than a measurement. Recorded rather than corrected, because correcting it means reading every mapping of every process and that is a different tool.
    pub processes: u32,
    /// Anything about this engine that a reader comparing the rows has to know.
    pub note: String,
}

impl Row {
    /// The whole resident set divided by the keys in it.
    ///
    /// What a machine has to have per key, which is the number an operator is buying memory against.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "entry counts run to tens of millions, which a double holds exactly"
    )]
    pub fn total_per_entry(&self) -> f64 {
        self.peak_rss as f64 / self.entries as f64
    }

    /// What is left after the keys and the values themselves, divided by the keys.
    ///
    /// What the design is actually about, and a different claim from the total. At a hundred-odd bytes of payload per key, an index that got twice as small moves this by half and moves the total by a few percent, so quoting whichever one flatters is the thing this pair of methods exists to stop.
    ///
    /// Negative where an engine holds less than the payload it was given, which is compression rather than an error, and is why this is signed.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "byte counts here are gigabytes at most, which a double holds exactly"
    )]
    pub fn overhead_per_entry(&self) -> f64 {
        (self.peak_rss as f64 - self.payload_bytes as f64) / self.entries as f64
    }
}

/// Every engine measured, in the order they were measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Report {
    /// The rows.
    pub rows: Vec<Row>,
}

impl Report {
    /// Read one back.
    ///
    /// # Errors
    ///
    /// If the text is not an array of rows.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Put this engine's row in, replacing the one that is there.
    ///
    /// A rerun of one engine has to replace that engine rather than add a second row for it, because two rows for one engine is two answers to a question that has one.
    pub fn put(&mut self, row: Row) {
        match self.rows.iter_mut().find(|have| have.cache == row.cache) {
            Some(have) => *have = row,
            None => self.rows.push(row),
        }
    }

    /// Write it out.
    ///
    /// Pretty printed with a trailing newline. This one is read by people and lands in a diff whenever it is remeasured, unlike a run file, which is read by the chart engine and is shaped for byte parity with the original.
    #[must_use]
    pub fn emit(&self) -> String {
        // Falls back to something that will not parse rather than panicking, on a shape that cannot occur.
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "null".to_owned());
        text.push('\n');
        text
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Report, Row};

    fn row(cache: &str, peak: u64) -> Row {
        Row {
            cache: cache.to_owned(),
            version: "1.0".to_owned(),
            entries: 1_000_000,
            peak_rss: peak,
            baseline_rss: 8_000_000,
            payload_bytes: 130_000_000,
            processes: 1,
            note: String::new(),
        }
    }

    #[test]
    fn a_row_round_trips() {
        let mut report = Report::default();
        report.put(row("rugo", 200_000_000));
        assert_eq!(Report::parse(&report.emit()).unwrap(), report);
    }

    // The shape the scoreboard in tamnd/rugo reads: a bare array, with those four names in it.
    #[test]
    fn the_file_is_an_array_and_names_the_fields_the_scoreboard_looks_for() {
        let mut report = Report::default();
        report.put(row("rugo", 200_000_000));
        let text = report.emit();
        assert!(text.trim_start().starts_with('['), "{text}");
        for field in ["cache", "entries", "peak_rss", "payload_bytes"] {
            assert!(text.contains(&format!("\"{field}\"")), "{field} missing");
        }
    }

    #[test]
    fn measuring_an_engine_again_replaces_its_row_rather_than_adding_one() {
        let mut report = Report::default();
        report.put(row("redis", 500_000_000));
        report.put(row("rugo", 200_000_000));
        report.put(row("rugo", 190_000_000));
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.rows[1].peak_rss, 190_000_000);
        // And the engine that was not remeasured is untouched, including its place in the file.
        assert_eq!(report.rows[0].cache, "redis");
    }

    // The point of reporting two numbers. An index that got twice as small halves one of these and barely moves the other.
    #[test]
    fn total_and_overhead_are_two_different_claims() {
        let fat = row("fat", 260_000_000);
        let lean = Row {
            peak_rss: 195_000_000,
            ..row("lean", 0)
        };
        assert!(
            (fat.overhead_per_entry() / lean.overhead_per_entry() - 2.0).abs() < 1e-9,
            "the overhead halved: {} against {}",
            fat.overhead_per_entry(),
            lean.overhead_per_entry()
        );
        assert!(
            fat.total_per_entry() / lean.total_per_entry() < 1.4,
            "and the total did not: {} against {}",
            fat.total_per_entry(),
            lean.total_per_entry()
        );
    }

    // An engine holding less than the payload it was given is compressing, which is a result rather than a parse error.
    #[test]
    fn an_engine_smaller_than_its_payload_reports_a_negative_overhead() {
        let squashed = Row {
            peak_rss: 60_000_000,
            ..row("squashed", 0)
        };
        assert!(squashed.overhead_per_entry() < 0.0);
    }
}
