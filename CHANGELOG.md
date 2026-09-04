# Changelog

What each release costs you, in the order the releases happened. New entries go on top.

## Unreleased

### Added

- The on disk run file model in `cb-core`, which reads and writes the original's result files byte for byte in both directions. Three real files from the original's committed results are in `testdata/golden/` and the round trip is tested against them.
- Result filenames as a parsed type, tested in both directions over every name a sweep on the reference profile can produce.
- The two fixed decimal number types the format uses, and a perf counter type that keeps whichever JSON shape it was read in, because a run file holds counters as strings and a chosen file holds the same counters as numbers.

Nothing measures anything yet. The workspace, the toolchain, the hardware profiles and the CI are in place, and the milestones say what lands next.
