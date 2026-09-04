//! The reduction as it should be.
//!
//! Sort each series by its own key, drop ten percent from each end, take the run in the middle.
//! Every step of that sentence is one the original gets wrong somewhere, and the four ways it does are in `divergences.md` rather than here.
//!
//! Two decisions worth stating, because both could reasonably have gone the other way.
//!
//! The first is that a chosen file is a run rather than a summary. `median` selects the run whose ops per second is the median and reports that run's latencies, rather than taking a median of each field independently. A row assembled from the best of one run and the p99 of another describes no run that ever happened, and every number in it would be defensible while the row as a whole was fiction.
//!
//! The second is that SET, GET and the counters are selected independently of each other. The original does that too, and it is right: they are three separate measurements taken in sequence, and the run with the median SET throughput has no particular reason to be the run with the median GET throughput.

use cb_core::num::{Counter, Fixed3};
use cb_core::run::{Info, Latency, Op, Perf, Run};
use cb_core::spread::{Dispersion, PerfSpread, Spread};

use crate::cell::{BadCell, check, count, trim_for};
use crate::kind::Kind;

/// Reduce a cell to one of the four aggregates.
///
/// # Errors
///
/// If the runs are not one cell. See [`BadCell`].
pub fn choose(runs: &[Run], kind: Kind) -> Result<Run, BadCell> {
    check(runs)?;
    let trim = trim_for(runs.len());
    let end = runs.len() - trim;

    let mut sets: Vec<&Op> = runs.iter().map(|r| &r.sets).collect();
    let mut gets: Vec<&Op> = runs.iter().map(|r| &r.gets).collect();
    sets.sort_by(|a, b| a.opsec.0.total_cmp(&b.opsec.0));
    gets.sort_by(|a, b| a.opsec.0.total_cmp(&b.opsec.0));
    let sets = &sets[trim..end];
    let gets = &gets[trim..end];

    // Sorted by its own key, which is the whole of the fix. The original sorts this one by nothing at all and then indexes it as if it had.
    let mut perf: Vec<&Perf> = runs.iter().map(|r| &r.perf).collect();
    perf.sort_by(|a, b| cycles(a).total_cmp(&cycles(b)));
    let perf = &perf[trim..end];

    let (sets, gets, perf) = if kind == Kind::Average {
        (mean_op(sets), mean_op(gets), mean_perf(perf))
    } else {
        let at = index(kind, sets.len());
        let op = |series: &[&Op]| series.get(at).map(|op| (*op).clone()).ok_or(BadCell::Empty);
        let counters = perf.get(at).map(|p| (*p).clone()).ok_or(BadCell::Empty)?;
        (op(sets)?, op(gets)?, counters)
    };

    Ok(Run {
        info: info_for(runs, kind)?,
        sets,
        gets,
        perf,
        spread: Some(spread(runs)),
    })
}

/// How noisy the cell was.
///
/// Over every run, including the ones the trim removes, because the trim exists to keep an outlier out of the chosen number rather than to pretend it never happened.
#[must_use]
pub fn spread(runs: &[Run]) -> Spread {
    let opsec = |op: &Op| op.opsec.0;
    Spread {
        n: runs.len(),
        trim: trim_for(runs.len()),
        sets: dispersion(&runs.iter().map(|r| opsec(&r.sets)).collect::<Vec<_>>()),
        gets: dispersion(&runs.iter().map(|r| opsec(&r.gets)).collect::<Vec<_>>()),
        perf: runs.iter().all(|r| r.perf.has_cycles()).then(|| {
            let cycles: Vec<f64> = runs.iter().map(|r| self::cycles(&r.perf)).collect();
            let (sd, cv) = deviation(&cycles);
            PerfSpread {
                cycles_sd: Fixed3(sd),
                cycles_cv: Fixed3(cv),
            }
        }),
    }
}

/// The dispersion of one series.
fn dispersion(values: &[f64]) -> Dispersion {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let (sd, cv) = deviation(values);
    Dispersion {
        opsec_p25: Fixed3(rank(&sorted, 25)),
        opsec_p75: Fixed3(rank(&sorted, 75)),
        opsec_sd: Fixed3(sd),
        opsec_cv: Fixed3(cv),
    }
}

