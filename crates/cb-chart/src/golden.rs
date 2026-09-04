//! The two fixtures the chart layer is checked against, and what checking against them means.
//!
//! `series.json` is what the original put on each of its 154 charts and `axes.json` is where it put it. Both were taken from the original by standing in for the thing it hands its work to, matplotlib in one case and Python itself in the other, so both are its own answers rather than a reading of them. `tools/series-vectors` and `tools/axis-vectors` are how, and `testdata/golden/README.md` is why.
//!
//! The shape of the fixtures lives here rather than in a test, because two callers check against them: `cargo test`, which fails a pull request, and `cache-bench verify`, which is the same claim made as a command.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::axis::{Axis, BAR_WIDTH, BORDER, Bars, DPI, FIGURE, GUTTER_X, RECT, Sizes};
use crate::palette::Rgb;
use crate::series::Chart;
use crate::spec::Scale;

/// The geometry fixture.
#[derive(Debug, Clone, Deserialize)]
pub struct Golden {
    /// What the original applies to every chart without looking at it.
    pub constants: Constants,
    /// One entry per chart, in the original's own order.
    pub charts: Vec<Recorded>,
}

/// The numbers that are the same on all 154 charts.
#[derive(Debug, Clone, Deserialize)]
pub struct Constants {
    /// The figure size in inches.
    pub figsize: [f64; 2],
    /// Dots per inch at save time.
    pub dpi: u32,
    /// The white border added afterwards, in pixels.
    pub border: u32,
    /// The fraction of the figure the plot is squeezed into.
    pub rect: [f64; 4],
    /// The width of one bar in x units.
    pub bar_width: f64,
    /// Where each bar in a group starts, one per cache server.
    pub bar_offsets: Vec<f64>,
    /// Where the thread count under a group goes.
    pub xtick_offset: f64,
    /// Where the quarter decade labels sit.
    pub gutter_x: f64,
    /// The point sizes.
    pub font_size: FontSize,
    /// The outline colour for each bar colour.
    pub edges: BTreeMap<String, [f64; 3]>,
}

/// The point sizes, as the original wrote them.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FontSize {
    /// The chart title.
    pub title: u32,
    /// Both axis labels.
    pub axis_label: u32,
    /// The thread counts.
    pub xtick: u32,
    /// The numbers beside a logarithmic y axis.
    pub ytick_logarithmic: u32,
    /// The numbers beside a linear y axis.
    pub ytick_linear: u32,
    /// The legend.
    pub legend: u32,
    /// The quarter decade numbers in the margin.
    pub gutter: u32,
}

/// The geometry of one chart, as matplotlib was given it.
#[derive(Debug, Clone, Deserialize)]
pub struct Recorded {
    /// The chart's filename, which is the key into the series fixture.
    pub file: String,
    /// Which of the two scales it was drawn on.
    pub scale: String,
    /// The bottom and top of the y axis.
    pub ylim: [f64; 2],
    /// The labelled ticks.
    pub yticks: Vec<RecordedTick>,
    /// The gridlines that are not ticks.
    pub lines: Vec<f64>,
    /// The text for those lines, on a logarithmic chart.
    pub gutter: Vec<String>,
    /// The thread counts under the x axis, as text.
    pub xticks: Vec<String>,
    /// The legend, in the order it is drawn.
    pub legend: Vec<String>,
}

/// One labelled tick, as recorded.
#[derive(Debug, Clone, Deserialize)]
pub struct RecordedTick {
    /// Where it sits.
    pub value: f64,
    /// The text beside it.
    pub label: String,
}

/// Somewhere a chart drawn here would differ from the one the original drew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The chart it is on.
    pub file: String,
    /// What differs, in the form a reader can act on.
    pub what: String,
}

/// How much of the fixture a run of [`Golden::check`] actually compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Charts compared.
    pub charts: usize,
    /// Labelled ticks compared.
    pub ticks: usize,
    /// Gridlines compared.
    pub lines: usize,
}

