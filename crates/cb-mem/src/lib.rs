//! The memory half of the benchmark.
//!
//! The throughput half of this project reproduces something that already existed. This half does not: the original measures no memory at all, and neither does anything else in this workspace, so half of what a reader wants to compare two cache servers on has never had a metric here.
//!
//! What it measures is one number and reports it two ways. Start the server, note what it holds before anything is in it, write a known number of distinct keys into it, let it settle, then read the largest resident set it ever had.
//!
//! # Two numbers, not one
//!
//! Total bytes per entry is that peak divided by the keys in it. It is what a machine has to have, and it is the number to buy memory against.
//!
//! Overhead bytes per entry is what is left after the keys and the values themselves. It is what a design actually controls, and it is the number an index is an argument about.
//!
//! They are different claims and this crate reports both, because at a hundred-odd bytes of payload per key an index that got twice as small halves the second and moves the first by a few percent. Quoting whichever one flatters is the failure mode this pair exists to prevent.
//!
//! # What it does not say
//!
//! It does not say an engine is wasteful. Garnet preallocates its index and Dragonfly preallocates per proactor, so a peak resident set for those is partly a configuration and not a consequence of the keys. The row carries a note and a baseline for exactly that, and the generated results README carries the caveat next to the number rather than in a document nobody following a link will open.
//!
//! It is also a high water mark and not a curve. An engine that peaked while rehashing and then released is reported at the peak, because a machine that cannot hold the peak cannot run the engine.

mod plan;
mod report;
mod sample;
mod status;

pub use plan::{BadPlan, Plan};
pub use report::{Report, Row};
pub use sample::{NoSample, Sample, group};
pub use status::{BadStatus, Resident, parse};
