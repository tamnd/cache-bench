//! Text, from an embedded outline to pixels on the canvas.
//!
//! The original asks matplotlib for Futura and Verdana and gets whatever the machine has, which is why two people running it produce two different pictures. Here the three faces are in the binary, laid out by this module and rasterized by this module, so the only thing a chart depends on is the chart.
//!
//! Layout is deliberately plain. One line, left to right, no kerning pairs and no shaping. Everything a chart says is digits, ASCII letters and a handful of punctuation, none of which needs any of that, and a shaping engine would be several thousand lines of dependency to arrive at the same advance widths.

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::ReadError;
use skrifa::{FontRef, MetadataProvider};
use zeno::{Command, Mask, Point};

use crate::canvas::{Canvas, Pixel};
use crate::font::Face;

/// Dots per inch, which turns a point size into a pixel size.
///
/// The same 150 the original saves at. A point is a seventy second of an inch, so a 20 point title is 41 and two thirds pixels tall.
pub const DPI: f64 = 150.0;

/// Where a piece of text sits relative to the position it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// The position is the left end.
    Left,
    /// The position is the middle.
    Center,
    /// The position is the right end.
    Right,
}

/// Which way the text runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Left to right.
    Level,
    /// Bottom to top, which is what a y axis label does.
    Up,
}

/// One of the embedded faces at one size, ready to measure and draw with.
#[derive(Debug, Clone, Copy)]
pub struct Text {
    /// Which face.
    pub face: Face,
    /// The size in points, as the original writes it.
    pub points: f64,
    /// The colour.
    pub color: Pixel,
}

/// A face that will not parse, which means the embedded bytes are not a font.
///
/// It cannot happen with the fonts committed here, because a test reads all three and checks their digests, but the parser returns a result and swallowing it would mean a chart with no text on it and no explanation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the embedded {0} will not parse: {1}")]
pub struct Unreadable(&'static str, String);

impl Text {
    /// A run of text in a face at a size.
    #[must_use]
    pub const fn new(face: Face, points: f64, color: Pixel) -> Self {
        Self {
            face,
            points,
            color,
        }
    }

    /// The size in pixels, which is the size in points scaled by the resolution.
    #[must_use]
    pub fn pixels(self) -> f64 {
        self.points * DPI / 72.0
    }

    /// How wide this text is, in pixels.
    ///
    /// The sum of the advances, with no trailing side bearing removed. A caller centring a title wants the width matplotlib would have centred, and that is the advance width.
    ///
    /// # Errors
    ///
    /// If the face will not parse.
    pub fn width(self, of: &str) -> Result<f64, Unreadable> {
        let font = self.font()?;
        Ok(self.advance(&font, of))
    }

    /// How tall a capital letter is, in pixels.
    ///
    /// Used to put a legend swatch beside a name and to centre a number on a gridline. The cap height rather than the ascent, because the ascent includes room for accents that nothing here uses and a swatch lined up with it sits visibly high.
    ///
    /// # Errors
    ///
    /// If the face will not parse.
    pub fn cap_height(self) -> Result<f64, Unreadable> {
        let font = self.font()?;
        let metrics = font.metrics(Size::new(as_f32(self.pixels())), LocationRef::default());
        Ok(f64::from(metrics.cap_height.unwrap_or(metrics.ascent)))
    }

    /// Draw text on the canvas.
    ///
    /// `x` and `y` are the anchor, `y` is the baseline, and `align` says which end of the run `x` is. Turning the text runs it up the canvas with the baseline on the right of the letters, which is the one rotation a chart needs.
    ///
    /// # Errors
    ///
    /// If the face will not parse.
    pub fn draw(
        self,
        canvas: &mut Canvas,
        what: &str,
        x: f64,
        y: f64,
        align: Align,
        turn: Turn,
    ) -> Result<(), Unreadable> {
        let font = self.font()?;
        let size = Size::new(as_f32(self.pixels()));
        let glyphs = font.charmap();
        let outlines = font.outline_glyphs();

        let width = self.advance(&font, what);
        let start = match align {
            Align::Left => 0.0,
            Align::Center => -width / 2.0,
            Align::Right => -width,
        };

        let mut pen = start;
        for c in what.chars() {
            let id = glyphs.map(c).unwrap_or_default();
            if let Some(glyph) = outlines.get(id) {
                let mut path = Collect::default();
                let settings = DrawSettings::unhinted(size, LocationRef::default());
                if glyph.draw(settings, &mut path).is_ok() {
                    // The origin is rounded to a whole pixel so that a glyph rasterizes to the same coverage everywhere it appears, which makes the same word look the same wherever it is on the chart.
                    let (ox, oy) = match turn {
                        Turn::Level => (x + pen, y),
                        Turn::Up => (x, y - pen),
                    };
                    path.stamp(canvas, turn, ox.round(), oy.round(), self.color);
                }
            }
            pen += f64::from(
                font.glyph_metrics(size, LocationRef::default())
                    .advance_width(id)
                    .unwrap_or_default(),
            );
        }
        Ok(())
    }

    /// Parse the face.
    fn font(self) -> Result<FontRef<'static>, Unreadable> {
        self.face
            .load()
            .map_err(|e: ReadError| Unreadable(self.face.name(), e.to_string()))
    }

    /// The width of a run, in pixels.
    fn advance(self, font: &FontRef<'_>, what: &str) -> f64 {
        let size = Size::new(as_f32(self.pixels()));
        let glyphs = font.charmap();
        let metrics = font.glyph_metrics(size, LocationRef::default());
        what.chars()
            .map(|c| {
                let id = glyphs.map(c).unwrap_or_default();
                f64::from(metrics.advance_width(id).unwrap_or_default())
            })
            .sum()
    }
}

