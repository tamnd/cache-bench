//! Pixels.
//!
//! Everything a chart is made of is one of two shapes. Bars, their outlines and the gridlines are axis aligned rectangles, and the text is glyph outlines. Rectangles are filled here with coverage worked out in closed form, because the area of a pixel square inside an axis aligned rectangle is a product of two clamped lengths and needs no scanning. Glyphs go through `text`.
//!
//! Nothing here is approximate for speed and nothing depends on the machine. Every coordinate arrives as `f64`, every blend is the same sequence of IEEE operations in the same order on every platform, and the byte the encoder gets is therefore the same byte everywhere. That is the whole reason this is written out rather than handed to a drawing library, and it is what the determinism job checks.

use std::io;

use crate::palette::Rgb;

/// A picture being drawn, held as three bytes per pixel with no alpha.
///
/// No alpha because a chart has a white background and nothing behind it. Compositing onto an opaque canvas means a blend is one multiply and add per channel with no divide, which is one fewer place for two platforms to disagree.
#[derive(Debug, Clone)]
pub struct Canvas {
    /// Pixels across.
    width: u32,
    /// Pixels down.
    height: u32,
    /// Row major, three bytes per pixel.
    pixels: Vec<u8>,
}

/// A colour as the canvas stores it.
pub type Pixel = [u8; 3];

/// White, which is what a chart starts as and what the border around it is.
pub const WHITE: Pixel = [255, 255, 255];

impl Canvas {
    /// A white canvas.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let count = (width as usize) * (height as usize) * 3;
        Self {
            width,
            height,
            pixels: vec![255; count],
        }
    }

    /// Pixels across.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pixels down.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The raw bytes, row major, three per pixel.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Put one pixel down over what is already there.
    ///
    /// `coverage` is how much of the pixel the shape covers, from nothing to all of it. Anything outside the canvas is dropped, which is what lets a caller lay text out without first working out whether it fits.
    pub fn blend(&mut self, x: i64, y: i64, coverage: f64, color: Pixel) {
        if coverage <= 0.0 {
            return;
        }
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        if x >= self.width as usize || y >= self.height as usize {
            return;
        }
        let a = coverage.min(1.0);
        let at = (y * self.width as usize + x) * 3;
        for (channel, &new) in color.iter().enumerate() {
            let old = f64::from(self.pixels[at + channel]);
            self.pixels[at + channel] = round_channel(old + (f64::from(new) - old) * a);
        }
    }

    /// Fill an axis aligned rectangle, with the edges antialiased.
    ///
    /// The rectangle is given as two corners in continuous coordinates, where pixel `n` covers the interval from `n` to `n + 1`, so a rectangle from 3.0 to 5.0 fills exactly two pixels and one from 3.5 to 5.5 fills two whole ones and two halves. Coverage of a pixel is the length of the overlap in x times the length in y, which is exact for this shape.
    pub fn fill_rect(&mut self, rect: Rect, color: Pixel) {
        let (x0, x1) = ordered(rect.x0, rect.x1);
        let (y0, y1) = ordered(rect.y0, rect.y1);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        for py in span(y0, y1, self.height) {
            let cy = overlap(f64::from(py), y0, y1);
            for px in span(x0, x1, self.width) {
                let cx = overlap(f64::from(px), x0, x1);
                self.blend(i64::from(px), i64::from(py), cx * cy, color);
            }
        }
    }

    /// Write a PNG.
    ///
    /// Eight bit RGB with no filtering chosen adaptively, because an adaptive filter is one more thing that could differ between two versions of an encoder for the same picture. The bytes are what the manifest hashes.
    ///
    /// # Errors
    ///
    /// If the writer fails, or if the canvas is not a size the encoder will accept.
    pub fn write_png<W: io::Write>(&self, to: W) -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(to, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        encoder.set_filter(png::Filter::Sub);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.pixels)?;
        writer.finish()
    }
}

