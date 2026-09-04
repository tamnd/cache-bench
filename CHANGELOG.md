# Changelog

What each release costs you, in the order the releases happened. New entries go on top.

## Unreleased

### Added

- The machine half of `cache-bench doctor`. It reads what the kernel publishes about the box it is on, the CPU, the memory, the governor, the mitigations, the load average and whether there are counters to count with, and then measures the profile that is about to be swept against it. A profile that names more cores than the machine has, a profile that sweeps the cycles half of the matrix on a host with no PMU, a working set that would not fit in memory, or a load average that says somebody else is using the machine: each of those is refused rather than warned about, because a warning printed at the start of a job that runs for eight days is read by nobody and each of them produces numbers rather than an error.
- `doctor --write`, which records what the machine is in `host.json` next to the results. A fact the machine does not publish is refused rather than written as unknown, since this file is the whole of what a published results directory says about where its numbers came from. Nothing in it names the machine, and if a `hosts.toml` is there its names are checked against the file before it is written.
- `doctor --deep`, which starts each of the seven servers in turn, waits for it to answer, stops it and checks the group is gone. A binary of the wrong architecture, a server built without unix socket support, a Garnet whose runtime is not installed: all of them read as a correct config and all of them fail on the first run of a sweep instead of here.
- Every file the kernel publishes is parsed by a function that takes a string, with tests over the text an ARM box, an x86 box and a container each produce. A parser that has only ever run on the machine it was written on is a parser nobody has checked.

## 0.5.0 - 2026-09-04

The runner.

This is the release where the project starts measuring. Everything before it was arithmetic over numbers somebody else took. `cache-bench run` starts a server, waits for it to answer, drives memtier against it, counts cycles over the server while it does, stops the server, checks nothing survived, and writes one file. Run it once per cell and the sweep is the loop around it, which is M6.

The sequence is the original's and it does not vary by engine. What is not the original's is what happens when the sequence goes wrong. Six things are refused here that the original would have measured through: a server left over from an earlier cell answering on the socket, two sweeps sharing a results directory, a perf cell on a machine whose counters do not answer, more I/O threads than the profile pinned cores, a profile written for a bigger machine than the one running it, and a memtier pass that did not complete the operations it was asked for. Every one of those produces a plausible number rather than an error, which is why they are checks rather than notes in a document.

A run that fails anywhere writes nothing, and a cell already on disk is skipped rather than measured again. That is the whole of how a sweep that takes days survives a reboot.

The exit condition on this milestone is a gate and it has not been run. Seven servers each starting, answering, completing a run and stopping cleanly, with and without perf, needs a Linux host with all seven built on it. Everything up to that point is verified: unit tests on every piece, one whole run end to end against fakes, and thirteen checks on three operating systems. The gate itself is hardware work and it stays open until it runs on hardware.

### Added

