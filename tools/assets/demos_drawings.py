"""Convert the demo application's shared vector drawings."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

try:
    from tools.assets.convert_surface_asset import convert_svg
except ModuleNotFoundError:
    from convert_surface_asset import convert_svg


def build_demo_drawings(output_directory: Path) -> None:
    sources = Path(__file__).resolve().parents[2] / "examples/demos/assets/drawings"
    output_directory.mkdir(parents=True, exist_ok=True)
    drawings = {}
    for source in sorted(sources.glob("*.svg")):
        target = f"{source.stem}.ippd"
        convert_svg(source, output_directory / target, 0.001)
        drawings[source.stem] = target
    if not drawings:
        raise FileNotFoundError(f"no demo drawings in {sources}")
    (output_directory / "assets.json").write_text(
        json.dumps({"version": 1, "drawings": drawings}, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_directory", type=Path)
    build_demo_drawings(parser.parse_args().output_directory)


if __name__ == "__main__":
    main()
