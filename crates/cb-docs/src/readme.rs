//! The README that goes in a results directory.
//!
//! It follows the original's shape, which is methodology bullets, then a version table, then five highlighted charts. The difference is that none of the facts in it are typed. The bullets carry the profile's real numbers, the hardware comes out of `host.json` and the version table is one row per engine read out of the results themselves, which is how the original's README came to disagree with its own data.
//!
//! Two things go in here that the original has nowhere. The divergences table, so that a reader who arrived at a chart from a link is told what this port does differently before they read a bar. And what these numbers may and may not be used for, in full, because a caveat that lives in a document nobody opens is a caveat that does not exist.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cb_chart::{Case, Metric, Percentile, Scale, Spec, Which};
use cb_core::{CacheKind, Chosen, Compat, Machine, Pmu, Profile};

use crate::divergence;

/// Where the PNGs sit relative to the document.
const GRAPHS: &str = "graphs";

/// What these numbers may be used for, word for word out of `docs/methodology.md`.
///
/// Duplicated rather than linked because this is the sentence that has to travel with the charts. A test asserts that the two copies are the same string, so the duplicate cannot drift.
pub const MAY: &str = "They may be used to compare these cache servers against each other, on the stated architecture, over unix sockets, for GET and SET of 1 to 1024 byte values at 256 connections, at the stated pipeline depths, with persistence off and no eviction. That is a narrow claim and it is the one this harness supports.";

/// What they may not be used for, word for word out of `docs/methodology.md`.
pub const MAY_NOT: &str = "They may not be used to say one engine is faster than another, full stop. This workload has no expiry, no eviction, no mixed command set, no large values, no network, no replication, no persistence and no multi key operations. It is a hot path measurement of two commands. Wherever a number from here is published, that sentence goes with it.";

/// One results directory's README.
#[derive(Debug)]
pub struct Readme<'a> {
    /// What the sweep ran on.
    pub machine: &'a Machine,
    /// The profile it ran, which is where the shape of the sweep comes from.
    pub profile: &'a Profile,
    /// Every engine in the results, by short name, with the version line it printed.
    pub versions: &'a BTreeMap<String, String>,
    /// The chart files that are actually sitting in the graphs directory.
    pub have: &'a BTreeSet<String>,
    /// Which statistics produced the numbers.
    pub compat: Compat,
}

