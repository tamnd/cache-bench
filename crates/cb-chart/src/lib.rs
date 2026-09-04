//! The chart engine.
//!
//! Extraction and rendering are separate.
//! Extraction turns a results file into a chart specification and is checked against 154 golden series taken from the original.
//! Rendering turns that specification into a PNG and is checked by hash on three platforms, which is only possible because the fonts are embedded rather than resolved from the system.

pub mod palette;
pub mod series;
pub mod spec;

pub use palette::{COLORS, TooManyCaches, color};
pub use series::{BadCorpus, Chart, Corpus, Series};
pub use spec::{Case, Metric, Percentile, Scale, Spec, Which};
