//! The two cells the tests run against.
//!
//! Both are the original's own committed runs, unmodified, one cell measured with perf and one without.
//! Thirty one runs and the four files the original reduced them to, so every test in this crate is checked against what the original actually produced rather than against a distribution somebody made up.
//!
//! They are one file per cell rather than seventy loose files because loose run files in a repository are how a results directory ends up committed by accident, and because the answer belongs next to the question.

use std::collections::BTreeMap;

use cb_core::run::Run;
use serde::Deserialize;

/// One cell, and what the original made of it.
#[derive(Debug, Deserialize)]
pub(crate) struct Cell {
    /// The filename stem, minus the run number.
    #[serde(rename = "cell")]
    #[allow(dead_code)]
    pub(crate) name: String,
    /// The runs, in run order, which is the order the original reads them in.
    pub(crate) runs: Vec<Run>,
    /// The four files the original wrote, by kind.
    pub(crate) upstream: BTreeMap<String, Run>,
}

/// A cell measured with perf attached, where the counters are strings and one of them is `<not supported>`.
pub(crate) fn with_perf() -> Cell {
    load(include_str!(
        "../../../testdata/golden/cells/dragonfly-threads_1-pipeline_1-perf_yes.json"
    ))
}

/// A cell measured without perf, where every perf object is `{}`.
pub(crate) fn without_perf() -> Cell {
    load(include_str!(
        "../../../testdata/golden/cells/dragonfly-threads_1-pipeline_1-perf_no.json"
    ))
}

/// Read one, loudly.
///
/// A fixture that does not parse is a broken test rather than a condition to handle, so this says which one and stops.
#[allow(clippy::expect_used)]
fn load(text: &str) -> Cell {
    serde_json::from_str(text).expect("the committed cell fixtures parse")
}

impl Cell {
    /// What the original wrote for one kind.
    #[allow(clippy::expect_used)]
    pub(crate) fn upstream(&self, kind: crate::Kind) -> &Run {
        self.upstream
            .get(kind.name())
            .expect("the fixture has all four kinds")
    }

    /// The runs' ops per second, ascending, for whichever half.
    pub(crate) fn sorted(&self, get: fn(&Run) -> f64) -> Vec<f64> {
        let mut values: Vec<f64> = self.runs.iter().map(get).collect();
        values.sort_by(f64::total_cmp);
        values
    }
}
