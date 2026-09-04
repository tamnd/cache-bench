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

The whole corpus is now measurable rather than arguable, and `cache-bench verify --against` is the command that measures it, so every number in the next two paragraphs is output rather than assertion. Reducing the original's 20160 committed run files in upstream mode reproduces all 2304 of its chosen files byte for byte, and combining them reproduces its published `output.json` byte for byte, so the same directory reduced in corrected mode is a like for like comparison of the statistics and of nothing else. Across all 576 cells the typical median is out by a tenth of a percent on GET and a quarter of a percent on SET, which is small, and the tail is not: the worst median SET is Garnet at 8 threads and pipeline 50 with counters attached, published at 19.86 million operations per second where the median of its 31 runs is 12.30 million, 61 percent too high. The worst median GET is 19 percent.

The direction is the part that matters. The published median GET is higher than the true median in 576 of 576 cells. Not most of them, all of them, on every engine at every thread count and every pipeline depth, which is what a one sided index error looks like from a distance. An error that size in one cell is noise. The same error with the same sign in every cell is a chart that reads slightly fast everywhere, and averaging more cells together makes it worse rather than better.

All four are corrected here. The corrected behaviour is the default. Each series is sorted by its own key, the trim is ten percent of 31 computed once for all four aggregates, and the median is the middle of the window. Passing `--compat=upstream` reproduces all four defects exactly, which is how the original's published output can still be regenerated byte for byte, and how the parity test in `cache-bench verify` proves the port is faithful before it starts being better.

This means our aggregate charts will not equal the original's aggregate charts even on identical input. That is the point, and it is the divergence most likely to surprise somebody.

One thing that is not a divergence: the three series are selected independently of each other, here as in the original. SET, GET and the counters are three measurements taken in sequence, and the run with the median SET throughput has no reason to be the run with the median GET throughput. A chosen file also stays a run rather than a summary, so the median is the run whose throughput is the median, reported whole with its own latencies. Taking a median of each field on its own would give a row holding the throughput of one run and the p99 of another, where every number is defensible and the row describes nothing that happened.

## D5, an added spread object

Each chosen file gains a `spread` object carrying the interquartile range, the standard deviation and the coefficient of variation for both throughput series, and for cycles when the cell has counters. Nothing plots it. It exists because a run disturbed by something else on the box looks exactly like a real result once it has been reduced to a single number, and the coefficient of variation is the cheapest way to see that a cell should not be trusted. Above a percent or two in these cells means something else was running.

It covers all 31 runs including the ones the trim drops, because the trim is there to keep an outlier out of the chosen number rather than to pretend it was never measured. Additive and absent in `--compat=upstream`, so an upstream consumer of the JSON is unaffected.

## D6, fonts

The original draws with Futura and Verdana, which are not redistributable and which only resolve on macOS. This port embeds Jost, a metric compatible open alternative to Futura, and DejaVu Sans in place of Verdana. Everything else about the chart geometry, the colours, the axis treatment and the legend is the original's, so charts are recognisably the same charts with different letter shapes.

Three faces, because the original asks for three. Jost Book stands in for Futura at its normal weight, which is the tick labels and the legend. Jost Bold stands in for the bold the original asks matplotlib for on the title and both axis labels, and it is a second file rather than a synthesised weight, because a smeared regular is not the same shape on two machines. DejaVu Sans stands in for the one place the original names a second family, the eight point gray quarter decade numbers in the margin of a logarithmic chart.

DejaVu Sans is a wider face than Verdana at the same size, so those margin numbers take more room here than they do there. They sit in the space to the left of the axis and nothing else is competing for it, so nothing moves as a result, but it is the one place where a chart here and a chart there differ in layout rather than only in letter shapes.

The reason any of this matters beyond aesthetics is determinism. A chart drawn against whatever font the host happened to have is a chart nobody else can reproduce, and the PNG hash manifest in `testdata/` only means something if the fonts are in the binary. `assets/fonts/README.md` records the exact release each file came from, and the digest of all three is written into `crates/cb-chart/src/font.rs` where a test checks it, so a font cannot be replaced without the commit that does it saying so.