/// A rectangle in continuous canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// One x edge.
    pub x0: f64,
    /// One y edge.
    pub y0: f64,
    /// The other x edge.
    pub x1: f64,
    /// The other y edge.
    pub y1: f64,
}

impl Rect {
    /// A rectangle from a corner and a size.
    #[must_use]
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x0: x,
            y0: y,
            x1: x + width,
            y1: y + height,
        }
    }

    /// The outline of this rectangle, as four rectangles lying inside it.
    ///
    /// Inside rather than centred on the edge, which is what keeps a bar the width it was asked to be. matplotlib strokes the edge centred and the bar comes out three quarters of a pixel wider on each side, which nobody would notice and which would make every bar in a group overlap its neighbour by a hair.
    #[must_use]
    pub fn outline(self, thickness: f64) -> [Self; 4] {
        let (x0, x1) = ordered(self.x0, self.x1);
        let (y0, y1) = ordered(self.y0, self.y1);
        let t = thickness.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        [
            Self {
                x0,
                y0,
                x1,
                y1: y0 + t,
            },
            Self {
                x0,
                y0: y1 - t,
                x1,
                y1,
            },
            Self {
                x0,
                y0: y0 + t,
                x1: x0 + t,
                y1: y1 - t,
            },
            Self {
                x0: x1 - t,
                y0: y0 + t,
                x1,
                y1: y1 - t,
            },
        ]
    }
}

impl From<Rgb> for Pixel {
    fn from(rgb: Rgb) -> Self {
        rgb.0.map(|c| round_channel(c * 255.0))
    }
}

/// A colour channel from a number that is already in range, rounded half away from zero.
///
/// `round` rather than `round_ties_even` because a channel is a quantity and not a measurement, and half away from zero is what every other tool that writes an eight bit colour does.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into zero to 255 on the line above the cast"
)]
fn round_channel(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// The two values in order.
fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// How much of the pixel starting at `at` lies between `low` and `high`.
fn overlap(at: f64, low: f64, high: f64) -> f64 {
    (high.min(at + 1.0) - low.max(at)).clamp(0.0, 1.0)
}

/// The pixels a span from `low` to `high` touches, clipped to a canvas `limit` wide.
fn span(low: f64, high: f64, limit: u32) -> std::ops::Range<u32> {
    let first = low.floor().max(0.0);
    let last = high.ceil().min(f64::from(limit));
    if last <= first {
        return 0..0;
    }
    // Cast is sound: both are clamped to zero and to the canvas size, which is a u32.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both are clamped into zero to limit, and limit is a u32"
    )]
    let range = (first as u32)..(last as u32);
    range
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "coverage arithmetic is exact for these shapes, so a tolerance would hide a mistake"
)]
#[allow(clippy::expect_used, reason = "a failed fixture is a failed test")]
mod tests {
    use super::{Canvas, Rect, WHITE};

    const BLACK: [u8; 3] = [0, 0, 0];

