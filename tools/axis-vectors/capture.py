#!/usr/bin/env python3
"""Records what the original's chart script asks matplotlib to draw.

The series fixture next to this one settles what goes on a chart. This settles where it goes: the y axis bounds, every tick and gridline, the label text on each of them, and the x offset of every bar in a group.

None of that is written down anywhere in the original. It is the result of about forty lines of Python in the middle of two Go string constants, and those forty lines are full of decisions that are only visible if you run them. So they are run, against the same 154 charts, with matplotlib replaced by something that writes down what it was told instead of drawing it.

The Python is not retyped here. It is sliced out of cmd/graph/main.go at the same marker the tool itself uses, so a transcription error is not one of the things that can go wrong.

Usage: tools/axis-vectors/capture.py /path/to/cache-benchmarks
"""

import copy
import json
import math
import os
import re
import sys
import types

import numpy as np

MARKER = "#" * 63


def templates(main_go):
    """The two chart scripts, without their header blocks."""
    src = open(main_go, encoding="utf-8").read()
    out = {}
    for scale, const in (("logarithmic", "BarScriptLogarithmic"), ("linear", "BarScriptLinear")):
        m = re.search(r"const %s = `(.*?)`" % const, src, re.S)
        if not m:
            raise SystemExit(f"no {const} in {main_go}")
        out[scale] = m.group(1).split(MARKER)[2]
    return out


def to_rgb(color):
    """matplotlib.colors.to_rgb for the one input the original gives it, a six digit hex string."""
    h = color.lstrip("#")
    return tuple(int(h[i : i + 2], 16) / 255 for i in (0, 2, 4))


class Label:
    """Stands in for a tick label. The original nudges these and asks nothing back."""

    def __init__(self):
        self.x = None
        self.y = None
        self.horizontalalignment = None

    def set_x(self, v):
        self.x = v

    def set_y(self, v):
        self.y = v

    def set_horizontalalignment(self, v):
        self.horizontalalignment = v


class Spine:
    def __init__(self):
        self.visible = True

    def set_visible(self, v):
        self.visible = v


class Axes:
    def __init__(self, record):
        self.record = record
        self.spines = {name: Spine() for name in ("left", "right", "top", "bottom")}
        self._x = [Label() for _ in range(64)]
        self._y = [Label() for _ in range(64)]

    def tick_params(self, **kw):
        self.record.setdefault("tick_params", []).append(kw)

    def get_xticklabels(self):
        return self._x

    def get_yticklabels(self):
        return self._y


class Pyplot:
    """Records the calls the original makes and returns whatever it reads back."""

    def __init__(self, record):
        self.record = record
        self.rcParams = {}
        self._axes = Axes(record)

    def figure(self, figsize=None):
        self.record["figsize"] = list(figsize)

    def bar(self, x, values, width=None, label=None, color=None, edgecolor=None, **kw):
        self.record["bars"].append(
            {
                "cache": label,
                "x": [float(v) for v in x],
                "heights": [float(v) for v in values],
                "width": float(width),
                "color": color,
                "edge": [float(c) for c in edgecolor],
            }
        )
        return [object()]

    def yscale(self, scale):
        self.record["yscale"] = scale

    def yticks(self, values, labels, fontsize=None):
        self.record["yticks"] = [
            {"value": float(v), "label": t} for v, t in zip(values, labels, strict=True)
        ]
        self.record["ytick_fontsize"] = fontsize

    def xticks(self, positions, labels, fontsize=None):
        self.record["xticks"] = {
            "positions": [float(p) for p in positions],
            "labels": [str(t) for t in labels],
            "fontsize": fontsize,
        }

    def ylim(self, bottom, top):
        self.record["ylim"] = [float(bottom), float(top)]

    def axhline(self, y, **kw):
        self.record["lines"].append(float(y))

    def text(self, x, y, s, **kw):
        self.record["gutter"].append({"x": float(x), "y": float(y), "text": s})

    def ylabel(self, text, **kw):
        self.record["ylabel"] = text

    def xlabel(self, text, **kw):
        self.record["xlabel"] = text

    def title(self, text, **kw):
        self.record["title"] = text

    def gca(self):
        return self._axes

    def grid(self, **kw):
        pass

    def legend(self, handles=None, labels=None, **kw):
        self.record["legend"] = list(labels)

    def tight_layout(self, rect=None):
        self.record["rect"] = list(rect)

    def savefig(self, filename, dpi=None, **kw):
        self.record["dpi"] = dpi


