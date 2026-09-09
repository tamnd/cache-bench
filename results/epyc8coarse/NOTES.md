# Notes on this sweep

Written by hand. Everything in `README.md` next to this file is generated from the data in this directory, which is what makes it checkable, and it is also why it cannot say any of what is below. This is what is true of these numbers in particular.

## Read this first

This box is not quiet enough to quote a single bar from. That is the first thing to know about this directory and it is not a small caveat.

The coefficient of variation over the runs of a cell, on GETs, one row per engine and one column per cell. `none` is a cell with no bar in it.

| engine | t1 p1 | t1 p10 | t1 p25 | t1 p50 | t2 p1 | t2 p10 | t2 p25 | t2 p50 | t4 p1 | t4 p10 | t4 p25 | t4 p50 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Dragonfly | 0.10 | 0.18 | 0.14 | 0.04 | 0.25 | 0.09 | 0.12 | 0.02 | 0.19 | 0.14 | 0.10 | 0.08 |
| Garnet | 0.09 | 0.22 | 0.17 | 0.06 | 0.07 | 0.11 | 0.12 | 0.14 | 0.12 | 0.10 | 0.39 | 0.02 |
| Memcached | 0.06 | 0.10 | 0.09 | 0.09 | 0.05 | 0.09 | 0.06 | 0.12 | 0.08 | 0.10 | 0.07 | 0.07 |
| Pogocache | 0.07 | 0.10 | 0.06 | 0.13 | 0.18 | 0.05 | 0.18 | 0.12 | 0.05 | 0.15 | 0.12 | 0.10 |
| Redis | 0.09 | 0.04 | 0.13 | 0.06 | 0.25 | 0.15 | 0.15 | 0.16 | 0.14 | 0.15 | 0.17 | 0.05 |
| rugo | 0.05 | 0.06 | 0.15 | 0.07 | 0.06 | none | 0.12 | 0.31 | 0.13 | 0.10 | 0.06 | 0.04 |
| Valkey | 0.11 | 0.05 | 0.06 | 0.09 | 0.05 | 0.13 | 0.23 | 0.09 | 0.14 | 0.07 | 0.12 | 0.04 |
| yo | 0.20 | 0.05 | 0.15 | 0.07 | 0.07 | 0.08 | 0.08 | 0.06 | 0.06 | 0.17 | 0.17 | 0.28 |

`cache-bench spread` puts ninety one of the ninety five plotted cells above 0.05, which is the line it holds a cell to. The table above is the GETs half of that and its median is 0.10. Compare the same table for the 32 core host, where five of the eight engines sit between 0.00 and 0.09 everywhere and eighteen of seventy eight cells are over the line. There is no engine here with a steady row and no cell that stands out as the exception, which is what says this is the machine rather than the engines.

The runs of a cell are consecutive and minutes apart, so this is not a sweep whose five runs straddle two different afternoons. It is a box with other people's work on it, and it varies by a tenth from one minute to the next whatever is running.

The sweep waits for a quiet machine, and 65 runs that were measured before that wait was tight enough were set aside on the host rather than published. Comparing those against the runs that replaced them is worth doing because it says how much the waiting bought: across the thirteen cells that were measured both ways, the median moved by about a tenth, and it moved up as often as down. So the filter did not clean anything up. A tenth is what a run costs on this box, and waiting for a quiet load average does not buy it back.

## What can still be read here

Two things survive a spread of a tenth.

A ratio measured inside one engine across configurations it was swept at, because the same statement then has to hold four times, once per pipeline depth, before it is believed. That is what the yo section below rests on and none of it depends on a single cell.

And a gap between two engines that is wider than the two ranges put together. Of the 329 engine pairs the twelve throughput charts put side by side, 186 have ranges that do not overlap at all and 143 do. So a little under half of the ordering in any chart in this directory is inside the noise, and reading a bar as beating the one next to it is wrong more often than a coin toss would be.

Do not quote a number from this directory as a rate. Quote the ratios.

## What is missing from the charts, and why

One of the ninety six cells is empty, rugo at two threads and pipeline ten, which was left with two runs where three is the fewest a median, a best, a worst and an average can be four different numbers over.