/// A percentile by nearest rank, which is the definition that always names a value that was actually measured.
fn rank(sorted: &[f64], percentile: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at = (percentile * sorted.len()).div_ceil(100).max(1) - 1;
    sorted.get(at).copied().unwrap_or_default()
}

/// Standard deviation and the same figure over the mean.
///
/// The population form, because a cell is not a sample of a larger set of runs, it is all the runs there are.
fn deviation(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = count(values.len());
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let sd = variance.sqrt();
    // A mean of zero means a cell that measured nothing, and a ratio against it says nothing either.
    let cv = if mean.abs() < f64::EPSILON {
        0.0
    } else {
        sd / mean
    };
    (sd, cv)
}

/// Which element of the trimmed series each kind takes.
///
/// The lower median, so that an even count takes the smaller of the two middle runs rather than inventing a value between them. Both are runs that happened, and the lower one is the conservative reading of a throughput number.
const fn index(kind: Kind, n: usize) -> usize {
    match kind {
        Kind::Worst => 0,
        Kind::Median => n.saturating_sub(1) / 2,
        // Average never gets here, and best is the last.
        Kind::Best | Kind::Average => n.saturating_sub(1),
    }
}

/// The cycle count, or zero for a run measured without perf.
fn cycles(perf: &Perf) -> f64 {
    perf.cycles.as_ref().map_or(0.0, Counter::as_f64)
}

/// The info block a chosen file carries.
///
/// The cell's own settings, with the aggregate named and the start time dropped. A chosen file describes a cell rather than a run, and a cell did not start at a time.
fn info_for(runs: &[Run], kind: Kind) -> Result<Info, BadCell> {
    let mut info = runs.first().ok_or(BadCell::Empty)?.info.clone();
    info.run_started = None;
    info.kind = Some(kind.name().to_owned());
    Ok(info)
}

/// The componentwise mean of a series.
fn mean_op(ops: &[&Op]) -> Op {
    let n = count(ops.len());
    let mean = |f: fn(&Op) -> f64| Fixed3(ops.iter().map(|op| f(op)).sum::<f64>() / n);
    Op {
        opsec: mean(|op| op.opsec.0),
        mbsec: mean(|op| op.mbsec.0),
        latency: Latency {
            min: mean(|op| op.latency.min.0),
            max: mean(|op| op.latency.max.0),
            avg: mean(|op| op.latency.avg.0),
            p50_00: mean(|op| op.latency.p50_00.0),
            p90_00: mean(|op| op.latency.p90_00.0),
            p99_00: mean(|op| op.latency.p99_00.0),
            p99_90: mean(|op| op.latency.p99_90.0),
            p99_99: mean(|op| op.latency.p99_99.0),
        },
    }
}

/// The componentwise mean of the counters, and `{}` when the cell was measured without perf.
fn mean_perf(perfs: &[&Perf]) -> Perf {
    if !perfs.iter().all(|p| p.has_cycles()) {
        return Perf::default();
    }
    // Written out one field at a time because a closure cannot be generic over the decimal places, and the places are part of the counter's type.
    let n = count(perfs.len());
    Perf {
        cpu_utilized: mean_counter(perfs, |p| p.cpu_utilized.as_ref(), n),
        cycles: mean_counter(perfs, |p| p.cycles.as_ref(), n),
        secsuser: mean_counter(perfs, |p| p.secsuser.as_ref(), n),
        secssys: mean_counter(perfs, |p| p.secssys.as_ref(), n),
        instructions: mean_counter(perfs, |p| p.instructions.as_ref(), n),
        branches: mean_counter(perfs, |p| p.branches.as_ref(), n),
        branch_misses: mean_counter(perfs, |p| p.branch_misses.as_ref(), n),
        page_faults: mean_counter(perfs, |p| p.page_faults.as_ref(), n),
    }
}