    fn at(canvas: &Canvas, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * canvas.width() + x) * 3) as usize;
        [
            canvas.bytes()[i],
            canvas.bytes()[i + 1],
            canvas.bytes()[i + 2],
        ]
    }

    #[test]
    fn a_new_canvas_is_white() {
        let canvas = Canvas::new(3, 2);
        assert_eq!(canvas.bytes().len(), 18);
        assert!(canvas.bytes().iter().all(|&b| b == 255));
        assert_eq!(at(&canvas, 2, 1), WHITE);
    }

    // A rectangle on the pixel grid covers whole pixels and no others, which is the case every gridline is.
    #[test]
    fn a_rectangle_on_the_grid_has_no_soft_edges() {
        let mut canvas = Canvas::new(4, 4);
        canvas.fill_rect(Rect::new(1.0, 1.0, 2.0, 1.0), BLACK);
        assert_eq!(at(&canvas, 1, 1), BLACK);
        assert_eq!(at(&canvas, 2, 1), BLACK);
        assert_eq!(at(&canvas, 0, 1), WHITE);
        assert_eq!(at(&canvas, 3, 1), WHITE);
        assert_eq!(at(&canvas, 1, 0), WHITE);
        assert_eq!(at(&canvas, 1, 2), WHITE);
    }

    // Half a pixel of black on white is half way between them, and the halfway byte rounds up.
    #[test]
    fn half_coverage_is_half_the_colour() {
        let mut canvas = Canvas::new(2, 1);
        canvas.fill_rect(Rect::new(0.5, 0.0, 1.0, 1.0), BLACK);
        assert_eq!(at(&canvas, 0, 0), [128, 128, 128]);
        assert_eq!(at(&canvas, 1, 0), [128, 128, 128]);
    }

    // A corner pixel gets the product of the two overlaps rather than either one of them.
    #[test]
    fn a_corner_gets_both_overlaps_multiplied() {
        let mut canvas = Canvas::new(2, 2);
        canvas.fill_rect(Rect::new(0.5, 0.5, 1.0, 1.0), BLACK);
        assert_eq!(at(&canvas, 0, 0), [191, 191, 191]);
        assert_eq!(at(&canvas, 1, 1), [191, 191, 191]);
    }

    // Anything off the edge is dropped rather than wrapped onto the far side, which is what lets a caller lay text out without measuring first.
    #[test]
    fn drawing_off_the_canvas_touches_nothing() {
        let mut canvas = Canvas::new(2, 2);
        canvas.fill_rect(Rect::new(-10.0, -10.0, 5.0, 5.0), BLACK);
        canvas.fill_rect(Rect::new(100.0, 100.0, 5.0, 5.0), BLACK);
        canvas.blend(-1, 0, 1.0, BLACK);
        canvas.blend(0, 9, 1.0, BLACK);
        assert!(canvas.bytes().iter().all(|&b| b == 255));
    }

    #[test]
    fn an_empty_rectangle_draws_nothing() {
        let mut canvas = Canvas::new(2, 2);
        canvas.fill_rect(Rect::new(1.0, 1.0, 0.0, 5.0), BLACK);
        assert!(canvas.bytes().iter().all(|&b| b == 255));
    }

    // An outline lies inside the rectangle it outlines, so a bar stays the width it was asked to be.
    #[test]
    fn an_outline_stays_inside_the_bar() {
        let bar = Rect::new(10.0, 10.0, 6.0, 20.0);
        for side in bar.outline(1.5) {
            assert!(side.x0 >= bar.x0 && side.x1 <= bar.x1);
            assert!(side.y0 >= bar.y0 && side.y1 <= bar.y1);
        }
    }

    // A thick outline on a thin bar becomes a solid bar rather than four sides crossing over each other.
    #[test]
    fn an_outline_thicker_than_the_bar_fills_it() {
        let bar = Rect::new(0.0, 0.0, 2.0, 40.0);
        let sides = bar.outline(1.5);
        assert_eq!(sides[0].y1 - sides[0].y0, 1.0);
        assert_eq!(sides[2].x1 - sides[2].x0, 1.0);
    }

    #[test]
    fn a_png_comes_back_as_the_pixels_that_went_in() {
        let mut canvas = Canvas::new(8, 4);
        canvas.fill_rect(Rect::new(2.0, 1.0, 3.0, 2.0), BLACK);
        let mut out = Vec::new();
        canvas.write_png(&mut out).expect("a canvas encodes");
        assert_eq!(&out[1..4], b"PNG");

        let decoder = png::Decoder::new(std::io::Cursor::new(&out));
        let mut reader = decoder.read_info().expect("it reads back");
        let mut back = vec![0; reader.output_buffer_size().expect("a known size")];
        let info = reader.next_frame(&mut back).expect("one frame");
        assert_eq!(info.width, 8);
        assert_eq!(info.height, 4);
        assert_eq!(&back[..info.buffer_size()], canvas.bytes());
    }
}
