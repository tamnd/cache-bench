//! Where things go on a chart, in the chart's own units.
//!
//! The original decides all of this in about forty lines of Python in the middle of a Go string constant, and none of it is written down anywhere. `tools/axis-vectors` runs those lines against the same 154 charts with matplotlib replaced by something that records what it was told, and `crates/cb-core/golden/axes.json` is what came back. Everything here is checked against it.
//!
//! Nothing in this module knows about pixels. A y axis has a bottom, a top, a set of ticks with the text that goes on them and a set of gridlines, and turning that into a picture is somebody else's job. Splitting it this way is what makes the arithmetic testable, and the arithmetic is where the original keeps its surprises.

use crate::series::Chart;
use crate::spec::Scale;

/// The figure size in inches, which with `DPI` is what the original asks matplotlib for.
pub const FIGURE: (f64, f64) = (12.0, 7.0);

/// Dots per inch at save time.
pub const DPI: u32 = 150;

/// The white border the original adds with PIL after matplotlib is finished, in pixels.
pub const BORDER: u32 = 40;

/// The fraction of the figure the plot is squeezed into, as `[left, bottom, right, top]`.
///
/// The right edge stops at 0.92 to leave the legend somewhere to sit and the top at 0.93 to leave the title somewhere to sit.
pub const RECT: [f64; 4] = [0.0, 0.0, 0.92, 0.93];

/// The width of one bar, in x units, where the gap between two thread counts is one.
pub const BAR_WIDTH: f64 = 0.12;

/// Where the quarter decade labels sit on a logarithmic chart, in x units.
///
/// Well to the left of the first bar, which is at zero, and right aligned so they end just short of the axis.
pub const GUTTER_X: f64 = -0.78;

/// What a bar's colour is multiplied by to get the colour of its outline.
pub const EDGE_SCALE: f64 = 0.4;

/// The step between one gridline and the next on a logarithmic chart, in decades.
///
/// The original calls these quarter decade lines and writes the step as `0.25/2`, so they are eighths of a decade. The name is kept because it is the original's, and because seven lines between one power of ten and the next is what the charts have.
pub const DECADE_STEP: f64 = 0.25 / 2.0;

/// How many major ticks a linear axis gets, counting both ends.
pub const LINEAR_TICKS: usize = 20;

/// How far above the tallest bar a linear axis stops.
pub const LINEAR_HEADROOM: f64 = 1.1;

/// The point sizes, which are the original's and differ between the two scales in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// The chart title.
    pub title: u32,
    /// Both axis labels.
    pub axis_label: u32,
    /// The thread counts under the x axis.
    pub xtick: u32,
    /// The numbers beside the y axis.
    pub ytick: u32,
    /// The cache server names in the legend.
    pub legend: u32,
    /// The quarter decade numbers in the margin, on a logarithmic chart.
    pub gutter: u32,
}

impl Sizes {
    /// The sizes for a scale.
    ///
    /// The y tick numbers are a point larger on a logarithmic chart than on a linear one. There is no reason for it in the original beyond that they are two separate scripts, and it is kept because the charts have to line up with the published ones.
    #[must_use]
    pub const fn of(scale: Scale) -> Self {
        Self {
            title: 20,
            axis_label: 18,
            xtick: 12,
            ytick: match scale {
                Scale::Logarithmic => 13,
                Scale::Linear => 12,
            },
            legend: 12,
            gutter: 8,
        }
    }
}

/// One labelled tick on the y axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Where it sits, in the chart's units.
    pub value: f64,
    /// The text beside it, with thousands separators.
    pub label: String,
}

/// A y axis, worked out from the bars that have to fit on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    /// The bottom of the axis.
    pub bottom: f64,
    /// The top of the axis.
    pub top: f64,
    /// The labelled ticks, in order.
    pub ticks: Vec<Tick>,
    /// The gridlines that are not ticks, in order. Eighths of a decade on a logarithmic chart, quarters of a tick gap on a linear one.
    pub lines: Vec<f64>,
    /// The text for each of those lines, on a logarithmic chart. Empty on a linear one, where the minor lines are drawn unlabelled.
    pub gutter: Vec<String>,
}

/// A chart with nothing on it to scale an axis from.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unscalable {
    /// Every bar on the chart is absent.
    #[error("no bar on the chart has a value")]
    Empty,
    /// A logarithmic axis was asked for and the shortest bar is not above zero.
    ///
    /// The original would raise here rather than draw anything, and the reason no published chart hits it is that the two charts with a zero on them are the two the original draws in linear only.
    #[error("a logarithmic axis needs every bar above zero, and the shortest is {0}")]
    NotPositive(i64),
}

