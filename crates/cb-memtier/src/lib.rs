//! The `memtier_benchmark` driver and its output parser.
//!
//! The parser is deliberately strict.
//! The original reads the JSON with a path query that yields zero for a missing field, which turns a version mismatch into a chart full of empty bars rather than into an error.
