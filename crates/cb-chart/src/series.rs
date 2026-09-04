//! What goes on a chart, worked out before anything is drawn.
//!
//! This is the half of the chart engine that has no pixels in it. A results file goes in, and what comes out is a title, an axis pair, a thread count for each group of bars and one number per bar. Everything a reader could disagree with is decided here, which is why it is a pure function and why it is checked against the original's own answers for all 154 charts.
//!
//! The numbers are the original's arithmetic rather than a tidier version of it. Throughput is truncated to whole thousands twice over, latency is rounded to whole microseconds, and cycles are divided by the operation count of both passes. Charts drawn here and charts drawn there have to be the same charts, and that starts with the bars being the same height.

use std::collections::BTreeMap;
use std::fmt;

use cb_core::{Compat, Latency, Op, Output, Run};
use serde::{Deserialize, Serialize};

use crate::palette::{TooManyCaches, color};
use crate::spec::{Metric, Percentile, Spec, Which};

/// One chart, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chart {
    /// The filename, which is the original's exactly.
    pub file: String,
    /// The heading across the top.
    pub title: String,
    /// The x axis label.
    #[serde(rename = "xtitle")]
    pub x_title: String,
    /// The y axis label.
    #[serde(rename = "ytitle")]
    pub y_title: String,
    /// The thread counts, ascending, which is one group of bars each.
    #[serde(rename = "xseries")]
    pub x_series: Vec<u32>,
    /// One entry per cache server, in the order the legend lists them.
    pub series: Vec<Series>,
}

/// One cache server's bars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    /// The server's short name, which is also its legend entry.
    pub cache: String,
    /// The bar colour, as hex.
    pub color: String,
    /// One value per thread count, in the same order as the x series.
    ///
    /// `None` is a bar that is not drawn, which happens when the cell was never measured, when the machine could not count cycles, and on the one chart pair where the original leaves a bar off by hand. In `Compat::Upstream` these are zeroes instead, because that is what the original plots and the parity check needs the same numbers.
    pub points: Vec<Option<i64>>,
}

/// A results file, indexed for the chart layer.
///
/// Built once and asked for charts many times, because the alternative is 154 walks over 2304 entries.
#[derive(Debug, Clone)]
pub struct Corpus<'a> {
    /// Cache servers, in the order the results file first mentions them, which is what decides the legend order and the colours.
    caches: Vec<&'a str>,
    /// The version string each server reported, at the same positions as `caches`.
    ///
    /// The original works these out and then never uses them. They are kept because the provenance stamp on a chart should say which build of an engine produced the bars, and a chart that names an engine without a version is a chart nobody can repeat.
    versions: Vec<&'a str>,
    /// Thread counts, ascending.
    threads: Vec<u32>,
    /// Connections, off the first entry.
    clients: u32,
    /// Operations per pass, off the first entry.
    operations: u64,
    /// Operations across both passes, as a float, which is the divisor for cycles per operation.
    both_passes: f64,
    /// Whether to reproduce the original's zeroes or leave the bar off.
    compat: Compat,
    /// Every cell, keyed by what a chart asks for.
    cells: BTreeMap<Key<'a>, &'a Run>,
}

/// What identifies one cell to the chart layer.
///
/// Perf is in the key because the two halves of the matrix never appear on the same chart, and kind is in it because only the median is ever drawn but all four are in the file.
type Key<'a> = (&'a str, u32, u32, bool, &'a str);

impl<'a> Corpus<'a> {
    /// Index a results file.
    ///
    /// # Errors
    ///
    /// If the file has nothing in it, or holds more cache servers than there are colours.
    pub fn new(output: &'a Output, compat: Compat) -> Result<Self, BadCorpus> {
        let first = output.entries.first().ok_or(BadCorpus::Empty)?;

        let mut caches = Vec::new();
        let mut versions = Vec::new();
        let mut threads = Vec::new();
        let mut cells = BTreeMap::new();
        for entry in &output.entries {
            let info = &entry.data.info;
            if !caches.contains(&info.cache.as_str()) {
                caches.push(info.cache.as_str());
                versions.push(info.version.as_str());
            }
            if !threads.contains(&info.threads) {
                threads.push(info.threads);
            }
            // Only an aggregate belongs on a chart, and a file with no kind is a raw run that wandered in.
            if let Some(kind) = info.kind.as_deref() {
                let key = (
                    info.cache.as_str(),
                    info.threads,
                    info.pipeline,
                    entry.data.perf.has_cycles(),
                    kind,
                );
                // First wins, which is what the original's query does with a duplicate.
                cells.entry(key).or_insert(&entry.data);
            }
        }
        threads.sort_unstable();

        // Checked once here rather than per chart, so that asking for a chart cannot fail.
        for at in 0..caches.len() {
            color(at).map_err(BadCorpus::TooManyCaches)?;
        }

        let operations = first.data.info.operations;
        Ok(Self {
            caches,
            versions,
            threads,
            clients: first.data.info.connections,
            operations,
            both_passes: widen(operations * 2),
            compat,
            cells,
        })
    }

