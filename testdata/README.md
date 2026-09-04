# Test data

Committed inputs and expected outputs, so that the parts of this project that
are hard to get right can be tested without a cache server, a load generator or
a PMU anywhere near them.

`golden/` holds real `memtier_benchmark` and `perf stat` output captured from an
actual run, the result files derived from it, and the `output.json` that the
aggregation produces. The statistics tests read these rather than synthesising
numbers, because the failures worth catching are the ones that come from what
these tools actually print, not from what the parser's author assumed they
print.

`manifest.json` holds a SHA-256 for every chart drawn from the golden data. CI
redraws all of them and fails if any hash moves. When a hash does move it is
usually a patch bump to the chart stack changing anti aliasing, which is exactly
what the manifest exists to make visible. The fix is to look at the visual diff
first and update the manifest in its own commit, never to regenerate it as part
of some other change.

Both land with M6.
