//! What a chart is, before anything has been read or drawn.
//!
//! A spec names one chart out of the 154. It carries no data, so it is the thing a filename parses into and the thing a command line turns into, and the set of all of them is a table rather than a sweep over whatever happens to be in a results directory.

use std::fmt;

use cb_core::Chosen;

/// Which measurement goes on the y axis, and which half of the run it comes from.
///
/// Cycles has no half because it is counted across the SET pass and the GET pass together, which is why its title says `GET+SET` and its operation count is doubled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// Operations per second, plotted in thousands.
    Throughput(Which),
    /// One of memtier's eight latency figures, plotted in microseconds.
    Latency(Percentile, Which),
    /// CPU cycles per operation.
    CpuCycles,
}

impl Metric {
    /// The leading part of the filename, before the first dash.
    fn slug(self) -> String {
        match self {
            Self::Throughput(_) => "opsec".to_owned(),
            Self::Latency(p, _) => format!("latency_{}", p.key()),
            Self::CpuCycles => "cpucycles".to_owned(),
        }
    }

    /// Which half of the run, for the metrics that have one.
    #[must_use]
    pub const fn which(self) -> Option<Which> {
        match self {
            Self::Throughput(w) | Self::Latency(_, w) => Some(w),
            Self::CpuCycles => None,
        }
    }

    /// The y axis label.
    #[must_use]
    pub fn y_title(self) -> String {
        match self {
            Self::Throughput(_) => "Throughput (Kops/sec)".to_owned(),
            Self::Latency(p, _) => format!("{} Latency (microseconds)", p.label()),
            Self::CpuCycles => "CPU Cycles (cycles/op)".to_owned(),
        }
    }

    /// Whether this metric is read from the half of the matrix that was measured with counters attached.
    ///
    /// The two halves never appear on the same chart. Cycles can only come from the runs that have them, and throughput and latency deliberately come from the runs that do not, because attaching a counter costs throughput and a chart comparing engines should not be comparing them under a measurement overhead.
    #[must_use]
    pub const fn needs_perf(self) -> bool {
        matches!(self, Self::CpuCycles)
    }
}

/// Which half of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Which {
    /// The GET pass.
    Gets,
    /// The SET pass.
    Sets,
}

impl Which {
    /// Both, in the order the original draws them.
    pub const ALL: [Self; 2] = [Self::Gets, Self::Sets];

    /// The key in a run file, and the word in a filename.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gets => "gets",
            Self::Sets => "sets",
        }
    }

    /// How it is written in a chart title.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gets => "GET",
            Self::Sets => "SET",
        }
    }
}

/// The eight latency figures memtier reports, which are eight separate charts each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Percentile {
    /// The fastest single operation.
    Min,
    /// The slowest single operation.
    Max,
    /// The mean.
    Avg,
    /// The 50th percentile.
    P50,
    /// The 90th percentile.
    P90,
    /// The 99th percentile.
    P99,
    /// The 99.9th percentile.
    P999,
    /// The 99.99th percentile.
    P9999,
}

impl Percentile {
    /// All eight, in the order the original draws them.
    pub const ALL: [Self; 8] = [
        Self::P50,
        Self::P90,
        Self::P99,
        Self::P999,
        Self::P9999,
        Self::Min,
        Self::Max,
        Self::Avg,
    ];

    /// The key in a run file, which is also the word in a filename.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Max => "max",
            Self::Avg => "avg",
            Self::P50 => "p50_00",
            Self::P90 => "p90_00",
            Self::P99 => "p99_00",
            Self::P999 => "p99_90",
            Self::P9999 => "p99_99",
        }
    }

    /// How it is written on the y axis.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::Avg => "AVG",
            Self::P50 => "P50",
            Self::P90 => "P90",
            Self::P99 => "P99",
            Self::P999 => "P999",
            Self::P9999 => "P9999",
        }
    }
}

/// How the y axis is spaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scale {
    /// Decade ticks with quarter decade lines between them, which is how six engines that differ by a factor of twenty fit on one chart.
    Logarithmic,
    /// Twenty evenly spaced ticks, which is how a reader sees that the factor of twenty is real.
    Linear,
}

impl Scale {
    /// Both, in the order the original draws them.
    pub const ALL: [Self; 2] = [Self::Logarithmic, Self::Linear];

    /// The word in a filename.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Logarithmic => "logarithmic",
            Self::Linear => "linear",
        }
    }
}

/// A chart the original draws by hand, outside its own loop.
///
/// There is exactly one, and it exists because Garnet's p99 at a single thread is far enough above everything else that a linear chart of the other five becomes a row of stubs. Leaving that one bar out is the only way the rest of the chart says anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Case {
    /// Leave Garnet's single thread bar off.
    NoGarnetAtOneThread,
}

impl Case {
    /// The number in a filename, which is what the original's `--scase` flag takes.
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::NoGarnetAtOneThread => 1,
        }
    }

    /// Whether this case drops a bar.
    #[must_use]
    pub fn drops(self, cache: &str, threads: u32) -> bool {
        match self {
            Self::NoGarnetAtOneThread => cache == "garnet" && threads == 1,
        }
    }
}

/// One chart out of the 154.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spec {
    /// What goes on the y axis.
    pub metric: Metric,
    /// The pipeline depth, which is one chart per depth rather than a line on a chart.
    pub pipeline: u32,
    /// Which of the four aggregates is plotted.
    pub kind: Chosen,
    /// How the y axis is spaced.
    pub scale: Scale,
    /// A bar left off by hand, if any.
    pub case: Option<Case>,
}

