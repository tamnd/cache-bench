//! Run selection and aggregation.
//!
//! Thirty one runs of one cell go in and four files come out, and every number on every chart comes from one of them.
//! This is also where the original has four defects, all of which change published numbers, and all of which are in `divergences.md` with what they do.
//!
//! Two modes ship, and shipping only one would be a mistake in either direction.
//! [`correct`] is the default and is what the numbers here are computed with.
//! Upstream mode reproduces the defects exactly, which is what keeps the original's published output regenerable and is the evidence that this is a port rather than a rewrite that resembles one.
//!
//! Nothing here does I/O. A cell is a slice of runs and a chosen file is a run, so all of it is testable against the original's own data with no cache server anywhere near it.

pub mod cell;
pub mod correct;
pub mod gosort;
pub mod kind;

pub use cell::{BadCell, check, trim_for};
pub use kind::{BadKind, Kind};

#[cfg(test)]
pub(crate) mod fixture;