impl Axis {
    /// Work out the axis a chart needs.
    ///
    /// # Errors
    ///
    /// If the chart has no bars with a value, or if a logarithmic axis was asked for and one of them is not above zero.
    pub fn new(scale: Scale, chart: &Chart) -> Result<Self, Unscalable> {
        let mut drawn = chart
            .series
            .iter()
            .flat_map(|s| s.points.iter().copied().flatten());
        let first = drawn.next().ok_or(Unscalable::Empty)?;
        let (low, high) = drawn.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v)));
        match scale {
            Scale::Logarithmic => Self::logarithmic(low, high),
            Scale::Linear => Ok(Self::linear(high)),
        }
    }

    fn logarithmic(low: i64, high: i64) -> Result<Self, Unscalable> {
        if low <= 0 {
            return Err(Unscalable::NotPositive(low));
        }
        // Two ways of taking the same logarithm, because the original takes it two ways.
        // The top goes through `math.log(x, 10)`, which is a division of two natural logs, and the bottom through `math.log10`, which is not. They agree on every published chart and they are not the same function, so both are written the way they were read.
        let top_exp = ceil(widen(high).ln() / 10f64.ln());
        let bottom_exp = floor(widen(low).log10());

        let ticks = (bottom_exp..=top_exp)
            .map(|e| {
                let value = decade(e);
                Tick {
                    label: commas(whole(value)),
                    value,
                }
            })
            .collect();

        // The original steps the exponent from the bottom decade to the top one, exclusive, and drops any step that lands on a decade because that already has a tick.
        // It drops them by testing the value against the tick list, which is the same test as asking whether the exponent is a whole number and does not depend on ten to a whole power coming out exact.
        let mut lines = Vec::new();
        let mut gutter = Vec::new();
        let span = f64::from(top_exp - bottom_exp);
        let steps = ceil(span / DECADE_STEP);
        for i in 0..steps {
            let exp = f64::from(bottom_exp) + f64::from(i) * DECADE_STEP;
            if exp.fract() == 0.0 {
                continue;
            }
            let value = 10f64.powf(exp);
            gutter.push(commas(nearest(value)));
            lines.push(value);
        }

        Ok(Self {
            bottom: decade(bottom_exp),
            top: decade(top_exp),
            ticks,
            lines,
            gutter,
        })
    }

    fn linear(high: i64) -> Self {
        let top = widen(high) * LINEAR_HEADROOM;
        // What `np.linspace(0, top, num=20)` does: a step of the span over nineteen, multiplied out rather than accumulated, and then the last one replaced by the endpoint so that the axis ends exactly where it was asked to.
        let gaps = LINEAR_TICKS - 1;
        let step = top / cast(gaps);
        let values: Vec<f64> = (0..LINEAR_TICKS)
            .map(|i| if i == gaps { top } else { cast(i) * step })
            .collect();

        // Three lines in each gap, at the quarters. Worked out from the pair either side rather than from the step, because a difference of two rounded numbers is not the rounded difference and the original uses the pair.
        let mut lines = Vec::new();
        for pair in values.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let quarter = (end - start) / 4.0;
            for j in 1..4 {
                lines.push(start + f64::from(j) * quarter);
            }
        }

        Self {
            bottom: 0.0,
            top,
            // Truncated rather than rounded, which is the original's `int(y)`, so a tick at 1764.74 is labelled 1,764.
            ticks: values
                .into_iter()
                .map(|value| Tick {
                    label: commas(whole(value)),
                    value,
                })
                .collect(),
            lines,
            gutter: Vec::new(),
        }
    }
}

/// Where each bar in a group sits, relative to the group's thread count.
#[derive(Debug, Clone, PartialEq)]
pub struct Bars {
    /// The width of one bar.
    pub width: f64,
    /// The left edge of each bar, one per cache server, in legend order.
    pub offsets: Vec<f64>,
    /// Where the thread count under the group goes.
    pub xtick: f64,
}

impl Bars {
    /// Lay out a group of `count` bars.
    ///
    /// The original writes the tick offset as `width * 2.5`, which centres the label under six bars and under no other number of them. With a seventh cache server that label sits half a bar to the left of the group it names, so here it is the middle of however many bars there are. For six the two agree exactly, which is why every published chart is unaffected. See D13 in `divergences.md`.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            width: BAR_WIDTH,
            offsets: (0..count).map(|i| cast(i) * BAR_WIDTH).collect(),
            xtick: BAR_WIDTH * (cast(count) - 1.0) / 2.0,
        }
    }

    /// The original's tick offset, which is `width * 2.5` whatever the group holds.
    #[must_use]
    pub fn upstream_xtick() -> f64 {
        BAR_WIDTH * 2.5
    }
}

/// Ten to a whole power, by multiplying rather than by calling anything.
///
/// The original writes `10 ** i` with both sides whole, which in Python is integer arithmetic and exact. Doing it with a power function would put a libm between the source and a gridline, and a gridline that lands on a different pixel on a different machine is the one thing the chart layer is not allowed to do.
fn decade(exponent: i32) -> f64 {
    let mut v = 1.0;
    for _ in 0..exponent {
        v *= 10.0;
    }
    v
}

/// A number with commas every three digits, which is Python's `f"{n:,}"`.
fn commas(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the exponents here are single digits, and a chart whose axis spans more than two billion decades is not a chart"
)]
fn ceil(v: f64) -> i32 {
    v.ceil() as i32
}

