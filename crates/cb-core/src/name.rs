//! Result filenames, which are the primary key of the whole harness.
//!
//! There is no index and no database.
//! A results directory is a few thousand files whose names say what they are, and both the selection step and the chart step work by listing the directory and reading the names back.
//! So the encoding and its inverse have to agree exactly, and they are tested against each other over the whole space the sweep can produce.

use std::fmt;
use std::str::FromStr;

use crate::cache::CacheKind;

/// Which of the 31 runs of a cell, or which of the four aggregates over them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// One measured run.
    /// One based, as in the filename.
    Run(u32),
    /// One of the four numbers the selection step picks out of the 31.
    Chosen(Chosen),
}

/// The four aggregates the selection step writes per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Chosen {
    /// The middle run of the trimmed set.
    Median,
    /// The fastest run of the trimmed set.
    Best,
    /// The slowest run of the trimmed set.
    Worst,
    /// The mean of the trimmed set.
    Average,
}

impl Chosen {
    /// All four, in the order the original writes them.
    pub const ALL: [Self; 4] = [Self::Median, Self::Best, Self::Worst, Self::Average];

    /// The name as it appears in a filename.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Median => "median",
            Self::Best => "best",
            Self::Worst => "worst",
            Self::Average => "average",
        }
    }
}

impl fmt::Display for Chosen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One cell of the matrix, plus which run of it.
///
/// This is what a result filename means, taken apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunName {
    /// Which cache server.
    pub cache: CacheKind,
    /// How many I/O threads that server was given.
    pub threads: u32,
    /// The memtier pipeline depth.
    pub pipeline: u32,
    /// Whether perf was attached.
    /// The two halves of the matrix are separate because attaching a counter is not free and the throughput numbers should not carry its cost.
    pub perf: bool,
    /// Which run, or which aggregate.
    pub slot: Slot,
}

impl fmt::Display for RunName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bench_{}-threads_{}-pipeline_{}-perf_{}-run_",
            self.cache,
            self.threads,
            self.pipeline,
            if self.perf { "yes" } else { "no" },
        )?;
        match self.slot {
            Slot::Run(n) => write!(f, "{n}.json"),
            Slot::Chosen(c) => write!(f, "{c}.json"),
        }
    }
}

/// A filename that is not a result filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadName {
    /// The name that did not parse.
    pub name: String,
    /// What was wrong with it.
    pub why: &'static str,
}

impl fmt::Display for BadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a result filename: {}", self.name, self.why)
    }
}

impl std::error::Error for BadName {}

impl FromStr for RunName {
    type Err = BadName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = |why| BadName {
            name: s.to_owned(),
            why,
        };

        let body = s.strip_suffix(".json").ok_or_else(|| bad("no .json"))?;
        let body = body
            .strip_prefix("bench_")
            .ok_or_else(|| bad("no bench_"))?;

        // Split on the field separators rather than on every dash, because a cache name is free to contain one and one day will.
        let (cache, rest) = body
            .split_once("-threads_")
            .ok_or_else(|| bad("no threads field"))?;
        let (threads, rest) = rest
            .split_once("-pipeline_")
            .ok_or_else(|| bad("no pipeline field"))?;
        let (pipeline, rest) = rest
            .split_once("-perf_")
            .ok_or_else(|| bad("no perf field"))?;
        let (perf, slot) = rest
            .split_once("-run_")
            .ok_or_else(|| bad("no run field"))?;

        let perf = match perf {
            "yes" => true,
            "no" => false,
            _ => return Err(bad("perf is neither yes nor no")),
        };

        let slot = match slot {
            "median" => Slot::Chosen(Chosen::Median),
            "best" => Slot::Chosen(Chosen::Best),
            "worst" => Slot::Chosen(Chosen::Worst),
            "average" => Slot::Chosen(Chosen::Average),
            n => Slot::Run(n.parse().map_err(|_| bad("run is not a number"))?),
        };

        Ok(Self {
            cache: cache.parse().map_err(|_| bad("unknown cache"))?,
            threads: threads
                .parse()
                .map_err(|_| bad("threads is not a number"))?,
            pipeline: pipeline
                .parse()
                .map_err(|_| bad("pipeline is not a number"))?,
            perf,
            slot,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{BadName, Chosen, RunName, Slot};
    use crate::cache::CacheKind;

    #[test]
    fn a_real_name_from_the_original() {
        let name = "bench_dragonfly-threads_1-pipeline_1-perf_no-run_1.json";
        let parsed: RunName = name.parse().unwrap();
        assert_eq!(parsed.cache, CacheKind::Dragonfly);
        assert_eq!(parsed.threads, 1);
        assert_eq!(parsed.pipeline, 1);
        assert!(!parsed.perf);
        assert_eq!(parsed.slot, Slot::Run(1));
        assert_eq!(parsed.to_string(), name);
    }

    #[test]
    fn a_chosen_name_from_the_original() {
        let name = "bench_valkey-threads_16-pipeline_50-perf_yes-run_median.json";
        let parsed: RunName = name.parse().unwrap();
        assert!(parsed.perf);
        assert_eq!(parsed.slot, Slot::Chosen(Chosen::Median));
        assert_eq!(parsed.to_string(), name);
    }

    // The whole space a sweep on the reference profile can produce, both directions, so a formatting change cannot quietly stop matching the parser that reads the directory back.
    #[test]
    fn every_name_the_sweep_can_write_round_trips() {
        let mut seen = 0u32;
        for cache in CacheKind::ALL {
            for threads in [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16] {
                for pipeline in [1, 10, 25, 50] {
                    for perf in [false, true] {
                        let mut slots: Vec<Slot> = (1..=31).map(Slot::Run).collect();
                        slots.extend(Chosen::ALL.map(Slot::Chosen));
                        for slot in slots {
                            let name = RunName {
                                cache,
                                threads,
                                pipeline,
                                perf,
                                slot,
                            };
                            let text = name.to_string();
                            assert_eq!(text.parse::<RunName>().unwrap(), name, "{text}");
                            seen += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(seen, 8 * 12 * 4 * 2 * 35);
    }

    #[test]
    fn rubbish_is_refused_with_a_reason() {
        let cases = [
            ("output.json", "no bench_"),
            ("bench_dragonfly-threads_1.json", "no pipeline field"),
            (
                "bench_nosuchcache-threads_1-pipeline_1-perf_no-run_1.json",
                "unknown cache",
            ),
            (
                "bench_redis-threads_1-pipeline_1-perf_maybe-run_1.json",
                "perf is neither yes nor no",
            ),
            (
                "bench_redis-threads_1-pipeline_1-perf_no-run_middling.json",
                "run is not a number",
            ),
        ];
        for (name, why) in cases {
            let err: BadName = name.parse::<RunName>().unwrap_err();
            assert_eq!(err.why, why, "{name}");
        }
    }
}