## D7, a --threads flag on yo

`yo` sizes its thread pool from `available_parallelism` and has no flag to override it. The x axis of every chart in this project is a thread count, so without that flag `yo` cannot be plotted at all. The flag is a change to `tamnd/yo`, not to this repository, and it goes there as its own pull request on that project's terms.

## D8, hardware profiles

The original hardcodes a 32 core box throughout: core pinning, thread sweep, memory limit and client count are all constants in the driver script and the Go source. Here they are data in `profiles.toml`, every result file records which profile produced it, and every chart is stamped with it. This is not an improvement to the methodology, it is the only way to run the thing on a machine that is not an AWS c8g.8xlarge.

`--key-maximum` is explicit in every profile rather than left at memtier's default of ten million. The original relies on the default and on a 32 GB memory limit chosen so that nothing is ever evicted. Shrinking the limit for a smaller box without shrinking the key space turns the benchmark into an eviction benchmark, and the engine with the cleverest eviction policy wins a contest nobody entered.

## D9, no Python

The original charts with matplotlib through a generated script. Here the chart engine is a Rust crate that lays out the axes and draws the pixels itself, so the chart layer has tests, golden series and a PNG hash manifest, and a fresh checkout draws a byte identical chart on Linux, macOS and Windows.

Drawing the pixels ourselves is not enthusiasm for writing a rasterizer. The two chart libraries in Rust that could have done it both take their text from somewhere we cannot control. plotters with its `ttf` feature resolves system fonts through font-kit, which means a chart drawn on this laptop and a chart drawn on a CI runner are two different pictures, and that is exactly the non determinism this project exists to remove. Its `ab_glyph` feature avoids that but sits on ttf-parser, which cargo-deny already fails the build on under RUSTSEC-2026-0192. What is left is small: the shapes on these charts are axis aligned rectangles, which have an exact coverage per pixel with no sampling involved, and glyph outlines, which skrifa reads out of the two font files committed here and zeno fills in scalar f32 with no SIMD path to diverge on. Every arithmetic step is fixed order f64 or f32, so byte identical output across platforms is a property of the code rather than a hope about somebody else's crate.

## D10, an added subject

`yo` is a seventh cache server that the original does not have. Pogocache, which is the original author's own engine, stays in. Dropping it would mean this is no longer a reproduction of the benchmark but a modified one that happens to omit the engine the original was written to showcase, and the fairness rules in the spec exist precisely so that adding our own engine does not become an excuse to tilt anything.

The seventh subject needs a seventh bar colour. The original has six, assigned by position in the sorted list of names rather than by which engine it is, and that rule is kept because a chart drawn here and a chart drawn there have to be comparable. It works out because `yo` sorts last, so the added colour goes on the end and the original's six land exactly where they were. It is purple, from the same matplotlib cycle five of the other six come from.

## D11, unsupported perf counters

A counter the machine cannot measure comes out of `perf stat` as the text `<not supported>`, and the original pulls it through a JSON accessor that returns zero for anything it cannot parse as a number. The zero then reaches a chart, where it is a bar of height nothing, claiming the engine took no branches. Our model keeps the distinction and the chart layer leaves such a cell out rather than drawing it as a zero. In `--compat=upstream` the zero is written as the original writes it, because the parity proof needs those bytes.

A cell that was never measured takes the same route to the same place. The chart layer asks the results file for one cache server at one thread count, the accessor finds nothing and hands back zero, and a sweep that was interrupted draws an engine that scored nothing rather than an engine that was not tested. Here that bar is not drawn. It matters more on the logarithmic charts than on the linear ones, because the y axis there is scaled from the smallest bar on the chart and one zero takes the whole axis with it.

## D12, Dragonfly's memory limit

Every server in the original is given 32 GB except Dragonfly, which is given 31. The limit is computed rather than written down: the thread count times 256 megabytes, floored at 32384, then divided by 1024 to get gigabytes. That last division is integer arithmetic, and 32384 over 1024 is 31 and a bit, so the answer is 31 for every thread count the sweep uses. The floor is what the formula was for, and the unit conversion is what defeated it.

