//! The run file, which is what one measurement looks like on disk.
//!
//! The bytes are the original's, and matching them is not tidiness.
//! The original's published `output.json` has to feed our chart engine and ours has to feed its `graph`, and that is the cheapest available proof that the port is faithful rather than merely plausible.
//! It stops being available the moment a field gets renamed for being nicer.
//!
//! ```
//! # use cb_core::run::Run;
//! let text = concat!(
//!     "{\n",
//!     "  \"info\": {\"cache\":\"redis\",\"version\":\"v=8.0.0\",\"threads\":1,\"bench_threads\":16,\"connections\":256,\"operations\":25600000,\"sizerange\":\"1-1024\",\"pipeline\":1},\n",
//!     "  \"sets\": {\"opsec\":1.000,\"mbsec\":2.000,\"latency\":{\"min\":0.001,\"max\":0.002,\"avg\":0.003,\"p50_00\":0.004,\"p90_00\":0.005,\"p99_00\":0.006,\"p99_90\":0.007,\"p99_99\":0.008}},\n",
//!     "  \"gets\": {\"opsec\":3.000,\"mbsec\":4.000,\"latency\":{\"min\":0.001,\"max\":0.002,\"avg\":0.003,\"p50_00\":0.004,\"p90_00\":0.005,\"p99_00\":0.006,\"p99_90\":0.007,\"p99_99\":0.008}},\n",
//!     "  \"perf\": {}\n",
//!     "}\n",
//! );
//! let run = Run::parse(text)?;
//! assert_eq!(run.emit(), text);
//! # Ok::<(), serde_json::Error>(())
//! ```

use serde::{Deserialize, Serialize};

use crate::num::{CpuCounter, EventCounter, Fixed3};
use crate::spread::Spread;

/// One measurement, or one aggregate over 31 of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// What was measured and how.
    pub info: Info,
    /// The SET half.
    pub sets: Op,
    /// The GET half.
    pub gets: Op,
    /// The counters, empty for a run with no perf attached.
    pub perf: Perf,

    /// How noisy the cell was.
    /// Ours, not the original's.
    ///
    /// Present only in a chosen file, and only in corrected mode, because it describes a cell rather than a run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<Spread>,
}

impl Run {
    /// Read a run file.
    ///
    /// # Errors
    ///
    /// If the text is not JSON, or is JSON of the wrong shape.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Write a run file, byte for byte as the original writes it.
    ///
    /// Two space indent at the top level and nowhere else, every nested value compact, and a trailing newline.
    /// The original gets this shape by pasting four compact strings into a template, and so does this, because no serialiser setting produces a document that is pretty at one level and compact at every level below it.
    ///
    #[must_use]
    pub fn emit(&self) -> String {
        let part =
            |name: &str, value: &dyn erased::Compact| format!("  \"{name}\": {}", value.compact());
        let mut parts = vec![
            part("info", &self.info),
            part("sets", &self.sets),
            part("gets", &self.gets),
            part("perf", &self.perf),
        ];
        // Ours, so it goes last, where a key added later would land.
        // A file without it is the original's file exactly, which is what the parity test needs.
        if let Some(spread) = &self.spread {
            parts.push(part("spread", spread));
        }
        format!("{{\n{}\n}}\n", parts.join(",\n"))
    }
}

/// A tiny bit of indirection so `emit` can loop over its four parts instead of repeating the same line four times with a different type each time.
mod erased {
    use serde::Serialize;

    pub(super) trait Compact {
        fn compact(&self) -> String;
    }

    impl<T: Serialize> Compact for T {
        fn compact(&self) -> String {
            // Falls back to the JSON literal `null` on the impossible case, so that a bad number produces a file that fails to parse loudly rather than a panic in the middle of a two week sweep.
            serde_json::to_string(self).unwrap_or_else(|_| "null".to_owned())
        }
    }
}

/// What was measured, and the settings it was measured under.
///
/// Field order here is the key order in the file, and both are the original's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// The cache server's short name.
    pub cache: String,
    /// The first line of the server's own version output.
    pub version: String,
    /// I/O threads given to the server.
    pub threads: u32,
    /// memtier's thread count.
    pub bench_threads: u32,
    /// Connections, which is `bench_threads` times connections per thread.
    pub connections: u32,
    /// Operations, per op type.
    pub operations: u64,
    /// memtier's value size range.
    pub sizerange: String,
    /// memtier's pipeline depth.
    pub pipeline: u32,

    /// Which hardware profile produced this.
    /// Ours, not the original's.
    ///
    /// Without it a results directory that mixes hosts is indistinguishable from one that does not, and this project mixes hosts by design, because the box with the thread count has no PMU and the box with the PMU does not have the thread count.
    /// Omitted in upstream compatibility mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// When the run started, RFC 3339 in UTC.
    /// Ours, not the original's.
    ///
    /// A sweep runs for days and gets interrupted.
    /// This is what lets somebody find the gap afterwards.
    /// Omitted in upstream compatibility mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_started: Option<String>,

    /// Which aggregate this is.
    /// Present only in a chosen file, and appended last, which is where the original's JSON library puts a new key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// One half of a run, either the SET pass or the GET pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Op {
    /// Operations per second.
    pub opsec: Fixed3,
    /// Megabytes per second, which is memtier's KB/sec over 1024.
    pub mbsec: Fixed3,
    /// Latency, in milliseconds on disk and microseconds on a chart.
    /// The thousandfold happens in the chart layer and nowhere else.
    pub latency: Latency,
}