impl Readme<'_> {
    /// The filename this document is written to.
    #[must_use]
    pub const fn file(&self) -> &'static str {
        "README.md"
    }

    /// Every chart this document links to, which is what an index cross check compares against.
    #[must_use]
    pub fn covers(&self) -> BTreeSet<String> {
        self.headline()
            .into_iter()
            .map(Spec::file)
            .filter(|file| self.have.contains(file))
            .collect()
    }

    /// The charts this README shows, which is five plus the redrawn pair on a results directory with Garnet in it.
    fn headline(&self) -> Vec<Spec> {
        let mut out = vec![
            throughput(Which::Sets),
            throughput(Which::Gets),
            p99(Which::Sets, None),
            p99(Which::Gets, None),
            cycles(),
        ];
        if self.versions.contains_key(CacheKind::Garnet.name()) {
            let case = Some(Case::NoGarnetAtOneThread);
            out.push(p99(Which::Sets, case));
            out.push(p99(Which::Gets, case));
        }
        out
    }

    /// The document.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.opening(&mut out);
        self.bullets(&mut out);
        self.hardware(&mut out);
        self.table(&mut out);
        self.charts(&mut out);
        method(&mut out);
        differences(&mut out);
        caveat(&mut out);
        out
    }

    /// The title, the subjects and the note that this file is generated.
    fn opening(&self, out: &mut String) {
        let _ = writeln!(out, "# Cache Benchmarks ({})\n", self.machine.profile);
        let subjects: Vec<String> = self
            .kinds()
            .map(|kind| format!("[{}]({})", kind.label(), kind.homepage()))
            .collect();
        let _ = writeln!(
            out,
            "Throughput, latency and CPU cycles for {}.\n",
            list(&subjects)
        );
        let _ = writeln!(
            out,
            "This is a Rust port of [tidwall/cache-benchmarks](https://github.com/tidwall/cache-benchmarks). The numbers here were measured by this harness on the hardware described below, and they are not the original's numbers. Do not compare a bar here against a bar there.\n"
        );
        let _ = writeln!(
            out,
            "This file is generated by `cache-bench docs` from `output.json` and `host.json` in this directory. Editing it by hand will not survive the next run.\n"
        );
    }

    /// The methodology bullets, with the profile's numbers in them.
    fn bullets(&self, out: &mut String) {
        let profile = self.profile;
        let cells = profile.threads.len() * profile.pipelines.len() * profile.perf.len();
        let _ = writeln!(
            out,
            "- Persistence is off for every server, no disk operations."
        );
        let _ = writeln!(
            out,
            "- Every connection is a local unix socket, so no network stack is measured."
        );
        let _ = writeln!(
            out,
            "- The server is pinned to CPUs {} and the load generator to CPUs {}.",
            profile.cache_pin, profile.bench_pin
        );
        let _ = writeln!(out, "- The load generator is {}.", self.machine.memtier);
        let _ = writeln!(
            out,
            "- {} threads of {} connections each, so {} connections, doing {} operations each per pass.",
            profile.bench_threads,
            profile.connections_per_thread,
            profile.connections(),
            profile.operations
        );
        let _ = writeln!(
            out,
            "- Values are {} bytes, over a key space of {}, with a {} memory limit on every server so that nothing is ever evicted.",
            profile.size_range, profile.key_maximum, profile.maxmemory
        );
        let _ = writeln!(
            out,
            "- Server I/O threads swept at {}.",
            numbers(&profile.threads)
        );
        let _ = writeln!(out, "- Pipelining at {}.", numbers(&profile.pipelines));
        let _ = writeln!(
            out,
            "- {} runs per cell, {} cells, {} runs in all.",
            profile.runs,
            cells * 7,
            profile.total_runs()
        );
        let _ = writeln!(out, "- {}", self.statistic());
        let _ = writeln!(
            out,
            "- Latency is reported at MIN, AVG, the 50th, 90th, 99th, 99.9th and 99.99th percentiles, and MAX."
        );
        let _ = writeln!(out, "- CPU cycles: {}.", self.machine.pmu.describe());
        let _ = writeln!(
            out,
            "- There is a warmup pass at the start of every run, a dry run of all the SET operations, and it is not part of the measurement.\n"
        );
        let _ = writeln!(
            out,
            "The Threads on the x axis of every chart is the number of I/O threads the server was given. It is not the number of clients, which is held at {} on every point of every chart. This is the single most misread thing about these charts.\n",
            profile.connections()
        );
    }

    /// Which statistic is plotted, which depends on how the runs were reduced.
    fn statistic(&self) -> String {
        match self.compat {
            Compat::Corrected => format!(
                "The median of the {} runs is plotted, taken from the middle of a window with 10 percent trimmed off each end.",
                self.profile.runs
            ),
            Compat::Upstream => format!(
                "The median of the {} runs is plotted, reproducing the original's own selection defects exactly. See D1 to D4 below.",
                self.profile.runs
            ),
        }
    }

    /// What it ran on.
    fn hardware(&self, out: &mut String) {
        let machine = self.machine;
        let _ = writeln!(out, "## The hardware\n");
        let _ = writeln!(out, "| | |\n|---|---|");
        let _ = writeln!(out, "| Profile | {} |", machine.profile);
        let _ = writeln!(out, "| What it is | {} |", self.profile.description);
        let _ = writeln!(out, "| CPU | {} |", machine.cpu_model);
        let _ = writeln!(out, "| Logical CPUs | {} |", machine.cpus);
        let _ = writeln!(out, "| Memory | {} |", cb_core::Bytes(machine.memory_bytes));
        let _ = writeln!(out, "| Kernel | {} |", machine.kernel);
        let _ = writeln!(out, "| Distribution | {} |", machine.distro);
        let _ = writeln!(out, "| Frequency governor | {} |", machine.governor);
        let _ = writeln!(out, "| CPU mitigations | {} |", machine.mitigations);
        let _ = writeln!(out, "| Hardware PMU | {} |", machine.pmu.describe());
        let _ = writeln!(out, "| Sweep started | {} |", machine.started);
        let _ = writeln!(
            out,
            "| Sweep finished | {} |\n",
            machine.finished.as_deref().unwrap_or("still running")
        );
        let _ = writeln!(
            out,
            "There is no hostname here and there is not going to be one. A results directory gets published and a machine name is not something to publish, so the host is described by what it is rather than by what it is called.\n"
        );
    }

    /// One row per engine, read out of the results.
    fn table(&self, out: &mut String) {
        let _ = writeln!(out, "## Versions\n");
        let _ = writeln!(out, "| Cache | Version |\n|---|---|");
        for (name, version) in self.versions {
            let label = CacheKind::ALL
                .into_iter()
                .find(|kind| kind.name() == name)
                .map_or_else(|| name.clone(), |kind| kind.label().to_owned());
            let _ = writeln!(out, "| {label} | {version} |");
        }
        let _ = writeln!(out, "| memtier_benchmark | {} |", self.machine.memtier);
        if let Some(rustc) = &self.machine.rustc {
            let _ = writeln!(out, "| rustc | {rustc} |");
        }
        let _ = writeln!(
            out,
            "| cache-bench | {} ({}) |\n",
            self.machine.cache_bench.version, self.machine.cache_bench.git
        );
        let _ = writeln!(
            out,
            "Every row above the last three is the version string the server itself printed when it was started for these runs, not a list kept by hand.\n"
        );
    }

    /// The five the original highlights, in its order.
    fn charts(&self, out: &mut String) {
        let _ = writeln!(out, "# Benchmarks\n");
        let _ = writeln!(
            out,
            "The five the original leads with, in linear scale, at pipeline depth 1. There are {} charts in this directory and the two indexes below have every one of them.\n",
            self.have.len()
        );
        let _ = writeln!(out, "- [All Benchmarks Linear Scale](LINEAR.md)");
        let _ = writeln!(
            out,
            "- [All Benchmarks Logarithmic Scale](LOGARITHMIC.md)\n"
        );

        let _ = writeln!(out, "## Throughput\n");
        self.pictures(out, &[throughput(Which::Sets), throughput(Which::Gets)]);

        let _ = writeln!(out, "## Latency 99th Percentile\n");
        self.p99s(out);

        let _ = writeln!(out, "## CPU Cycles\n");
        self.pictures(out, &[cycles()]);
    }

    /// The P99 pair, which is the one place the original hides a bar.
    ///
    /// The redrawn charts only exist because Garnet's P99 at one thread is far enough above everything else to flatten the rest, so on a results directory that has no Garnet in it there is nothing to leave out and the plain pair is the whole story.
    fn p99s(&self, out: &mut String) {
        let plain = [p99(Which::Sets, None), p99(Which::Gets, None)];
        if !self.versions.contains_key(CacheKind::Garnet.name()) {
            self.pictures(out, &plain);
            return;
        }
        let _ = writeln!(
            out,
            "Garnet is left out at one thread, where its P99 is far enough above the rest that on a linear scale the other engines become stubs.\n"
        );
        let _ = writeln!(out, "<details>\n<summary>See it with Garnet</summary>\n");
        self.pictures(out, &plain);
        let _ = writeln!(out, "</details>\n");
        let case = Some(Case::NoGarnetAtOneThread);
        self.pictures(out, &[p99(Which::Sets, case), p99(Which::Gets, case)]);
    }

    /// Images, or a line saying why there are none.
    fn pictures(&self, out: &mut String, specs: &[Spec]) {
        let mut gone = Vec::new();
        for &spec in specs {
            let file = spec.file();
            if self.have.contains(&file) {
                let _ = writeln!(out, "![{}]({GRAPHS}/{file})", alt(spec));
            } else {
                gone.push(spec);
            }
        }
        // The blank line separates images from what follows, so a section with no images at all does not get two of them in a row.
        if gone.len() < specs.len() {
            let _ = writeln!(out);
        }
        if !gone.is_empty() {
            let files: Vec<String> = gone.iter().map(|spec| spec.file()).collect();
            let _ = writeln!(
                out,
                "*Not drawn: {}. {}*\n",
                files.join(", "),
                self.why(&gone)
            );
        }
    }

    /// Why a chart is not here, which is a different sentence depending on what is missing.
    ///
    /// A cycles chart missing on a machine with no counters is expected and the reader should be told that rather than left to wonder. Anything else missing is a gap in the sweep and saying so plainly is better than inventing a reason for it.
    fn why(&self, gone: &[Spec]) -> &'static str {
        let counters = gone.iter().all(|spec| spec.metric == Metric::CpuCycles);
        if counters && self.machine.pmu == Pmu::Absent {
            return "This host exposes no hardware PMU, so cycles per operation was never measured here.";
        }
        "That measurement is not in this results directory."
    }

    /// The engines in the results, in the order the original sweeps them.
    fn kinds(&self) -> impl Iterator<Item = CacheKind> + '_ {
        CacheKind::ALL
            .into_iter()
            .filter(|kind| self.versions.contains_key(kind.name()))
    }
}