- `cache-bench run`, which measures one cell once and writes one run file. Version, start, wait for an answer, warmup, attach perf, the two measured passes, detach, stop, confirm the group is gone, write. A run that fails anywhere in that writes nothing, because a partial file cannot be told from a complete one by the stage that reads it next, and a cell that is already on disk is skipped rather than measured again, which is the whole of how a sweep that takes days is restartable.
- One whole run in the test suite, against a cache server and a load generator that are not real. Every piece of the runner had tests around it already, and none of them could check the sequence: that the socket in the argv is the socket the readiness probe connects to, that the file memtier is told to write is the file that gets read back, and that what comes out the far end is a result file of the shape the rest of the harness reads. It takes about a second and it needs no cache server installed.
- A stray check before every run. If anything is already answering on the socket, the run stops and says so, rather than measuring a server left over from an earlier cell. The original's answer to that state is to `pkill` every process whose name it recognises, including ones it did not start, which on a shared machine kills somebody else's work and still leaves the failure it was meant to catch producing a full set of entirely plausible numbers for the wrong engine.
- A lock on the results directory, held for the length of a run and given back on the way out. Two sweeps sharing a directory is not a race over a file, it is a race over the machine: both pin the same cores, both bind the same socket, and both write result files that look exactly like the ones a healthy sweep writes.
- Result files are flushed to the disk before the run is called finished, and so is the directory entry. A sweep is restartable by which files exist, so a machine that lost power holding a file of the right name and no contents would never measure that run again.
- A perf cell is refused up front on a machine whose counters do not answer. Without that check the run would write a file full of missing counters, which is indistinguishable from a machine where one counter happens to be unsupported, and the charts cannot tell those apart either.
- A thread count larger than the cores the profile pinned the server to is refused. Seventeen I/O threads on sixteen cores measures the scheduler, and it produces a bar rather than an error, which is the kind of wrong number nobody goes looking for.
- A profile written for a bigger machine than the one running it is refused before anything starts. A pin naming cores that are not there is refused by the kernel halfway through a run, and a pin naming some cores that are there and some that are not is accepted and quietly narrowed, so a server meant to have sixteen cores gets four and the run still produces numbers.
- The run's own timestamp, in about forty lines of civil calendar arithmetic rather than a date dependency. This project needs seconds in UTC and nothing else, so the trade is a page of code with tests on the leap days against a dependency tree to audit on every update.
- All seven servers wired to the supervisor, through one path that does not vary by engine. Clear the socket, start the binary pinned to the cache half of the cores with its output going to a file next to the run, wait for it to answer, and later stop the group and confirm nothing survived. An engine given special handling here would be an engine measured differently from the rest, which is the thing the fairness rules exist to prevent.
- A leftover socket is cleared before a server is started, because several of these refuse to bind a path that already exists and a run whose server was killed leaves one behind. Only a socket is removed. Anything else at that path is somebody's file, and a benchmark deleting one would be a worse failure than the one it was avoiding.
- Version detection, run once per server and recorded verbatim in every result file that server produces. It is `<binary> --version`, the first line, colour escapes stripped, exactly as the original takes it, and nothing tries to parse a number out of it, because the seven do not agree on a format and a version parsed wrong is worse than the line the server printed. The generated README's version table is written from these strings, so a build swapped halfway through a sweep shows up in the results rather than in a footnote nobody wrote.
- The perf driver. Counters are taken over the server process for the length of the measured passes only, never over memtier and never over the machine, and perf is stopped with `SIGINT` because that is the signal it answers by printing its counters rather than by dying with them unwritten. Its output goes to a file in the run directory, so a capture survives a harness that fell over between the interrupt and the read.
- A PMU probe that is two checks rather than one. A `cpu` event source has to exist, and a live `perf stat -e cycles` has to come back with a number, because a virtual machine can have the directory and still answer `<not supported>` for every hardware event. `perf_event_paranoid` is read separately so that a machine with real counters and a locked down kernel is told it has a setting to change rather than told it has no PMU.
- perf output read from `-x,` rather than from the table perf prints for a person, and `CPUs utilized` computed from `task-clock` and a measured duration rather than scraped out of a comment column. Recorded as D21.
- The process supervisor, which is the one place in the tree allowed to start a process. clippy fails the build on a bare `Command::spawn` anywhere else. Servers are pinned with `sched_setaffinity` inside the child between fork and exec rather than by wrapping the command in taskset, so the pid the harness holds is the server's own rather than a wrapper's, which matters because that pid is what perf attaches to and what the stray check looks for. Recorded as D20.
- A stopping sequence that is checked rather than requested. The child puts itself in a new process group, so stopping takes forked workers with it, a server that has not gone by the end of the grace period is killed, and the group is confirmed gone before the run counts as finished. A group that survives is a failed run, because a server that outlives its run competes with the next one for the same cores and the number that comes out of that is low and entirely plausible. Killing also happens on drop, so a panic partway through a run cannot leave a server up.
- Readiness by protocol round trip rather than by sleeping. A `PING` for the six servers that speak RESP and a `version` for memcached, retried until the server answers correctly. The original sleeps, and a sleep that is too short means memtier measures a server that is still growing its hash table. Accepting a connection is not the same as being ready, which is the case a sleep cannot tell apart from a healthy start. A server that exits while being waited for is reported as having exited rather than as a timeout.
- memtier actually run, pinned to the load generator half of the machine and with the text it prints going to a log next to the run, which is where a refused connection says so. A pass has a ceiling on how long it may take, because a pass that never finishes is memtier waiting on a server that stopped answering and every later run in the sweep is queued behind it.
- The warmup pass, checked exactly like the two measured ones. Its numbers are thrown away, but a warmup that did not run means the measured SET pass is partly a measurement of hash table growth, which is the one thing the warmup exists to prevent, so it fails the run rather than passing quietly.
- The result file for a pass is cleared before that pass runs. A file left by an earlier pass holds real numbers from a real run, so a memtier that died before writing would otherwise produce a result that looks entirely credible and belongs to something else.
- The memtier driver's argv, built in one place as data. The original spreads the same flags across a shell script and a Go program whose defaults disagree with it, so reading either one tells you what might have been measured rather than what was. A test compares the generated argv against the exact command line the original's published numbers came out of.
- A strict parser for memtier's output. Three checks before a result is accepted: the stats object exists, the completed operation count is within a thousandth of the one requested, and all five requested percentiles came back. The original reads the same JSON with a path query that yields zero for a missing field, so a memtier that renamed something, or a run where connections died halfway through, produces a result file full of zeros that then charts as real bars sitting on the axis. Each of the three is a failed run here instead.

