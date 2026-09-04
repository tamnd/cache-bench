//! The reduction as the original performs it, defects and all.
//!
//! This exists so the original's published results stay regenerable. A port that only ships the corrected numbers asks to be taken on trust, and there is no way to check it against anything. With this, the original's own run files go in and its own published `output.json` comes back out byte for byte, and only then is the corrected mode's disagreement with it worth reading.
//!
//! Nothing here should be tidied. Every part of it that looks wrong is wrong on purpose, and `divergences.md` says which parts those are and what each one costs. If a change here makes the parity test fail, the change is the bug.
//!
//! The four aggregates cannot be computed independently, which is D4. The original keeps its run count in a package level variable that the trimming step decrements in place, so the second call sees what the first one left behind. That is why the entry point here produces all four at once rather than taking a kind.

use cb_core::num::{Counter, Fixed3};
use cb_core::run::{Info, Latency, Op, Perf, Run};

use crate::cell::{BadCell, check, count};
use crate::gosort::{Aliased, ByKey, slice};
use crate::kind::Kind;

/// All four aggregates, in the order the original writes them.
///
/// The order matters and the results are not independent. `median` is computed from all 31 runs, `best` from the 25 that survived the median's trim, `worst` from 21 and `average` from 17, because the run count is a global that each call leaves smaller than it found it.
///
/// # Errors
///
/// If the runs are not one cell, or if the cell is small enough that the original's index arithmetic would run off the end of its own slice. See [`BadCell`].
pub fn choose_all(runs: &[Run]) -> Result<[Run; 4], BadCell> {
    check(runs)?;

    // The mutated global, local at last.
    let mut count = runs.len();
    let median = choose_one(runs, Kind::Median, &mut count)?;
    let best = choose_one(runs, Kind::Best, &mut count)?;
    let worst = choose_one(runs, Kind::Worst, &mut count)?;
    let average = choose_one(runs, Kind::Average, &mut count)?;
    Ok([median, best, worst, average])
}

/// One aggregate, the way the original computes it.
///
/// # Errors
///
/// If the runs are not one cell, or if the original would index off the end.
pub fn choose(runs: &[Run], kind: Kind) -> Result<Run, BadCell> {
    let all = choose_all(runs)?;
    let at = Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
    all.into_iter().nth(at).ok_or(BadCell::Empty)
}

/// One call of the original's `choose`, against a run count it will leave smaller.
fn choose_one(all: &[Run], kind: Kind, count: &mut usize) -> Result<Run, BadCell> {
    // Only the first `count` files are opened, so `best` never sees runs 26 to 31 and `average` never sees 18 to 31. This is the half of D4 that a trimming error does not describe.
    let runs = all.get(..*count).ok_or(BadCell::Empty)?;
    let info = runs.last().ok_or(BadCell::Empty)?.info.clone();

    let mut gets: Vec<Op> = runs.iter().map(|r| r.gets.clone()).collect();
    let mut sets: Vec<Op> = runs.iter().map(|r| r.sets.clone()).collect();
    // Never sorted, and indexed later as though it had been. That is D3.
    let perf: Vec<Perf> = runs.iter().map(|r| r.perf.clone()).collect();

    let n = runs.len();
    slice(&mut ByKey::new(&mut gets, |op: &Op| op.opsec.0), n);
    slice(&mut ByKey::new(&mut sets, |op: &Op| op.opsec.0), n);
    // The third sort, which permutes the SET results while reading the perf list. It throws away the sort above it and leaves an order that is not an order. That is D2, and it needs Go's algorithm rather than a sort, because the answer depends on the questions asked and not on the data.
    slice(&mut Aliased::new(&mut sets, &perf, cycles), n);

    let (gets, sets, perf) = trim(gets, sets, perf, count);

    let (gets, sets, perf) = if kind == Kind::Average {
        average(&gets, &sets, &perf, *count)
    } else {
        let at = index(kind, *count);
        let pick = |series: &[Op]| {
            series
                .get(at)
                .cloned()
                .ok_or(BadCell::WouldPanic { at, len: n })
        };
        let counters = perf
            .get(at)
            .cloned()
            .ok_or(BadCell::WouldPanic { at, len: n })?;
        (pick(&gets)?, pick(&sets)?, counters)
    };

    Ok(Run {
        info: named(info, kind),
        sets,
        gets,
        perf: clean(&perf),
        // A chosen file the original wrote has four keys in it, so upstream mode writes four keys.
        spread: None,
    })
}

/// Take ten percent off each end, and leave the count smaller than it was found.
///
/// The slicing uses the count on the way in and the decrement happens after, so the window is right and everything that reads the count next time is not.
fn trim(
    gets: Vec<Op>,
    sets: Vec<Op>,
    perf: Vec<Perf>,
    count: &mut usize,
) -> (Vec<Op>, Vec<Op>, Vec<Perf>) {
    let runs = *count;
    if runs <= 10 {
        return (gets, sets, perf);
    }
    let outs = runs / 10;
    let window = outs..runs - outs;
    *count -= outs * 2;
    (
        gets[window.clone()].to_vec(),
        sets[window.clone()].to_vec(),
        perf[window].to_vec(),
    )
}

