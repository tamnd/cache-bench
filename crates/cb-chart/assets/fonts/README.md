# Fonts

The chart engine embeds its fonts in the binary rather than asking the host for them. A chart drawn against whatever font happened to be installed is a chart nobody else can reproduce, and the PNG hash manifest in `crates/cb-core/golden` only means anything if the letter shapes are fixed.

The original sets `plt.rcParams['font.family'] = "Futura"`, asks matplotlib for bold on the title and both axis labels, and names Verdana for the small gray quarter decade numbers in the margin of a logarithmic chart. Neither face is redistributable and both only resolve on macOS, so this port substitutes:

| Original | Here | Draws | Licence |
| --- | --- | --- | --- |
| Futura | Jost\* Book | Tick labels and the legend | SIL Open Font License 1.1 |
| Futura Bold | Jost\* Bold | The title and both axis labels | SIL Open Font License 1.1 |
| Verdana | DejaVu Sans | The quarter decade labels on a logarithmic chart | Bitstream Vera Fonts Copyright |

Jost is metric compatible enough with Futura that the chart geometry did not have to change. Both licences are permissive enough to embed in a binary that is itself Apache-2.0, and the licence text for each font sits in the directory next to the font file rather than being summarised here.

## What is here

```
jost/Jost-400-Book.ttf     from indestructible-type/Jost, tag 3.5, fonts/ttf
jost/Jost-700-Bold.ttf     from indestructible-type/Jost, tag 3.5, fonts/ttf
jost/LICENSE.md            the SIL OFL 1.1, as that tag ships it
jost/FONTLOG.txt           the font's own changelog, which the OFL asks a redistributor to carry
dejavu/DejaVuSans.ttf      from dejavu-fonts/dejavu-fonts, release version_2_37, ttf
dejavu/LICENSE             the Bitstream Vera and Arev terms, as that release ships them
dejavu/AUTHORS             the credit list the licence asks a redistributor to carry
```

Static cuts rather than the variable font in either family, because a weight axis is one more thing that has to land on the same value on three platforms for the hashes to agree.

## Changing one

Do not, without meaning to. `crates/cb-chart/src/font.rs` carries the SHA-256 of all three files and a test compares it against the bytes it embedded, so a font swapped for a different cut of the same family fails the build instead of quietly redrawing every chart in the manifest. If a font really has to move, update the digest in the same commit and say in the message what moved and why, and expect the whole hash manifest to change with it.
