//! How noisy the cell was, which the original throws away.
//!
//! Thirty one runs go into a cell and one number comes out of it. Once that has happened, a cell that was measured while something else was running on the box looks exactly like a cell that was not.
//! This is the object that keeps the difference. Nothing plots it, and that is fine, because the question it answers is asked by a reader who does not believe the chart.
//!
//! It is ours rather than the original's, so it is additive, it is omitted in upstream compatibility mode, and the original's tooling ignores it.
//! It sits after `perf` for the same reason `kind` sits last in `info`: a new key goes where a new key would land, so the diff against an original file is one added block rather than a reshuffle.

use serde::{Deserialize, Serialize};

use crate::num::Fixed3;

/// The dispersion of one cell, over every run in it.
///
/// Over every run, including the ones the trim throws away. The trim is there to keep an outlier out of the chosen number, not to pretend it never happened, and a cell with one bad run is exactly what this is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    /// How many runs the cell had.
    pub n: usize,
    /// How many were trimmed from each end before selection.
    pub trim: usize,
    /// The SET pass.
    pub sets: Dispersion,
    /// The GET pass.
    pub gets: Dispersion,
    /// The counters, absent on a cell measured without perf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf: Option<PerfSpread>,
}

/// The dispersion of one half of a cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dispersion {
    /// Lower quartile of operations per second, by nearest rank.
    pub opsec_p25: Fixed3,
    /// Upper quartile of operations per second, by nearest rank.
    pub opsec_p75: Fixed3,
    /// Standard deviation of operations per second, over the whole cell.
    ///
    /// The population form rather than the sample form, because a cell is not a sample of a larger set of runs. It is all the runs there are.
    pub opsec_sd: Fixed3,
    /// Standard deviation over the mean, which is the figure to compare between cells.
    ///
    /// A tenth of a percent is a quiet box. A few percent is a box with something else on it, and the chart cannot tell you that on its own.
    pub opsec_cv: Fixed3,
}

/// The dispersion of the counters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfSpread {
    /// Standard deviation of the cycle count, over the whole cell.
    pub cycles_sd: Fixed3,
    /// Standard deviation of the cycle count over its mean.
    ///
    /// Cycles per operation is far steadier run to run than throughput is, so this one is usually small enough to be boring, and a cell where it is not is a cell worth looking at.
    pub cycles_cv: Fixed3,
}
