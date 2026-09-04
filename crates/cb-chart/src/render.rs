//! One chart, drawn.
//!
//! Everything that decides what the picture says has already happened by the time anything here runs. `series` settled what goes on the chart and `axis` settled where it goes in the chart's own units, both of them checked against the original's own answers. This module turns those units into pixels and puts them down, so a chart that is wrong is wrong in exactly one of three places and the first two have fixtures.
//!
//! The picture is the same size every time. The original saves with a tight bounding box, so its images come out 1714, 1715 or 1716 pixels wide depending on how many digits its y axis numbers happen to have, and a reader flipping between two charts is looking at two different croppings. Here the canvas is the figure size the original asks for, at the resolution it asks for, with the border it adds afterwards, and it does not move. That is D14.

use crate::axis::{Axis, BAR_WIDTH, BORDER, Bars, DPI, FIGURE, Sizes};
use crate::canvas::{Canvas, Pixel, Rect};
use crate::font::Face;
use crate::palette::{BadColor, Rgb};
use crate::series::Chart;
use crate::spec::Scale;
use crate::text::{Align, Text, Turn, Unreadable};

/// Ink.
pub const INK: Pixel = [0, 0, 0];

/// The grey a gridline and the numbers beside it are drawn in.
///
/// The original asks for `gray` at three tenths opacity for a minor line and seven tenths for a major one, on white. Those are worked out here rather than blended at draw time, because a chart has no transparency in it once it is finished and a colour that is already the answer is one fewer thing to get wrong.
pub const MAJOR_LINE: Pixel = [166, 166, 166];

/// The grey of a minor gridline, which is the same grey at three tenths rather than seven.
pub const MINOR_LINE: Pixel = [217, 217, 217];

/// The grey the quarter decade numbers are written in.
pub const GUTTER_INK: Pixel = [128, 128, 128];

/// The grey the provenance stamp is written in, dark enough to read and light enough not to compete with the chart.
pub const STAMP_INK: Pixel = [110, 110, 110];

/// How thick a major gridline is, in pixels.
pub const MAJOR_WIDTH: f64 = 1.5;

/// How thick a minor gridline is.
pub const MINOR_WIDTH: f64 = 1.0;

/// How thick the outline around a bar is.
pub const EDGE_WIDTH: f64 = 1.5;

/// Where the plot sits on the canvas, and where everything around it goes.
///
/// All of it in whole pixels, all of it fixed. None of these depend on the chart, which is what makes every one of the 154 the same shape and what makes two of them comparable by flipping between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The left edge of the plot.
    pub left: u32,
    /// The top edge of the plot.
    pub top: u32,
    /// The right edge of the plot.
    pub right: u32,
    /// The bottom edge of the plot, which is where the bars stand.
    pub bottom: u32,
}

/// The layout every chart is drawn with.
pub const LAYOUT: Layout = Layout {
    left: 290,
    top: 160,
    right: 1600,
    bottom: 940,
};

/// How far left of the plot the y axis numbers end.
pub const Y_LABEL_GAP: f64 = 16.0;

/// Where the turned y axis label sits.
pub const Y_TITLE_X: f64 = 96.0;

/// The baseline of the title.
pub const TITLE_BASELINE: f64 = 104.0;

/// The baseline of the thread counts under the x axis.
pub const XTICK_BASELINE: f64 = 985.0;

/// The baseline of the x axis label.
pub const X_TITLE_BASELINE: f64 = 1052.0;

/// The baseline of the provenance stamp.
pub const STAMP_BASELINE: f64 = 1108.0;

/// The point size of the provenance stamp.
pub const STAMP_POINTS: f64 = 9.0;

/// The side of a legend swatch, in pixels.
pub const SWATCH: f64 = 22.0;

/// How far right of the plot the legend starts.
pub const LEGEND_GAP: f64 = 30.0;

/// The gap between a swatch and the name beside it.
pub const LEGEND_PAD: f64 = 14.0;

/// The gap from one legend entry to the next.
pub const LEGEND_STEP: f64 = 56.0;

/// The gap at each end of the x axis, in chart units.
///
/// Half the gap between one group of bars and the next, so the first group is as far from the left edge as it is from its neighbour and the chart does not look pushed to one side.
#[must_use]
pub fn end_gap(caches: usize) -> f64 {
    (1.0 - BAR_WIDTH * cast(caches)) / 2.0
}

