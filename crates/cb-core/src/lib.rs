//! Types, the on disk JSON model, config and hardware profiles.
//!
//! Nothing in here does I/O beyond serialising and deserialising.
//! It builds and tests on every platform, which matters because the parts of this project that are hard to get right are the JSON bytes, the statistics and the chart arithmetic, and none of those should need a benchmark host to work on.

pub mod cache;
pub mod name;
pub mod num;
pub mod run;

pub use cache::{CacheKind, UnknownCache};
pub use name::{BadName, Chosen, RunName, Slot};
pub use num::{Counter, Fixed0, Fixed3};
pub use run::{Info, Latency, Op, Perf, Run};
