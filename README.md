# cache-bench

Throughput, latency and CPU cycles for eight cache servers, measured with `memtier_benchmark` and charted from committed data.

This is a Rust port of [tidwall/cache-benchmarks](https://github.com/tidwall/cache-benchmarks), which is the original and which deserves the credit for the methodology. The port exists for three reasons. It adds [yo](https://github.com/tamnd/yo) and [rugo](https://github.com/tamnd/rugo) as a seventh and an eighth subject. It runs on hardware that is not a 32 core AWS instance, which the original assumes throughout. And it draws the charts in Rust instead of shelling out to Python and matplotlib, so the chart layer can be tested and so the same input produces the same PNG on every machine.

## Status

Every stage is written. `doctor`, `run`, `sweep`, `mem`, `choose`, `combine`, `chart`, `docs` and `verify` all work: `run` measures one cell and writes one file, `sweep` is the loop that measures the other ten thousand and keeps a record of what it did, and everything downstream of those files has been working since the chart milestone. What is missing is the hardware gate that says all eight servers come up and go away again on a real Linux box, and the results themselves. The [milestones](https://github.com/tamnd/cache-bench/milestones) say what each stage has to land and what it is gated on.

No results have been published. When they are, they will come with the raw `output.json` next to them, so anyone can redraw every chart without trusting us.

## What gets measured

Eight cache servers, all with persistence off, all over a local unix socket, so no network stack is in the measurement.

| Server | Thread flag |
| --- | --- |
| [memcached](https://github.com/memcached/memcached) | `-t` |
| [Redis](https://github.com/redis/redis) | `--io-threads` |
| [Valkey](https://github.com/valkey-io/valkey) | `--io-threads` |
| [Dragonfly](https://github.com/dragonflydb/dragonfly) | `--proactor_threads` |
| [Garnet](https://github.com/microsoft/garnet) | `--miniothreads`, `--maxiothreads`, `--minthreads`, `--maxthreads` |
| [Pogocache](https://github.com/tidwall/pogocache) | `-t` |
| [yo](https://github.com/tamnd/yo) | `--threads` |
| [rugo](https://github.com/tamnd/rugo) | `--threads` |

The x axis of every chart is that thread count, swept from 1 to 16. Each point is the trimmed median of 31 runs. Each run is 100,000 SET operations and 100,000 GET operations per connection across 256 connections, with a warmup pass before the measured one, at pipeline depths of 1, 10, 25 and 50, with values of 1 to 1024 bytes.

Throughput comes out in Kops/sec, latency in microseconds at MIN, AVG, P50, P90, P99, P99.9, P99.99 and MAX, and CPU cycles in cycles per operation from `perf`. That is 154 charts, in linear and logarithmic scale.

### Memory

Separately, and not part of any run. `cache-bench mem` starts each server, gives it a known number of distinct keys, lets it settle and reads the largest resident set it ever had. The key count is known rather than estimated: the filling pass writes one operation per key over a range the clients divide evenly, so nothing is written twice and nothing asks the server for a count it defines its own way.

It reports two numbers because there are two claims. Total bytes per entry is the peak divided by the keys in it, which is what a machine has to have. Overhead bytes per entry is what is left after the keys and the values themselves, which is what a design controls. At a hundred-odd bytes of payload per key an index that got twice as small halves the second and moves the first by a few percent, so quoting one of them is picking the flattering number.

It reports and does not judge. Garnet sizes its index at startup and Dragonfly preallocates per proactor, so the baseline each server held before a single key went in is recorded beside the peak rather than subtracted from it. Linux only, because there is no portable high water mark and no number here comes from a machine without `/proc`.

## What it does not measure

No expiry, no eviction, no mixed command set, no large values, no network, no replication, no persistence, no multi key operations. It is a hot path measurement of two commands. Numbers from it should not be used to say one engine is faster than another in general, and the results README will say so where the numbers appear.

## Layout

```
crates/cb-core      types, the JSON model, config, profiles
crates/cb-cache     the eight cache adapters and process lifecycle
crates/cb-mem       the memory measurement, its plan and its result file
crates/cb-memtier   the memtier driver and its output parser
crates/cb-perf      the perf driver, the PMU probe, CSV parsing
crates/cb-stats     run selection and aggregation
crates/cb-chart     the chart engine, axes and layout and its own rasterizer
crates/cb-docs      generated LINEAR.md, LOGARITHMIC.md and README.md
crates/cache-bench  the binary
tools/provision     one script that turns a fresh Ubuntu box into one that can be swept
```

`crates/cb-core/golden` holds the original's own files, which is what every test in the workspace and `cache-bench verify` are checked against, and `crates/cb-chart/assets/fonts` holds the three faces the charts are drawn with. Both are in the crates that read them rather than beside them, because a published crate can only carry what is inside it.

`cb-chart`, `cb-stats`, `cb-core` and `cb-docs` build and test on any platform with no cache server installed, which is where the work that is hard to get right lives. `cb-cache` needs Linux.

## Install

```
cargo install cache-bench
```

That builds the binary from source and needs nothing else. A release also carries a tarball per platform on its [releases page](https://github.com/tamnd/cache-bench/releases), with a SHA-256 next to each one, which is what to fetch on a box that has no Rust toolchain on it and no reason to grow one.

The eight cache servers and `memtier_benchmark` are not installed by any of this. They are named in `config.jsonc` by path, and `cache-bench doctor` says which ones it found. On Ubuntu, `tools/provision/install.sh` builds all nine of them at the versions pinned in `tools/provision/versions.env` and puts them where `config.jsonc` already looks, which is the half hour between a fresh box and a box that can be swept.

## Usage

```
cache-bench doctor  --profile wsl32        what this host can and cannot measure
cache-bench doctor  --profile wsl32 --deep every server started once and stopped again
cache-bench run     redis --threads 8 --pipeline 10 --perf no --run 1
cache-bench sweep   --profile wsl32        the whole matrix, restartable
cache-bench mem     --profile wsl32        what each engine costs to hold ten million keys
cache-bench choose  --dir results/wsl32
cache-bench combine --dir results/wsl32
cache-bench chart   --dir results/wsl32 --profile wsl32
cache-bench docs    --dir results/wsl32
cache-bench verify  --against /path/to/cache-benchmarks/results
```

`doctor` refuses rather than warns. A machine with fewer cores than the profile names, a profile that sweeps cycles on a host with no counters, a working set that would not fit in memory, a load average that says somebody else is using the box: each of those is a sweep that produces numbers rather than an error, so each of them stops here instead. `--write` records what the machine is in `host.json` next to the results, and `--deep` starts each of the eight servers in turn and stops it again, which is the check no file can make.

`sweep` takes days. It measures in the original's order, engine then threads then pipeline depth then counters then run number, so that all the runs of one cell happen together in time and a noisy hour shows up as one bad cell rather than as a tilt across the whole matrix. It is restartable, and the restart rule is file existence and nothing else: a cell whose file is there is skipped, a file that will not parse is measured again rather than trusted, and one results directory takes one sweep at a time. `--dry-run` prints the cells it would measure and touches nothing.

A sweep keeps a record of itself next to the numbers. `logs/sweep.jsonl` is one line per attempt with the load average taken before it, which is what answers the question somebody asks a week later when one cell in one chart looks wrong. `failures.json` names every cell that was attempted and produced no file, with the reason verbatim and a count of how many times it has been tried, because the alternative to naming a missing cell is a chart that draws a zero and a zero is a claim while an absence is not. An engine whose cells fail three times in a row is put down for the rest of the session and named in that file, since the other six are still worth measuring and this is day three of eight.

`verify` is the claim this port makes about itself. With no arguments it checks the golden files committed here and runs anywhere in under a second. Pointed at a checkout of the original it reads all 20160 of its committed run files, reproduces all 2304 of its chosen files and its whole published `output.json` byte for byte, rebuilds all 154 of its charts down to the last bar, and then prints how far the corrected statistics sit from the original's.

## Hardware profiles

The original was measured on a 32 core ARM64 AWS c8g.8xlarge with the cache pinned to 16 cores and the load generator pinned to the other 16. Profiles let the same harness run on a box that is not that, and every result file and every chart records which profile produced it. Mixing two profiles into one chart set without saying so is the failure this is built to prevent.

A profile can also say that its numbers are not fit to publish. `smoke` is one: two thread counts, pipeline one and ten, three runs, on a four core box, which answers whether a change helped in minutes and answers nothing about how these engines compare. `docs` refuses to write a results README from a profile marked that way, since a README is where a measurement stops being a note to oneself. The sweep runs as often as is useful.

## Differences from the original

Recorded in [divergences.md](divergences.md), with the reasoning for each one. The ones worth knowing about before reading any chart:

The statistics were rewritten. The original picks its median at the wrong index, sorts the SET results with a comparator that reads a different slice, never sorts the perf results at all before indexing into them as if it had, and carries the run count in a mutated global so that three of its four aggregates see fewer than 31 runs. All four are fixed, and `--compat=upstream` reproduces them exactly so the original's published output can still be regenerated byte for byte.

The charts use Jost and DejaVu Sans, which are open fonts embedded in the binary, rather than Futura and Verdana, which are not and which only resolve on macOS. Everything else about the chart layout is the original's.

A chart here is a function of its data and nothing else. The same numbers produce the same PNG, byte for byte, on Linux, macOS and Windows, and CI checks that on every push by drawing all 154 charts from a committed series on three operating systems and hashing every one of them. Every chart drawn from real measurements also carries a line along the bottom naming the profile and the machine, because two throughput charts from two machines are not comparable and the original's charts do not say which machine they came off.

The chart indexes are generated rather than maintained. The original writes `LINEAR.md` and `LOGARITHMIC.md` by hand, down to the anchor suffixes GitHub appends to repeated headings, and between them they link 120 of the 154 charts it publishes. Here both come out of the same table the charts are drawn from, so every chart that exists is linked, a chart that was not drawn is named rather than quietly dropped, and every image says what is on it.

The README in a results directory is generated too, and none of the facts in it are typed. The methodology bullets carry the profile's own numbers, the hardware table comes out of the `host.json` written before the sweep, and the version table is one row per engine read out of the results themselves. Two things go in it that the original has nowhere: the table of everything this port does differently, and what these numbers may and may not be used for in full, because a caveat that lives in a document nobody opens is a caveat that does not exist. CI regenerates every published document and fails if one of them was edited by hand.

## Licence

Apache-2.0.

The three fonts in the binary are not. Jost is under the SIL Open Font License 1.1 and DejaVu Sans is under the Bitstream Vera Fonts Copyright, both permissive enough to embed, and both licence texts ship in the directory beside the font file they cover. See [crates/cb-chart/assets/fonts/README.md](crates/cb-chart/assets/fonts/README.md) for which release each file came from.