def install(record):
    """Put the stubs where the script's imports will find them."""
    mpl = types.ModuleType("matplotlib")
    pyplot = Pyplot(record)
    mpl_pyplot = types.ModuleType("matplotlib.pyplot")
    mpl_pyplot.__dict__.update(
        {k: getattr(pyplot, k) for k in dir(pyplot) if not k.startswith("_")}
    )
    mpl_colors = types.ModuleType("matplotlib.colors")
    mpl_colors.to_rgb = to_rgb

    image = types.ModuleType("PIL.Image")
    image.open = lambda path: object()
    ops = types.ModuleType("PIL.ImageOps")

    def expand(img, border=None, fill=None):
        record["border"] = border
        return types.SimpleNamespace(save=lambda path: None)

    ops.expand = expand
    pil = types.ModuleType("PIL")
    pil.Image = image
    pil.ImageOps = ops

    sys.modules.update(
        {
            "matplotlib": mpl,
            "matplotlib.pyplot": mpl_pyplot,
            "matplotlib.colors": mpl_colors,
            "PIL": pil,
            "PIL.Image": image,
            "PIL.ImageOps": ops,
        }
    )
    return pyplot


def run(script, chart):
    names = {
        "xseries": chart["xseries"],
        # Insertion order is what the original iterates, and it is the legend order.
        "data": {s["cache"]: s["points"] for s in chart["series"]},
        "colors": {s["cache"]: s["color"] for s in chart["series"]},
        "title": chart["title"],
        "ytitle": chart["ytitle"],
        "xtitle": chart["xtitle"],
        "filename": chart["file"],
        "fontfamily": "Futura",
        "np": np,
        "math": math,
    }
    exec(script, names)  # noqa: S102, the script is the original's own


def constant(records, pick, what):
    """The value of something the original writes the same way on all 154 charts, or a failure saying it is not."""
    seen = {json.dumps(pick(r)) for r in records}
    if len(seen) != 1:
        raise SystemExit(f"{what} is not the same on every chart: {sorted(seen)[:4]}")
    return pick(records[0])


