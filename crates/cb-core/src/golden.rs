//! The original's own files, in the crate rather than beside it.
//!
//! Every fixture this project is checked against lives in `crates/cb-core/golden` and is read from here, by the tests in four crates and by `cache-bench verify`, which is the same claim made as a command. One copy, one path, and a binary that carries its own evidence so that verifying a build needs nothing on the disk it came from.
//!
//! They are in this crate because this is where the file formats are defined and because everything else in the workspace already depends on it. `golden/README.md` says where each one came from and what it pins down.

/// A single run with perf detached, where `perf` has to come back as an empty object rather than as null.
pub const RUN_PLAIN: &str =
    include_str!("../golden/bench_dragonfly-threads_1-pipeline_1-perf_no-run_1.json");

/// A single run with perf attached, where every counter is a JSON string and one of them is not a number at all.
pub const RUN_PERF: &str =
    include_str!("../golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_1.json");

/// A chosen file, where the same counters are JSON numbers and `kind` has been appended last.
pub const CHOSEN: &str =
    include_str!("../golden/bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json");

/// Three entries lifted out of the published `output.json`, which pin the combined layout and its odd indentation.
pub const COMBINED: &str = include_str!("../golden/output-three-cells.json");

/// One whole cell measured with counters, its 31 runs and the four files the original reduced them to.
pub const CELL_PERF: &str =
    include_str!("../golden/cells/dragonfly-threads_1-pipeline_1-perf_yes.json");

/// The same cell measured without counters, which escapes one of the four defects by accident.
pub const CELL_PLAIN: &str =
    include_str!("../golden/cells/dragonfly-threads_1-pipeline_1-perf_no.json");

/// All 154 charts as the original drew them, taken out of its own generated drawing scripts.
pub const SERIES: &str = include_str!("../golden/series.json");

/// Where the original put all of that, taken by standing in for matplotlib while the same scripts ran.
pub const AXES: &str = include_str!("../golden/axes.json");

/// The SHA-256 of every one of the 154 charts drawn from `SERIES`, which is the determinism proof.
pub const CHARTS: &str = include_str!("../golden/charts.sha256");

/// 142 sort cases produced by Go, which is the one fixture here that was generated rather than copied.
pub const GOSORT: &str = include_str!("../golden/gosort.json");

#[cfg(test)]
mod tests {
    use super::{AXES, CELL_PERF, CHARTS, GOSORT, SERIES};

    // The fixtures are compared byte for byte, so a checkout that rewrote their line endings has to be a test failure here rather than a mystery in four other crates.
    #[test]
    fn nothing_arrived_with_windows_line_endings() {
        for (name, text) in [
            ("series.json", SERIES),
            ("axes.json", AXES),
            ("cells", CELL_PERF),
            ("gosort.json", GOSORT),
            ("charts.sha256", CHARTS),
        ] {
            assert!(!text.contains('\r'), "{name} has carriage returns in it");
        }
    }
}