/// The short version of how a number got made.
fn method(out: &mut String) {
    let _ = writeln!(out, "## How a number got made\n");
    let _ = writeln!(
        out,
        "One run is one cell measured once: a server, a thread count, a pipeline depth, whether counters were attached, and a run index. The server is started fresh for every run with persistence off and a memory limit large enough that nothing is evicted, the load generator warms it up and the warmup is thrown away, then the measured pass runs and the server is stopped. Runs with counters attached are a separate half of the matrix, because attaching a counter is not free and the throughput numbers should not carry its cost.\n"
    );
    let _ = writeln!(
        out,
        "A counter the machine cannot measure comes back as text rather than as a number. It is carried as not measured all the way through and the bar is left off the chart, rather than being read as a zero, which would be a claim about the engine that the hardware never made.\n"
    );
    let _ = writeln!(
        out,
        "Charts are drawn from the `output.json` sitting next to them, and redrawing all {} of them is one command that needs nothing installed but Rust. The point of that is that nobody has to trust us. If you think the selection is wrong or a scale is misleading, the data to redraw it your way is here.\n",
        Spec::all().len()
    );
    let _ = writeln!(
        out,
        "The long version is [docs/methodology.md](../../docs/methodology.md), and it is the document that decides whether these charts are believed.\n"
    );
}