    /// The cache servers, in legend order.
    #[must_use]
    pub fn caches(&self) -> &[&'a str] {
        &self.caches
    }

    /// The version each server reported, in the same order as [`Corpus::caches`].
    #[must_use]
    pub fn versions(&self) -> &[&'a str] {
        &self.versions
    }

    /// The thread counts, ascending.
    #[must_use]
    pub fn threads(&self) -> &[u32] {
        &self.threads
    }

    /// Work out one chart.
    #[must_use]
    pub fn chart(&self, spec: Spec) -> Chart {
        let series = self
            .caches
            .iter()
            .enumerate()
            .map(|(at, cache)| Series {
                cache: (*cache).to_owned(),
                // Checked in `new`, and the fallback is the last colour rather than a panic, because a chart with a repeated colour is a worse outcome than no chart only if somebody sees it.
                color: color(at).unwrap_or("#9467bd").to_owned(),
                points: self
                    .threads
                    .iter()
                    .map(|threads| self.point(spec, cache, *threads))
                    .collect(),
            })
            .collect();
        Chart {
            file: spec.file(),
            title: spec.title(self.clients, self.operations),
            x_title: spec.x_title().to_owned(),
            y_title: spec.y_title(),
            x_series: self.threads.clone(),
            series,
        }
    }

    /// Every chart, in the order the original draws them.
    #[must_use]
    pub fn charts(&self) -> Vec<Chart> {
        Spec::all().into_iter().map(|s| self.chart(s)).collect()
    }

    /// One bar.
    fn point(&self, spec: Spec, cache: &str, threads: u32) -> Option<i64> {
        if let Some(case) = spec.case
            && case.drops(cache, threads)
        {
            return self.absent();
        }
        let key = (
            cache,
            threads,
            spec.pipeline,
            spec.metric.needs_perf(),
            spec.kind.name(),
        );
        let Some(run) = self.cells.get(&key) else {
            return self.absent();
        };
        match spec.metric {
            Metric::Throughput(which) => Some(kops(half(run, which).opsec.0)),
            Metric::Latency(percentile, which) => {
                Some(micros(&half(run, which).latency, percentile))
            }
            Metric::CpuCycles => {
                let cycles = run.perf.cycles.as_ref()?;
                // A counter the machine could not measure reads as zero, and a zero cycles per operation bar is a claim that the engine ran for free.
                if !cycles.is_measured() && !self.compat.is_upstream() {
                    return None;
                }
                Some(nearest(cycles.as_f64() / self.both_passes))
            }
        }
    }

    /// What to plot where there is nothing to plot.
    ///
    /// The original has no way to say nothing, so it says zero, and a zero bar on a log scale takes the whole axis with it.
    fn absent(&self) -> Option<i64> {
        self.compat.is_upstream().then_some(0)
    }
}

/// One half of a run.
const fn half(run: &Run, which: Which) -> &Op {
    match which {
        Which::Gets => &run.gets,
        Which::Sets => &run.sets,
    }
}

/// Throughput in thousands of operations per second.
///
/// Truncated twice, once on the way out of the file and once by the division, so 218689.490 operations per second is plotted as 218 and not as 219. Both truncations are the original's and both are kept, because the two charts are meant to be comparable and a bar that is a thousand operations taller is still a difference.
fn kops(opsec: f64) -> i64 {
    whole(opsec.trunc()) / 1000
}