/// Which element of the window each kind reads.
///
/// `median` is the one to look at. The middle of a window of 25 is at 12 and this says 13, so the published median is the run above the median. That is D1.
const fn index(kind: Kind, runs: usize) -> usize {
    match kind {
        Kind::Worst => 0,
        Kind::Best => runs.saturating_sub(1),
        // Average never reaches here.
        Kind::Median | Kind::Average => runs / 2 + 1,
    }
}

/// `calcAverage`, summing in window order and dividing by the count the trim left.
fn average(gets: &[Op], sets: &[Op], perf: &[Perf], count: usize) -> (Op, Op, Perf) {
    let n = self::count(count);
    (mean_op(gets, n), mean_op(sets, n), mean_perf(perf, n))
}

/// The mean of one series, added up in the order the original adds it up.
///
/// Floating point addition is not associative, so the order is part of the answer.
fn mean_op(ops: &[Op], n: f64) -> Op {
    let mean = |f: fn(&Op) -> f64| Fixed3(ops.iter().map(f).sum::<f64>() / n);
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

/// The mean of the counters.
///
/// `sumperf` returns its accumulator untouched when there are no cycles, so a cell measured without perf averages to `{}`.
fn mean_perf(perfs: &[Perf], n: f64) -> Perf {
    let Some(first) = perfs.first() else {
        return Perf::default();
    };
    if !first.has_cycles() {
        return first.clone();
    }
    let mut out = first.clone();
    // `sumperf` and `avgperf` touch six counters and no others. Anything else in the object keeps the value the first run of the window had, which is not an average of anything.
    out.cpu_utilized = Some(mean_counter(perfs, |p| p.cpu_utilized.as_ref(), n));
    out.cycles = Some(mean_counter(perfs, |p| p.cycles.as_ref(), n));
    out.instructions = Some(mean_counter(perfs, |p| p.instructions.as_ref(), n));
    out.branches = Some(mean_counter(perfs, |p| p.branches.as_ref(), n));
    out.branch_misses = Some(mean_counter(perfs, |p| p.branch_misses.as_ref(), n));
    out.page_faults = Some(mean_counter(perfs, |p| p.page_faults.as_ref(), n));
    out
}

/// The mean of one counter, where anything unparseable is a zero.
///
/// A `<not supported>` counter reads as zero and averages in as zero, so an engine on a machine with no branch counter is reported as having taken no branches. That is D11 and it is reproduced here.
fn mean_counter<const PLACES: usize>(
    perfs: &[Perf],
    pick: fn(&Perf) -> Option<&Counter<PLACES>>,
    n: f64,
) -> Counter<PLACES> {
    let total: f64 = perfs
        .iter()
        .map(|p| pick(p).map_or(0.0, Counter::as_f64))
        .sum();
    Counter::Number(total / n)
}

/// `cleanperf`, which rewrites six counters as numbers and leaves the rest alone.
fn clean(perf: &Perf) -> Perf {
    if !perf.has_cycles() {
        return perf.clone();
    }
    let number = |c: Option<&Counter<0>>| c.map(|c| Counter::Number(c.as_f64()));
    let ratio = |c: Option<&Counter<3>>| c.map(|c| Counter::Number(c.as_f64()));
    Perf {
        cpu_utilized: ratio(perf.cpu_utilized.as_ref()),
        cycles: number(perf.cycles.as_ref()),
        instructions: number(perf.instructions.as_ref()),
        branches: number(perf.branches.as_ref()),
        branch_misses: number(perf.branch_misses.as_ref()),
        page_faults: number(perf.page_faults.as_ref()),
        // Not in `cleanperf`'s list, so whatever the run file had survives as it was written.
        secsuser: perf.secsuser.clone(),
        secssys: perf.secssys.clone(),
    }
}

/// The info block, with `kind` appended.
///
/// It comes from the last run the call read rather than the first, because the original assigns it inside its read loop and keeps the last assignment.
fn named(mut info: Info, kind: Kind) -> Info {
    info.kind = Some(kind.name().to_owned());
    info
}

/// The cycle count as Go's JSON reader returns it, which is zero for anything that is not a number.
fn cycles(perf: &Perf) -> i64 {
    let Some(counter) = &perf.cycles else {
        return 0;
    };
    match counter {
        Counter::Text(text) => text.parse::<i64>().unwrap_or(0),
        #[allow(clippy::cast_possible_truncation)]
        Counter::Number(n) => *n as i64,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::{choose, choose_all};
    use crate::fixture::{with_perf, without_perf};
    use crate::kind::Kind;

    // The gate. Both cells, all four kinds, byte for byte against files the original wrote.
    // Every defect has to be present and correct at once for this to pass, which is why there is no test here for any one of them on its own.
    #[test]
    fn both_cells_come_back_byte_for_byte() {
        for cell in [with_perf(), without_perf()] {
            let ours = choose_all(&cell.runs).unwrap();
            for (kind, got) in Kind::ALL.into_iter().zip(ours) {
                assert_eq!(
                    got.emit(),
                    cell.upstream(kind).emit(),
                    "{} {kind}",
                    cell.name
                );
            }
        }
    }

    // D4, which is the one that is visible without opening a file.
    // Each call leaves the count smaller, so the four aggregates are computed from 31, 25, 21 and 17 of the runs.
    #[test]
    fn each_aggregate_sees_fewer_runs_than_the_one_before() {
        let cell = with_perf();
        let average = choose_all(&cell.runs).unwrap().into_iter().nth(3).unwrap();

        // 17 files opened, sorted, one off each end, and the mean of the 15 that are left.
        let mut window: Vec<f64> = cell.runs[..17].iter().map(|r| r.gets.opsec.0).collect();
        window.sort_by(f64::total_cmp);
        let want: f64 = window[1..16].iter().sum::<f64>() / 15.0;

        assert_eq!(average.gets.opsec.to_string(), format!("{want:.3}"));
        assert_eq!(
            average.gets.opsec.to_string(),
            cell.upstream(Kind::Average).gets.opsec.to_string()
        );

        // The mean over all 31 runs, which is what the label claims, is a different number.
        let all: f64 = cell.runs.iter().map(|r| r.gets.opsec.0).sum::<f64>() / 31.0;
        assert_ne!(format!("{all:.3}"), format!("{want:.3}"));
    }

    // Upstream mode writes the four keys the original writes and no others.
    #[test]
    fn no_spread_object_is_added() {
        for cell in [with_perf(), without_perf()] {
            for run in choose_all(&cell.runs).unwrap() {
                assert!(run.spread.is_none());
                assert!(!run.emit().contains("spread"));
            }
        }
    }

    // Asking for one kind still replays the three calls before it, because there is no other way to arrive at the state the original would be in.
    #[test]
    fn one_kind_is_the_same_as_the_one_out_of_four() {
        let cell = with_perf();
        let all = choose_all(&cell.runs).unwrap();
        for (kind, want) in Kind::ALL.into_iter().zip(all) {
            assert_eq!(choose(&cell.runs, kind).unwrap().emit(), want.emit());
        }
    }

    // Every cell the original published, rather than the two that are committed here.
    //
    // The two fixtures are one engine at one thread and one pipeline depth, and passing on them says little about the other 645 cells. This walks the original's whole runs directory, reduces each cell four ways and compares against the files the original wrote, which is the real claim upstream mode makes.
    //
    // Ignored by default because it needs 20160 run files that do not belong in this repository:
    //
    // ```
    // CB_PARITY_RUNS=/path/to/cache-benchmarks/results/runs \
    //   cargo test -p cb-stats -- --ignored --nocapture
    // ```
    #[test]
    #[ignore = "needs the original's runs directory in CB_PARITY_RUNS"]
    #[allow(clippy::expect_used)]
    fn every_published_cell_comes_back_byte_for_byte() {
        use std::collections::BTreeMap;

        let Ok(dir) = std::env::var("CB_PARITY_RUNS") else {
            return;
        };

        let mut cells: BTreeMap<String, BTreeMap<String, cb_core::run::Run>> = BTreeMap::new();
        for entry in std::fs::read_dir(&dir).expect("the runs directory opens") {
            let path = entry.expect("the entry reads").path();
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some((cell, at)) = name.rsplit_once("-run_") else {
                continue;
            };
            let text = std::fs::read_to_string(&path).expect("the run file reads");
            let run = serde_json::from_str(&text).expect("the run file parses");
            cells
                .entry(cell.to_owned())
                .or_default()
                .insert(at.to_owned(), run);
        }
        assert!(cells.len() > 500, "only found {} cells", cells.len());

        let (mut checked, mut skipped) = (0_usize, 0_usize);
        for (cell, files) in &cells {
            let mut runs = Vec::new();
            for at in 1.. {
                match files.get(&at.to_string()) {
                    Some(run) => runs.push(run.clone()),
                    None => break,
                }
            }
            if runs.len() < 11 {
                skipped += 1;
                continue;
            }
            let ours = choose_all(&runs).expect("the cell reduces");
            for (kind, got) in Kind::ALL.into_iter().zip(ours) {
                let want = files.get(kind.name()).expect("the cell has all four kinds");
                assert_eq!(got.emit(), want.emit(), "{cell} {kind}");
                checked += 1;
            }
        }
        println!(
            "{checked} chosen files matched across {} cells, {skipped} skipped",
            cells.len()
        );
        assert_eq!(checked, (cells.len() - skipped) * 4);
        assert!(checked > 2000, "only checked {checked}");
    }

    // A cell too small for the original's median index is an error here rather than the panic it is there.
    #[test]
    fn a_cell_the_original_would_crash_on_is_refused() {
        let cell = with_perf();
        assert!(
            choose_all(&cell.runs[..1])
                .unwrap_err()
                .to_string()
                .contains("panic")
        );
    }
}