/// What this port does that the original does not.
fn differences(out: &mut String) {
    let _ = writeln!(out, "## Differences from the original\n");
    let _ = writeln!(
        out,
        "Every one of them, with the reasoning in [divergences.md](../../divergences.md). The rule is that a divergence is either listed there or it is a bug, and nothing gets to be an undocumented improvement.\n"
    );
    out.push_str(&divergence::table());
    out.push('\n');
}

/// The part that has to travel with the charts.
fn caveat(out: &mut String) {
    let _ = writeln!(out, "## What these numbers may and may not be used for\n");
    let _ = writeln!(out, "{MAY}\n");
    let _ = writeln!(out, "{MAY_NOT}\n");
}

/// A linear throughput chart at pipeline depth 1.
fn one(metric: Metric, case: Option<Case>) -> Spec {
    Spec {
        metric,
        pipeline: 1,
        kind: Chosen::Median,
        scale: Scale::Linear,
        case,
    }
}

/// Throughput at pipeline depth 1.
fn throughput(which: Which) -> Spec {
    one(Metric::Throughput(which), None)
}

/// P99 latency at pipeline depth 1.
fn p99(which: Which, case: Option<Case>) -> Spec {
    one(Metric::Latency(Percentile::P99, which), case)
}

/// Cycles per operation at pipeline depth 1.
fn cycles() -> Spec {
    one(Metric::CpuCycles, None)
}

