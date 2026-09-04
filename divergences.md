# Divergences from the original

Every place this port does something the original does not, and why. If you are comparing a chart here against a chart at [tidwall/cache-benchmarks](https://github.com/tidwall/cache-benchmarks), this is the list of reasons two numbers that should agree might not.

The rule for this file is that a divergence is either listed here or it is a bug. Nothing gets to be an undocumented improvement.

## D1 to D4, the statistics

The original selects one run out of the 31 it made, per cell, four times over, for median, best, worst and mean. All four selections have a defect. What follows is read off `cmd/choose/main.go` rather than inferred from the output, and then checked against the output.

The numbers quoted are the original's own committed data for dragonfly at one thread and pipeline depth 1. Both fixtures are in `testdata/golden/cells`, each holding the 31 runs and the four files the original reduced them to, so every claim below can be re-derived from files in this repository. Ranks are positions in the 31 runs sorted ascending, counting from zero.

**D1, the median is off by one inside the trimmed window.** With 31 runs and 10 percent trimmed from each end the surviving window is 25 wide, so the middle of it is the 13th, at index 12. The original computes the index as the window length divided by two plus one, which is 13, so it reads the 14th. This is one position out on a sorted list of near identical numbers, which is why nobody has ever noticed it.

The published GET median is 218689.49 where the middle run is 218545.08, and in the cell without perf it is 217513.54 where the middle run is 217454.15. Rank 16 both times where rank 15 is the median. The error is small and the direction is not: it is one run too fast, in every cell, on every chart, for every engine, which is the kind of bias that survives averaging.

**D2, the SET results get re-sorted by a perf comparator.** The selection sorts the GET results by GET throughput and the SET results by SET throughput, and then sorts the SET results a third time using a comparator that reads cycles out of the perf list.

```go
sort.Slice(sets, func(i, j int) bool {
	return perf[i].Get("cycles").Int() > perf[j].Get("cycles").Int()
})
```

It permutes `sets` while reading `perf`, so the SET rankings that survive are ordered by an unrelated quantity and the correct sort above it is thrown away. It is not random, which is worse: it is systematically the wrong run.

The published SET median for the perf cell is 197123.77, which is rank 7, the 8th slowest run of 31. The middle run is 198278.65. That is not a median with an error in it, it is an arbitrary run wearing the label.

The cell without perf escapes by accident. Every `cycles` there is absent, `Int()` returns 0 for all of them, the comparator is false for every pair, and the sort leaves an order it cannot distinguish alone. The SET median there lands at rank 16, which is D1 and nothing worse. So the perf half of the matrix carries a defect the other half does not, and the two halves of the same chart set are not computed the same way.

**D3, the perf results are never sorted.** That third sort was clearly meant to order the perf list and instead orders the SET list, so the perf list stays in the order the files were read. The cycles selection then indexes into it as if it had been ordered.

The published median cycles is 640031542073, which is exactly run 17's, and run 17 is rank 11. The middle run is 640978543265. The published best is run 23's count and the published worst is run 3's, and neither is the highest or the lowest of anything. They are whichever runs landed on those indices in file order. Cycles per operation is the y axis of a fifth of the charts.

**D4, the run count is a mutated global.** The count is a package level variable that the trimming step decrements in place, and the four selections run one after another against it. So the first sees 31 runs and trims to 25, the second sees 25 and trims to 21, the third sees 21 and trims to 17, and the fourth sees 17 and trims to 15. Only the first of the four trims the amount the code says it trims.

The loop that reads the run files is bounded by the same variable, so it is worse than a trimming error. `best` only ever opens runs 1 through 25, and runs 26 through 31 are measured, written to disk and never read by three of the four aggregates. If the fastest run of a cell is run 29 then the published best cannot be it.

In this cell the fastest run is run 8, so the unread files do not bite and the shrinking trim does something subtler instead. The published best is the third fastest of the 25 runs it read, where the corrected best is the fourth fastest of all 31, and the published number is the higher of the two. Trimming less off the top is not conservative. Average is the extreme case: 17 files read, 15 surviving the trim, and a mean over those 15 labelled as the average of a cell that was measured 31 times.

D4 is checkable on its own. Simulating the file counts, the shrinking trim and the index arithmetic, with no sorting defect involved, reproduces all four of the original's published GET numbers and all four of its published cycles numbers exactly, for both cells. SET is the only series that also needs Go's sort ported.

All four are corrected here. The corrected behaviour is the default. Each series is sorted by its own key, the trim is ten percent of 31 computed once for all four aggregates, and the median is the middle of the window. Passing `--compat=upstream` reproduces all four defects exactly, which is how the original's published output can still be regenerated byte for byte, and how the parity test in `cache-bench verify` proves the port is faithful before it starts being better.

This means our aggregate charts will not equal the original's aggregate charts even on identical input. That is the point, and it is the divergence most likely to surprise somebody.

One thing that is not a divergence: the three series are selected independently of each other, here as in the original. SET, GET and the counters are three measurements taken in sequence, and the run with the median SET throughput has no reason to be the run with the median GET throughput. A chosen file also stays a run rather than a summary, so the median is the run whose throughput is the median, reported whole with its own latencies. Taking a median of each field on its own would give a row holding the throughput of one run and the p99 of another, where every number is defensible and the row describes nothing that happened.

## D5, an added spread object

Each chosen file gains a `spread` object carrying the interquartile range, the standard deviation and the coefficient of variation for both throughput series, and for cycles when the cell has counters. Nothing plots it. It exists because a run disturbed by something else on the box looks exactly like a real result once it has been reduced to a single number, and the coefficient of variation is the cheapest way to see that a cell should not be trusted. Above a percent or two in these cells means something else was running.

It covers all 31 runs including the ones the trim drops, because the trim is there to keep an outlier out of the chosen number rather than to pretend it was never measured. Additive and absent in `--compat=upstream`, so an upstream consumer of the JSON is unaffected.

## D6, fonts

The original draws with Futura and Verdana, which are not redistributable and which only resolve on macOS. This port embeds Jost, a metric compatible open alternative to Futura, and DejaVu Sans in place of Verdana. Everything else about the chart geometry, the colours, the axis treatment and the legend is the original's, so charts are recognisably the same charts with different letter shapes.

The reason it matters beyond aesthetics is determinism. A chart drawn against whatever font the host happened to have is a chart nobody else can reproduce, and the PNG hash manifest in `testdata/` only means something if the fonts are in the binary.

## D7, a --threads flag on yo

`yo` sizes its thread pool from `available_parallelism` and has no flag to override it. The x axis of every chart in this project is a thread count, so without that flag `yo` cannot be plotted at all. The flag is a change to `tamnd/yo`, not to this repository, and it goes there as its own pull request on that project's terms.

## D8, hardware profiles

The original hardcodes a 32 core box throughout: core pinning, thread sweep, memory limit and client count are all constants in the driver script and the Go source. Here they are data in `profiles.toml`, every result file records which profile produced it, and every chart is stamped with it. This is not an improvement to the methodology, it is the only way to run the thing on a machine that is not an AWS c8g.8xlarge.

`--key-maximum` is explicit in every profile rather than left at memtier's default of ten million. The original relies on the default and on a 32 GB memory limit chosen so that nothing is ever evicted. Shrinking the limit for a smaller box without shrinking the key space turns the benchmark into an eviction benchmark, and the engine with the cleverest eviction policy wins a contest nobody entered.

## D9, no Python

The original charts with matplotlib through a generated script. Here the chart engine is a Rust crate built on plotters, so the chart layer has tests, golden series and a PNG hash manifest, and a fresh checkout draws a byte identical chart on Linux, macOS and Windows.

## D10, an added subject

`yo` is a seventh cache server that the original does not have. Pogocache, which is the original author's own engine, stays in. Dropping it would mean this is no longer a reproduction of the benchmark but a modified one that happens to omit the engine the original was written to showcase, and the fairness rules in the spec exist precisely so that adding our own engine does not become an excuse to tilt anything.

## D11, unsupported perf counters

A counter the machine cannot measure comes out of `perf stat` as the text `<not supported>`, and the original pulls it through a JSON accessor that returns zero for anything it cannot parse as a number. The zero then reaches a chart, where it is a bar of height nothing, claiming the engine took no branches. Our model keeps the distinction and the chart layer leaves such a cell out rather than drawing it as a zero. In `--compat=upstream` the zero is written as the original writes it, because the parity proof needs those bytes.

## D12, Dragonfly's memory limit

Every server in the original is given 32 GB except Dragonfly, which is given 31. The limit is computed rather than written down: the thread count times 256 megabytes, floored at 32384, then divided by 1024 to get gigabytes. That last division is integer arithmetic, and 32384 over 1024 is 31 and a bit, so the answer is 31 for every thread count the sweep uses. The floor is what the formula was for, and the unit conversion is what defeated it.

A gigabyte in 32 is not going to move a throughput number when the working set is about six gigabytes and nothing is ever evicted. It is here because Dragonfly is the one engine in the set running under a limit nobody chose, and a reader comparing engines deserves to know that one of them was configured by an arithmetic accident.

Here the profile's `maxmemory` goes to all seven servers unchanged. In `--compat=upstream` the formula is reproduced, arithmetic and all, rather than the 31 it happens to produce.