def fold(records, series):
    """Turns the raw recordings into the fixture.

    Two things come out rather than one. Everything the original decided once and applied to all 154 charts becomes a block of constants, checked here to actually be constant, and everything that depends on the numbers on a chart stays per chart. The bar heights are dropped because they are the series fixture, which is checked here rather than copied.
    """
    by_file = {c["file"]: c for c in series}
    for r in records:
        chart = by_file[r["file"]]
        heights = {b["cache"]: b["heights"] for b in r["bars"]}
        for s in chart["series"]:
            if [float(p) for p in s["points"]] != heights[s["cache"]]:
                raise SystemExit(f"{r['file']} plots something the series fixture does not have")

    width = constant(records, lambda r: r["bars"][0]["width"], "the bar width")
    counts = {len(r["bars"]) for r in records}
    if counts != {len(series[0]["series"])}:
        raise SystemExit(f"the number of bars per group varies: {sorted(counts)}")

    logs = [r for r in records if r["scale"] == "logarithmic"]

    return {
        "constants": {
            "figsize": constant(records, lambda r: r["figsize"], "the figure size"),
            "dpi": constant(records, lambda r: r["dpi"], "the resolution"),
            "border": constant(records, lambda r: r["border"], "the white border"),
            "rect": constant(records, lambda r: r["rect"], "the layout rectangle"),
            "bar_width": width,
            # One step per bar in the group, which is what puts the group side by side.
            "bar_offsets": constant(
                records, lambda r: [b["x"][0] for b in r["bars"]], "the bar offsets"
            ),
            # The original writes this as width * 2.5, which is the middle of six bars and of no other number of them.
            "xtick_offset": constant(
                records, lambda r: r["xticks"]["positions"][0], "the x tick offset"
            ),
            "gutter_x": constant(logs, lambda r: r["gutter"][0]["x"], "the gutter position"),
            "font_size": {
                "title": 20,
                "axis_label": 18,
                "xtick": constant(records, lambda r: r["xticks"]["fontsize"], "the x tick size"),
                "ytick_logarithmic": constant(logs, lambda r: r["ytick_fontsize"], "the log y size"),
                "ytick_linear": constant(
                    [r for r in records if r["scale"] == "linear"],
                    lambda r: r["ytick_fontsize"],
                    "the linear y size",
                ),
                "legend": 12,
                "gutter": 8,
            },
            # A bar is drawn in its colour and outlined in the same colour multiplied by 0.4, so the fixture says what that comes to for each of the six the original uses.
            "edges": constant(
                records,
                lambda r: {b["color"]: b["edge"] for b in r["bars"]},
                "the edge colours",
            ),
        },
        "charts": [
            {
                "file": r["file"],
                "scale": r["scale"],
                "ylim": r["ylim"],
                "yticks": r["yticks"],
                "lines": r["lines"],
                "gutter": [g["text"] for g in r["gutter"]],
                "xticks": r["xticks"]["labels"],
                "legend": r["legend"],
            }
            for r in records
        ],
    }


def render(fixture):
    """Laid out by hand rather than with an indent setting, so that one chart is one block and one list is one line."""
    out = ["{", '  "constants": ' + json.dumps(fixture["constants"], indent=2).replace(
        "\n", "\n  "
    ) + ",", '  "charts": [']
    charts = fixture["charts"]
    for i, c in enumerate(charts):
        tail = "," if i + 1 < len(charts) else ""
        out.append("    {")
        out.append(f'      "file": {json.dumps(c["file"])},')
        out.append(f'      "scale": {json.dumps(c["scale"])},')
        out.append(f'      "ylim": {json.dumps(c["ylim"])},')
        out.append(f'      "yticks": {json.dumps(c["yticks"])},')
        out.append(f'      "lines": {json.dumps(c["lines"])},')
        out.append(f'      "gutter": {json.dumps(c["gutter"])},')
        out.append(f'      "xticks": {json.dumps(c["xticks"])},')
        out.append(f'      "legend": {json.dumps(c["legend"])}')
        out.append("    }" + tail)
    out.append("  ]")
    out.append("}")
    return "\n".join(out) + "\n"


def main():
    upstream = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    golden = os.path.join(here, "../../testdata/golden")
    series = json.load(open(os.path.join(golden, "series.json"), encoding="utf-8"))
    scripts = templates(os.path.join(upstream, "cmd/graph/main.go"))

    # One dict, emptied between charts, because the stubs close over it and rebinding the name here would leave half of them writing to the old one.
    record = {}
    install(record)

    records = []
    for chart in series:
        scale = "logarithmic" if "scale_logarithmic" in chart["file"] else "linear"
        record.clear()
        record.update(file=chart["file"], scale=scale, bars=[], lines=[], gutter=[])
        run(scripts[scale], chart)
        records.append(copy.deepcopy(record))

    if len(records) != 154:
        raise SystemExit(f"recorded {len(records)} charts, expected 154")

    path = os.path.join(golden, "axes.json")
    with open(path, "w", encoding="utf-8") as f:
        f.write(render(fold(records, series)))
    print(f"wrote {path} from {len(records)} charts")


if __name__ == "__main__":
    main()
