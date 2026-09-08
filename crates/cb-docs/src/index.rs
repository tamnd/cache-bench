//! The two chart indexes, `LINEAR.md` and `LOGARITHMIC.md`.
//!
//! One document per scale, four pipeline blocks in each, ten sections in each block. The section order is the original's, so the two projects can be opened side by side and scrolled together.
//!
//! Each of them opens with where its numbers came from, because a link to a chart index is the link that gets shared and the README is the document nobody following it opens.
//!
//! What it is generated from matters more than what it looks like. The sections come from the same `Spec` table the charts are drawn from, so a document cannot name a chart that was never specified, and the presence check makes it skip one that was specified but not drawn. Those two together are the promise this milestone makes: every chart that exists is linked and nothing else is.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cb_chart::{Case, Metric, Percentile, Scale, Spec, Which};
use cb_core::{Chosen, Machine};

use crate::anchor::Anchors;
use crate::readme::MAY_NOT;

/// Where the PNGs sit relative to the document, which is next to it rather than under `results` as in the original, because here each host has its own results directory and the documents live in it.
const GRAPHS: &str = "graphs";

/// One `##` section of a pipeline block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Section {
    /// The heading, which is also the link text in the contents block.
    heading: &'static str,
    /// What goes under it.
    what: What,
}

/// Which charts a section holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    /// Operations per second, SET then GET.
    Throughput,
    /// One latency figure, SET then GET.
    Latency(Percentile),
    /// Cycles per operation, which is one chart because it covers both passes.
    Cycles,
}

/// The ten sections of a pipeline block, in the order they are written.
///
/// The first seven and the last are the original's, in its order and with its wording. MIN and AVG are ours. The original draws those 32 charts on every run and then links none of them, which leaves a reader who has the results directory in front of them with no way to find them, so they go in after MAX where they read as a continuation of the latency run rather than in the middle of the percentiles the original does link.
const SECTIONS: [Section; 10] = [
    Section {
        heading: "Throughput",
        what: What::Throughput,
    },
    Section {
        heading: "Latency 50th Percentile",
        what: What::Latency(Percentile::P50),
    },
    Section {
        heading: "Latency 90th Percentile",
        what: What::Latency(Percentile::P90),
    },
    Section {
        heading: "Latency 99th Percentile",
        what: What::Latency(Percentile::P99),
    },
    Section {
        heading: "Latency 99.9th Percentile",
        what: What::Latency(Percentile::P999),
    },
    Section {
        heading: "Latency 99.99th Percentile",
        what: What::Latency(Percentile::P9999),
    },
    Section {
        heading: "Latency MAX",
        what: What::Latency(Percentile::Max),
    },
    Section {
        heading: "Latency MIN",
        what: What::Latency(Percentile::Min),
    },
    Section {
        heading: "Latency AVG",
        what: What::Latency(Percentile::Avg),
    },
    Section {
        heading: "CPU Cycles",
        what: What::Cycles,
    },
];

/// Where the charts in one of these documents came from.
///
/// A reader who follows a link straight to `LINEAR.md` never passes the README, and what they land on is 77 pictures of bars with nothing next to them saying what was measured or on what. That is how one bar ends up quoted as a general statement about an engine. So the host, the way back to the README and the notes, and the sentence about what these numbers are not evidence for all go at the top of the document the reader actually opened.
#[derive(Debug)]
pub struct Measured<'a> {
    /// What the sweep ran on.
    pub machine: &'a Machine,
    /// What the profile is in words, out of `profiles.toml`.
    pub description: &'a str,
    /// How many client connections were held on every point of every chart, which is the number the x axis is misread as.
    pub connections: u32,
    /// Whether there is a `NOTES.md` beside the document to send a reader to.
    pub notes: bool,
}

/// One chart index, ready to render.
#[derive(Debug)]
pub struct Index<'a> {
    /// Which of the two documents this is.
    pub scale: Scale,
    /// The chart files that are actually sitting in the graphs directory.
    pub have: &'a BTreeSet<String>,
    /// A sentence saying why a chart might be missing on this host, written under any section that is short of one. Empty says nothing beyond naming the file.
    pub absent: &'a str,
    /// Where the numbers came from. Absent in golden mode, where the document is drawn from the spec alone and there is no host and no measurement behind any of it.
    pub measured: Option<Measured<'a>>,
}

