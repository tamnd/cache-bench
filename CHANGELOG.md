# Changelog

What each release costs you, in the order the releases happened. New entries go on top.

## Unreleased

### Added

- The series layer, which is the half of the chart engine with no pixels in it. A results file goes in and what comes out is a title, both axis labels, a thread count for each group of bars, and one number per bar, for each of the 154 charts. Everything a reader could disagree with is decided here, so it is a pure function and it is tested before anything is drawn.
- `testdata/golden/series.json`, which is all 154 charts as the original worked them out. Its `graph` tool pastes the numbers into a Python script and deletes the script after drawing, so `tools/series-vectors` stands in for Python, keeps the script and throws the picture away. The fixture is the original's answer rather than a description of it.
- Two levels of check on that fixture. The filenames, titles, axes, legend order and colours are checked in `cargo test` against a results file with the original's shape and none of its numbers, so it runs in CI on a checkout with nothing measured in it. The bar heights go through `cache-bench verify --against`, where they come out of our own reduction of the original's run files, which makes a matching chart one where every bar survived the run files, the selection, the combining and the extraction.

### Fixed

- `Kind` now formats through `pad`, so a width in a format string does what it says. `verify` prints the four aggregates in a column and asks for eight characters, and the old implementation threw the width away without complaining, which is why that column was never a column.

### Notes

- All 154 charts and all 11088 bars come back as the original's. That is the first half of the M3 gate met, and it is met before a single pixel has been drawn, which was the point of splitting the layer in two.
- A bar can now be absent rather than zero. The original has no way to say that a cell was never measured or that the machine could not count cycles, so it says zero, and a zero bar claims an engine scored nothing rather than that it was not tested. On a logarithmic chart one of those takes the whole y axis with it. `--compat=upstream` still writes the zero.

## 0.2.0 - 2026-09-04

M2 is done, which means the statistics are finished and both modes are proved against the original's own data. Still nothing measures anything.

The milestone exits on a gate and the gate holds in one command. From the original's 20160 committed run files, upstream mode reproduces all 2304 of its chosen files byte for byte and the whole 1.7 MB of its published `output.json` byte for byte. Run `cache-bench verify --against` a checkout of the original and it will say so, in under a second, along with how far the corrected numbers sit from the ones it just reproduced.

That last part is the point of shipping both modes. The four defects now have sizes instead of descriptions. The typical median moves by a tenth of a percent on GET and a quarter of a percent on SET, and the worst median SET moves by 61 percent, which is Garnet at 8 threads and pipeline 50 published at 19.86 million operations per second where the median of its 31 runs is 12.30 million. The published median GET is the higher of the two in 576 of 576 cells, on every engine at every thread count and every pipeline depth, which is a chart that reads slightly fast everywhere rather than a chart with noise in it.

Worth saying plainly, because it is the argument for having built the gate at all: two of the four defects were described incorrectly when they were read off the Go source at the start of the milestone, and byte parity is what corrected them. Two more behaviours were not visible in the source at all. The info block of a chosen file comes from the last run read rather than from the run selected, and `cleanperf` rewrites exactly six counters and leaves the others alone. Neither is guessable and both are needed.

M3 is the charts, which is where the numbers finally become something to look at.

### Added

- `cache-bench choose`, which reduces every cell in a results directory to its median, best, worst and average. `--compat=upstream` reproduces the original's four defects, `--out` writes somewhere else so that two modes can be compared without either overwriting the other, and `--cell` does one cell for when you are looking at a single number rather than a sweep.
- `cache-bench combine`, which gathers the chosen files into the `output.json` the charts read. No computation in it. Every number was decided by `choose` and this collects them in the order a directory listing gives them, which is the original's order because the original builds the file straight out of one.
- `cache-bench verify`, which is the claim this port makes about itself run as a command. With no arguments it checks the golden files committed here and runs anywhere in under a second, which is why it is in CI. Pointed at a checkout of the original with `--against` it reads all 20160 committed run files, reproduces all 2304 chosen files and the whole published `output.json` byte for byte, and then prints how far the corrected statistics sit from the original's. The numbers in `divergences.md` under D1 to D4 are that output rather than an assertion about it.
- The results directory layer the two of them share. A gap in a cell's run numbering stops that cell at the gap and says how many files sit above it, rather than reducing 30 runs and calling them 31.

### Changed

- The eight crates in the workspace are marked as not published, and the version requirements on the paths between them are gone. Those requirements have to move in lockstep with the workspace version or the build stops resolving, which is what happened when this release was first cut, and they buy nothing when the crate is never resolved from a registry. Nothing here goes to crates.io and the manifests now say so.

### Notes

- The M2 gate is met end to end. From the original's 20160 committed run files, `choose --compat=upstream` writes all 2304 of its chosen files byte for byte and `combine` writes its published `output.json` byte for byte, all 1.7 MB of it.
- That makes the size of the four defects a measurement. The same directory reduced in corrected mode moves the typical median by a tenth of a percent on GET and a quarter of a percent on SET, and the worst median SET by 61 percent, which is Garnet at 8 threads and pipeline 50 published at 19.86 million operations per second where the median of its 31 runs is 12.30 million. The published median GET is higher than the true median in 576 of 576 cells, which is what a one sided index error looks like once you can see all of them at once.