/// Latency in whole microseconds.
///
/// The file holds milliseconds with three decimal places, and the thousandfold happens here and nowhere else.
fn micros(latency: &Latency, percentile: Percentile) -> i64 {
    let ms = match percentile {
        Percentile::Min => latency.min,
        Percentile::Max => latency.max,
        Percentile::Avg => latency.avg,
        Percentile::P50 => latency.p50_00,
        Percentile::P90 => latency.p90_00,
        Percentile::P99 => latency.p99_00,
        Percentile::P999 => latency.p99_90,
        Percentile::P9999 => latency.p99_99,
    };
    nearest(ms.0 * 1000.0)
}

/// Round to a whole number the way Go's `%.0f` does.
///
/// Both round to nearest and send a tie to the even digit, and both work on the exact value of the double rather than on a decimal approximation of it, so this is the same operation rather than a close one. It matters because latency in milliseconds times a thousand lands on a tie often, three decimal places being what the file holds.
fn nearest(v: f64) -> i64 {
    whole(v.round_ties_even())
}

/// A float that is already whole, as an integer.
///
/// Every bar on every chart comes through here. They are microseconds, cycles per operation and thousands of operations per second, so the largest is in the millions and nothing is lost. It is a named function so that the one place in this crate where a float becomes an integer is somewhere a reader can find.
#[allow(clippy::cast_possible_truncation)]
fn whole(v: f64) -> i64 {
    v as i64
}

/// An operation count as a float.
///
/// Counts run to tens of millions, which a double holds exactly.
#[allow(clippy::cast_precision_loss)]
fn widen(v: u64) -> f64 {
    v as f64
}

/// A results file the chart layer cannot work with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadCorpus {
    /// Nothing in it.
    Empty,
    /// More cache servers than colours.
    TooManyCaches(TooManyCaches),
}

