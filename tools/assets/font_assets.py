"""Prepare shared font sources and convert the complete application font."""

from pathlib import Path
import sys

from convert_surface_asset import convert_font
from font_sources import font_source, sources


def main() -> None:
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    fonts = {name: font_source(name) for name in sources()}
    convert_font(fonts["shure-tech-mono"], output / "shure-tech-mono.ippf")


if __name__ == "__main__":
    main()