## 0.1.1 - 2026-09-04

Both halves of the statistics, and the one that matters is the half that reproduces the original's mistakes.

All 2304 of the original's published chosen files come back byte for byte from its own run files, across all 576 cells, with nothing skipped. That is not a formality. Until it held, every claim about what the four defects cost was a reading of the Go source, and now each one is a subtraction anybody can repeat. The corrected numbers are worth reading because the numbers they disagree with can be regenerated on demand.

Nothing here measures anything yet, and the milestone is not finished. `combine` and `verify` are what is left.

### Added

- Upstream mode, which reproduces all four statistics defects exactly. Given the original's own run files it regenerates the original's own chosen files, and all 2304 of them come back byte for byte across all 576 cells. That is the first half of the M2 gate and it is what makes the corrected numbers worth reading, because the disagreement between the two modes is now measured rather than asserted.
- Go's `sort.Slice` ported, which upstream mode needs and nothing else does. The original sorts its SET results with a comparator that reads a different slice, so the order that comes out is a property of the algorithm rather than of the data, and reproducing its published SET numbers means reproducing Go's `pdqsort` rather than merely sorting the same values. Checked against 142 cases produced by Go itself, including the aliased comparator at the four lengths the original's mutated run count produces.
- The corrected reduction. Thirty one runs of one cell go in, and a median, a best, a worst and an average come out. Each series is sorted by its own key, ten percent comes off each end, and the median is the middle of what is left. All four aggregates see all 31 runs.
- The `spread` object in a chosen file. Interquartile range, standard deviation and coefficient of variation for both throughput series and for cycles, over every run including the ones the trim drops. Nothing plots it. It is the only way to tell a cell that was measured on a quiet machine from one that was not, once both have been reduced to a single number.
- Two golden cells in `testdata/golden/cells`, which are the original's own committed runs for dragonfly at one thread and pipeline depth 1, one cell with perf attached and one without, together with the four files the original reduced each of them to. Every statistics test here is checked against what the original actually produced rather than against a distribution somebody made up.
- `divergences.md` gains the evidence for D1 to D4. Each defect now carries the line of Go that causes it and the published number it changes, all of it re-derivable from the two golden cells.

### Changed

- A counter the hardware cannot measure stays unmeasured through the reduction rather than averaging to zero. `<not supported>` was surviving the median, best and worst, which clone a run whole, and being flattened to a 0 by the average. The chart layer needs that distinction to leave the cell out instead of drawing a bar saying the engine took no branches, which is D11.

### Notes

- The original's published median SET throughput for the perf cell is the 8th slowest run of 31. That is the sort whose comparator reads the perf slice while it permutes the sets slice, and it is not a small error. The cell measured without perf escapes it, because every cycles count there is absent and the comparator is false for every pair, so the two halves of the same chart set are not computed the same way.
- Reproducing the mutated run count alone regenerates all four of the original's published GET numbers and all four of its cycles numbers exactly, for both cells. SET is the only series that also needs Go's sort to be ported.

## 0.1.0 - 2026-09-04

M1 is done, which means the data model is finished and the port is proved faithful in both directions. Still nothing measures anything.

The milestone exits on two things and both have now been run. The original's published `output.json` parses and comes back byte for byte, all 2304 entries of it, and the original's `graph` reads a file our emitter wrote and draws throughput, latency and cycles from it in both scales. That cross check is the cheapest proof available that this is a port rather than a rewrite that resembles one, and it stops being available the moment a field gets renamed for being nicer.

What is left of the milestone is data rather than code: the config, the profiles and the hosts file, which are what turn a harness that assumes one 32 core AWS instance into one that runs on a machine you actually have.

### Added

- `config.jsonc`, the same file the original reads, with the same keys and the same `${arch}` placeholder, so a config that works there works here and the other way round. Comments and trailing commas are allowed and nothing else is, because the original hands everything past those to a strict JSON parser and a file that only works on one side defeats the point of sharing it.
- `profiles.toml`, which is the machine shape the original hardcodes. Core pinning, the thread sweep, the memory limit and the client count are constants in the original's driver script, and none of the machines this port runs on is that box.
- Profile validation, which refuses the three mistakes that produce numbers rather than errors: a thread sweep wider than the cores it is pinned to, a load generator sharing cores with the server under test, and a key space too large for the memory limit. All three make a chart that looks fine and measures something else.
- `hosts.toml`, absent by default, absent meaning run here. Only `hosts.example.toml` is committed, with ssh config names rather than addresses, and a test that fails if anything in it starts to look like a real machine.
- `cache-bench doctor`, which reads all three files and says what it found, or says what is wrong with them and exits non-zero. This is the file half. The machine half, which probes cores, memory and the PMU, lands with the runner.
- `CB_PARITY_EMIT`, which writes out what our emitter produced so the original's `graph` can be pointed at it. The commands are in `testdata/golden/README.md`, so the claim that the original's chart tool reads our file is something you can repeat rather than something we assert.

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