/// Who and what produced the numbers, written along the bottom of every chart.
///
/// A throughput chart with no machine on it is a number without a unit. Two of these from two machines are not comparable and there is nothing in the picture to say so, which is the single most likely way for a chart drawn here to end up misleading somebody. The original has no stamp at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stamp {
    /// The profile name, which is what `--profile` was given.
    pub profile: String,
    /// What the machine is, in a sentence, from the profile.
    pub machine: String,
    /// Anything else worth carrying, such as the engine versions.
    pub note: String,
}

impl Stamp {
    /// The one line that goes on the chart.
    #[must_use]
    pub fn line(&self) -> String {
        [
            self.profile.as_str(),
            self.machine.as_str(),
            self.note.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  |  ")
    }
}

/// Something that stopped a chart being drawn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Undrawable {
    /// A series carries a colour that is not a colour.
    #[error(transparent)]
    Color(#[from] BadColor),
    /// A face will not parse, which cannot happen with the fonts committed here.
    #[error(transparent)]
    Font(#[from] Unreadable),
}

/// Draw a chart.
///
/// The axis has already been worked out, and is passed in rather than computed here so that the same axis a fixture checked is the one the picture is drawn from.
///
/// # Errors
///
/// If a series carries a colour that will not parse, or if one of the embedded faces will not.
pub fn draw(chart: &Chart, axis: &Axis, scale: Scale, stamp: &Stamp) -> Result<Canvas, Undrawable> {
    let (mut canvas, plot) = blank();
    let sizes = Sizes::of(scale);

    gridlines(&mut canvas, axis, scale, &plot);
    y_labels(&mut canvas, axis, scale, &plot, sizes)?;
    bars(&mut canvas, chart, axis, scale, &plot)?;
    x_labels(&mut canvas, chart, &plot, sizes)?;
    titles(&mut canvas, chart, &plot, sizes)?;
    legend(&mut canvas, chart, &plot, sizes)?;
    provenance(&mut canvas, stamp)?;

    Ok(canvas)
}

/// The canvas size every chart is drawn at.
///
/// The figure size at the resolution the original saves at, plus the white border it adds afterwards on all four sides.
#[must_use]
pub fn size() -> (u32, u32) {
    let across = whole(FIGURE.0 * f64::from(DPI)) + BORDER * 2;
    let down = whole(FIGURE.1 * f64::from(DPI)) + BORDER * 2;
    (across, down)
}

/// An empty chart and the box the plot goes in.
fn blank() -> (Canvas, Plot) {
    let (across, down) = size();
    (Canvas::new(across, down), Plot::new())
}

/// The plot box in pixels, with the arithmetic that turns chart units into them.
#[derive(Debug, Clone, Copy)]
struct Plot {
    /// The left edge.
    left: f64,
    /// The top edge.
    top: f64,
    /// The right edge.
    right: f64,
    /// The bottom edge.
    bottom: f64,
}

impl Plot {
    /// The fixed box.
    fn new() -> Self {
        Self {
            left: f64::from(LAYOUT.left),
            top: f64::from(LAYOUT.top),
            right: f64::from(LAYOUT.right),
            bottom: f64::from(LAYOUT.bottom),
        }
    }

    /// How wide the plot is.
    fn width(self) -> f64 {
        self.right - self.left
    }

    /// How tall the plot is.
    fn height(self) -> f64 {
        self.bottom - self.top
    }

    /// Where a value on the y axis sits, in pixels down the canvas.
    fn y(self, value: f64, axis: &Axis, scale: Scale) -> f64 {
        let fraction = match scale {
            Scale::Logarithmic => {
                let low = axis.bottom.log10();
                (value.log10() - low) / (axis.top.log10() - low)
            }
            Scale::Linear => (value - axis.bottom) / (axis.top - axis.bottom),
        };
        self.bottom - fraction * self.height()
    }

    /// Where a position along the x axis sits, in pixels across the canvas.
    ///
    /// The unit is the original's: one group of bars to the next is one, and a bar is `BAR_WIDTH` of it. The plot holds exactly as many units as there are groups, with half a gap spare at each end.
    fn x(self, at: f64, groups: usize, caches: usize) -> f64 {
        let span = cast(groups.max(1));
        self.left + (at + end_gap(caches)) * self.width() / span
    }
}

/// The gridlines, drawn first so that everything else sits on top of them.
fn gridlines(canvas: &mut Canvas, axis: &Axis, scale: Scale, plot: &Plot) {
    for value in &axis.lines {
        line(
            canvas,
            plot,
            plot.y(*value, axis, scale),
            MINOR_WIDTH,
            MINOR_LINE,
        );
    }
    for tick in &axis.ticks {
        line(
            canvas,
            plot,
            plot.y(tick.value, axis, scale),
            MAJOR_WIDTH,
            MAJOR_LINE,
        );
    }
}

/// One horizontal rule across the plot, centred on the value it marks.
fn line(canvas: &mut Canvas, plot: &Plot, y: f64, thickness: f64, color: Pixel) {
    if y < plot.top - 1.0 || y > plot.bottom + 1.0 {
        return;
    }
    canvas.fill_rect(
        Rect {
            x0: plot.left,
            y0: y - thickness / 2.0,
            x1: plot.right,
            y1: y + thickness / 2.0,
        },
        color,
    );
}

/// The numbers beside the y axis, both the labelled ticks and the quarter decade text.
fn y_labels(
    canvas: &mut Canvas,
    axis: &Axis,
    scale: Scale,
    plot: &Plot,
    sizes: Sizes,
) -> Result<(), Unreadable> {
    let major = Text::new(Face::Body, f64::from(sizes.ytick), INK);
    let lift = major.cap_height()? / 2.0;
    for tick in &axis.ticks {
        let y = plot.y(tick.value, axis, scale);
        major.draw(
            canvas,
            &tick.label,
            plot.left - Y_LABEL_GAP,
            y + lift,
            Align::Right,
            Turn::Level,
        )?;
    }

    let minor = Text::new(Face::Gutter, f64::from(sizes.gutter), GUTTER_INK);
    let small = minor.cap_height()? / 2.0;
    for (value, label) in axis.lines.iter().zip(&axis.gutter) {
        let y = plot.y(*value, axis, scale);
        minor.draw(
            canvas,
            label,
            plot.left - Y_LABEL_GAP,
            y + small,
            Align::Right,
            Turn::Level,
        )?;
    }
    Ok(())
}

/// The bars, each one filled and then outlined in its own colour darkened.
fn bars(
    canvas: &mut Canvas,
    chart: &Chart,
    axis: &Axis,
    scale: Scale,
    plot: &Plot,
) -> Result<(), Undrawable> {
    let groups = chart.x_series.len();
    let caches = chart.series.len();
    let offsets = Bars::new(caches).offsets;

    for (which, series) in chart.series.iter().enumerate() {
        let fill: Pixel = Rgb::parse(&series.color)?.into();
        let edge: Pixel = Rgb::parse(&series.color)?.edge().into();
        let offset = offsets.get(which).copied().unwrap_or_default();

        for (group, point) in series.points.iter().enumerate() {
            // A bar with no value is a cell that was never measured, and it is left off rather than drawn as a zero. That is D11.
            let Some(value) = point else { continue };
            let top = plot.y(widen(*value), axis, scale);
            let x0 = plot.x(cast(group) + offset, groups, caches);
            let x1 = plot.x(cast(group) + offset + BAR_WIDTH, groups, caches);
            let bar = Rect {
                x0,
                y0: top.max(plot.top),
                x1,
                y1: plot.bottom,
            };
            if bar.y1 <= bar.y0 {
                continue;
            }
            canvas.fill_rect(bar, fill);
            for side in bar.outline(EDGE_WIDTH) {
                canvas.fill_rect(side, edge);
            }
        }
    }
    Ok(())
}

/// The thread counts under the x axis, one per group of bars.
fn x_labels(
    canvas: &mut Canvas,
    chart: &Chart,
    plot: &Plot,
    sizes: Sizes,
) -> Result<(), Unreadable> {
    let text = Text::new(Face::Body, f64::from(sizes.xtick), INK);
    let groups = chart.x_series.len();
    let caches = chart.series.len();
    let middle = Bars::new(caches).xtick;
    for (at, threads) in chart.x_series.iter().enumerate() {
        let x = plot.x(cast(at) + middle, groups, caches);
        text.draw(
            canvas,
            &threads.to_string(),
            x,
            XTICK_BASELINE,
            Align::Center,
            Turn::Level,
        )?;
    }
    Ok(())
}

/// The heading and the two axis labels, all three in the bold face.
fn titles(canvas: &mut Canvas, chart: &Chart, plot: &Plot, sizes: Sizes) -> Result<(), Unreadable> {
    let middle = f64::midpoint(plot.left, plot.right);
    Text::new(Face::Heading, f64::from(sizes.title), INK).draw(
        canvas,
        &chart.title,
        middle,
        TITLE_BASELINE,
        Align::Center,
        Turn::Level,
    )?;
    let label = Text::new(Face::Heading, f64::from(sizes.axis_label), INK);
    label.draw(
        canvas,
        &chart.x_title,
        middle,
        X_TITLE_BASELINE,
        Align::Center,
        Turn::Level,
    )?;
    label.draw(
        canvas,
        &chart.y_title,
        Y_TITLE_X,
        f64::midpoint(plot.top, plot.bottom),
        Align::Center,
        Turn::Up,
    )?;
    Ok(())
}

/// The legend down the right hand side, a swatch and a name per cache server.
fn legend(canvas: &mut Canvas, chart: &Chart, plot: &Plot, sizes: Sizes) -> Result<(), Undrawable> {
    let text = Text::new(Face::Body, f64::from(sizes.legend), INK);
    let lift = text.cap_height()? / 2.0;
    let x = plot.right + LEGEND_GAP;
    let count = cast(chart.series.len());
    let first = f64::midpoint(plot.top, plot.bottom) - (count - 1.0) * LEGEND_STEP / 2.0;

    for (at, series) in chart.series.iter().enumerate() {
        let middle = first + cast(at) * LEGEND_STEP;
        let swatch = Rect::new(x, middle - SWATCH / 2.0, SWATCH, SWATCH);
        let fill: Pixel = Rgb::parse(&series.color)?.into();
        let edge: Pixel = Rgb::parse(&series.color)?.edge().into();
        canvas.fill_rect(swatch, fill);
        for side in swatch.outline(EDGE_WIDTH) {
            canvas.fill_rect(side, edge);
        }
        text.draw(
            canvas,
            &series.cache,
            x + SWATCH + LEGEND_PAD,
            middle + lift,
            Align::Left,
            Turn::Level,
        )?;
    }
    Ok(())
}

/// The provenance stamp along the bottom.
fn provenance(canvas: &mut Canvas, stamp: &Stamp) -> Result<(), Unreadable> {
    let line = stamp.line();
    if line.is_empty() {
        return Ok(());
    }
    Text::new(Face::Gutter, STAMP_POINTS, STAMP_INK).draw(
        canvas,
        &line,
        f64::from(LAYOUT.left),
        STAMP_BASELINE,
        Align::Left,
        Turn::Level,
    )
}

/// A count as a number to compute with.
#[allow(
    clippy::cast_precision_loss,
    reason = "a chart has at most a few dozen groups and seven series"
)]
fn cast(v: usize) -> f64 {
    v as f64
}

