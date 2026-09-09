# Notes on this sweep

Written by hand. Everything in `README.md` next to this file is generated from the data in this directory, which is what makes it checkable, and it is also why it cannot say any of what is below. This is what is true of these numbers in particular.

## What changed since the last version of this directory

Two things, and the second is larger than the first.

This is the same profile, the same box and the same matrix as the sweep published here before, re-measured against yo 0.3.28 instead of 0.3.26.

And the harness now refuses a class of run it used to accept. memtier reports `Ops/sec` as the whole completed operation count over `Total duration`, and `Total duration` is measured against the first of its load generator threads to finish rather than the last. Where the threads finish together those are the same number to within a hundredth of a percent, which is why nobody noticed. Where they do not, the number can be an order of magnitude above what the server did. One pass here had a thread finish after 1.387 seconds while the last took 17.682, and 25.6 million operations over the first of those came out as 19.4 million a second, higher than any other engine anywhere on this box. A pass whose last load generator thread ran more than a quarter longer than its first is now refused. That is [D25](../../divergences.md#d25-a-rate-that-has-to-cover-the-run-it-is-a-rate-for), and the previous version of this file quoted that 19.4 million as a real ceiling. It was not one.

The refusal was applied to this directory with `cache-bench recheck` after the fact, from the load generator's own output for every pass of every run, which is kept. 83 of the 480 runs no longer pass. `failures.json` names every one of them and what refused it.

## What is missing from the charts, and why

18 of the 96 cells are empty. Twelve lost every run they had and six were left with fewer than three, which is the fewest a median, a best, a worst and an average can be four different numbers over.

They are not spread evenly. All eight of yo's cells at eight and sixteen threads are gone, and so are all eight of rugo's. Garnet lost two of its twelve. Dragonfly, Memcached, Pogocache, Redis and Valkey lost nothing at all.

That is the finding rather than a gap in the data. The check is a question about whether a server served all 256 of its connections alike, and the two engines that lose their whole multithreaded half are the two whose bars the previous version of this file already said not to quote. What is new is that there is now a mechanism named rather than a variance observed. Garnet is the addition: it was called steady before and it loses two cells here.

## Read this before quoting anything

The coefficient of variation over the runs of a cell, on GETs, one row per engine and one column per cell. `none` is a cell with no bar in it.

| engine | t1 p1 | t1 p10 | t1 p25 | t1 p50 | t8 p1 | t8 p10 | t8 p25 | t8 p50 | t16 p1 | t16 p10 | t16 p25 | t16 p50 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Dragonfly | 0.06 | 0.01 | 0.01 | 0.03 | 0.01 | 0.00 | 0.01 | 0.00 | 0.01 | 0.00 | 0.00 | 0.01 |
| Garnet | 0.01 | 0.04 | 0.07 | none | 0.02 | 0.01 | 0.02 | none | 0.02 | 0.09 | 0.02 | 0.03 |
| Memcached | 0.02 | 0.02 | 0.07 | 0.03 | 0.00 | 0.02 | 0.01 | 0.02 | 0.02 | 0.01 | 0.02 | 0.04 |
| Pogocache | 0.02 | 0.01 | 0.01 | 0.01 | 0.01 | 0.01 | 0.02 | 0.01 | 0.01 | 0.02 | 0.06 | 0.02 |
| Redis | 0.08 | 0.01 | 0.01 | 0.01 | 0.24 | 0.00 | 0.01 | 0.01 | 0.01 | 0.00 | 0.00 | 0.00 |
| rugo | 0.15 | 0.05 | 0.01 | 0.04 | none | none | none | none | none | none | none | none |
| Valkey | 0.01 | 0.02 | 0.01 | 0.01 | 0.01 | 0.02 | 0.01 | 0.03 | 0.31 | 0.01 | 0.08 | 0.01 |
| yo | 0.01 | 0.05 | 0.11 | 0.22 | none | none | none | none | none | none | none | none |

Five of the eight engines sit between 0.00 and 0.09 in every cell but one, which is what says the box was quiet and that what is left is the engines rather than the machine.

Three cells are the exceptions and none of them is a pattern. Redis at eight threads and pipeline one is 0.24, Valkey at sixteen threads and pipeline one is 0.31, and yo at one thread and pipeline fifty is 0.22. Each is one cell in an otherwise steady row, and each is contention at the shallowest depth or the deepest rather than a property of the engine. Do not quote a bar from any of them.

Note that the numbers in this table are not comparable with the ones in the version of this file published before, because most of the cells that were loose there no longer exist and the ones that remain were computed over the runs that survived rather than over all five.

## What can be said about yo here, and what cannot

Only the single threaded column survives, so this directory says nothing at all about yo above one I/O thread. The question it was swept to answer, whether yo 0.3.28 closed the spread that yo 0.3.26 had at eight and sixteen threads, is not answered here. It cannot be answered by a directory where those cells have no measurements in them.

At one I/O thread yo is second of eight on GETs at pipeline one, at 393 thousand a second against Pogocache's 444 and Memcached's 336, and its GET p99 of 0.999 ms is the second lowest on the board behind Pogocache's 0.991. It falls back as the pipeline deepens, to third at depth ten, sixth at depth twenty five and fifth of the seven engines still plotted at depth fifty, and its SET rate falls further than its GET rate does: at depth twenty five it does 1397 thousand GETs a second and 903 thousand SETs, where Pogocache does 1799 and 1865. A gap that opens between the two commands as the depth grows is worth somebody's attention and it is the one durable thing in this directory about yo.

The steadiness at one thread is real: 0.01, 0.05, 0.11 and 0.22 across the four depths. The 0.22 at depth fifty is the loosest of them and is the one cell of yo's not to quote.

## What else is in here

`rugo` 0.1.0 is unchanged from the previous sweep. It fails the same check in every one of its eight multithreaded cells, so it has the same problem yo has, with no fix behind it. Its four single threaded cells survive, and three of them are steady, but the pipeline one cell varies by 0.15 and is not one to quote. On the three deeper depths at one thread it is the fastest engine on the board.

Garnet lost its two pipeline fifty cells at one and eight threads and kept the rest. It is the one engine that fails the check at one I/O thread, which is worth noting because at one thread there is no distribution of connections across server threads to be uneven. Its GET p99 at one thread and pipeline twenty five is 62 ms against every other engine's 3 to 16, so whatever is happening there is visible in the latency as well.

## What this profile gives up

`wsl32coarse` is the `wsl32` box and the `wsl32` working set sampled coarsely, so a number exists before the full matrix finishes. Three thread counts instead of six and five runs a cell instead of thirty one. The thread axis of every chart here has three points on it, and now has one point on it for two of the engines, so the curve between them is drawn by the chart and not measured, and reading a shape into it is reading more than is there.

This host has no hardware PMU, so cycles per operation was never measured. That is why there are 146 charts here rather than 154, and why the cycles sections of the indexes name what is missing instead of showing it.

Five runs a cell is a small sample, so the spread figures above are themselves coarse. They separate an engine that is steady from one that is not, and they do not support finer statements than that. One cell, Garnet at one thread and pipeline twenty five, survived the recheck with three runs rather than five, which is coarser still.

## How this sweep was taken

In sessions across two days rather than in one pass, because the box had other work on it for part of that time and the sweep waits for a quiet machine rather than measuring through somebody else's load. The restartable sweep is what makes that safe: a cell is measured once and its file is kept, so a session that is interrupted costs the cell it was in the middle of and nothing else. All 480 runs come from one build of each engine, and the version strings in `README.md` are the ones each server printed when it was started for these runs.

The refused runs were not deleted. They are under `runs/refused` in [the archive that goes with this directory](https://github.com/tamnd/cache-bench/releases/tag/results-wsl32coarse-20260909), with the load generator output that refused them, so anybody who thinks the quarter is the wrong line can find out what a different one would have kept. That archive is 4636 files: the 397 runs the charts come from and the 312 reductions taken off them, four a cell, the 83 runs that were refused, and all 3841 log files, which are the bytes memtier and the servers wrote and the only reason a check could be applied to this sweep a month after it was measured. `cache-bench archive --check` reads it back against the manifest inside it.