/// A glyph outline, collected as it is drawn.
///
/// skrifa hands the path out one command at a time and zeno wants the whole thing, so this is the join between them. The coordinates arrive already scaled to the pixel size, with y running up.
#[derive(Debug, Default)]
struct Collect {
    /// The path, in the font's own orientation.
    commands: Vec<Command>,
}

impl Collect {
    /// Rasterize the collected path and blend it onto the canvas at a whole pixel origin.
    ///
    /// The path is flipped in y here rather than at collection time, because the font draws with y running up and the canvas has it running down. Turning the text is the same flip about the other diagonal.
    fn stamp(&self, canvas: &mut Canvas, turn: Turn, x: f64, y: f64, color: Pixel) {
        if self.commands.is_empty() {
            return;
        }
        let placed: Vec<Command> = self
            .commands
            .iter()
            .map(|c| map_command(*c, |p| turned(p, turn)))
            .collect();

        let (mask, place) = Mask::new(placed.as_slice()).render();
        for (i, &coverage) in mask.iter().enumerate() {
            if coverage == 0 {
                continue;
            }
            let across = i % (place.width as usize);
            let down = i / (place.width as usize);
            canvas.blend(
                as_i64(x) + i64::from(place.left) + as_i64_from_usize(across),
                as_i64(y) + i64::from(place.top) + as_i64_from_usize(down),
                f64::from(coverage) / 255.0,
                color,
            );
        }
    }
}

impl OutlinePen for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::MoveTo(Point::new(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(Command::LineTo(Point::new(x, y)));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.commands
            .push(Command::QuadTo(Point::new(cx, cy), Point::new(x, y)));
    }

    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        self.commands.push(Command::CurveTo(
            Point::new(ax, ay),
            Point::new(bx, by),
            Point::new(x, y),
        ));
    }

    fn close(&mut self) {
        self.commands.push(Command::Close);
    }
}

/// A point in the font's orientation, put into the canvas's.
///
/// Level text flips y, because the font draws with y running up. Turned text swaps the two and flips x instead, which runs the letters up the canvas reading from the bottom.
fn turned(p: Point, turn: Turn) -> Point {
    match turn {
        Turn::Level => Point::new(p.x, -p.y),
        Turn::Up => Point::new(-p.y, -p.x),
    }
}

/// Apply a point transform to a path command.
fn map_command(command: Command, at: impl Fn(Point) -> Point) -> Command {
    match command {
        Command::MoveTo(to) => Command::MoveTo(at(to)),
        Command::LineTo(to) => Command::LineTo(at(to)),
        Command::QuadTo(control, to) => Command::QuadTo(at(control), at(to)),
        Command::CurveTo(first, second, to) => Command::CurveTo(at(first), at(second), at(to)),
        Command::Close => Command::Close,
    }
}

/// A pixel size as the font APIs want it.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a point size on a chart is between 8 and 20, which an f32 holds exactly"
)]
fn as_f32(v: f64) -> f32 {
    v as f32
}

/// A whole pixel coordinate.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the caller has already rounded, and a chart is under two thousand pixels wide"
)]
fn as_i64(v: f64) -> i64 {
    v as i64
}

/// An index into a coverage mask, which is at most a glyph wide.
#[allow(
    clippy::cast_possible_wrap,
    reason = "a glyph mask is tens of pixels across, not four billion"
)]
fn as_i64_from_usize(v: usize) -> i64 {
    v as i64
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a font that will not parse is a failed test, and the digest test upstream of this one would have caught it"
)]
mod tests {
    use super::{Align, Text, Turn};
    use crate::canvas::Canvas;
    use crate::font::Face;

