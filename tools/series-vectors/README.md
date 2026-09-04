# series-vectors

Produces `testdata/golden/series.json`, which is what the chart layer is tested against.

The original's `graph` tool decides what goes on a chart, pastes the numbers into a Python script, runs `python3` on it and then deletes the script. The PNG is the only thing that survives, and a PNG is not something a test can compare against. So this stands in for Python: a shell script on `PATH` named `python3` that copies the script it was handed and draws nothing. The Go side sees a successful render and carries on, and what lands in the fixture is the original's own answer for all 154 charts rather than a description of one.

```
tools/series-vectors/capture.sh /path/to/cache-benchmarks
```

It needs Go, a `python3` for the assembling step, and a checkout of the original with `results/output.json` in it. Nothing in CI runs it and the output is committed, because a fixture that moves is not a fixture.

The loop is `bench-all.sh`'s loop, in its order, plus the two charts the original adds by hand afterwards. If the capture comes back with anything other than 154 scripts it stops rather than writing a short fixture.

What gets kept is the header block at the top of each script, which is the title, both axis labels, the thread counts, the colour per cache server and one number per bar. Everything below it is matplotlib and is none of our business, since this project draws the picture itself.

`--force` is the one flag the original's own script does not pass. Without it `graph` skips any chart whose PNG is already sitting in `results/graphs`, which in a checkout of the original is all of them.
