"""Build curve waveform assets and index the GUI demo projector resources."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


try:
    from tools.convert_surface_asset import convert_svg
except ModuleNotFoundError:  # Direct execution from the tools directory.
    from convert_surface_asset import convert_svg


def build_gallery_gui_assets(output_directory: Path) -> None:
    source_directory = (
        Path(__file__).resolve().parent.parent
        / "examples/world-gallery/worlds/gui/authoring/svg"
    )
    # Every authored SVG becomes a drawing, so adding a curve needs no build change.
    sources = sorted(source_directory.glob("*.svg"), key=lambda path: path.name)
    if not sources:
        raise FileNotFoundError(f"no SVG assets found in {source_directory}")
    output_directory.mkdir(parents=True, exist_ok=True)
    drawings: dict[str, str] = {}
    for source in sources:
        drawing = f"{source.stem}.ippd"
        convert_svg(source, output_directory / drawing, 0.001)
        drawings[source.stem] = drawing
    projector_output = output_directory / "projector"
    projector: dict[str, str] = {}
    for source in sorted(projector_output.iterdir(), key=lambda path: path.name):
        if source.suffix not in {".ippm", ".ippt", ".json"}:
            continue
        projector[source.stem] = f"projector/{source.name}"
    (output_directory / "assets.json").write_text(
        json.dumps(
            {"version": 1, "drawings": drawings, "projector": projector},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_directory", type=Path)
    args = parser.parse_args()
    build_gallery_gui_assets(args.output_directory)


if __name__ == "__main__":
    main()
