//! Reading what `memtier_benchmark` wrote.
//!
//! This is the strictest thing in the tree and it is strict on purpose. The original reads memtier's JSON with a path query that returns zero for a path that is not there, so a memtier version that renamed a field, or a run where every connection dropped halfway through, produces a result file full of zeros that then charts as real bars sitting on the axis. Nothing downstream can tell that apart from a server that was genuinely slow.
//!
//! So every field is required, and three things are checked before a result is accepted: that the stats object exists at all, that the operation count is the one that was asked for, and that all five requested percentiles came back. A run that fails any of them is a failed run, and `sweep` records it and carries on rather than writing a number nobody can trust.

use cb_core::{Fixed3, Latency, Op};
use serde::Deserialize;

use crate::argv::Pass;

/// What memtier wrote, as far as anything here cares.
#[derive(Debug, Deserialize)]
struct File {
    /// Present in every memtier version this has been run against.
    #[serde(rename = "ALL STATS")]
    all: Option<All>,
}

/// The one object in it that matters.
#[derive(Debug, Deserialize)]
struct All {
    #[serde(rename = "Sets")]
    sets: Option<Stats>,
    #[serde(rename = "Gets")]
    gets: Option<Stats>,
}

/// One pass, as memtier reports it.
#[derive(Debug, Deserialize)]
struct Stats {
    #[serde(rename = "Ops/sec")]
    opsec: f64,
    /// Kilobytes per second. The result file holds megabytes, which is this over 1024.
    #[serde(rename = "KB/sec")]
    kbsec: f64,
    /// How many operations actually completed, which is the number that catches a run where connections died.
    #[serde(rename = "Count")]
    count: f64,
    #[serde(rename = "Min Latency")]
    min: f64,
    #[serde(rename = "Max Latency")]
    max: f64,
    #[serde(rename = "Average Latency")]
    avg: f64,
    #[serde(rename = "Percentile Latencies")]
    percentiles: Option<Percentiles>,
}

/// The five that were asked for.
#[derive(Debug, Deserialize)]
struct Percentiles {
    #[serde(rename = "p50.00")]
    p50_00: f64,
    #[serde(rename = "p90.00")]
    p90_00: f64,
    #[serde(rename = "p99.00")]
    p99_00: f64,
    #[serde(rename = "p99.90")]
    p99_90: f64,
    #[serde(rename = "p99.99")]
    p99_99: f64,
}

/// How far the completed operation count may sit from the requested one before the run is refused.
///
/// A thousandth. memtier distributes operations across connections and the arithmetic does not always come out whole, so an exact match is not something to demand, but anything past this is connections that died rather than rounding.
const TOLERANCE: f64 = 0.001;

/// Read one pass out of a memtier JSON file.
///
/// `wanted` is the operation count that was asked for, which is operations per connection times connections.
///
/// # Errors
///
/// If the stats object is missing, if the operation count is not the one that was requested, if any percentile is missing, or if the throughput is zero.
pub fn read(text: &str, pass: Pass, wanted: u64) -> Result<Op, BadOutput> {
    let file: File = serde_json::from_str(text).map_err(|e| BadOutput::Shape(e.to_string()))?;
    let all = file.all.ok_or_else(|| BadOutput::NoStats {
        pass,
        keys: keys(text),
    })?;
    let stats = match pass {
        Pass::Warmup | Pass::Sets => all.sets,
        Pass::Gets => all.gets,
    };
    let stats = stats.ok_or_else(|| BadOutput::NoStats {
        pass,
        keys: keys(text),
    })?;

    if stats.opsec <= 0.0 {
        return Err(BadOutput::NoThroughput { pass });
    }
    // The count memtier reports is completed operations. A run where a third of the connections were refused still writes a JSON file, and its Ops/sec is a real rate over a workload nobody asked for.
    // The cast is exact for anything under 2^53, and the largest operation count any profile asks for is under 2^26.
    #[allow(
        clippy::cast_precision_loss,
        reason = "operation counts are far below the point where an f64 stops being exact"
    )]
    let asked = wanted as f64;
    let drift = (stats.count - asked).abs() / asked;
    if drift > TOLERANCE {
        return Err(BadOutput::WrongCount {
            pass,
            wanted,
            got: stats.count,
        });
    }
    let percentiles = stats.percentiles.ok_or(BadOutput::NoPercentiles { pass })?;

    Ok(Op {
        opsec: Fixed3(stats.opsec),
        mbsec: Fixed3(stats.kbsec / 1024.0),
        latency: Latency {
            min: Fixed3(stats.min),
            max: Fixed3(stats.max),
            avg: Fixed3(stats.avg),
            p50_00: Fixed3(percentiles.p50_00),
            p90_00: Fixed3(percentiles.p90_00),
            p99_00: Fixed3(percentiles.p99_00),
            p99_90: Fixed3(percentiles.p99_90),
            p99_99: Fixed3(percentiles.p99_99),
        },
    })
}

/// The top level keys of whatever was handed to us, for an error message.
///
/// A missing stats object is nearly always a memtier version that names things differently, and the useful thing to put in front of somebody at that point is what the file did have rather than what it did not.
fn keys(text: &str) -> Vec<String> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    map.keys().cloned().collect()
}