/// A bar height as a number to compute with.
#[allow(
    clippy::cast_precision_loss,
    reason = "the tallest bar the benchmark can produce is far inside what an f64 holds exactly"
)]
fn widen(v: i64) -> f64 {
    v as f64
}

/// A pixel count from a size in inches times a resolution.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "twelve inches at 150 dots is 1800, and neither number comes from anywhere but the two constants above"
)]
fn whole(v: f64) -> u32 {
    v.round() as u32
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a chart that will not draw is a failed test"
)]
mod tests {
    use super::{LAYOUT, Stamp, draw, end_gap, size};
    use crate::axis::{Axis, BAR_WIDTH};
    use crate::canvas::Canvas;
    use crate::series::{Chart, Series};
    use crate::spec::Scale;

    fn chart() -> Chart {
        Chart {
            file: "graph_test.png".to_owned(),
            title: "GET+SET - 256 Clients - 51200000 Ops - Pipeline 1".to_owned(),
            x_title: "Threads".to_owned(),
            y_title: "CPU Cycles (cycles/op)".to_owned(),
            x_series: vec![1, 2, 4, 8],
            series: vec![
                Series {
                    cache: "dragonfly".to_owned(),
                    color: "#ff7f0e".to_owned(),
                    points: vec![Some(12_000), Some(13_500), Some(14_000), Some(15_000)],
                },
                Series {
                    cache: "valkey".to_owned(),
                    color: "#2ca02c".to_owned(),
                    points: vec![Some(9_500), Some(16_000), None, Some(31_000)],
                },
            ],
        }
    }