#[allow(clippy::cast_possible_truncation, reason = "see ceil")]
fn floor(v: f64) -> i32 {
    v.floor() as i32
}

/// Python's `int(v)`, which truncates toward zero.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a tick above nine quintillion is not a tick"
)]
fn whole(v: f64) -> i64 {
    v.trunc() as i64
}

/// Python's `round`, which is nearest with ties to even.
fn nearest(v: f64) -> i64 {
    whole(v.round_ties_even())
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bar counts and tick indices are single digits"
)]
fn cast(v: usize) -> f64 {
    v as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "the tallest bar the benchmark can produce is far inside what an f64 holds exactly"
)]
fn widen(v: i64) -> f64 {
    v as f64
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a fixture that will not scale is a failed test"
)]
#[allow(
    clippy::float_cmp,
    reason = "the claim being made is that the arithmetic lands on the same bits, so a tolerance would be the wrong test"
)]
mod tests {
    use super::{Axis, Bars, Sizes, commas, decade};
    use crate::series::{Chart, Series};
    use crate::spec::Scale;

    fn chart(points: &[i64]) -> Chart {
        Chart {
            file: "test.png".to_owned(),
            title: String::new(),
            x_title: String::new(),
            y_title: String::new(),
            x_series: vec![1],
            series: vec![Series {
                cache: "yo".to_owned(),
                color: "#9467bd".to_owned(),
                points: points.iter().map(|&p| Some(p)).collect(),
            }],
        }
    }

    #[test]
    fn commas_are_pythons_commas() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(1334), "1,334");
        assert_eq!(commas(51_200_000), "51,200,000");
        assert_eq!(commas(-1234), "-1,234");
    }

    #[test]
    fn a_decade_is_exact() {
        assert_eq!(decade(0), 1.0);
        assert_eq!(decade(3), 1000.0);
        assert_eq!(decade(8), 100_000_000.0);
    }

    // Seven gridlines between one power of ten and the next, and none of them on a power of ten.
    #[test]
    fn a_logarithmic_axis_has_seven_lines_per_decade() {
        let axis = Axis::new(Scale::Logarithmic, &chart(&[1200, 90_000])).unwrap();
        assert_eq!(axis.bottom, 1000.0);
        assert_eq!(axis.top, 100_000.0);
        assert_eq!(axis.ticks.len(), 3);
        assert_eq!(axis.lines.len(), 14);
        assert_eq!(axis.gutter.len(), 14);
        assert_eq!(axis.gutter[0], "1,334");
        assert_eq!(
            axis.ticks
                .iter()
                .map(|t| t.label.as_str())
                .collect::<Vec<_>>(),
            ["1,000", "10,000", "100,000"]
        );
    }

    #[test]
    fn a_linear_axis_stops_a_tenth_above_the_tallest_bar() {
        let axis = Axis::new(Scale::Linear, &chart(&[100, 1000])).unwrap();
        assert_eq!(axis.bottom, 0.0);
        assert_eq!(axis.top, 1_100.000_000_000_000_1);
        assert_eq!(axis.ticks.len(), 20);
        assert_eq!(axis.ticks[0].value, 0.0);
        assert_eq!(axis.ticks[19].value, axis.top);
        assert_eq!(axis.lines.len(), 57);
    }

    // The zero the original writes for a bar it never measured is fine on a linear axis and is not a number a logarithm has anything to say about.
    #[test]
    fn a_zero_is_a_linear_chart_only() {
        assert!(Axis::new(Scale::Logarithmic, &chart(&[0, 1000])).is_err());
        assert!(Axis::new(Scale::Linear, &chart(&[0, 1000])).is_ok());
    }

    #[test]
    fn an_empty_chart_is_an_error_rather_than_a_panic() {
        let mut empty = chart(&[1]);
        empty.series[0].points = vec![None];
        assert!(Axis::new(Scale::Linear, &empty).is_err());
        assert!(Axis::new(Scale::Logarithmic, &empty).is_err());
    }

    // The original's hardcoded offset is the middle of six bars, and the middle of six bars is what the rule here gives for six.
    #[test]
    fn six_bars_land_where_the_original_puts_them() {
        let bars = Bars::new(6);
        assert_eq!(bars.offsets, [0.0, 0.12, 0.24, 0.36, 0.48, 0.6]);
        assert_eq!(bars.xtick, Bars::upstream_xtick());
        assert_eq!(bars.xtick, 0.3);
    }

    // And with a seventh they part company, which is the whole reason the rule is written out.
    #[test]
    fn a_seventh_bar_moves_the_tick_and_the_original_would_not() {
        let bars = Bars::new(7);
        assert_eq!(bars.offsets.len(), 7);
        assert_ne!(bars.xtick, Bars::upstream_xtick());
        assert_eq!(bars.xtick, 0.36);
    }

    #[test]
    fn the_y_numbers_are_a_point_bigger_on_a_log_chart() {
        assert_eq!(Sizes::of(Scale::Logarithmic).ytick, 13);
        assert_eq!(Sizes::of(Scale::Linear).ytick, 12);
        assert_eq!(Sizes::of(Scale::Linear).title, 20);
    }
}
