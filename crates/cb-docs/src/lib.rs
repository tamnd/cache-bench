//! Generated result documents.
//!
//! The original maintains its two chart indexes by hand, including the anchor suffixes GitHub appends to repeated headings.
//! With two results directories and three hundred charts that is not maintainable, so all of it is generated and the anchor numbering is a unit test.

mod anchor;
pub mod divergence;
mod index;
mod readme;

pub use anchor::{Anchors, slug};
pub use divergence::{DIVERGENCES, Divergence};
pub use index::Index;
pub use readme::{MAY, MAY_NOT, Readme};
