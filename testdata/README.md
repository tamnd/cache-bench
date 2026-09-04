# Test data

Committed inputs and expected outputs, so that the parts of this project that are hard to get right can be tested without a cache server, a load generator or a PMU anywhere near them.

`golden/` holds real `memtier_benchmark` and `perf stat` output captured from an actual run, and a handful of the result files derived from it. The tests read these rather than synthesising numbers, because the failures worth catching are the ones that come from what these tools actually print, not from what the parser's author assumed they print.

A handful and not all of them. The parity test needs the original's published `output.json`, which is 1.7 MB, and the golden fixtures for the whole matrix would be thousands of files. None of that is committed. Raw measurement data does not go in this repository at all, in `testdata/` any more than in `results/`: the fixtures here are the smallest set that makes a failure diagnosable, and the bulk data is fetched from the original at a pinned commit when the parity test runs.

`manifest.json` holds a SHA-256 for every chart drawn from the golden data. CI redraws all of them and fails if any hash moves. When a hash does move it is usually a patch bump to the chart stack changing anti aliasing, which is exactly what the manifest exists to make visible. The fix is to look at the visual diff first and update the manifest in its own commit, never to regenerate it as part of some other change.

Three result files are here now, taken unmodified from the original's committed results, and they are what the round trip test in `cb-core` reads. They were picked to cover the three shapes the format has: a run with no perf attached, a run with perf counters as JSON strings including a `<not supported>` one, and the median aggregate over those runs with the same counters as JSON numbers. The memtier and perf capture lands with M2 and the chart manifest with M3.