impl Index<'_> {
    /// The filename this document is written to.
    #[must_use]
    pub const fn file(&self) -> &'static str {
        match self.scale {
            Scale::Linear => "LINEAR.md",
            Scale::Logarithmic => "LOGARITHMIC.md",
        }
    }

    /// Every chart this document links to, whether or not it is on disk.
    ///
    /// This is the set the exit condition is checked against: the two documents together have to cover every chart the spec names.
    #[must_use]
    pub fn covers(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for pipeline in Spec::PIPELINES {
            for section in SECTIONS {
                for spec in self.charts(section, pipeline) {
                    out.insert(spec.file());
                }
            }
        }
        out
    }

    /// The document.
    #[must_use]
    pub fn render(&self) -> String {
        let mut anchors = Anchors::default();
        let title = format!("Cache Benchmarks ({} Scale)", self.scale_word());
        anchors.assign(&title);
        anchors.assign("Contents");

        let mut contents = String::new();
        let mut body = String::new();
        // The widest pipeline number, so the four labels in the contents block line up the way the original's do.
        let width = Spec::PIPELINES
            .iter()
            .map(|p| p.to_string().len())
            .max()
            .unwrap_or(1);

        for pipeline in Spec::PIPELINES {
            anchors.assign(&format!("Pipeline {pipeline}"));
            let _ = write!(body, "## Pipeline {pipeline}\n\n");

            let mut links = Vec::new();
            for section in SECTIONS {
                let anchor = anchors.assign(section.heading);
                links.push(format!("[{}](#{anchor})", section.heading));
                let _ = write!(body, "## {}\n\n", section.heading);
                body.push_str(&self.pictures(section, pipeline));
            }

            let label = format!("**Pipeline {pipeline}**");
            let pad = " ".repeat(width - pipeline.to_string().len());
            let _ = writeln!(contents, "{label}{pad}: {}", links.join(",\n"));
            contents.push('\n');
        }

        let mut out = format!("# {title}\n\n");
        out.push_str("- [All Benchmarks Linear Scale](LINEAR.md)\n");
        out.push_str("- [All Benchmarks Logarithmic Scale](LOGARITHMIC.md)\n\n");
        if self.scale == Scale::Logarithmic {
            out.push_str(
                "**All graphs are [logarithmic scale](https://en.wikipedia.org/wiki/Logarithmic_scale).**\n\n",
            );
        }
        out.push_str(&self.provenance());
        out.push_str("This file is generated by `cache-bench docs` and editing it by hand will not survive the next run.\n\n");
        out.push_str("## Contents\n\n");
        out.push_str(&contents);
        out.push_str(&body);
        out
    }

    /// What was measured, on what, and what it does not show.
    ///
    /// Empty in golden mode. A document generated from the spec with no results behind it has no host to name and no numbers to warn anybody off, and a caveat printed where there is nothing to caveat is a caveat people learn to skip.
    fn provenance(&self) -> String {
        let Some(measured) = &self.measured else {
            return String::new();
        };
        let machine = measured.machine;
        let mut out = String::new();
        let _ = write!(
            out,
            "Measured by this harness on a {} with {} logical CPUs running {}, under the `{}` profile. {} This is a Rust port of [tidwall/cache-benchmarks](https://github.com/tidwall/cache-benchmarks), and these are not the original's numbers, so do not compare a bar here against a bar there. [README.md](README.md) carries the method, the versions and the rest of the hardware.\n\n",
            machine.cpu_model, machine.cpus, machine.distro, machine.profile, measured.description
        );
        let _ = write!(
            out,
            "The Threads on the x axis of every chart below is the number of I/O threads the server was given. It is not the number of clients, which is held at {} on every point of every chart.\n\n",
            measured.connections
        );
        if measured.notes {
            out.push_str("Read [NOTES.md](NOTES.md) first. It is written by hand and it says what is true of these numbers in particular rather than of the harness.\n\n");
        }
        let _ = write!(out, "{MAY_NOT}\n\n");
        out
    }

    /// The images under one section, and a note about any that are not there.
    fn pictures(&self, section: Section, pipeline: u32) -> String {
        let mut out = String::new();
        let mut gone = Vec::new();
        self.images(&self.plain(section, pipeline), &mut out, &mut gone);
        if self.hand_drawn(section, pipeline) {
            out.push_str("Garnet's P99 at one thread is far enough above the rest that on a linear scale the other five become stubs. The same two charts follow with that one bar left off, which is the pair the original leads with.\n\n");
            self.images(&self.redrawn(pipeline), &mut out, &mut gone);
        }
        if !gone.is_empty() {
            let tail = if self.absent.is_empty() {
                String::new()
            } else {
                format!(" {}", self.absent)
            };
            let _ = write!(out, "*Not drawn: {}.{tail}*\n\n", gone.join(", "));
        }
        out
    }

    /// Write one run of images, putting the ones that are not on disk aside to be named further down.
    fn images(&self, specs: &[Spec], out: &mut String, gone: &mut Vec<String>) {
        let mut drawn = 0_usize;
        for &spec in specs {
            let file = spec.file();
            if self.have.contains(&file) {
                let _ = writeln!(out, "![{}]({GRAPHS}/{file})", alt(spec));
                drawn += 1;
            } else {
                gone.push(file);
            }
        }
        if drawn > 0 {
            out.push('\n');
        }
    }

    /// The charts a section holds, in the order they are shown, SET before GET.
    ///
    /// The original's own loop draws GET first and the document shows SET first, which is why the order here is written out rather than taken from `Which::ALL`.
    fn charts(&self, section: Section, pipeline: u32) -> Vec<Spec> {
        let mut out = self.plain(section, pipeline);
        if self.hand_drawn(section, pipeline) {
            out.extend(self.redrawn(pipeline));
        }
        out
    }

    /// The two charts a section is really about, before any redraw.
    fn plain(&self, section: Section, pipeline: u32) -> Vec<Spec> {
        let metrics = match section.what {
            What::Throughput => vec![
                Metric::Throughput(Which::Sets),
                Metric::Throughput(Which::Gets),
            ],
            What::Latency(p) => vec![
                Metric::Latency(p, Which::Sets),
                Metric::Latency(p, Which::Gets),
            ],
            What::Cycles => vec![Metric::CpuCycles],
        };
        metrics
            .into_iter()
            .map(|metric| self.spec(metric, pipeline, None))
            .collect()
    }

    /// The two the original draws by hand with Garnet's single thread bar left off.
    fn redrawn(&self, pipeline: u32) -> Vec<Spec> {
        [Which::Sets, Which::Gets]
            .into_iter()
            .map(|which| {
                self.spec(
                    Metric::Latency(Percentile::P99, which),
                    pipeline,
                    Some(Case::NoGarnetAtOneThread),
                )
            })
            .collect()
    }

    /// Whether this section is the one the original adds two charts to by hand.
    ///
    /// Two of the 154 are drawn outside the loop, and the original links them from its README rather than from either index, which leaves the two indexes covering 152. They belong under the section they are a redraw of, so they go here as well.
    fn hand_drawn(&self, section: Section, pipeline: u32) -> bool {
        self.scale == Scale::Linear
            && pipeline == 1
            && section.what == What::Latency(Percentile::P99)
    }

    /// One chart of this document's scale.
    fn spec(&self, metric: Metric, pipeline: u32, case: Option<Case>) -> Spec {
        Spec {
            metric,
            pipeline,
            kind: Chosen::Median,
            scale: self.scale,
            case,
        }
    }

    /// How the scale is written in prose.
    const fn scale_word(&self) -> &'static str {
        match self.scale {
            Scale::Linear => "Linear",
            Scale::Logarithmic => "Logarithmic",
        }
    }
}

