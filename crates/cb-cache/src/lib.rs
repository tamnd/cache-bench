//! Cache server adapters and process lifecycle.
//!
//! Each adapter holds one server's flags and nothing else. Memory limits,
//! thread counts and CPU pins come from the profile, so a change to how the
//! benchmark is shaped does not mean editing seven files.
//!
//! This crate needs Linux. On other platforms it compiles to a stub that
//! reports the platform is unsupported, which keeps the chart and statistics
//! crates testable on a laptop.