    const BLACK: [u8; 3] = [0, 0, 0];

    fn body(points: f64) -> Text {
        Text::new(Face::Body, points, BLACK)
    }

    // A point is a seventy second of an inch and the charts are saved at 150 of them, so the arithmetic is fixed even though the number is not round.
    #[test]
    fn a_point_size_becomes_a_pixel_size_at_the_charts_resolution() {
        assert!((body(72.0).pixels() - 150.0).abs() < 1e-9);
        assert!((body(12.0).pixels() - 25.0).abs() < 1e-9);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the width of nothing is a sum over no glyphs, which is exactly zero"
    )]
    fn wider_text_measures_wider() {
        let t = body(12.0);
        assert!(t.width("11").unwrap() < t.width("1111").unwrap());
        assert_eq!(t.width("").unwrap(), 0.0);
    }

    #[test]
    fn a_bigger_size_measures_bigger() {
        assert!(body(24.0).width("threads").unwrap() > body(12.0).width("threads").unwrap());
    }

    #[test]
    fn a_capital_is_shorter_than_the_size_and_more_than_half_of_it() {
        let cap = body(20.0).cap_height().unwrap();
        let em = body(20.0).pixels();
        assert!(cap < em, "{cap} is not under {em}");
        assert!(cap > em / 2.0, "{cap} is not over half of {em}");
    }

    fn ink(canvas: &Canvas) -> usize {
        canvas.bytes().iter().filter(|&&b| b != 255).count()
    }

    #[test]
    fn drawing_text_puts_something_down() {
        let mut canvas = Canvas::new(200, 60);
        body(20.0)
            .draw(&mut canvas, "1,000", 10.0, 40.0, Align::Left, Turn::Level)
            .unwrap();
        assert!(ink(&canvas) > 0);
    }

    // Right aligned text ends where left aligned text of the same run would have started, one width earlier.
    #[test]
    fn alignment_moves_the_run_and_not_its_shape() {
        let t = body(16.0);
        let width = t.width("512").unwrap();
        let mut left = Canvas::new(300, 50);
        let mut right = Canvas::new(300, 50);
        t.draw(&mut left, "512", 40.0, 35.0, Align::Left, Turn::Level)
            .unwrap();
        t.draw(
            &mut right,
            "512",
            40.0 + width,
            35.0,
            Align::Right,
            Turn::Level,
        )
        .unwrap();
        assert!(ink(&left) > 0);
        assert_eq!(ink(&left), ink(&right));
    }

    // The y axis label is the one piece of turned text on a chart, and turning it has to put ink somewhere different.
    #[test]
    fn turned_text_runs_up_the_canvas() {
        let t = body(16.0);
        let mut level = Canvas::new(120, 120);
        let mut up = Canvas::new(120, 120);
        t.draw(&mut level, "Threads", 10.0, 60.0, Align::Left, Turn::Level)
            .unwrap();
        t.draw(&mut up, "Threads", 60.0, 110.0, Align::Left, Turn::Up)
            .unwrap();
        assert!(ink(&level) > 0);
        assert!(ink(&up) > 0);
        assert_ne!(level.bytes(), up.bytes());
    }

    // A space has an advance and no outline, so it moves the pen and marks nothing.
    #[test]
    fn a_space_takes_room_and_draws_nothing() {
        let t = body(16.0);
        let mut canvas = Canvas::new(200, 50);
        t.draw(&mut canvas, "   ", 10.0, 35.0, Align::Left, Turn::Level)
            .unwrap();
        assert_eq!(ink(&canvas), 0);
        assert!(t.width("   ").unwrap() > 0.0);
    }

    // Every face a chart uses can draw every character a chart puts on it, which the alphabet test in `font` asserts and this one draws.
    #[test]
    fn every_face_draws() {
        for face in Face::ALL {
            let mut canvas = Canvas::new(600, 60);
            Text::new(face, 14.0, BLACK)
                .draw(
                    &mut canvas,
                    "GET+SET 1,024 (ops/sec)",
                    5.0,
                    40.0,
                    Align::Left,
                    Turn::Level,
                )
                .unwrap();
            assert!(ink(&canvas) > 0, "{} drew nothing", face.name());
        }
    }

    // Text laid out past the edge is clipped rather than wrapped, so a long legend entry cannot corrupt the far side of the chart.
    #[test]
    fn text_off_the_canvas_is_clipped() {
        let mut canvas = Canvas::new(40, 40);
        body(20.0)
            .draw(
                &mut canvas,
                "1,000,000",
                -500.0,
                20.0,
                Align::Left,
                Turn::Level,
            )
            .unwrap();
        assert_eq!(ink(&canvas), 0);
    }
}