/// What an image says to a reader who cannot see it.
///
/// The original writes `Alt text` on all 120 of its images, which is the placeholder out of the markdown documentation and tells a screen reader nothing. Every chart here is the same shape, so the description is the four things that tell one apart from another: the measurement, the half of the run, the pipeline depth and the scale.
fn alt(spec: Spec) -> String {
    let what = match spec.metric {
        Metric::Throughput(which) => format!("{} throughput", which.label()),
        Metric::Latency(percentile, which) => {
            format!("{} {} latency", which.label(), percentile.label())
        }
        Metric::CpuCycles => "CPU cycles per operation".to_owned(),
    };
    let scale = match spec.scale {
        Scale::Linear => "linear",
        Scale::Logarithmic => "logarithmic",
    };
    let tail = match spec.case {
        Some(Case::NoGarnetAtOneThread) => ", without Garnet at one thread",
        None => "",
    };
    format!(
        "{what} by thread count at pipeline depth {}, {scale} scale{tail}",
        spec.pipeline
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a document that will not generate is a failed test"
)]
mod tests {
    use std::collections::BTreeSet;

    use cb_chart::{Scale, Spec};

    use super::Index;

    /// Every chart the spec names, which is what a sweep that finished has on disk.
    fn everything() -> BTreeSet<String> {
        Spec::all().iter().map(|s| s.file()).collect()
    }