## 0.4.0 - 2026-09-04

The documents.

Everything a reader sees around a chart is generated now. Both indexes, and a README in every results directory that says what the numbers were measured on, which versions were measured, what the method was and what the numbers may not be used for. None of it is written by hand, which is the point. 154 chart references maintained by a person go stale on the first rename and nobody finds out until a reader hits a broken image, and the original's README already disagrees with the data sitting next to it.

The generator is a function of its inputs and CI checks that on every push, twice over. It generates everything twice and diffs the two, and it runs `docs --check` over every published results directory so that a file somebody edited by hand fails the build rather than being quietly overwritten by the next sweep.

The exit gate was run rather than asserted. All 154 charts drawn, all three documents generated, then every image reference pulled out of the three documents and diffed against the directory listing. 154 references, 154 files, no diff in either direction. Deleting a chart produces documents that name it as not drawn and link to it from nowhere.

Still nothing measured. The runner is M5.

### Added

- `cache-bench docs`, which writes the documents that go with a results directory. The two chart indexes, `LINEAR.md` and `LOGARITHMIC.md`, and the results `README.md`. All three come out of the same table the charts are drawn from, so a document cannot name a chart that was never specified, and all three are built against the list of PNGs actually sitting in `graphs`, so a chart that was specified but not drawn is named rather than left out. `--check` writes nothing and fails if a document on disk is not the one that would be generated.
- The anchor numbering GitHub appends to repeated headings, as a counter with a unit test on it rather than as suffixes typed by hand. Each index has the same ten headings under each of four pipeline blocks, so nearly every link in it points at a heading that appears four times, and getting the numbering wrong produces a document whose links all land in the first block.
- `results/<host>/host.json`, the record of what a results directory was measured on. The kernel, the CPU, the core count, the memory, the governor, the mitigations, whether there was a hardware PMU to count with, the load generator version and when the sweep started and finished. Nothing in it names the machine, and there is a check that fails if a hostname ever gets into one. Recorded as D17.
- A generated `README.md` in every results directory. The methodology bullets carry the profile's own numbers rather than numbers typed next to them, the hardware table comes out of `host.json`, and the version table is one row per engine read out of the results themselves. The original's is written by hand, which is how it came to disagree with its own data. Recorded as D18.
- The divergences table, in the generated README, so a reader who followed a link to a chart is told what this port does differently before they read a bar. It is generated from the same list as `divergences.md` and a test fails if the two ever disagree.
- What these numbers may and may not be used for, word for word out of `docs/methodology.md`, next to the charts rather than in a document nobody following a chart link will open. A test fails if the two copies drift. Recorded as D19.
- A `generated` job in CI. It generates the documents twice and diffs the two, which is what proves the generator is a function of its inputs, then runs `docs --check` over every published results directory. A generated file that somebody edited by hand fails the build instead of being silently thrown away by the next sweep.