/// What an image says to a reader who cannot see it.
fn alt(spec: Spec) -> String {
    let what = match spec.metric {
        Metric::Throughput(which) => format!("{} throughput", which.label()),
        Metric::Latency(percentile, which) => {
            format!("{} {} latency", which.label(), percentile.label())
        }
        Metric::CpuCycles => "CPU cycles per operation".to_owned(),
    };
    let tail = match spec.case {
        Some(Case::NoGarnetAtOneThread) => ", without Garnet at one thread",
        None => "",
    };
    format!(
        "{what} by thread count at pipeline depth {}, linear scale{tail}",
        spec.pipeline
    )
}

/// A list of numbers as prose, such as `1, 10, 25 and 50`.
fn numbers(values: &[u32]) -> String {
    let words: Vec<String> = values.iter().map(u32::to_string).collect();
    list(&words)
}

/// Comma separated with an `and` before the last one.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a document that will not generate is a failed test"
)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use cb_chart::Spec;
    use cb_core::{Compat, Machine, Pmu, Profile, Profiles, Tool};

    use super::{GRAPHS, MAY, MAY_NOT, Readme, list};

    /// The long version of the caveat, which the two constants above are copied out of.
    const METHODOLOGY: &str = include_str!("../../../docs/methodology.md");

    fn machine(pmu: Pmu) -> Machine {
        Machine {
            profile: "wsl32".to_owned(),
            kernel: "Linux 6.18.33.2 x86_64".to_owned(),
            distro: "Ubuntu 26.04 LTS".to_owned(),
            cpu_model: "A CPU".to_owned(),
            cpus: 32,
            memory_bytes: 34_359_738_368,
            pmu,
            governor: "performance".to_owned(),
            mitigations: "mitigations=on".to_owned(),
            memtier: "memtier_benchmark 2.4.4".to_owned(),
            cache_bench: Tool {
                version: "0.4.0".to_owned(),
                git: "4b18e6b".to_owned(),
            },
            rustc: None,
            started: "2026-09-01T09:12:03Z".to_owned(),
            finished: None,
        }
    }

    fn profile() -> Profile {
        Profiles::parse(include_str!("../../../profiles.toml"))
            .expect("the committed profiles parse")
            .profiles
            .get("wsl32")
            .expect("wsl32 is a profile we ship")
            .clone()
    }

    fn versions() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("redis".to_owned(), "v=8.2.1".to_owned()),
            ("memcache".to_owned(), "1.6.38".to_owned()),
            ("yo".to_owned(), "0.1.0".to_owned()),
        ])
    }

    fn readme(pmu: Pmu, have: &BTreeSet<String>) -> String {
        Readme {
            machine: &machine(pmu),
            profile: &profile(),
            versions: &versions(),
            have,
            compat: Compat::Corrected,
        }
        .render()
    }

    fn everything() -> BTreeSet<String> {
        Spec::all().iter().map(|s| s.file()).collect()
    }

    // The caveat is duplicated into the generated output on purpose, so the copy has to be the original.
    #[test]
    fn the_caveat_is_word_for_word_the_one_in_the_methodology() {
        assert!(METHODOLOGY.contains(MAY), "{MAY}");
        assert!(METHODOLOGY.contains(MAY_NOT), "{MAY_NOT}");
    }

    #[test]
    fn the_caveat_is_in_the_generated_document() {
        let text = readme(Pmu::Present, &everything());
        assert!(text.contains(MAY_NOT), "{text}");
    }

    #[test]
    fn the_subjects_are_the_ones_in_the_results() {
        let text = readme(Pmu::Present, &everything());
        assert!(
            text.contains("[Memcached](https://github.com/memcached/memcached)"),
            "{text}"
        );
        assert!(text.contains("[yo](https://github.com/tamnd/yo)"), "{text}");
        // Nothing that was not measured is listed as a subject. Dragonfly still shows up further down, in the divergence about its memory limit.
        assert!(
            !text.contains("[Dragonfly](https://github.com/dragonflydb/dragonfly)"),
            "{text}"
        );
    }

    // The version table is the reason this document is generated rather than written.
    #[test]
    fn the_version_table_comes_from_the_results() {
        let text = readme(Pmu::Present, &everything());
        assert!(text.contains("| Memcached | 1.6.38 |"), "{text}");
        assert!(
            text.contains("| memtier_benchmark | memtier_benchmark 2.4.4 |"),
            "{text}"
        );
    }

    #[test]
    fn a_host_with_no_counters_says_so_where_the_cycles_chart_would_be() {
        let mut have = everything();
        let cycles = "graph_cpucycles-pipeline_1-kind_median-scale_linear.png";
        assert!(have.remove(cycles));
        let text = readme(Pmu::Absent, &have);
        assert!(text.contains(&format!("*Not drawn: {cycles}.")), "{text}");
        assert!(text.contains("no hardware PMU"), "{text}");
    }

    // The redrawn pair only exists to hide a Garnet bar, so on a results directory without Garnet in it the note would be explaining an absence to nobody.
    #[test]
    fn the_garnet_note_is_only_there_when_garnet_was_measured() {
        let have = everything();
        let text = readme(Pmu::Present, &have);
        assert!(!text.contains("See it with Garnet"), "{text}");
        assert!(!text.contains("case_1"), "{text}");

        let mut with = versions();
        with.insert("garnet".to_owned(), "1.0.83".to_owned());
        let text = Readme {
            machine: &machine(Pmu::Present),
            profile: &profile(),
            versions: &with,
            have: &have,
            compat: Compat::Corrected,
        }
        .render();
        assert!(text.contains("See it with Garnet"), "{text}");
        assert!(text.contains("case_1"), "{text}");
    }

    // What the document says it links to has to be what it links to, because the index cross check trusts it.
    #[test]
    fn what_it_covers_is_what_it_actually_links_to() {
        let have = everything();
        let readme = Readme {
            machine: &machine(Pmu::Present),
            profile: &profile(),
            versions: &versions(),
            have: &have,
            compat: Compat::Corrected,
        };
        let text = readme.render();
        for file in readme.covers() {
            assert!(text.contains(&format!("({GRAPHS}/{file})")), "{file}");
        }
        let linked = text.matches(&format!("]({GRAPHS}/")).count();
        assert_eq!(linked, readme.covers().len());
    }

    #[test]
    fn nothing_in_it_names_the_machine() {
        let text = readme(Pmu::Present, &everything());
        assert!(text.contains("no hostname here"), "{text}");
    }

    #[test]
    fn the_bullets_carry_the_profiles_own_numbers() {
        let text = readme(Pmu::Present, &everything());
        let profile = profile();
        assert!(
            text.contains(&format!("so {} connections", profile.connections())),
            "{text}"
        );
        assert!(text.contains("Pipelining at 1, 10, 25 and 50."), "{text}");
    }

    #[test]
    fn a_list_reads_as_prose() {
        let one = ["1".to_owned()];
        let two = ["1".to_owned(), "2".to_owned()];
        let three = ["1".to_owned(), "2".to_owned(), "3".to_owned()];
        assert_eq!(list(&one), "1");
        assert_eq!(list(&two), "1 and 2");
        assert_eq!(list(&three), "1, 2 and 3");
        assert_eq!(list(&[]), "");
    }
}
