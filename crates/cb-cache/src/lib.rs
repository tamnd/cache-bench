//! Cache server adapters and process lifecycle.
//!
//! Each adapter holds one server's flags and nothing else.
//! Memory limits, thread counts and CPU pins come from the profile, so a change to how the benchmark is shaped does not mean editing eight files.
//!
//! This crate needs Linux.
//! On other platforms it compiles to a stub that reports the platform is unsupported, which keeps the chart and statistics crates testable on a laptop.

pub mod ready;
pub mod server;
pub mod supervise;
pub mod version;

pub use ready::{NotReady, anybody_there, wait};
pub use server::{NotRunning, Server};
pub use supervise::{BadProcess, Running, Stopped, Supervisor, as_root};
pub use version::{NoVersion, version};