A gigabyte in 32 is not going to move a throughput number when the working set is about six gigabytes and nothing is ever evicted. It is here because Dragonfly is the one engine in the set running under a limit nobody chose, and a reader comparing engines deserves to know that one of them was configured by an arithmetic accident.

Here the profile's `maxmemory` goes to all seven servers unchanged. In `--compat=upstream` the formula is reproduced, arithmetic and all, rather than the 31 it happens to produce.

## D13, the x tick offset

Under each group of bars the original writes the thread count, and it places that label at `width * 2.5` from the left edge of the group. Six bars of width `width` make a group `6 * width` wide, whose middle is at `width * 3`, so the label is half a bar to the left of centre. It looks centred because the bar the label is under is drawn from its own left edge, and the middle of the last bar in a group of six is exactly `width * 2.5`. The number is right for six bars by construction and for no other count.

We draw seven. Keeping the constant would push every thread count on every chart half a bar off the group it names, which is a visible fault on a chart nobody would think to check, so here the offset is the middle of however many bars there are. At six the two expressions produce the same number, so nothing the original published moves.

`Bars::upstream_xtick` keeps the original's expression and a test asserts the two agree at six and disagree at seven, which is what makes this a divergence that only shows up once a seventh engine is on the chart.

## D14, one canvas size

The original asks matplotlib for a figure of a fixed size and then saves it with `bbox_inches='tight'`, which crops the image to whatever the drawing turned out to need. What it needed depends on how wide the widest y axis number is, so the 154 published PNGs come in three sizes. 112 of them are 1715 pixels wide, 21 are 1716 and 21 are 1714, all 1038 tall. Nobody chose that and nothing reads it, but it means two charts cannot be flipped between without everything on them shifting a pixel or two sideways.

Here every chart is 1880 by 1130, which is the figure size the original asks for at the resolution it asks for, plus the white border it adds afterwards on all four sides. The plot area sits at the same place on all 154, so two charts in a browser tab are comparable by switching between them, and a diff of two PNGs is a diff of the bars rather than of the crop. The cost is that the y axis label column is as wide as the widest number needs on any chart rather than on this one, which on a chart with short numbers leaves a little more white to the left of the axis than the original had.

## D15, the provenance stamp

The original's charts say what was measured and not where. A throughput number is meaningless without the machine it came off, and two of these charts from two machines are not comparable with nothing in the picture to say so, which is the most likely way for a chart drawn here to mislead somebody.

Every chart drawn from real measurements carries a line along the bottom naming the profile, what the machine is and its core count. Charts drawn from the golden series carry nothing, because that is what CI hashes and a hostname in the picture would make the hash depend on which machine drew it.

## D16, the chart indexes

The original maintains `LINEAR.md` and `LOGARITHMIC.md` by hand, including the `-1`, `-2` and `-3` suffixes GitHub appends to headings that repeat. Here both documents are generated by `cache-bench docs` from the same chart table the charts themselves are drawn from, and the suffixes come out of a counter that is fed the headings in the order they are written. Four things about the generated documents differ from the hand written ones.

The MIN and AVG latency sections are new. The original draws all eight of memtier's latency figures, which is 32 charts of MIN and AVG per results directory, and then links neither from either index. They are in the results directory, so they are in the index, after MAX where they read as a continuation of the latency run.

The two charts drawn with Garnet's single thread bar left off are linked from the P99 section they are a redraw of. The original links them only from its README, which leaves its two indexes covering 152 of the 154 charts it publishes.

A chart that was specified but not drawn is named rather than left out. A sweep on a machine with no hardware counters produces no cycles charts, and a section that quietly disappears reads as a chart set that was never meant to have one, which is exactly the sort of gap a reader should be told about.

Every image has a real alt text saying what is on it. The original writes `Alt text` on all 120 of its images, which is the placeholder out of the markdown documentation and tells a screen reader nothing.
