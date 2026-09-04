//! The geometry of all 154 charts, checked against the numbers matplotlib was given.
//!
//! The fixture is the original's own arithmetic rather than a reading of it. `tools/axis-vectors` slices the two chart scripts out of `cmd/graph/main.go`, runs them against the same 154 charts with matplotlib replaced by something that records what it was told, and writes down the answers. See `crates/cb-core/golden/README.md`.
//!
//! Nothing here needs measurement data. The series fixture already holds every bar height the original plotted, and the axis is a function of those, so this runs in CI on a checkout with no results in it.
//!
//! The comparison itself lives in `cb_chart::golden` rather than here, because `cache-bench verify` makes the same claim from the command line and there should be one definition of what the claim is.

#![allow(clippy::unwrap_used, reason = "a failed fixture is a failed test")]

use cb_chart::{Chart, Golden};
use std::collections::BTreeMap;

use cb_core::golden::{AXES, SERIES};

fn golden() -> Golden {
    Golden::parse(AXES).unwrap()
}

fn series() -> BTreeMap<String, Chart> {
    let charts: Vec<Chart> = serde_json::from_str(SERIES).unwrap();
    charts.into_iter().map(|c| (c.file.clone(), c)).collect()
}

// The numbers the original applies to every chart without looking at what is on it, including the outline colour of each of the six bars.
#[test]
fn the_constants_are_the_originals() {
    let golden = golden();
    assert_eq!(golden.constants.edges.len(), 6);
    assert_eq!(golden.constants(), []);
}

// The whole of it, on all 154, bit for bit. An axis bound, a tick, a gridline and the text on each of them.
#[test]
fn every_axis_is_the_one_the_original_drew() {
    let golden = golden();
    assert_eq!(golden.charts.len(), 154);

    let (tally, wrong) = golden.check(&series());
    for m in &wrong {
        println!("{} {}", m.file, m.what);
    }
    assert_eq!(wrong.len(), 0, "{} charts differ", wrong.len());
    assert_eq!(tally.charts, 154);
    assert!(tally.ticks > 0 && tally.lines > 0);
}

// The two halves of the fixture describe the same 154 charts, so a regeneration of one without the other is caught here rather than as a hundred unrelated differences.
#[test]
fn the_two_fixtures_are_about_the_same_charts() {
    let charts = series();
    let golden = golden();
    let mut ours: Vec<&str> = charts.keys().map(String::as_str).collect();
    let mut theirs: Vec<&str> = golden.charts.iter().map(|c| c.file.as_str()).collect();
    ours.sort_unstable();
    theirs.sort_unstable();
    assert_eq!(ours, theirs);

    for recorded in &golden.charts {
        let logarithmic = recorded.file.contains("scale_logarithmic");
        assert_eq!(
            logarithmic,
            recorded.scale == "logarithmic",
            "{}",
            recorded.file
        );
        // A logarithmic chart labels every gridline in the margin. A linear one draws them and says nothing.
        assert_eq!(
            recorded.gutter.len(),
            if logarithmic { recorded.lines.len() } else { 0 }
        );
    }
}