impl fmt::Display for BadCorpus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the results file has no entries in it"),
            Self::TooManyCaches(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BadCorpus {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::{Compat, Entry, Output, Run};

    use super::{Corpus, kops, nearest};
    use crate::spec::{Case, Metric, Percentile, Scale, Spec, Which};

    use cb_core::golden::CHOSEN;

    // A cell built out of a real chosen file with the numbers moved, so that the shape is the original's and the values are ones a test can reason about.
    fn cell(cache: &str, threads: u32, pipeline: u32, perf: bool, gets: f64, cycles: f64) -> Entry {
        let mut run = Run::parse(CHOSEN).unwrap();
        run.info.cache = cache.to_owned();
        run.info.threads = threads;
        run.info.pipeline = pipeline;
        run.info.kind = Some("median".to_owned());
        run.gets.opsec.0 = gets;
        run.sets.opsec.0 = gets / 2.0;
        run.gets.latency.p99_00.0 = 0.5;
        run.perf = if perf {
            let mut p = run.perf;
            p.cycles = Some(cb_core::EventCounter::Number(cycles));
            p
        } else {
            cb_core::Perf::default()
        };
        Entry {
            file: format!(
                "bench_{cache}-threads_{threads}-pipeline_{pipeline}-perf_yes-run_median.json"
            ),
            data: run,
        }
    }

    fn corpus(entries: Vec<Entry>) -> Output {
        Output { entries }
    }

    fn spec(metric: Metric, case: Option<Case>) -> Spec {
        Spec {
            metric,
            pipeline: 1,
            kind: cb_core::Chosen::Median,
            scale: Scale::Linear,
            case,
        }
    }

    // The original divides in integer arithmetic after truncating, so a bar loses whatever is under a thousand rather than rounding to the nearest.
    #[test]
    fn throughput_truncates_to_whole_thousands() {
        assert_eq!(kops(218_689.490), 218);
        assert_eq!(kops(999.999), 0);
        assert_eq!(kops(2_935_000.0), 2935);
    }

    // Milliseconds with three places times a thousand lands on a tie often, and Go sends a tie to the even digit.
    #[test]
    fn microseconds_round_a_tie_to_even() {
        assert_eq!(nearest(191.5), 192);
        assert_eq!(nearest(190.5), 190);
        assert_eq!(nearest(191.4), 191);
    }

    // Throughput and latency come from the half measured without counters, and cycles from the half measured with them, and the two never mix.
    #[test]
    fn the_two_halves_of_the_matrix_stay_apart() {
        let out = corpus(vec![
            cell("redis", 1, 1, false, 100_000.0, 0.0),
            cell("redis", 1, 1, true, 90_000.0, 1_000_000.0),
        ]);
        let c = Corpus::new(&out, Compat::Corrected).unwrap();
        let throughput = c.chart(spec(Metric::Throughput(Which::Gets), None));
        assert_eq!(throughput.series[0].points, [Some(100)]);
        let cycles = c.chart(spec(Metric::CpuCycles, None));
        // A million cycles over two passes of 25600000 operations each rounds to nothing.
        assert_eq!(cycles.series[0].points, [Some(0)]);
    }

    // A cell nobody measured is a bar that is not drawn, and in upstream mode it is a zero, because that is what the original plots.
    #[test]
    fn a_missing_cell_is_a_gap_here_and_a_zero_there() {
        let out = corpus(vec![
            cell("redis", 1, 1, false, 100_000.0, 0.0),
            cell("redis", 2, 1, false, 200_000.0, 0.0),
            cell("valkey", 1, 1, false, 300_000.0, 0.0),
        ]);
        let ours = Corpus::new(&out, Compat::Corrected).unwrap();
        let chart = ours.chart(spec(Metric::Throughput(Which::Gets), None));
        assert_eq!(chart.x_series, [1, 2]);
        assert_eq!(chart.series[1].cache, "valkey");
        assert_eq!(chart.series[1].points, [Some(300), None]);

        let theirs = Corpus::new(&out, Compat::Upstream).unwrap();
        let chart = theirs.chart(spec(Metric::Throughput(Which::Gets), None));
        assert_eq!(chart.series[1].points, [Some(300), Some(0)]);
    }

    // The one chart pair the original carves out by hand, where Garnet's single thread bar is left off so the other five are readable.
    #[test]
    fn the_special_case_drops_the_bar_it_names() {
        let out = corpus(vec![
            cell("garnet", 1, 1, false, 100_000.0, 0.0),
            cell("garnet", 2, 1, false, 200_000.0, 0.0),
            cell("redis", 1, 1, false, 300_000.0, 0.0),
            cell("redis", 2, 1, false, 400_000.0, 0.0),
        ]);
        let metric = Metric::Latency(Percentile::P99, Which::Gets);
        let c = Corpus::new(&out, Compat::Corrected).unwrap();
        let plain = c.chart(spec(metric, None));
        assert_eq!(plain.series[0].points, [Some(500), Some(500)]);
        let carved = c.chart(spec(metric, Some(Case::NoGarnetAtOneThread)));
        assert_eq!(carved.series[0].cache, "garnet");
        assert_eq!(carved.series[0].points, [None, Some(500)]);
        assert_eq!(carved.series[1].points, [Some(500), Some(500)]);
    }

    // A counter the machine could not measure reads as zero through the original's accessor, and a zero cycles per operation bar says the engine ran for free.
    #[test]
    fn an_unmeasured_counter_is_a_gap_rather_than_a_free_lunch() {
        let mut entry = cell("redis", 1, 1, true, 100_000.0, 0.0);
        entry.data.perf.cycles = Some(cb_core::EventCounter::Text("<not supported>".to_owned()));
        let out = corpus(vec![entry]);
        let ours = Corpus::new(&out, Compat::Corrected).unwrap();
        assert_eq!(
            ours.chart(spec(Metric::CpuCycles, None)).series[0].points,
            [None]
        );
        let theirs = Corpus::new(&out, Compat::Upstream).unwrap();
        assert_eq!(
            theirs.chart(spec(Metric::CpuCycles, None)).series[0].points,
            [Some(0)]
        );
    }

    // Legend order and colours come from the order the results file first mentions a server, which is the original's rule.
    #[test]
    fn the_legend_is_in_the_order_the_file_mentions_them() {
        let out = corpus(vec![
            cell("valkey", 1, 1, false, 100_000.0, 0.0),
            cell("dragonfly", 1, 1, false, 200_000.0, 0.0),
        ]);
        let c = Corpus::new(&out, Compat::Corrected).unwrap();
        assert_eq!(c.caches(), ["valkey", "dragonfly"]);
        let chart = c.chart(spec(Metric::Throughput(Which::Gets), None));
        assert_eq!(chart.series[0].color, "#ff7f0e");
        assert_eq!(chart.series[1].color, "#d62728");
    }

    #[test]
    fn an_empty_results_file_is_an_error() {
        let out = corpus(vec![]);
        assert!(Corpus::new(&out, Compat::Corrected).is_err());
    }
}
