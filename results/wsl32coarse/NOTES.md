# Notes on this sweep

Written by hand. Everything in `README.md` next to this file is generated from the data in this directory, which is what makes it checkable, and it is also why it cannot say any of what is below. This is what is true of these numbers in particular.

## Read this before quoting yo or rugo

The `yo` measured here is 0.3.26, which is before the fix in [tamnd/yo#481](https://github.com/tamnd/yo/pull/481). Every worker held the same listener in its own poller and drained the whole backlog into itself, so a benchmark that opens all 256 connections at once had its thread split settled by one race and then kept it for the length of the run.

What that looks like in this data is not a slow engine, it is an unrepeatable one. Across the five runs of a cell, yo's coefficient of variation is 0.40 to 0.85 at eight threads and 0.12 to 0.73 at sixteen. Redis, Valkey and Dragonfly sit between 0.00 and 0.02 in the same cells on the same box. Yo's bars at eight and sixteen threads are therefore a picture of which race won, and the fact that sixteen threads plots lower than eight is that and not a scaling result. Do not quote them. Yo's one thread column is steady, at 0.01 to 0.10, and reads like every other engine's.

`rugo` 0.1.0 has the same shape of problem and no fix behind it yet: 0.17 to 0.61 at eight threads and 0.14 to 0.62 at sixteen. Its bars at those two thread counts should be read the same way, which is to say not read.

Nothing here has been re-swept against yo v0.3.27. When it is, the thing to check first is the coefficient of variation, not the throughput.

## What else the spread says

The rest of the field splits in two. Redis, Valkey, Dragonfly and Pogocache are tight everywhere, 0.00 to 0.08 across every cell. Garnet and Memcached are looser at sixteen threads, reaching 0.31 and 0.26, which is worth knowing when two of their bars are close together but is a different thing from the two engines above.

Five runs a cell is a small sample, so these spread figures are themselves coarse. They separate an engine that is steady from one that is not, and they do not support finer statements than that.

## What this profile gives up

`wsl32coarse` is the `wsl32` box and the `wsl32` working set sampled coarsely, so a number exists before the full matrix finishes. Three thread counts instead of six and five runs a cell instead of thirty one. The thread axis of every chart here has three points on it, so the curve between them is drawn by the chart and not measured, and reading a shape into it is reading more than is there.

This host has no hardware PMU, so cycles per operation was never measured. That is why there are 146 charts here rather than 154, and why the cycles sections of the indexes name what is missing instead of showing it.

This is a draft sweep published on purpose. A draft is what finds the layout bugs and the provenance holes, and finding them in a sweep that took a day is better than finding them in one that took a week.