impl Spec {
    /// The four pipeline depths the sweep uses.
    pub const PIPELINES: [u32; 4] = [1, 10, 25, 50];

    /// Every chart, in the order the original draws them.
    ///
    /// This is a table rather than a walk over a results directory on purpose. A chart set that shrinks because a cell is missing is a chart set that quietly answers a different question, and the count is part of the exit condition for this milestone: 154, the same 154 the original published.
    ///
    /// Only the median is plotted. The other three aggregates are computed, written and combined, and the original draws none of them, so the `kind` field exists to say median out loud rather than to leave it implied.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut out = Vec::with_capacity(154);
        let mut push = |metric, pipeline, scale| {
            out.push(Self {
                metric,
                pipeline,
                kind: Chosen::Median,
                scale,
                case: None,
            });
        };
        for scale in Scale::ALL {
            for pipeline in Self::PIPELINES {
                for which in Which::ALL {
                    push(Metric::Throughput(which), pipeline, scale);
                }
            }
            for pipeline in Self::PIPELINES {
                for percentile in Percentile::ALL {
                    for which in Which::ALL {
                        push(Metric::Latency(percentile, which), pipeline, scale);
                    }
                }
            }
            for pipeline in Self::PIPELINES {
                push(Metric::CpuCycles, pipeline, scale);
            }
        }
        // The two the original adds after its loop, SET first, linear only.
        for which in [Which::Sets, Which::Gets] {
            out.push(Self {
                metric: Metric::Latency(Percentile::P99, which),
                pipeline: 1,
                kind: Chosen::Median,
                scale: Scale::Linear,
                case: Some(Case::NoGarnetAtOneThread),
            });
        }
        out
    }

    /// The chart title.
    ///
    /// `clients` and `operations` are the connection count and the per pass operation count, which the original reads off the first entry in the results file and uses on every chart.
    #[must_use]
    pub fn title(self, clients: u32, operations: u64) -> String {
        let (label, ops) = match self.metric.which() {
            Some(which) => (which.label().to_owned(), operations),
            // Cycles are counted over both passes, so the operation count doubles and the label says so.
            None => ("GET+SET".to_owned(), operations * 2),
        };
        format!(
            "{label} - {clients} Clients - {ops} Ops - Pipeline {}",
            self.pipeline
        )
    }

    /// The y axis label.
    #[must_use]
    pub fn y_title(self) -> String {
        self.metric.y_title()
    }

    /// The x axis label, which is the same on every chart.
    #[must_use]
    pub const fn x_title(self) -> &'static str {
        "Threads"
    }

    /// The filename, which is the original's exactly.
    #[must_use]
    pub fn file(self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "graph_{}", self.metric.slug())?;
        if let Some(which) = self.metric.which() {
            write!(f, "-which_{}", which.name())?;
        }
        write!(
            f,
            "-pipeline_{}-kind_{}-scale_{}",
            self.pipeline,
            self.kind.name(),
            self.scale.name()
        )?;
        if let Some(case) = self.case {
            write!(f, "-case_{}", case.number())?;
        }
        f.write_str(".png")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Case, Metric, Percentile, Scale, Spec, Which};

    #[test]
    fn there_are_a_hundred_and_fifty_four_of_them_and_no_two_are_the_same() {
        let all = Spec::all();
        assert_eq!(all.len(), 154);
        let names: BTreeSet<String> = all.iter().map(|s| s.file()).collect();
        assert_eq!(names.len(), 154);
    }

    #[test]
    fn filenames_are_the_originals() {
        let cycles = Spec {
            metric: Metric::CpuCycles,
            pipeline: 10,
            kind: cb_core::Chosen::Median,
            scale: Scale::Logarithmic,
            case: None,
        };
        assert_eq!(
            cycles.file(),
            "graph_cpucycles-pipeline_10-kind_median-scale_logarithmic.png"
        );
        let special = Spec {
            metric: Metric::Latency(Percentile::P99, Which::Gets),
            pipeline: 1,
            kind: cb_core::Chosen::Median,
            scale: Scale::Linear,
            case: Some(Case::NoGarnetAtOneThread),
        };
        assert_eq!(
            special.file(),
            "graph_latency_p99_00-which_gets-pipeline_1-kind_median-scale_linear-case_1.png"
        );
    }

    // Cycles are counted over the SET pass and the GET pass together, so its title claims twice the operations the other two claim.
    #[test]
    fn the_cycles_title_counts_both_passes() {
        let all = Spec::all();
        let cycles = all.iter().find(|s| s.metric == Metric::CpuCycles).unwrap();
        assert_eq!(
            cycles.title(256, 25_600_000),
            "GET+SET - 256 Clients - 51200000 Ops - Pipeline 1"
        );
        let gets = all
            .iter()
            .find(|s| s.metric == Metric::Throughput(Which::Gets))
            .unwrap();
        assert_eq!(
            gets.title(256, 25_600_000),
            "GET - 256 Clients - 25600000 Ops - Pipeline 1"
        );
    }

    #[test]
    fn only_cycles_comes_from_the_half_with_counters() {
        for spec in Spec::all() {
            assert_eq!(
                spec.metric.needs_perf(),
                spec.metric == Metric::CpuCycles,
                "{spec}"
            );
        }
    }

    #[test]
    fn the_special_case_drops_one_bar_and_no_others() {
        let case = Case::NoGarnetAtOneThread;
        assert!(case.drops("garnet", 1));
        assert!(!case.drops("garnet", 2));
        assert!(!case.drops("redis", 1));
    }
}
