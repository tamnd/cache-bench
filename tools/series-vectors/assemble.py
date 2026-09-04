#!/usr/bin/env python3
"""Turns the captured graph scripts into one JSON file.

Every script the original writes starts with a header block holding everything the chart layer decided, and everything after it is drawing.
This keeps the header and drops the drawing, so what lands in the fixture is the original's answer to what goes on each chart rather than a picture of it.
"""

import json
import os
import sys

MARKER = "#" * 63


def header(path):
    """Read back the block of assignments at the top of a captured script."""
    with open(path, encoding="utf-8") as f:
        block = f.read().split(MARKER)[1]
    names = {}
    exec(block, names)  # noqa: S102, the script is one we just generated
    return names


def chart(path):
    n = header(path)
    return {
        "file": os.path.basename(n["filename"]),
        "title": n["title"],
        "xtitle": n["xtitle"],
        "ytitle": n["ytitle"],
        "xseries": n["xseries"],
        # Insertion order, which is the order the legend is drawn in.
        "series": [
            {"cache": cache, "color": n["colors"][cache], "points": points}
            for cache, points in n["data"].items()
        ],
    }


def main():
    root = sys.argv[1]
    charts = [chart(os.path.join(root, name)) for name in sorted(os.listdir(root))]

    # Laid out by hand rather than with an indent setting, so that one chart is one block and one series is one line.
    out = ["["]
    for i, c in enumerate(charts):
        tail = "," if i + 1 < len(charts) else ""
        out.append("  {")
        for key in ("file", "title", "xtitle", "ytitle"):
            out.append(f'    "{key}": {json.dumps(c[key])},')
        out.append(f'    "xseries": {json.dumps(c["xseries"])},')
        out.append('    "series": [')
        for j, s in enumerate(c["series"]):
            comma = "," if j + 1 < len(c["series"]) else ""
            out.append(f"      {json.dumps(s)}{comma}")
        out.append("    ]")
        out.append("  }" + tail)
    out.append("]")
    print("\n".join(out))


if __name__ == "__main__":
    main()
