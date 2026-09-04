# axis-vectors

Produces `crates/cb-core/golden/axes.json`, which is where the original put everything that `series.json` says is on a chart.

`series-vectors` keeps the top of each generated Python script, which is the data. Everything below it is matplotlib, and that part is not none of our business after all, because it decides the bounds of the y axis, which ticks get labels, where the gridlines go and what text sits beside each one. That arithmetic is about forty lines of Python living inside two Go string constants, it is never written down anywhere, and reading it and reimplementing it is exactly the kind of work that produces something which looks right and is a decade off at the top.

So the same trick again, one layer further in. This runs the original's own script and stands in for the thing the script draws with.

```
tools/axis-vectors/capture.py /path/to/cache-benchmarks
```

It slices `BarScriptLogarithmic` and `BarScriptLinear` straight out of `cmd/graph/main.go`, so nothing here is a retyped copy of the original and a change on that side shows up as a changed fixture rather than as agreement with a stale transcription. It splits each on the marker the original uses to separate the data header from the body, installs fake `matplotlib`, `matplotlib.pyplot`, `matplotlib.colors`, `PIL.Image` and `PIL.ImageOps` modules that record what they are told instead of drawing, and then executes the body once per chart with the data bound from `series.json`.

numpy is real, because `linspace` and `arange` are two of the places where an obvious reimplementation is subtly wrong. `to_rgb` is real for the same reason. Needs Python and numpy and a checkout of the original. Nothing in CI runs it and the output is committed.

## What comes out

Two blocks. `constants` is everything the original applies to every chart without looking at what is on it, and the capture refuses to write unless all 154 charts agree on each of those values, so the block is an assertion rather than a sample. `charts` is one entry per chart with the axis bounds, the ticks and their labels, the gridlines, the margin text, the thread counts and the legend.

Bar heights are deliberately not in it. They are already in `series.json`, and carrying them twice makes the fixture five times bigger and gives two places for the same number to be wrong in. Instead the capture cross checks every recorded bar against `series.json` as it goes and stops if any of them disagree, which is the same check done once at generation time rather than on every test run.

## Three things it turned up

The x tick offset is hardcoded at `width * 2.5`. With six cache servers that is the middle of the group and the thread count sits under it correctly. With any other number of servers it does not, so a seventh engine moves every x label off centre. That is D13.

The quarter decade gridlines step by `0.25 / 2`, which is an eighth of a decade, so there are seven of them between one labelled tick and the next rather than three.

The two `case_1` charts are the only ones with a zero in them, and both are linear. That is why a zero has never reached the original's logarithmic path, where it would be an error rather than a chart.
