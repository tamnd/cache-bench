//! Reading what `memtier_benchmark` wrote.
//!
//! This is the strictest thing in the tree and it is strict on purpose. The original reads memtier's JSON with a path query that returns zero for a path that is not there, so a memtier version that renamed a field, or a run where every connection dropped halfway through, produces a result file full of zeros that then charts as real bars sitting on the axis. Nothing downstream can tell that apart from a server that was genuinely slow.
//!
//! So every field is required, and four things are checked before a result is accepted: that the stats object exists at all, that the operation count is the one that was asked for, that the load generator's threads finished together, and that all five requested percentiles came back. A run that fails any of them is a failed run, and `sweep` records it and carries on rather than writing a number nobody can trust.
//!
//! The third of those is the one that is not obvious. memtier's `Ops/sec` is the whole operation count divided by `Total duration`, and `Total duration` tracks the first of its load generator threads to finish rather than the last. On a run where the threads finish together those are the same number. On a run where one thread finishes in a second and fifteen are still going twelve seconds later, the rate it reports is the whole run's work over the fast thread's window, and it can be an order of magnitude above what the server actually did. That is not a slow server reading slow, it is a server that served some connections far better than others reading faster than any server on the box. It is checked here rather than corrected, because a run whose threads finished that far apart was not offering the load it was asked to offer for most of its length, and there is no honest single number to put in its place.

use std::collections::BTreeMap;

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
    #[serde(rename = "CPU")]
    cpu: Option<Cpu>,
    #[serde(rename = "Sets")]
    sets: Option<Stats>,
    #[serde(rename = "Gets")]
    gets: Option<Stats>,
}

/// What the load generator itself spent, of which one part is read.
#[derive(Debug, Deserialize)]
struct Cpu {
    /// One entry per load generator thread, keyed `Thread 0` upwards.
    #[serde(rename = "Per Thread")]
    per_thread: Option<BTreeMap<String, ThreadCpu>>,
}