    fn index(scale: Scale, have: &BTreeSet<String>) -> String {
        Index {
            scale,
            have,
            absent: "",
            measured: None,
        }
        .render()
    }

    /// A host file of the shape doctor writes, so the provenance block is built from a parsed one rather than from a struct assembled to suit the test.
    const HOST: &str = r#"{
      "profile": "epyc8",
      "kernel": "Linux 6.14.0 x86_64",
      "distro": "Debian 13",
      "cpu_model": "AMD EPYC 7003",
      "cpus": 8,
      "memory_bytes": 34359738368,
      "pmu": "present",
      "governor": { "set": "performance" },
      "mitigations": "19 known, all of them mitigated",
      "memtier": "memtier_benchmark v=2.5.1",
      "cache_bench": { "version": "0.6.0", "git": "abc1234" },
      "started": "2026-09-01T00:00:00Z"
    }"#;

    fn measured(notes: bool, machine: &cb_core::Machine) -> super::Measured<'_> {
        super::Measured {
            machine,
            description: "8 core x86_64. A quarter of the reference, scaled coherently.",
            connections: 256,
            notes,
        }
    }

    // The exit condition for this milestone, as an assertion: every chart that exists is linked and nothing that does not exist is.
    #[test]
    fn the_two_documents_between_them_cover_all_154() {
        let have = everything();
        let mut covered = BTreeSet::new();
        for scale in Scale::ALL {
            covered.extend(
                Index {
                    scale,
                    have: &have,
                    absent: "",
                    measured: None,
                }
                .covers(),
            );
        }
        assert_eq!(covered, have);
        assert_eq!(covered.len(), 154);
    }

    #[test]
    fn every_link_lands_on_a_heading_in_the_document() {
        let have = everything();
        for scale in Scale::ALL {
            let text = index(scale, &have);
            let headings: BTreeSet<String> = text
                .lines()
                .filter_map(|line| line.strip_prefix("## ").or_else(|| line.strip_prefix("# ")))
                .scan(crate::anchor::Anchors::default(), |anchors, heading| {
                    Some(anchors.assign(heading))
                })
                .collect();
            let links = text
                .lines()
                .filter_map(|line| line.split_once("](#"))
                .filter_map(|(_, rest)| rest.split(')').next())
                .map(str::to_owned);
            let mut counted = 0_usize;
            for link in links {
                assert!(
                    headings.contains(&link),
                    "{link} is not a heading in {text}"
                );
                counted += 1;
            }
            // Ten sections under each of four pipeline depths.
            assert_eq!(counted, 40);
        }
    }

    // The suffixes the original wrote by hand, which is the only part of these documents that a reader will notice being wrong.
    #[test]
    fn the_anchors_are_the_ones_the_original_wrote() {
        let have = everything();
        let text = index(Scale::Linear, &have);
        assert!(text.contains("[Throughput](#throughput),"), "{text}");
        assert!(text.contains("[Throughput](#throughput-3),"), "{text}");
        assert!(
            text.contains("[Latency 99.9th Percentile](#latency-999th-percentile-2)"),
            "{text}"
        );
        assert!(text.contains("[CPU Cycles](#cpu-cycles-1)"), "{text}");
    }

    #[test]
    fn a_chart_that_was_not_drawn_is_said_so_rather_than_left_out() {
        let mut have = everything();
        let gone = "graph_cpucycles-pipeline_25-kind_median-scale_linear.png";
        assert!(have.remove(gone));
        let text = Index {
            scale: Scale::Linear,
            have: &have,
            absent: "This host has no hardware counters.",
            measured: None,
        }
        .render();
        assert!(!text.contains(&format!("({}/{gone})", "graphs")), "{text}");
        assert!(
            text.contains(&format!(
                "*Not drawn: {gone}. This host has no hardware counters.*"
            )),
            "{text}"
        );
    }

    #[test]
    fn the_images_are_set_before_get() {
        let have = everything();
        let text = index(Scale::Logarithmic, &have);
        let sets = text
            .find("graph_opsec-which_sets-pipeline_1-")
            .expect("the SET throughput chart is linked");
        let gets = text
            .find("graph_opsec-which_gets-pipeline_1-")
            .expect("the GET throughput chart is linked");
        assert!(sets < gets, "{text}");
    }

    #[test]
    fn the_two_hand_drawn_charts_are_only_in_the_linear_document() {
        let have = everything();
        let linear = index(Scale::Linear, &have);
        let logarithmic = index(Scale::Logarithmic, &have);
        assert!(linear.contains("-case_1.png"), "{linear}");
        assert!(!logarithmic.contains("case_1"), "{logarithmic}");
    }

    // The reader this is for arrived on a link to this file and will not open anything else, so everything they need to read a bar correctly has to be above the first bar.
    #[test]
    fn a_measured_document_names_its_host_and_says_what_it_is_not_evidence_for() {
        let have = everything();
        let machine = cb_core::Machine::parse(HOST).expect("the host file parses");
        let text = Index {
            scale: Scale::Linear,
            have: &have,
            absent: "",
            measured: Some(measured(true, &machine)),
        }
        .render();
        let first = text.find("![").expect("there are charts in it");
        for wanted in [
            "AMD EPYC 7003",
            "8 logical CPUs",
            "Debian 13",
            "`epyc8` profile",
            "A quarter of the reference",
            "[README.md](README.md)",
            "[NOTES.md](NOTES.md)",
            "held at 256 on every point",
            crate::MAY_NOT,
        ] {
            assert!(
                text.find(wanted).is_some_and(|at| at < first),
                "{wanted} is missing, or below the first chart, in {text}"
            );
        }
    }

    // A directory with no note by hand should not send anybody looking for a file that is not there.
    #[test]
    fn a_document_beside_no_notes_file_does_not_link_one() {
        let have = everything();
        let machine = cb_core::Machine::parse(HOST).expect("the host file parses");
        let text = Index {
            scale: Scale::Logarithmic,
            have: &have,
            absent: "",
            measured: Some(measured(false, &machine)),
        }
        .render();
        assert!(text.contains("[README.md](README.md)"), "{text}");
        assert!(!text.contains("NOTES.md"), "{text}");
    }

    // Golden mode draws from the spec with no sweep behind it, and a caveat about numbers that do not exist is one more paragraph teaching people to scroll past caveats.
    #[test]
    fn a_golden_document_claims_no_host_and_no_numbers() {
        let have = everything();
        for scale in Scale::ALL {
            let text = index(scale, &have);
            assert!(!text.contains("Measured by this harness"), "{text}");
            assert!(!text.contains(crate::MAY_NOT), "{text}");
        }
    }

    #[test]
    fn no_image_carries_the_placeholder_the_original_left_in() {
        let have = everything();
        for scale in Scale::ALL {
            let text = index(scale, &have);
            assert!(!text.contains("![Alt text]"), "{text}");
        }
    }
}