    fn drawn(scale: Scale) -> Canvas {
        let chart = chart();
        let axis = Axis::new(scale, &chart).unwrap();
        draw(&chart, &axis, scale, &Stamp::default()).unwrap()
    }

    fn ink(canvas: &Canvas) -> usize {
        canvas.bytes().iter().filter(|&&b| b != 255).count()
    }

    // The figure size the original asks for, at the resolution it saves at, with the border it adds afterwards.
    #[test]
    fn every_chart_is_the_same_size() {
        assert_eq!(size(), (1880, 1130));
        for scale in [Scale::Logarithmic, Scale::Linear] {
            let canvas = drawn(scale);
            assert_eq!((canvas.width(), canvas.height()), size());
        }
    }

    #[test]
    fn the_plot_fits_on_the_canvas_with_room_for_the_legend() {
        let (across, down) = size();
        assert!(LAYOUT.left < LAYOUT.right && LAYOUT.right < across);
        assert!(LAYOUT.top < LAYOUT.bottom && LAYOUT.bottom < down);
        // Enough room to the right of the plot for a swatch and the longest cache name a chart carries.
        assert!(across - LAYOUT.right > 200);
    }

    #[test]
    fn both_scales_draw_something() {
        for scale in [Scale::Logarithmic, Scale::Linear] {
            assert!(ink(&drawn(scale)) > 0);
        }
    }

