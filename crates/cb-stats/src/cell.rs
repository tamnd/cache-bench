//! A cell, which is the runs of one point on one chart.
//!
//! Everything that both modes need: what a cell is, what makes one unusable, and how many runs come off each end before anything is selected.

use cb_core::run::Run;

/// How many runs come off each end.
///
/// Ten percent, rounded down, and nothing at all below eleven runs. The original's rule, kept, because the trim is part of the published methodology rather than a detail of the implementation.
#[must_use]
pub const fn trim_for(n: usize) -> usize {
    if n > 10 { n / 10 } else { 0 }
}

/// Anything that stops a cell being reducible.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadCell {
    /// No runs at all.
    #[error("a cell with no runs in it cannot be reduced")]
    Empty,
    /// The runs are not all the same measurement.
    #[error(
        "these runs are not one cell, {first} and {other} differ, so reducing them would produce a number describing nothing that was measured"
    )]
    NotOneCell {
        /// What the first run is.
        first: String,
        /// The first one that does not match it.
        other: String,
    },
    /// Some runs have counters and some do not.
    #[error(
        "{with} of {total} runs have perf counters, so the cell was measured two different ways and the counters cannot be reduced"
    )]
    MixedPerf {
        /// How many have cycles.
        with: usize,
        /// How many runs there are.
        total: usize,
    },
    /// Fewer runs than the sweep says the cell should have.
    #[error("the cell has {found} runs and the sweep says {wanted}")]
    Short {
        /// How many are there.
        found: usize,
        /// How many were asked for.
        wanted: usize,
    },
}

/// What a cell is, said in a way a message can carry.
fn identity(run: &Run) -> String {
    format!(
        "{} threads {} pipeline {}",
        run.info.cache, run.info.threads, run.info.pipeline
    )
}

/// Whether these runs are all the same measurement, and all measured the same way.
///
/// Worth checking because the runs of a cell are gathered by filename, and a filename is a claim rather than a fact. A cell assembled out of two different cells reduces to a number describing nothing that happened.
///
/// # Errors
///
/// If the cell is empty, if the runs are not all the same measurement, or if only some of them carry counters.
pub fn check(runs: &[Run]) -> Result<(), BadCell> {
    let first = runs.first().ok_or(BadCell::Empty)?;
    for run in runs {
        if run.info.cache != first.info.cache
            || run.info.threads != first.info.threads
            || run.info.pipeline != first.info.pipeline
        {
            return Err(BadCell::NotOneCell {
                first: identity(first),
                other: identity(run),
            });
        }
    }
    let with = runs.iter().filter(|r| r.perf.has_cycles()).count();
    if with != 0 && with != runs.len() {
        return Err(BadCell::MixedPerf {
            with,
            total: runs.len(),
        });
    }
    Ok(())
}

/// Turn a count into a divisor.
///
/// A run count is a small integer and every one of them is exact in a double. This is a named function so that the one place in this crate that could lose precision is somewhere a reader can find.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub(crate) const fn count(n: usize) -> f64 {
    n as f64
}

#[cfg(test)]
mod tests {
    use super::trim_for;

    // The trim boundary, which is where an off by one would hide.
    #[test]
    fn ten_percent_and_nothing_under_eleven() {
        for (n, want) in [
            (1, 0),
            (5, 0),
            (10, 0),
            (11, 1),
            (19, 1),
            (20, 2),
            (31, 3),
            (100, 10),
        ] {
            assert_eq!(trim_for(n), want, "{n} runs");
        }
    }
}
