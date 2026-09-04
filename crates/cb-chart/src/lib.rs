//! The chart engine.
//!
//! Extraction and rendering are separate.
//! Extraction turns a results file into a chart specification and is checked against 154 golden series taken from the original.
//! Rendering turns that specification into a PNG and is checked by hash on three platforms, which is only possible because the fonts are embedded rather than resolved from the system.

pub mod axis;
pub mod canvas;
pub mod font;
pub mod golden;
pub mod palette;
pub mod render;
pub mod series;
pub mod spec;
pub mod text;

pub use axis::{Axis, Bars, Sizes, Tick, Unscalable};
pub use canvas::{Canvas, Rect};
pub use font::Face;
pub use golden::{Golden, Mismatch, Tally};
pub use palette::{BadColor, COLORS, Rgb, TooManyCaches, color};
pub use render::{Stamp, Undrawable, draw};
pub use series::{BadCorpus, Chart, Corpus, Series};
pub use spec::{Case, Metric, Percentile, Scale, Spec, Which};
pub use text::{Align, Text, Turn};