    // The border is what the original adds with PIL after the fact, and nothing may be drawn in it.
    #[test]
    fn the_border_stays_white() {
        let canvas = drawn(Scale::Logarithmic);
        let (across, down) = (canvas.width(), canvas.height());
        for x in 0..across {
            for y in [0_u32, 39, down - 40, down - 1] {
                let at = ((y * across + x) * 3) as usize;
                assert_eq!(&canvas.bytes()[at..at + 3], [255, 255, 255], "{x},{y}");
            }
        }
    }

    // Two charts with the same numbers are the same picture, which is the whole claim the manifest rests on.
    #[test]
    fn drawing_the_same_chart_twice_gives_the_same_bytes() {
        assert_eq!(drawn(Scale::Linear).bytes(), drawn(Scale::Linear).bytes());
    }

    // The two scales put the same bars in different places, so a chart drawn on the wrong one is not quietly the same picture.
    #[test]
    fn the_two_scales_are_different_pictures() {
        assert_ne!(
            drawn(Scale::Linear).bytes(),
            drawn(Scale::Logarithmic).bytes()
        );
    }

    // A bar that is not there leaves white where a zero would have left a line along the axis.
    #[test]
    fn a_missing_bar_is_not_drawn() {
        let mut chart = chart();
        let axis = Axis::new(Scale::Linear, &chart).unwrap();
        let with = draw(&chart, &axis, Scale::Linear, &Stamp::default()).unwrap();
        chart.series[1].points[0] = None;
        let without = draw(&chart, &axis, Scale::Linear, &Stamp::default()).unwrap();
        assert!(ink(&without) < ink(&with));
    }

    // The stamp is the one thing on the chart the original has no equivalent of, so it has to actually appear.
    #[test]
    fn the_stamp_is_drawn_when_there_is_one() {
        let chart = chart();
        let axis = Axis::new(Scale::Linear, &chart).unwrap();
        let bare = draw(&chart, &axis, Scale::Linear, &Stamp::default()).unwrap();
        let stamped = draw(
            &chart,
            &axis,
            Scale::Linear,
            &Stamp {
                profile: "gamingpc".to_owned(),
                machine: "AMD Ryzen 9, 16 cores, 64 GB".to_owned(),
                note: String::new(),
            },
        )
        .unwrap();
        assert!(ink(&stamped) > ink(&bare));
    }

    #[test]
    fn a_stamp_with_nothing_in_it_says_nothing() {
        assert_eq!(Stamp::default().line(), "");
        assert_eq!(
            Stamp {
                profile: "server1".to_owned(),
                machine: "a box".to_owned(),
                note: String::new()
            }
            .line(),
            "server1  |  a box"
        );
    }

    // Six bars of 0.12 fill 0.72 of the unit, so the spare 0.28 is split between the two ends and every gap on the axis is the same width.
    #[test]
    fn the_gap_at_each_end_is_half_the_gap_between_groups() {
        let between = 1.0 - BAR_WIDTH * 6.0;
        assert!((end_gap(6) - between / 2.0).abs() < 1e-12);
        assert!(end_gap(7) < end_gap(6));
    }

    #[test]
    fn a_colour_that_is_not_a_colour_is_an_error_rather_than_a_panic() {
        let mut chart = chart();
        chart.series[0].color = "orange".to_owned();
        let axis = Axis::new(Scale::Linear, &chart).unwrap();
        assert!(draw(&chart, &axis, Scale::Linear, &Stamp::default()).is_err());
    }
}
