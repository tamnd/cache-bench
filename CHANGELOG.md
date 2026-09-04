# Changelog

What each release costs you, in the order the releases happened. New entries go on top.

## Unreleased

### Added

- `config.jsonc`, the same file the original reads, with the same keys and the same `${arch}` placeholder, so a config that works there works here and the other way round. Comments and trailing commas are allowed and nothing else is, because the original hands everything past those to a strict JSON parser and a file that only works on one side defeats the point of sharing it.
- `profiles.toml`, which is the machine shape the original hardcodes. Core pinning, the thread sweep, the memory limit and the client count are constants in the original's driver script, and none of the machines this port runs on is that box.
- Profile validation, which refuses the three mistakes that produce numbers rather than errors: a thread sweep wider than the cores it is pinned to, a load generator sharing cores with the server under test, and a key space too large for the memory limit. All three make a chart that looks fine and measures something else.
- `hosts.toml`, absent by default, absent meaning run here. Only `hosts.example.toml` is committed, with ssh config names rather than addresses, and a test that fails if anything in it starts to look like a real machine.
- `cache-bench doctor`, which reads all three files and says what it found, or says what is wrong with them and exits non-zero. This is the file half. The machine half, which probes cores, memory and the PMU, lands with the runner.

### Changed

- CI checks the data files with `doctor` instead of with a Python approximation of the same checks. The Python could only look at shape. `doctor` reads the files with the parser the harness uses and applies the checks that matter, so a profile that would evict fails in CI rather than two days into a sweep.

## 0.0.2 - 2026-09-04

The combined file and the seven command lines. Still nothing measures anything.

What this release is for is that the format half of the port is now proved rather than asserted. The original's entire published `output.json` reads in and writes back out byte for byte, all 2304 entries of it, in both directions. That was the cheapest available proof that the port is faithful and it is now spent, which means the statistics work in M2 starts from a known good floor rather than from a hope.

### Added

- `output.json`, the combined file the chart layer reads, matching the original's layout field for field and number for number. Verified against the original's whole published file, all 2304 entries, byte for byte in both directions.
- The command line each of the seven servers is started with, as a table with no I/O in it, so it is testable on a machine with no cache server installed. Six of the seven are the original's argv word for word, with the thread count and the memory limit coming from the profile instead of being constants.
- A memory size type, because the profile says `32gb` and Garnet wants `32g` and memcached wants `32768` in megabytes with no unit at all. Parsed once, spelled on demand.
- `--compat`, as a type. Corrected is the default and upstream reproduces the original's defects, which is what makes the parity proof possible.

### Documented

- D12 in `divergences.md`. Every server in the original gets 32 GB except Dragonfly, which gets 31, because its limit is computed with a unit conversion in integer arithmetic that throws the remainder away. Here every server gets the profile's limit, and `--compat=upstream` reproduces the formula rather than the number it happens to produce.

### Fixed

- Numeric perf counters are written with the decimal places the original writes them with. How many places a counter gets is a property of which counter it is rather than of its value, so `cpu_utilized` goes out with three and every event count with none. Before this, a CPU figure that happened to land on 0.99 was written as `0.99` where the original writes `0.990`. The full file test is what caught it, since three entries were not enough to reach a value that lands on 0.99.

### Notes

The full file test is ignored by default, because the file it reads is 1.7 MB of measurement data and raw data does not go in this repository. Run it with `CB_PARITY_OUTPUT=/path/to/cache-benchmarks/results/output.json cargo test -p cb-core -- --ignored`. `cache-bench verify` in M8 wires it up so it is not something you have to remember.

The other half of the parity claim, a file we write being accepted by the original's `graph`, needs a Go toolchain and is not done yet. It lands with `verify` as well.

## 0.0.1 - 2026-09-04

The skeleton, plus the on disk format. Nothing measures anything.

There is a binary and it builds for four targets, but every subcommand prints where to find the milestones and exits. What this release is actually for is that the tree can be worked in and the format is settled, and both of those are things later work would have had to redo.

### Added

- A workspace of eight crates plus an xtask, edition 2024, pinned to Rust 1.98.0 with the floor at 1.94. The crates that need real hardware are separated from the crates that do not, so the statistics and the chart work can be developed and tested on any machine with no cache server anywhere near it.
- The on disk run file model, which reads and writes the original's result files byte for byte in both directions. Three real files from the original's committed results are in `testdata/golden/` and the round trip is tested against them.
- Result filenames as a parsed type, tested in both directions over every name a sweep on the reference profile can produce. There is no index and no database in this harness, so the filename is the primary key and both directions have to agree exactly.
- The two fixed decimal number types the format uses, and a perf counter type that keeps whichever JSON shape it was read in, because a run file holds counters as strings and a chosen file holds the same counters as numbers.
- Hardware profiles in `profiles.toml`, so the core pinning, the thread sweep, the memory limit and the client count are data rather than constants in a driver script. The original hardcodes a 32 core box throughout and this is what makes the harness runnable on anything else.
- `config.jsonc` with the same keys and the same shape as the original, so a config that works there works here.
- CI covering formatting, clippy, three platforms, the MSRV floor, docs, licences, advisories and typos, with a hygiene job that fails if raw measurement data or the private host list is ever tracked. Nothing in CI needs a cache server, a load generator or a PMU, and nothing in CI ever will, because a benchmark measured on a shared runner is not a benchmark.
- The methodology document, written before there was anything to be wrong about, and `divergences.md`, which is the list of every place this port does something the original does not.

### Notes

Two of the four upstream statistics defects were described incorrectly in the first draft of `divergences.md` and are corrected here, from the original's source rather than from its output. The median defect is one position inside the trimmed window rather than an index into the untrimmed list, and the sort defect is a comparator meant for the perf list being applied to the SET list. Neither correction changes what this port will do, but both change what the document claims the original does.

Nothing is published to crates.io. This is a harness, not a library, and the only useful artefact is a binary that runs on the box the sweep will happen on.
