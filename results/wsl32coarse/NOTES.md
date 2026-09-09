# Notes on this sweep

Written by hand. Everything in `README.md` next to this file is generated from the data in this directory, which is what makes it checkable, and it is also why it cannot say any of what is below. This is what is true of these numbers in particular.

## What changed since the last version of this directory

This is the same profile, the same box and the same 96 cells as the sweep that was published here before, re-measured against yo 0.3.28 instead of 0.3.26. The previous notes said that yo's multithreaded bars were a picture of which race won rather than of how fast the engine is, that the fix was in [tamnd/yo#481](https://github.com/tamnd/yo/pull/481), and that the thing to check when it was re-swept was the coefficient of variation rather than the throughput.

It has been re-swept and the answer is that the fix did not close it. The numbers are below and the shape of the problem has changed, which is worth more than the fact that it is still there.

## Read this before quoting yo or rugo

The coefficient of variation over the five runs of a cell, on GETs, one row per engine and one column per cell:

| engine | t1 p1 | t1 p10 | t1 p25 | t1 p50 | t8 p1 | t8 p10 | t8 p25 | t8 p50 | t16 p1 | t16 p10 | t16 p25 | t16 p50 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Dragonfly | 0.06 | 0.01 | 0.01 | 0.03 | 0.01 | 0.00 | 0.01 | 0.00 | 0.01 | 0.00 | 0.00 | 0.01 |
| Garnet | 0.01 | 0.04 | 0.07 | 0.14 | 0.02 | 0.01 | 0.02 | 0.13 | 0.02 | 0.09 | 0.02 | 0.03 |
| Memcached | 0.02 | 0.02 | 0.07 | 0.03 | 0.00 | 0.02 | 0.01 | 0.02 | 0.02 | 0.01 | 0.02 | 0.04 |
| Pogocache | 0.02 | 0.01 | 0.01 | 0.01 | 0.01 | 0.01 | 0.02 | 0.01 | 0.01 | 0.02 | 0.06 | 0.02 |
| Redis | 0.08 | 0.01 | 0.00 | 0.01 | 0.24 | 0.00 | 0.01 | 0.01 | 0.01 | 0.00 | 0.00 | 0.00 |
| rugo | 0.15 | 0.05 | 0.01 | 0.04 | 0.57 | 0.17 | 0.27 | 0.41 | 0.52 | 0.36 | 0.32 | 0.30 |
| Valkey | 0.01 | 0.02 | 0.01 | 0.01 | 0.00 | 0.02 | 0.01 | 0.03 | 0.31 | 0.01 | 0.08 | 0.01 |
| yo | 0.01 | 0.05 | 0.11 | 0.22 | 0.17 | 0.35 | 0.56 | 0.54 | 0.05 | 0.38 | 0.18 | 0.33 |

Five of the eight engines are between 0.00 and 0.08 in every cell but one, which is what says the box was quiet and that what follows is the engines and not the machine. Redis at eight threads and pipeline one, and Valkey at sixteen threads and pipeline one, each have a single cell where two of the five runs came in low, and those two cells are contention and not a pattern. Garnet has two cells in the low teens and is otherwise steady.

yo and rugo are the two that are not steady, and the same caveat as last time applies to both: their bars at eight and sixteen threads are a picture of which run won. Do not quote them.

## What the shape of yo's spread says

It is not the same shape as last time and it is worth saying how. Last time yo was tight at one thread and loose at eight and sixteen, which is what a race over who accepts a connection looks like. This time it is tight at one thread and pipeline one, at 0.01, and it gets looser with pipeline depth at one thread as well, reaching 0.22 at depth fifty on a single I/O thread. A single threaded server has nobody to race with, so whatever this is now, it is not only the listener.

The runs themselves are not drifting, they are landing in more than one place. At eight threads and pipeline twenty five the five GET figures in Kops/sec are 6927, 19402, 6103, 9236 and 5113. At depth fifty they are 9858, 6132, 6675, 19219 and 5546. Pogocache in the same cell, on the same box, in the same session, reads 12098, 12413, 11811, 12392 and 12294.

Two things follow from that and both of them matter. The nineteen million is real and it was measured, so the ceiling in that cell is above every rival's median. And it happens one run in five, so the median is not a number anybody should be quoted.

## What yo's throughput looks like anyway, with that said

At eight threads yo's best run is 1.39x the best rival's median at pipeline twenty five and 1.44x at pipeline fifty. At sixteen threads its best run is 0.66x and 0.57x of the same thing. Going from eight threads to sixteen makes yo slower on this box, and that is a separate problem from the spread rather than another face of it.

## What else is in here

`rugo` 0.1.0 is unchanged from the previous sweep and has the same problem with no fix behind it yet.

## What this profile gives up

`wsl32coarse` is the `wsl32` box and the `wsl32` working set sampled coarsely, so a number exists before the full matrix finishes. Three thread counts instead of six and five runs a cell instead of thirty one. The thread axis of every chart here has three points on it, so the curve between them is drawn by the chart and not measured, and reading a shape into it is reading more than is there.

This host has no hardware PMU, so cycles per operation was never measured. That is why there are 146 charts here rather than 154, and why the cycles sections of the indexes name what is missing instead of showing it.

Five runs a cell is a small sample, so the spread figures above are themselves coarse. They separate an engine that is steady from one that is not, and they do not support finer statements than that.

## How this sweep was taken

In sessions across two days rather than in one pass, because the box had other work on it for part of that time and the sweep waits for a quiet machine rather than measuring through somebody else's load. The restartable sweep is what makes that safe: a cell is measured once and its file is kept, so a session that is interrupted costs the cell it was in the middle of and nothing else. All 480 runs come from one build of each engine, and the version strings in `README.md` are the ones each server printed when it was started for these runs.