Fourteen of the 480 runs were refused by [D25](../../divergences.md#d25-a-rate-that-has-to-cover-the-run-it-is-a-rate-for), which asks whether the load generator's threads finished close enough together for the rate beside them to be a rate over the run. Ten of those are rugo, two are yo and two are Garnet, and no other engine lost a run. `failures.json` names every one of them and what refused it.

That is a much lighter refusal than the same check made of the 32 core host, where 83 runs went and two engines lost their whole multithreaded half. The difference is the load: this profile offers 64 connections over 4 load generator threads where that one offers 256 over 16. An accept split that is badly uneven at 256 connections is only slightly uneven at 64, so this host measures the engines that host could not.

## What can be said about yo here

Two things, both of them about threads, and neither of them inside the noise.

The first is that yo has the fastest or nearly the fastest single thread on this box and the worst use of a second and a fourth. GETs, median of the kept runs, with each engine's own one to four thread ratio beside it:

| engine | t1 p25 | t2 p25 | t4 p25 | t4 over t1 |
| --- | --- | --- | --- | --- |
| yo | 406838 | 525914 | 595110 | 1.46 |
| Garnet | 384194 | 650030 | 581695 | 1.51 |
| Pogocache | 344715 | 564728 | 735340 | 2.13 |
| rugo | 342935 | 698033 | 740237 | 2.16 |
| Valkey | 311601 | 311651 | 396827 | 1.27 |
| Redis | 300791 | 376287 | 363090 | 1.21 |
| Memcached | 232238 | 429856 | 597782 | 2.57 |
| Dragonfly | 90784 | 216924 | 347083 | 3.82 |

yo leads the single thread column and finishes fourth of eight at four threads. The ratio holds at every depth: 1.26, 1.36, 1.46 and 1.31 across pipeline 1, 10, 25 and 50, against 1.83 to 2.57 for Memcached, Pogocache and rugo. Redis is at 1.05 to 1.39 and Redis does not thread its data path at all, so yo is nearer to an engine that does not use the extra cores than to one that does.

The second is heavier. yo's SET rate does not respond to I/O threads or to pipeline depth on this box, and every other engine's does. SET ops per second, median of the kept runs:

| engine | t1 p1 | t1 p50 | t2 p50 | t4 p50 |
| --- | --- | --- | --- | --- |
| Garnet | 49094 | 510787 | 722225 | 905493 |
| Pogocache | 66477 | 343497 | 606943 | 820890 |
| rugo | 79138 | 271795 | 429953 | 612845 |
| Dragonfly | 48514 | 137513 | 250479 | 463660 |
| Valkey | 59320 | 271182 | 322786 | 326542 |
| Redis | 58456 | 314253 | 297844 | 320961 |
| Memcached | 49494 | 135880 | 236713 | 301509 |
| yo | 68640 | 132440 | 131155 | 131777 |

yo is second of eight at one thread and pipeline one and then sits between 111 thousand and 140 thousand a second for the whole of the rest of the grid, at every thread count and every depth from ten upward. Its best SET cell anywhere is 190 thousand, at one thread and pipeline ten, and adding threads to that cell makes it worse rather than better: 190, then 111, then 132. At four threads and pipeline fifty it does 132 thousand where Garnet does 905 thousand, which is a factor of six and is not a number the spread can explain.

A ceiling that does not move with threads and does not move with depth is a serialised write path. The GET number is off the same shape from the other side: reads scale, but only by a third where the field gets double.

This is not the number the 32 core host produced, where yo's single thread SET rate at depth twenty five was 903 thousand a second. Whether that is a difference between the two machines or between the two working sets is not answered here, and it is worth answering before anything is concluded about the absolute figure. What the two hosts agree on is the shape: on both of them yo's SET rate falls away from its GET rate as the pipeline deepens, and that gap was already the one durable finding about yo on the other box.

## What this profile gives up

`epyc8coarse` is the `epyc8` box and the `epyc8` working set sampled coarsely, so a number exists before the full matrix finishes. Three thread counts instead of six and five runs a cell instead of thirty one. The thread axis of every chart here has three points on it, so the curve between them is drawn by the chart and not measured, and reading a shape into it is reading more than is there.

This host has a hardware PMU and a live counter answered when it was asked, which is what this box was wanted for and is the thing neither published directory has yet. This profile still sweeps without counters attached, so there are 146 charts here rather than 154 and the cycles sections name what is missing instead of showing it. Attaching them doubles the matrix, so it belongs to a sweep run for that purpose rather than to the draft.

Five cells sit on four runs and three on three, because of the refusals above, and the one left with two is the empty cell named earlier. Five runs a cell is already a small sample and three is smaller, so the spread figures are themselves coarse, and on a box this loose they separate nothing at all.

## How this sweep was taken

In sessions across two days rather than in one pass, because the box has other work on it and the sweep waits for a quiet machine rather than measuring through somebody else's load. The restartable sweep is what makes that safe: a cell is measured once and its file is kept, so a session that is interrupted costs the cell it was in the middle of and nothing else. The first run started at 18:05 on the eighth and the last at 16:49 on the ninth. All 480 runs come from one build of each engine, and the version strings in `README.md` are the ones each server printed when it was started for these runs.

The refused runs were not deleted. They are under `runs/refused` in [the archive that goes with this directory](https://github.com/tamnd/cache-bench/releases/tag/results-epyc8coarse-20260910), with the load generator output that refused them. That archive is 4705 files: the 466 runs the charts come from and the 380 reductions taken off them, four a cell, the 14 runs that were refused, and all 3841 log files, which are the bytes memtier and the servers wrote. `cache-bench archive --check` reads it back against the manifest inside it, and `cache-bench recheck` can judge this sweep against a check written after it was measured without measuring anything again, which is the whole reason the logs are kept.