impl Golden {
    /// Read the fixture.
    ///
    /// # Errors
    ///
    /// If it is not the JSON this module describes.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Check the constants against the ones this crate carries.
    ///
    /// Returns everything that differs, which is empty on a clean run.
    #[must_use]
    #[allow(
        clippy::float_cmp,
        reason = "the claim being made is that the arithmetic lands on the same bits, so a tolerance would be the wrong test"
    )]
    pub fn constants(&self) -> Vec<Mismatch> {
        let c = &self.constants;
        let mut out = Vec::new();
        let mut check = |ok: bool, what: &str| {
            if !ok {
                out.push(Mismatch {
                    file: "constants".to_owned(),
                    what: what.to_owned(),
                });
            }
        };
        check((c.figsize[0], c.figsize[1]) == FIGURE, "the figure size");
        check(c.dpi == DPI, "the resolution");
        check(c.border == BORDER, "the white border");
        check(c.rect == RECT, "the layout rectangle");
        check(c.bar_width == BAR_WIDTH, "the bar width");
        check(c.gutter_x == GUTTER_X, "the gutter position");

        let bars = Bars::new(c.bar_offsets.len());
        check(bars.offsets == c.bar_offsets, "the bar offsets");
        // Six bars, which is the one group size where the original's hardcoded offset is the middle of the group.
        check(bars.xtick == c.xtick_offset, "the x tick offset");

        let log = Sizes::of(Scale::Logarithmic);
        check(log.title == c.font_size.title, "the title size");
        check(log.axis_label == c.font_size.axis_label, "the label size");
        check(log.xtick == c.font_size.xtick, "the x tick size");
        check(log.legend == c.font_size.legend, "the legend size");
        check(log.gutter == c.font_size.gutter, "the gutter size");
        check(log.ytick == c.font_size.ytick_logarithmic, "the log y size");
        check(
            Sizes::of(Scale::Linear).ytick == c.font_size.ytick_linear,
            "the linear y size",
        );

        for (hex, want) in &c.edges {
            let ours = Rgb::parse(hex).map(Rgb::edge);
            check(ours == Ok(Rgb(*want)), &format!("the outline for {hex}"));
        }
        out
    }

    /// Draw every axis in the fixture from the series that go on it, and say where any of them differs.
    ///
    /// A clean run has an empty list, and the tally is then the size of the claim that just passed.
    #[must_use]
    #[allow(
        clippy::float_cmp,
        reason = "the claim being made is that the arithmetic lands on the same bits, so a tolerance would be the wrong test"
    )]
    pub fn check(&self, series: &BTreeMap<String, Chart>) -> (Tally, Vec<Mismatch>) {
        let mut tally = Tally::default();
        let mut out = Vec::new();
        for recorded in &self.charts {
            let mut note = |what: String| {
                out.push(Mismatch {
                    file: recorded.file.clone(),
                    what,
                });
            };
            let Some(chart) = series.get(&recorded.file) else {
                note("is not in the series fixture".to_owned());
                continue;
            };
            let scale = match recorded.scale.as_str() {
                "logarithmic" => Scale::Logarithmic,
                "linear" => Scale::Linear,
                other => {
                    note(format!("is drawn on {other}, which is not a scale"));
                    continue;
                }
            };
            let axis = match Axis::new(scale, chart) {
                Ok(axis) => axis,
                Err(e) => {
                    note(format!("will not scale: {e}"));
                    continue;
                }
            };

            if [axis.bottom, axis.top] != recorded.ylim {
                note(format!(
                    "runs from {} to {} where the original runs from {} to {}",
                    axis.bottom, axis.top, recorded.ylim[0], recorded.ylim[1]
                ));
            }
            if axis.ticks.len() == recorded.yticks.len() {
                for (ours, theirs) in axis.ticks.iter().zip(&recorded.yticks) {
                    if ours.value != theirs.value || ours.label != theirs.label {
                        note(format!(
                            "has a tick at {} labelled {:?} where the original has {} labelled {:?}",
                            ours.value, ours.label, theirs.value, theirs.label
                        ));
                    }
                }
                tally.ticks += axis.ticks.len();
            } else {
                note(format!(
                    "has {} ticks where the original has {}",
                    axis.ticks.len(),
                    recorded.yticks.len()
                ));
            }

            if axis.lines.len() == recorded.lines.len() {
                for (ours, theirs) in axis.lines.iter().zip(&recorded.lines) {
                    if ours != theirs {
                        note(format!(
                            "has a gridline at {ours} where the original has one at {theirs}"
                        ));
                    }
                }
                tally.lines += axis.lines.len();
            } else {
                note(format!(
                    "has {} gridlines where the original has {}",
                    axis.lines.len(),
                    recorded.lines.len()
                ));
            }

            if axis.gutter != recorded.gutter {
                note("labels its gridlines differently".to_owned());
            }
            let threads: Vec<String> = chart.x_series.iter().map(u32::to_string).collect();
            if threads != recorded.xticks {
                note("has different thread counts under it".to_owned());
            }
            let legend: Vec<&str> = chart.series.iter().map(|s| s.cache.as_str()).collect();
            if legend != recorded.legend {
                note("has a different legend".to_owned());
            }
            tally.charts += 1;
        }
        (tally, out)
    }
}
