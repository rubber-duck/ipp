"""Prepare shared font sources and convert the complete application font."""

import json
from pathlib import Path
import sys

from fontTools.ttLib import TTFont

from convert_surface_asset import convert_font
from font_sources import font_source, sources


def main() -> None:
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    fonts = {name: font_source(name) for name in sources()}
    source = fonts["shure-tech-mono"]
    convert_font(source, output / "shure-tech-mono.ippf")
    with TTFont(source) as font:
        cmap = font.getBestCmap()
        metrics = {
            "unitsPerEm": font["head"].unitsPerEm,
            "ascent": font["hhea"].ascent,
            "advance": font["hmtx"].metrics[cmap[ord("M")]][0],
            "glyphs": {chr(code): font.getGlyphID(name) for code, name in cmap.items()},
        }
    (output / "shure-tech-mono.json").write_text(json.dumps(metrics) + "\n")


if __name__ == "__main__":
    main()