/// Anything that stops a memtier result being usable.
#[derive(Debug, thiserror::Error)]
pub enum BadOutput {
    /// Not JSON, or JSON whose fields are not the ones memtier writes.
    #[error("memtier output is not readable: {0}")]
    Shape(String),
    /// No stats object, which is a memtier that reports under different names.
    #[error(
        "memtier wrote no {pass} statistics, and the file has {keys:?} at the top level, so check the memtier version"
    )]
    NoStats {
        /// Which pass was being read.
        pass: Pass,
        /// What the file did have.
        keys: Vec<String>,
    },
    /// Zero throughput, which is a run that did not happen.
    #[error("memtier reports no {pass} throughput at all, so the run did not happen")]
    NoThroughput {
        /// Which pass was being read.
        pass: Pass,
    },
    /// Fewer operations than were asked for, which is connections that died.
    #[error(
        "memtier completed {got} {pass} operations where {wanted} were asked for, so connections were lost mid run"
    )]
    WrongCount {
        /// Which pass was being read.
        pass: Pass,
        /// What was asked for.
        wanted: u64,
        /// What came back.
        got: f64,
    },
    /// A percentile that was requested and not reported.
    #[error("memtier reported no {pass} percentiles, and all five were requested")]
    NoPercentiles {
        /// Which pass was being read.
        pass: Pass,
    },
}

impl std::fmt::Display for Pass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{BadOutput, Pass, read};

    /// A memtier file with the shape the real one has, cut down to the fields that are read.
    fn output(count: f64) -> String {
        format!(
            r#"{{
              "configuration": {{"pipeline": 1}},
              "ALL STATS": {{
                "Sets": {{
                  "Count": {count},
                  "Ops/sec": 198924.388,
                  "KB/sec": 107093.0,
                  "Min Latency": 0.082,
                  "Max Latency": 6.299,
                  "Average Latency": 1.286,
                  "Percentile Latencies": {{
                    "p50.00": 1.287, "p90.00": 1.430, "p99.00": 1.495,
                    "p99.90": 1.554, "p99.99": 1.744
                  }}
                }},
                "Gets": {{
                  "Count": {count},
                  "Ops/sec": 216764.059,
                  "KB/sec": 115622.0,
                  "Min Latency": 0.079,
                  "Max Latency": 5.414,
                  "Average Latency": 1.180,
                  "Percentile Latencies": {{
                    "p50.00": 1.180, "p90.00": 1.310, "p99.00": 1.400,
                    "p99.90": 1.470, "p99.99": 1.660
                  }}
                }}
              }}
            }}"#
        )
    }

    #[test]
    fn a_good_pass_reads_back_with_kilobytes_turned_into_megabytes() {
        let op = read(&output(25_600_000.0), Pass::Sets, 25_600_000).unwrap();
        assert_eq!(op.opsec.to_string(), "198924.388");
        assert_eq!(op.mbsec.to_string(), "104.583");
        assert_eq!(op.latency.p99_99.to_string(), "1.744");
    }

    #[test]
    fn the_warmup_reads_the_set_half_like_the_measured_pass_does() {
        let warm = read(&output(25_600_000.0), Pass::Warmup, 25_600_000).unwrap();
        let sets = read(&output(25_600_000.0), Pass::Sets, 25_600_000).unwrap();
        assert_eq!(warm, sets);
    }

    // The check that matters most. The original takes whatever count it finds and never looks at it, so this run charts as a real bar.
    #[test]
    fn a_run_that_lost_connections_is_refused_rather_than_charted() {
        let why = read(&output(17_000_000.0), Pass::Sets, 25_600_000).unwrap_err();
        assert!(matches!(why, BadOutput::WrongCount { .. }), "{why}");
        assert!(why.to_string().contains("connections were lost"), "{why}");
    }

    // memtier does not always divide the requested operations evenly across connections.
    #[test]
    fn rounding_in_the_operation_count_is_not_a_failure() {
        assert!(read(&output(25_599_000.0), Pass::Sets, 25_600_000).is_ok());
    }

    #[test]
    fn a_file_with_no_stats_object_names_what_it_did_have() {
        let why = read(r#"{"configuration": {}, "RUN #1": {}}"#, Pass::Gets, 100).unwrap_err();
        let text = why.to_string();
        assert!(text.contains("configuration"), "{text}");
        assert!(text.contains("RUN #1"), "{text}");
        assert!(text.contains("memtier version"), "{text}");
    }

    // Zeros here are what the original writes into a result file, and they draw as bars sitting on the axis.
    #[test]
    fn zero_throughput_is_an_error_and_not_a_measurement() {
        let text = output(25_600_000.0).replace("198924.388", "0.0");
        let why = read(&text, Pass::Sets, 25_600_000).unwrap_err();
        assert!(matches!(why, BadOutput::NoThroughput { .. }), "{why}");
    }

    #[test]
    fn a_missing_percentile_block_is_an_error() {
        let text = output(25_600_000.0).replace("Percentile Latencies", "Percentiles");
        let why = read(&text, Pass::Sets, 25_600_000).unwrap_err();
        assert!(matches!(why, BadOutput::NoPercentiles { .. }), "{why}");
    }

    // A memtier that renamed one of the five would otherwise read as a zero at that percentile.
    #[test]
    fn a_missing_single_percentile_is_an_error() {
        let text = output(25_600_000.0).replace("\"p99.90\"", "\"p99.9\"");
        assert!(read(&text, Pass::Sets, 25_600_000).is_err());
    }

    #[test]
    fn text_that_is_not_json_says_so() {
        let why = read("memtier_benchmark: command not found", Pass::Sets, 100).unwrap_err();
        assert!(matches!(why, BadOutput::Shape(_)), "{why}");
    }
}