### Changed

- The chart indexes cover all 154 charts rather than 120. The MIN and AVG latency sections are new, the two charts drawn with Garnet's single thread bar left off are linked from the P99 section they are a redraw of, and every image carries a real alt text instead of the `Alt text` placeholder. Recorded as D16.

## 0.3.0 - 2026-09-04

The charts.

All 154 of them come out as PNGs now, and the same numbers produce the same bytes on Linux, macOS and Windows. That last part is the milestone. The original's chart layer writes a matplotlib script, shells out to python3 and deletes the script, and what comes back depends on which fonts the machine has, so two people running it produce two different pictures and neither can check the other. A chart drawn here is a function of its data and nothing else, and CI proves it on three operating systems on every push rather than asserting it in a README.

The renderer is written here, which was not the plan. Both ways plotters can draw text are closed to us: font-kit resolves system fonts, which is the thing being removed, and ab_glyph sits on a crate cargo-deny already fails the build on. What was left turned out to be small, because a chart is only two kinds of shape. Rectangles have an exact coverage per pixel with no sampling in it, and glyphs are outlines that skrifa reads out of the fonts already embedded here and zeno fills in scalar arithmetic with no SIMD path to diverge on.

The check that mattered most is the one that says the fixtures were not fooling us. All 154 charts drawn straight from the original's published output.json in upstream mode are byte for byte identical to the 154 drawn from the golden series committed here.

Nothing has been measured yet. The runner is M5 and the first real sweep is M6, so every chart drawn so far is drawn from the original's numbers.

### Added

- The renderer, which is the other half of the chart engine. All 154 charts now come out as PNGs. The shapes on a chart are axis aligned rectangles and glyphs, so the rectangles are filled by exact coverage per pixel with no sampling anywhere, and the glyphs are outlines read out of the embedded fonts and filled in scalar arithmetic. Nothing in the path touches a system font, a SIMD path or a floating point operation whose order is not fixed, which is what makes the output the same on every platform rather than nearly the same.
- `cache-bench chart`, which draws them. Point it at a results directory, or pass `--golden` to draw the 154 committed here without measuring anything. `--manifest` writes a SHA-256 per chart and `--check` reads one back, so two machines are compared by diffing two text files rather than 154 pictures.
- `testdata/golden/charts.sha256`, the hash of every chart drawn from the committed series, and a `determinism` job that redraws them on Linux, macOS and Windows and checks all 154 against it on every push. A quicker version of the same check runs in `cargo test` over four charts chosen to cover both scales and the widest axis.
- A provenance line along the bottom of every chart drawn from real measurements, naming the profile, what the machine is and its core count. The original's charts say what was measured and not where, which is the most likely way for a chart drawn here to mislead somebody. Charts drawn from the golden series carry nothing, because that is what CI hashes.

### Changed

- Every chart is 1880 by 1130. The original saves with `bbox_inches='tight'`, so its own 154 come out in three different widths depending on how many digits the y axis numbers happen to need, and two of them cannot be flipped between without everything shifting sideways. Recorded as D14.

## 0.2.1 - 2026-09-04

Everything a chart is, except the picture.

