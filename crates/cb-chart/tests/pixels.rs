//! The pixels are the ones in the manifest.
//!
//! `cache-bench chart --golden --check` draws all 154 and hashes every one of them, and that is what the determinism job in CI runs. It takes two seconds in release and three quarters of a minute in a debug build, which is too slow to sit in the test matrix, so what runs here is a handful chosen to cover both scales and the awkward cases. A glyph that moved or an encoder that changed its mind shows up on any one of them.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a failed fixture is a failed test"
)]

use std::collections::BTreeMap;

use cb_chart::render::{Stamp, draw};
use cb_chart::{Axis, Chart, Scale};

/// The whole corpus, which is what the charts are drawn from.
const SERIES: &str = include_str!("../../../testdata/golden/series.json");

/// The hash of every chart, written by `cache-bench chart --golden --manifest`.
const MANIFEST: &str = include_str!("../../../testdata/golden/charts.sha256");

/// One chart of each shape the corpus has. Both scales, a counter chart with no GET or SET in it, a latency chart whose axis spans four decades, and the deepest pipeline depth, where the throughput numbers run to seven digits and the y axis column is at its widest.
const SAMPLE: [&str; 4] = [
    "graph_cpucycles-pipeline_1-kind_median-scale_linear.png",
    "graph_latency_p50_00-which_gets-pipeline_10-kind_median-scale_logarithmic.png",
    "graph_opsec-which_sets-pipeline_50-kind_median-scale_linear.png",
    "graph_opsec-which_gets-pipeline_1-kind_median-scale_logarithmic.png",
];

/// The SHA-256 of some bytes, as lower case hex.
fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The committed manifest, by chart name.
fn manifest() -> BTreeMap<&'static str, &'static str> {
    MANIFEST
        .lines()
        .filter_map(|line| line.split_once("  "))
        .map(|(hash, file)| (file, hash))
        .collect()
}

/// Draw one chart and hash the PNG.
fn drawn(chart: &Chart) -> String {
    let scale = if chart.file.contains("scale_logarithmic") {
        Scale::Logarithmic
    } else {
        Scale::Linear
    };
    let axis = Axis::new(scale, chart).expect("the golden charts all have an axis");
    let canvas = draw(chart, &axis, scale, &Stamp::default()).expect("the golden charts all draw");
    let mut png = Vec::new();
    canvas
        .write_png(&mut png)
        .expect("a vector takes the bytes");
    digest(&png)
}

#[test]
fn the_sample_charts_hash_to_what_is_committed() {
    let charts: Vec<Chart> = serde_json::from_str(SERIES).expect("the golden series parses");
    let manifest = manifest();
    for name in SAMPLE {
        let chart = charts
            .iter()
            .find(|c| c.file == name)
            .unwrap_or_else(|| panic!("{name} is in the golden series"));
        let want = manifest
            .get(name)
            .unwrap_or_else(|| panic!("{name} is in the manifest"));
        assert_eq!(
            &drawn(chart),
            want,
            "{name} did not come out as it is committed"
        );
    }
}

// A manifest one line short would let a chart stop being drawn without anything going red.
#[test]
fn the_manifest_covers_the_whole_corpus() {
    let charts: Vec<Chart> = serde_json::from_str(SERIES).expect("the golden series parses");
    let manifest = manifest();
    assert_eq!(manifest.len(), 154);
    for chart in &charts {
        assert!(
            manifest.contains_key(chart.file.as_str()),
            "{} is not in the manifest",
            chart.file
        );
    }
}