/// The eight latency figures memtier reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Latency {
    /// Fastest single operation.
    pub min: Fixed3,
    /// Slowest single operation.
    pub max: Fixed3,
    /// Mean.
    pub avg: Fixed3,
    /// 50th percentile.
    pub p50_00: Fixed3,
    /// 90th percentile.
    pub p90_00: Fixed3,
    /// 99th percentile.
    pub p99_00: Fixed3,
    /// 99.9th percentile.
    pub p99_90: Fixed3,
    /// 99.99th percentile.
    pub p99_99: Fixed3,
}

/// The perf counters.
///
/// Empty for a run with perf detached, and it has to serialise as `{}` rather than as `null` or as a missing key, because the chart layer decides whether a cell has cycles by looking for the key.
///
/// Field order is the order the original discovers them in `perf stat` output, which is the order they end up in the file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Perf {
    /// CPUs utilised, a ratio rather than a count, and the one counter the original writes with decimal places.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_utilized: Option<CpuCounter>,
    /// CPU cycles.
    /// The presence of this key is what marks a cell as having usable perf data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycles: Option<EventCounter>,
    /// Seconds of user time.
    ///
    /// Three places, like `cpu_utilized`, because that is the shape `perf stat` prints seconds in.
    /// The original never writes this one as a number at all, since its selection step converts six named counters and this is not one of them, so the choice is ours and it only shows up in a file the original would not have written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secsuser: Option<CpuCounter>,
    /// Seconds of system time.
    /// Three places, for the same reason as `secsuser`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secssys: Option<CpuCounter>,
    /// Instructions retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<EventCounter>,
    /// Branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<EventCounter>,
    /// Mispredicted branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_misses: Option<EventCounter>,
    /// Page faults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_faults: Option<EventCounter>,
}

impl Perf {
    /// Whether this run has perf data a chart can use.
    ///
    /// Keyed on cycles, which is the same test the original applies, and the same one the chart layer applies when it decides whether a cell belongs on a cycles chart at all.
    #[must_use]
    pub fn has_cycles(&self) -> bool {
        self.cycles.is_some()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Run;
    use crate::golden::{CHOSEN, RUN_PERF as WITH_PERF, RUN_PLAIN as NO_PERF};

    // The whole point of the crate, in three assertions.
    // These are real files from the original's committed results, unmodified.
    #[test]
    fn real_files_round_trip_byte_for_byte() {
        for text in [NO_PERF, WITH_PERF, CHOSEN] {
            let run = Run::parse(text).unwrap();
            assert_eq!(run.emit(), text);
        }
    }

    #[test]
    fn a_run_with_no_perf_still_has_the_key() {
        let run = Run::parse(NO_PERF).unwrap();
        assert!(!run.perf.has_cycles());
        assert!(run.emit().contains(r#""perf": {}"#));
    }

    // The run file holds counters as strings and the chosen file holds the same counters as numbers, because the selection step reparses them on the way through.
    // Both shapes are in the golden data above and both round trip, which is the only reason the model keeps the distinction.
    #[test]
    fn counters_are_strings_in_a_run_and_numbers_in_an_aggregate() {
        assert!(WITH_PERF.contains(r#""cycles":"642245372237""#));
        assert!(CHOSEN.contains(r#""cycles":640031542073"#));
    }

    // An unsupported counter is a string that is not a number, and the original turns it into a zero on the way into the chosen file.
    // That zero is a real bar on a real chart claiming the engine took no branches.
    #[test]
    fn an_unsupported_counter_becomes_a_zero() {
        assert!(WITH_PERF.contains(r#""branches":"<not supported>""#));
        assert!(CHOSEN.contains(r#""branches":0"#));
        let run = Run::parse(WITH_PERF).unwrap();
        let branches = run.perf.branches.unwrap();
        assert!(!branches.is_measured());
        assert!((branches.as_f64() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn kind_is_present_only_in_an_aggregate() {
        assert_eq!(Run::parse(NO_PERF).unwrap().info.kind, None);
        assert_eq!(
            Run::parse(CHOSEN).unwrap().info.kind.as_deref(),
            Some("median")
        );
    }
}
