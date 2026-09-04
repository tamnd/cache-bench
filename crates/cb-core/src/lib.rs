//! Types, the on disk JSON model, config and hardware profiles.
//!
//! Nothing in here does I/O beyond serialising and deserialising.
//! It builds and tests on every platform, which matters because the parts of this project that are hard to get right are the JSON bytes, the statistics and the chart arithmetic, and none of those should need a benchmark host to work on.

pub mod cache;
pub mod clock;
pub mod compat;
pub mod config;
pub mod cpuset;
pub mod golden;
pub mod hosts;
pub mod journal;
pub mod machine;
pub mod name;
pub mod num;
pub mod output;
pub mod profile;
pub mod run;
pub mod size;
pub mod spread;

pub use cache::{CacheKind, Endpoint, Launch, Protocol, UnknownCache};
pub use clock::{now, stamp};
pub use compat::{BadCompat, Compat};
pub use config::{Arch, BadConfig, Config};
pub use cpuset::{BadCpuSet, CpuSet};
pub use hosts::{BadHosts, Host, Hosts};
pub use journal::{Abandoned, BadJournal, Failure, Failures, Outcome, Step};
pub use machine::{BadMachine, Machine, Pmu, Tool};
pub use name::{BadName, Chosen, RunName, Slot};
pub use num::{Counter, CpuCounter, EventCounter, Fixed0, Fixed3};
pub use output::{Entry, Output};
pub use profile::{BadProfile, PerfMode, Profile, Profiles, SizeRange};
pub use run::{Info, Latency, Op, Perf, Run};
pub use size::{BadSize, Bytes};
pub use spread::{Dispersion, PerfSpread, Spread};
