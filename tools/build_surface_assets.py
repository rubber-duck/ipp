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
        glyf = font["glyf"]

        def outlined(code: int) -> bool:
            return code in cmap and glyf[cmap[code]].numberOfContours != 0

        # Every printable ASCII glyph with an outline, keyed by its character.
        ids = {
            chr(code): font.getGlyphID(cmap[code])
            for code in range(0x21, 0x7F)
            if outlined(code)
        }
        # Outlined glyphs outside ASCII that a terminal workload introduces as
        # unseen text: Latin-1, box drawing and Font Awesome icon ranges.
        unseen = [
            font.getGlyphID(cmap[code])
            for block in (
                range(0xA1, 0x100),
                range(0x2500, 0x25A0),
                range(0xF000, 0xF2E1),
            )
            for code in block
            if outlined(code)
        ]
    (output / "glyphs.json").write_text(json.dumps(ids) + "\n")
    (output / "unseen-glyphs.json").write_text(json.dumps(unseen) + "\n")


if __name__ == "__main__":
    main()
