# cache-bench

Throughput, latency and CPU cycles for seven cache servers, measured with `memtier_benchmark` and charted from committed data.

This is a Rust port of [tidwall/cache-benchmarks](https://github.com/tidwall/cache-benchmarks), which is the original and which deserves the credit for the methodology. The port exists for three reasons. It adds [yo](https://github.com/tamnd/yo) as a seventh subject. It runs on hardware that is not a 32 core AWS instance, which the original assumes throughout. And it draws the charts in Rust instead of shelling out to Python and matplotlib, so the chart layer can be tested and so the same input produces the same PNG on every machine.

## Status

Skeleton. The workspace, the toolchain and the CI are here. Nothing measures anything yet. The [milestones](https://github.com/tamnd/cache-bench/milestones) say what each stage has to land and what it is gated on.

No results have been published. When they are, they will come with the raw `output.json` next to them, so anyone can redraw every chart without trusting us.

## What gets measured

Seven cache servers, all with persistence off, all over a local unix socket, so no network stack is in the measurement.

| Server | Thread flag |
| --- | --- |
| [memcached](https://github.com/memcached/memcached) | `-t` |
| [Redis](https://github.com/redis/redis) | `--io-threads` |
| [Valkey](https://github.com/valkey-io/valkey) | `--io-threads` |
| [Dragonfly](https://github.com/dragonflydb/dragonfly) | `--proactor_threads` |
| [Garnet](https://github.com/microsoft/garnet) | `--miniothreads`, `--maxiothreads`, `--minthreads`, `--maxthreads` |
| [Pogocache](https://github.com/tidwall/pogocache) | `-t` |
| [yo](https://github.com/tamnd/yo) | `--threads` |

The x axis of every chart is that thread count, swept from 1 to 16. Each point is the trimmed median of 31 runs. Each run is 100,000 SET operations and 100,000 GET operations per connection across 256 connections, with a warmup pass before the measured one, at pipeline depths of 1, 10, 25 and 50, with values of 1 to 1024 bytes.

Throughput comes out in Kops/sec, latency in microseconds at MIN, AVG, P50, P90, P99, P99.9, P99.99 and MAX, and CPU cycles in cycles per operation from `perf`. That is 154 charts, in linear and logarithmic scale.

## What it does not measure

No expiry, no eviction, no mixed command set, no large values, no network, no replication, no persistence, no multi key operations. It is a hot path measurement of two commands. Numbers from it should not be used to say one engine is faster than another in general, and the results README will say so where the numbers appear.

## Layout

```
crates/cb-core      types, the JSON model, config, profiles
crates/cb-cache     the seven cache adapters and process lifecycle
crates/cb-memtier   the memtier driver and its output parser
crates/cb-perf      the perf driver, the PMU probe, CSV parsing
crates/cb-stats     run selection and aggregation
crates/cb-chart     the chart engine, built on plotters
crates/cb-docs      generated LINEAR.md, LOGARITHMIC.md and README.md
crates/cb-cli       the cache-bench binary
```

`cb-chart`, `cb-stats`, `cb-core` and `cb-docs` build and test on any platform with no cache server installed, which is where the work that is hard to get right lives. `cb-cache` needs Linux.

## Usage

```
cache-bench doctor  --profile wsl32        what this host can and cannot measure
cache-bench run     redis --threads 8 --pipeline 10 --perf no --run 1
cache-bench sweep   --profile wsl32        the whole matrix, restartable
cache-bench choose  --dir results/wsl32
cache-bench combine --dir results/wsl32
cache-bench chart   --dir results/wsl32 --all
cache-bench docs    --dir results/wsl32
cache-bench verify                         parity against the original
```

`sweep` takes days. It is restartable, it skips runs whose result file already exists, and it prints an ETA once it has enough completed runs to derive one.

## Hardware profiles

The original was measured on a 32 core ARM64 AWS c8g.8xlarge with the cache pinned to 16 cores and the load generator pinned to the other 16. Profiles let the same harness run on a box that is not that, and every result file and every chart records which profile produced it. Mixing two profiles into one chart set without saying so is the failure this is built to prevent.

## Differences from the original

Recorded in [divergences.md](divergences.md), with the reasoning for each one. The ones worth knowing about before reading any chart:

The statistics were rewritten. The original picks its median at the wrong index, sorts the SET results with a comparator that reads a different slice, never sorts the perf results at all before indexing into them as if it had, and carries the run count in a mutated global so that three of its four aggregates see fewer than 31 runs. All four are fixed, and `--compat=upstream` reproduces them exactly so the original's published output can still be regenerated byte for byte.

The charts use Jost and DejaVu Sans, which are open fonts embedded in the binary, rather than Futura and Verdana, which are not and which only resolve on macOS. Everything else about the chart layout is the original's.

## Licence

Apache-2.0.
