# Fonts

The chart engine embeds its fonts in the binary rather than asking the host for
them. A chart drawn against whatever font happened to be installed is a chart
nobody else can reproduce, and the PNG hash manifest in `testdata/` only means
anything if the letter shapes are fixed.

The original uses Futura and Verdana. Neither is redistributable and both only
resolve on macOS, so this port substitutes:

| Original | Here | Licence |
| --- | --- | --- |
| Futura | Jost | SIL Open Font License 1.1 |
| Verdana | DejaVu Sans | Bitstream Vera Fonts Copyright |

Jost is metric compatible enough with Futura that the chart geometry did not
have to change. The licence text for each font ships next to the font file, and
both are permissive enough to embed in a binary that is itself Apache-2.0.

The font files land with M3.
