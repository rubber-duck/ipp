"""Build the terminal example's immutable assets through the maintained converters."""

import json
from pathlib import Path
import struct
import sys

from fontTools.ttLib import TTFont

from convert_surface_asset import convert_svg
from font_sources import font_source


def main() -> None:
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    example = Path(__file__).resolve().parents[1] / "examples/surface-terminal"
    source_font = font_source("shure-tech-mono")
    for name in ("panel", "icon"):
        convert_svg(
            example / "authoring" / "svg" / f"{name}.svg",
            output / f"{name}.ippd",
            0.001,
        )
    size = 32
    pixels = bytearray()
    for y in range(size):
        for x in range(size):
            alpha = 128 if (x - 15.5) ** 2 + (y - 15.5) ** 2 <= 14**2 else 0
            pixels.extend((255, 160, 32, alpha))
    (output / "badge.ippt").write_bytes(
        b"IPPT" + struct.pack("<III", 3, size, size) + pixels
    )
    with TTFont(source_font) as font:
        cmap = font.getBestCmap()
        ids = {char: font.getGlyphID(cmap[ord(char)]) for char in "AOg0"}
    (output / "glyphs.json").write_text(json.dumps(ids) + "\n")


if __name__ == "__main__":
    main()