The chart engine is being built in two halves and this is the first one finished. What goes on each of the 154 charts and where it goes are both settled, both taken from the original rather than from reading the original, and both checked bit for bit in CI on a checkout with nothing measured in it. 154 titles, 11088 bars, 1872 ticks and 6098 gridlines, all of them the original's.

That was worth doing before the renderer rather than after. The original's chart layer leaves nothing behind but a PNG, so every decision it makes about what a chart says and where the axis starts is invisible once it has run. Both fixtures were taken by standing in for the thing it hands its work to, Python in one case and matplotlib in the other, which means neither is a transcription that could quietly drift from what the original does. Once the renderer exists, a wrong pixel will be a rendering bug and nothing else, because everything upstream of the pixel is already pinned.

The fonts are in the binary too, so nothing depends on what happens to be installed. Three faces, three licences, three digests checked against the embedded bytes.

M3 is not finished. The renderer, the hash manifest, the provenance stamp and the determinism job are what is left, and the exit gate needs two machines to agree on the bytes.

### Added

- The series layer, which is the half of the chart engine with no pixels in it. A results file goes in and what comes out is a title, both axis labels, a thread count for each group of bars, and one number per bar, for each of the 154 charts. Everything a reader could disagree with is decided here, so it is a pure function and it is tested before anything is drawn.
- `testdata/golden/series.json`, which is all 154 charts as the original worked them out. Its `graph` tool pastes the numbers into a Python script and deletes the script after drawing, so `tools/series-vectors` stands in for Python, keeps the script and throws the picture away. The fixture is the original's answer rather than a description of it.
- Two levels of check on that fixture. The filenames, titles, axes, legend order and colours are checked in `cargo test` against a results file with the original's shape and none of its numbers, so it runs in CI on a checkout with nothing measured in it. The bar heights go through `cache-bench verify --against`, where they come out of our own reduction of the original's run files, which makes a matching chart one where every bar survived the run files, the selection, the combining and the extraction.
- The three faces the charts are drawn with, in the binary. Jost Book and Jost Bold for the original's Futura and its bold, DejaVu Sans for the Verdana it names on the quarter decade labels. Each licence sits in the directory next to the font it covers, `assets/fonts/README.md` records the exact release each file came from, and the SHA-256 of all three is written into `font.rs` where a test checks it against the bytes it embedded.
- The geometry layer. Where the y axis starts and stops, which ticks get a label, where the gridlines between them go, what number sits beside each one, how wide a bar is and where in its group it starts. All of it is the original's arithmetic, including the two different logarithms it uses for the two ends of a log axis and the eighth of a decade it calls a quarter.
- `testdata/golden/axes.json`, which is that geometry for all 154 charts as the original produced it. Its layout lives in about forty lines of Python inside two Go string constants and leaves nothing behind but a PNG, so `tools/axis-vectors` slices those constants out of `cmd/graph/main.go` and runs them with matplotlib replaced by something that records what it was told. Every bound, tick, gridline and label is compared bit for bit in `cargo test`, and `cache-bench verify` now prints an `axes` line making the same check from the command line. Like the series fixture it needs no measurements, because an axis is a function of the bars.

### Fixed

- `Kind` now formats through `pad`, so a width in a format string does what it says. `verify` prints the four aggregates in a column and asks for eight characters, and the old implementation threw the width away without complaining, which is why that column was never a column.

### Notes

- All 154 charts and all 11088 bars come back as the original's. That is the first half of the M3 gate met, and it is met before a single pixel has been drawn, which was the point of splitting the layer in two.
- Running the original's own layout code turned up three things that reading it did not. The thread count under each group of bars is placed at a hardcoded offset that is the middle of the group for six bars and for no other number, so our seventh engine would have knocked every x label off centre. That is D13 and the offset is now computed. The minor gridlines step by an eighth of a decade rather than the quarter their variable is named after, so there are seven between labelled ticks. And the only two charts with a zero on them are linear, which is the whole reason a zero has never reached the original's logarithmic path, where it has no answer.
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