/// One load generator thread.
#[derive(Debug, Deserialize)]
struct ThreadCpu {
    /// How long it ran for, which is the only field here anything reads. The user and system times beside it in the file are how busy it was, and that is a different question.
    wall_seconds: f64,
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

/// How much longer the last load generator thread may run than the first before the run is refused.
///
/// A quarter. The line is set from measurement rather than from taste. Across the 960 passes of a 32 core sweep and the 740 of an 8 core one, Dragonfly, Memcached, Redis and Valkey never once went past 1.09, and Pogocache reached 1.21 in eight passes, all of them at sixteen threads and all under two seconds long, where a third of a second of stagger is a fifth of the run. Nothing that was serving its connections evenly came near a quarter, and the passes that went past it went a long way past: 1.4 to 1.7 for the ones that were nearly all right, and up to 12.75 for the ones where a thread finished in a second and the rest were still going twelve seconds later.
const SPREAD: f64 = 1.25;

/// Read one pass out of a memtier JSON file.
///
/// `wanted` is the operation count that was asked for, which is operations per connection times connections.
///
/// # Errors
///
/// If the stats object is missing, if the throughput is zero, if the operation count is not the one that was requested, if the load generator threads did not finish together, or if any percentile is missing.
pub fn read(text: &str, pass: Pass, wanted: u64) -> Result<Op, BadOutput> {
    let file: File = serde_json::from_str(text).map_err(|e| BadOutput::Shape(e.to_string()))?;
    let all = file.all.ok_or_else(|| BadOutput::NoStats {
        pass,
        keys: keys(text),
    })?;
    let All { cpu, sets, gets } = all;
    let stats = match pass {
        Pass::Warmup | Pass::Sets => sets,
        Pass::Gets => gets,
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
    together(cpu, pass)?;
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

/// Whether the load generator threads finished close enough together for the rate beside them to be a rate over the run.
///
/// See [`SPREAD`] for where the line is and why it is there. The comparison is between the shortest thread and the longest rather than against the run's own duration, because the duration is the field this is checking and using it to check itself would pass everything.
fn together(cpu: Option<Cpu>, pass: Pass) -> Result<(), BadOutput> {
    let threads = cpu
        .and_then(|cpu| cpu.per_thread)
        .filter(|threads| !threads.is_empty())
        .ok_or(BadOutput::NoThreadTimes { pass })?;
    let mut first = f64::MAX;
    let mut last = 0.0_f64;
    for thread in threads.values() {
        first = first.min(thread.wall_seconds);
        last = last.max(thread.wall_seconds);
    }
    // A thread that recorded no time at all is a file this cannot be read out of, and dividing by it would say the run was fine.
    if first <= 0.0 {
        return Err(BadOutput::NoThreadTimes { pass });
    }
    if last / first > SPREAD {
        return Err(BadOutput::Ragged {
            pass,
            threads: threads.len(),
            first,
            last,
        });
    }
    Ok(())
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
    /// No per thread timings, which is a memtier from before it wrote them.
    #[error(
        "memtier wrote no per thread timings for the {pass} pass, and those are what say whether its rate covers the whole run, so this wants memtier 2.4.4 or newer"
    )]
    NoThreadTimes {
        /// Which pass was being read.
        pass: Pass,
    },
    /// Load generator threads that finished far apart, which makes the reported rate a rate over part of the run.
    #[error(
        "on the {pass} pass the first of {threads} load generator threads finished after {first:.3} seconds where the last took {last:.3}, and memtier divides the whole operation count by the first, so its rate is over a window most of the run was not in"
    )]
    Ragged {
        /// Which pass was being read.
        pass: Pass,
        /// How many load generator threads there were.
        threads: usize,
        /// How long the first thread to finish ran for, in seconds.
        first: f64,
        /// How long the last one ran for, in seconds.
        last: f64,
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
        threads(count, &[128.640, 128.651, 128.633, 128.644])
    }

    /// The same file with the load generator threads given the wall times in `wall`.
    fn threads(count: f64, wall: &[f64]) -> String {
        let per: Vec<String> = wall
            .iter()
            .enumerate()
            .map(|(at, seconds)| {
                format!(
                    r#""Thread {at}": {{"user_seconds": 2.2, "sys_seconds": 2.5, "total_seconds": 4.7, "wall_seconds": {seconds}, "cores_used": 0.05}}"#
                )
            })
            .collect();
        let per = per.join(", ");
        let counted = wall.len();
        format!(
            r#"{{
              "configuration": {{"pipeline": 1}},
              "ALL STATS": {{
                "CPU": {{
                  "cpu_wall_seconds": 128.651,
                  "threads_counted": {counted},
                  "Per Thread": {{{per}}}
                }},
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

    // The check the published wsl32coarse numbers were missing. This is the shape of a real refused pass: one thread finished in 1.387 seconds and the last took 17.682, and memtier reported nineteen million operations a second off the first of them.
    #[test]
    fn a_pass_whose_threads_finished_far_apart_is_refused_rather_than_charted() {
        let text = threads(25_600_000.0, &[1.387, 17.682, 16.904, 17.001]);
        let why = read(&text, Pass::Sets, 25_600_000).unwrap_err();
        assert!(matches!(why, BadOutput::Ragged { .. }), "{why}");
        let said = why.to_string();
        assert!(said.contains("1.387"), "{said}");
        assert!(said.contains("17.682"), "{said}");
    }

    // Threads never finish at exactly the same instant, and a rule that expected them to would refuse every pass on the board.
    #[test]
    fn threads_that_finished_a_little_apart_are_a_normal_pass() {
        // The widest a steady engine came in the two sweeps this line was set from.
        let text = threads(25_600_000.0, &[1.58, 1.72, 1.66, 1.91]);
        assert!(read(&text, Pass::Sets, 25_600_000).is_ok());
    }

    // A memtier from before the per thread block would otherwise skip the check silently, which is the whole failure this exists to stop.
    #[test]
    fn a_file_with_no_per_thread_timings_is_an_error() {
        let text = output(25_600_000.0).replace("Per Thread", "Per Core");
        let why = read(&text, Pass::Sets, 25_600_000).unwrap_err();
        assert!(matches!(why, BadOutput::NoThreadTimes { .. }), "{why}");
        assert!(why.to_string().contains("2.4.4"), "{why}");
    }

    #[test]
    fn text_that_is_not_json_says_so() {
        let why = read("memtier_benchmark: command not found", Pass::Sets, 100).unwrap_err();
        assert!(matches!(why, BadOutput::Shape(_)), "{why}");
    }
}
