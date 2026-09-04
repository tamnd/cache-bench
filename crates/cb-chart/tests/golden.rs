//! The 154 charts, checked against the ones the original drew.
//!
//! The fixture is the original's own answer, not a description of it. Its `graph` tool pastes the numbers it picked into a Python script and deletes the script after drawing, so `tools/series-vectors` stands in for Python, keeps the script and throws away the picture. What is committed is the header of all 154 of them.
//!
//! What runs here is everything that does not need the measurements: the filenames, the titles, both axis labels, the thread counts, the legend order and the colours. That is the whole of the chart layer's judgement except the bar heights, and it runs in CI on a checkout with no results in it.
//!
//! The bar heights need the original's 1.7 MB `output.json`, which is measurement data and does not belong in this repository, so `cache-bench verify --against` does that half against a checkout.

#![allow(clippy::unwrap_used, reason = "a failed fixture is a failed test")]

use cb_chart::{Chart, Corpus, Spec};
use cb_core::{Compat, Entry, Info, Latency, Op, Output, Perf, Run};

const SERIES: &str = include_str!("../../../testdata/golden/series.json");

/// The six the original measured, in the order sorted result filenames produce.
const CACHES: [&str; 6] = [
    "dragonfly",
    "garnet",
    "memcache",
    "pogocache",
    "redis",
    "valkey",
];

/// The sweep's thread counts.
const THREADS: [u32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16];

fn golden() -> Vec<Chart> {
    serde_json::from_str(SERIES).unwrap()
}

/// A results file with the original's shape and none of its numbers.
///
/// Every cell the original measured, all 576 of them, holding zeroes. That is enough to settle everything about a chart except how tall the bars are, and it means this test needs no measurement data to run.
fn shaped_like_the_original() -> Output {
    let mut entries = Vec::new();
    for cache in CACHES {
        for threads in THREADS {
            for pipeline in Spec::PIPELINES {
                for perf in [false, true] {
                    entries.push(cell(cache, threads, pipeline, perf));
                }
            }
        }
    }
    Output { entries }
}

fn cell(cache: &str, threads: u32, pipeline: u32, perf: bool) -> Entry {
    let zeros = Op {
        opsec: cb_core::Fixed3(0.0),
        mbsec: cb_core::Fixed3(0.0),
        latency: Latency {
            min: cb_core::Fixed3(0.0),
            max: cb_core::Fixed3(0.0),
            avg: cb_core::Fixed3(0.0),
            p50_00: cb_core::Fixed3(0.0),
            p90_00: cb_core::Fixed3(0.0),
            p99_00: cb_core::Fixed3(0.0),
            p99_90: cb_core::Fixed3(0.0),
            p99_99: cb_core::Fixed3(0.0),
        },
    };
    let mut counters = Perf::default();
    if perf {
        counters.cycles = Some(cb_core::EventCounter::Number(0.0));
    }
    let run = Run {
        info: Info {
            cache: cache.to_owned(),
            version: format!("{cache} 0.0.0"),
            threads,
            bench_threads: 16,
            connections: 256,
            operations: 25_600_000,
            sizerange: "1-1024".to_owned(),
            pipeline,
            profile: None,
            run_started: None,
            kind: Some("median".to_owned()),
        },
        sets: zeros.clone(),
        gets: zeros,
        perf: counters,
        spread: None,
    };
    let yes_no = if perf { "yes" } else { "no" };
    Entry {
        file: format!(
            "bench_{cache}-threads_{threads}-pipeline_{pipeline}-perf_{yes_no}-run_median.json"
        ),
        data: run,
    }
}

// The count is part of the exit condition for this milestone. Not roughly all of them, and not however many a directory happens to yield: the same 154 the original published, with the same names.
#[test]
fn the_set_of_charts_is_the_originals_set() {
    let mut ours: Vec<String> = Spec::all().into_iter().map(Spec::file).collect();
    let mut theirs: Vec<String> = golden().into_iter().map(|c| c.file).collect();
    ours.sort();
    theirs.sort();
    assert_eq!(ours.len(), 154);
    assert_eq!(ours, theirs);
}

// Everything a reader sees except the bars.
#[test]
fn every_chart_carries_the_originals_titles_axes_and_legend() {
    let output = shaped_like_the_original();
    let corpus = Corpus::new(&output, Compat::Upstream).unwrap();
    let mut ours: Vec<Chart> = corpus.charts();
    ours.sort_by(|a, b| a.file.cmp(&b.file));
    let theirs = golden();
    assert_eq!(ours.len(), theirs.len());

    for (ours, theirs) in ours.iter().zip(&theirs) {
        assert_eq!(ours.file, theirs.file);
        assert_eq!(ours.title, theirs.title, "{}", theirs.file);
        assert_eq!(ours.x_title, theirs.x_title, "{}", theirs.file);
        assert_eq!(ours.y_title, theirs.y_title, "{}", theirs.file);
        assert_eq!(ours.x_series, theirs.x_series, "{}", theirs.file);
        let names: Vec<&str> = ours.series.iter().map(|s| s.cache.as_str()).collect();
        let want: Vec<&str> = theirs.series.iter().map(|s| s.cache.as_str()).collect();
        assert_eq!(names, want, "{}", theirs.file);
        let colors: Vec<&str> = ours.series.iter().map(|s| s.color.as_str()).collect();
        let wanted: Vec<&str> = theirs.series.iter().map(|s| s.color.as_str()).collect();
        assert_eq!(colors, wanted, "{}", theirs.file);
        // Same number of bars in every group, one per thread count, drawn or not.
        for series in &ours.series {
            assert_eq!(
                series.points.len(),
                theirs.x_series.len(),
                "{}",
                theirs.file
            );
        }
    }
}

// The fixture itself should not drift. If somebody regenerates it against a different results directory these numbers move, and the failure should say so rather than showing up as 154 unrelated differences.
#[test]
fn the_fixture_is_the_corpus_it_claims_to_be() {
    let charts = golden();
    assert_eq!(charts.len(), 154);
    for chart in &charts {
        assert_eq!(chart.x_series, THREADS);
        assert_eq!(chart.x_title, "Threads");
        let names: Vec<&str> = chart.series.iter().map(|s| s.cache.as_str()).collect();
        assert_eq!(names, CACHES);
        for series in &chart.series {
            assert_eq!(series.points.len(), THREADS.len());
            assert!(series.points.iter().all(Option::is_some));
        }
    }
}
