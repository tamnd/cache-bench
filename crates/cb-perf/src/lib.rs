//! The `perf` driver, the PMU probe and counter parsing.
//!
//! Counters are read from `perf stat -x,` rather than scraped out of the human readable table, so parsing does not depend on column alignment or on the locale.
//!
//! The probe is two checks rather than one.
//! A `cpu` entry has to exist under `/sys/bus/event_source/devices`, and a live `perf stat -e cycles` has to return a number, because a virtual machine can have the directory and still answer `<not supported>` for every hardware event.
//!
//! A counter that comes back as `<not supported>` is not a zero.
//! The original reads it as one and draws it, and `cb_core::Counter` keeps the distinction so that this crate can hand the chart layer something it can leave off the plot.
