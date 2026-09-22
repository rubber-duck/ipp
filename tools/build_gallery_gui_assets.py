"""Build curve waveform assets and index the GUI demo projector resources."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


try:
    from tools.convert_surface_asset import convert_svg
except ModuleNotFoundError:
    from convert_surface_asset import convert_svg


def build_gallery_gui_assets(output_directory: Path) -> None:
    output_directory.mkdir(parents=True, exist_ok=True)
    source_directory = (
        Path(__file__).resolve().parent.parent
        / "examples/world-gallery/worlds/gui/authoring/svg"
    )
    drawings = {}
    for name in ("waveform-grid", "waveform", "waveform-pulse"):
        drawing = f"{name}.ippd"
        convert_svg(source_directory / f"{name}.svg", output_directory / drawing, 0.001)
        drawings[name] = drawing
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