/// The mean of one counter across the cell.
///
/// `None` when any run is missing it, because a mean over some of the runs is not the mean of the cell.
fn mean_counter<const PLACES: usize>(
    perfs: &[&Perf],
    pick: fn(&Perf) -> Option<&Counter<PLACES>>,
    n: f64,
) -> Option<Counter<PLACES>> {
    let mut total = 0.0;
    for perf in perfs {
        let counter = pick(perf)?;
        // A counter the hardware cannot measure has no mean, so the text survives instead of a zero being averaged in. That is what lets the chart layer leave the cell out rather than draw a bar saying the engine took no branches.
        if !counter.is_measured() {
            return Some(counter.clone());
        }
        total += counter.as_f64();
    }
    Some(Counter::Number(total / n))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use cb_core::num::Counter;
    use cb_core::run::Run;

    use super::choose;
    use crate::cell::BadCell;
    use crate::fixture::{with_perf, without_perf};
    use crate::kind::Kind;

    fn gets(run: &Run) -> f64 {
        run.gets.opsec.0
    }

    fn sets(run: &Run) -> f64 {
        run.sets.opsec.0
    }

    fn cycles(run: &Run) -> f64 {
        run.perf.cycles.as_ref().map_or(0.0, Counter::as_f64)
    }

    // A chosen file is a run that happened. Every number in a median row comes from the same run, because a row assembled out of the throughput of one run and the p99 of another describes nothing that was measured.
    #[test]
    fn every_selected_number_comes_from_a_run() {
        for cell in [with_perf(), without_perf()] {
            for kind in [Kind::Median, Kind::Best, Kind::Worst] {
                let out = choose(&cell.runs, kind).unwrap();
                assert!(
                    cell.runs.iter().any(|r| r.gets == out.gets),
                    "{kind} gets is not one of the runs"
                );
                assert!(
                    cell.runs.iter().any(|r| r.sets == out.sets),
                    "{kind} sets is not one of the runs"
                );
            }
        }
    }

    #[test]
    fn best_is_at_least_median_is_at_least_worst() {
        for cell in [with_perf(), without_perf()] {
            let at = |kind| choose(&cell.runs, kind).unwrap();
            let (best, median, worst) = (at(Kind::Best), at(Kind::Median), at(Kind::Worst));
            assert!(gets(&best) >= gets(&median) && gets(&median) >= gets(&worst));
            assert!(sets(&best) >= sets(&median) && sets(&median) >= sets(&worst));
        }
    }

    // The median of 31 runs trimmed to 25 is the 16th of the 31, and the original picks the 17th.
    // Both cells, because the off by one is in the index arithmetic and applies whether perf was attached or not.
    #[test]
    fn the_median_is_the_middle_run_and_the_originals_is_one_above_it() {
        for cell in [with_perf(), without_perf()] {
            let ours = choose(&cell.runs, Kind::Median).unwrap();
            let sorted = cell.sorted(gets);
            assert_eq!(gets(&ours), sorted[15]);
            assert_eq!(gets(cell.upstream(Kind::Median)), sorted[16]);
        }
    }

    // The published SET row of this cell is the 8th slowest run of 31, and it is labelled median.
    // That is the sort whose comparator reads the perf slice while it permutes the sets slice, and this is it landing in data that was published.
    #[test]
    fn the_originals_set_row_is_not_a_median_at_all_when_perf_was_attached() {
        let cell = with_perf();
        let sorted = cell.sorted(sets);
        let theirs = sets(cell.upstream(Kind::Median));
        let rank = sorted.iter().position(|v| *v == theirs).unwrap();
        assert_eq!(
            rank, 7,
            "the original's SET median sits at rank {rank} of 31"
        );
        assert_eq!(sets(&choose(&cell.runs, Kind::Median).unwrap()), sorted[15]);
    }

    // The counters are sorted by cycles before being indexed, which is the one the original never does at all.
    // Its published cycles number is whichever run happened to be 17th in run order, and here that is the 12th lowest of 31.
    #[test]
    fn the_counters_are_sorted_before_they_are_indexed() {
        let cell = with_perf();
        let sorted = cell.sorted(cycles);
        let ours = choose(&cell.runs, Kind::Median).unwrap();
        assert_eq!(cycles(&ours), sorted[15]);

        let theirs = cycles(cell.upstream(Kind::Median));
        assert_eq!(theirs, cycles(&cell.runs[16]));
        assert_eq!(sorted.iter().position(|v| *v == theirs).unwrap(), 11);
    }

    #[test]
    fn the_average_is_the_mean_of_what_survived_the_trim() {
        let cell = with_perf();
        let out = choose(&cell.runs, Kind::Average).unwrap();
        let sorted = cell.sorted(gets);
        let want = sorted[3..28].iter().sum::<f64>() / 25.0;
        assert_eq!(out.gets.opsec.to_string(), format!("{want:.3}"));

        // Every kind sees all 31 runs, so the count that goes into the mean is the trimmed 25 rather than the 15 the original ends up with.
        assert_eq!(out.spread.as_ref().unwrap().n, 31);
    }

    // This cell was measured on a machine with no branch counter, so every run says `<not supported>`.
    // The original averages that to a zero and draws it as a bar. Here it stays unmeasured through both the selection and the mean, which is the whole of what the chart layer needs to leave it out.
    #[test]
    fn an_unsupported_counter_stays_unmeasured() {
        for kind in Kind::ALL {
            let out = choose(&with_perf().runs, kind).unwrap();
            let branches = out.perf.branches.unwrap();
            assert!(!branches.is_measured(), "{kind} lost the distinction");
            assert_eq!(branches.as_f64(), 0.0);
            assert!(out.perf.cycles.unwrap().is_measured());
        }
    }

    #[test]
    fn a_cell_measured_without_perf_keeps_an_empty_perf_object() {
        for kind in Kind::ALL {
            let out = choose(&without_perf().runs, kind).unwrap();
            assert!(!out.perf.has_cycles());
            assert!(out.emit().contains(r#""perf": {}"#));
            assert!(out.spread.unwrap().perf.is_none());
        }
    }

    #[test]
    fn the_chosen_file_names_its_kind_and_carries_the_spread() {
        for kind in Kind::ALL {
            let out = choose(&with_perf().runs, kind).unwrap();
            assert_eq!(out.info.kind.as_deref(), Some(kind.name()));
            let text = out.emit();
            assert!(text.contains(&format!(r#""kind":"{kind}""#)));
            assert!(text.contains(r#"  "spread": {"n":31,"trim":3,"#));
        }
    }

    #[test]
    fn the_spread_describes_the_whole_cell_including_the_runs_the_trim_drops() {
        let cell = with_perf();
        let spread = super::spread(&cell.runs);
        assert_eq!((spread.n, spread.trim), (31, 3));

        let sorted = cell.sorted(gets);
        assert_eq!(spread.gets.opsec_p25.0, sorted[7]);
        assert_eq!(spread.gets.opsec_p75.0, sorted[23]);
        // A quiet box. Anything over a percent or two here is a cell measured while something else was running.
        assert!(spread.gets.opsec_cv.0 < 0.01, "{}", spread.gets.opsec_cv);
        assert!(spread.perf.unwrap().cycles_cv.0 < 0.01);
    }

    // Under eleven runs nothing is trimmed, so every run is a candidate and the median of five is the third.
    #[test]
    fn a_short_cell_is_not_trimmed() {
        let cell = with_perf();
        let five = &cell.runs[..5];
        let out = choose(five, Kind::Median).unwrap();
        let mut sorted: Vec<f64> = five.iter().map(gets).collect();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(gets(&out), sorted[2]);
        assert_eq!(out.spread.unwrap().trim, 0);
    }

    // The runs of a cell are gathered by filename, and a filename is a claim rather than a fact.
    #[test]
    fn runs_from_two_different_cells_are_refused() {
        let cell = with_perf();
        let mut runs = cell.runs.clone();
        runs[4].info.threads = 8;
        assert!(matches!(
            choose(&runs, Kind::Median),
            Err(BadCell::NotOneCell { .. })
        ));
    }

    #[test]
    fn a_cell_measured_two_different_ways_is_refused() {
        let mut runs = with_perf().runs;
        runs[0].perf = cb_core::run::Perf::default();
        assert!(matches!(
            choose(&runs, Kind::Median),
            Err(BadCell::MixedPerf {
                with: 30,
                total: 31
            })
        ));
    }

    #[test]
    fn a_cell_with_no_runs_is_refused() {
        assert_eq!(choose(&[], Kind::Median), Err(BadCell::Empty));
    }
}
