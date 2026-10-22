"""Split the unchanged spherical source into registered imagegen edit inputs.

Run with Blender in background mode. This is a lossless pixel crop, not a new
projection. The odd source height keeps its exact normalized split latitude.
"""

import json
from pathlib import Path

import bpy
import numpy as np

ROOT = Path(__file__).resolve().parent


def prepare():
    source = bpy.data.images.load(str(ROOT / "bridge-panorama-v2.png"))
    width, height = source.size
    pixels = np.empty(width * height * 4, np.float32)
    source.pixels.foreach_get(pixels)
    pixels = pixels.reshape(height, width, 4)
    split_x, split_y = width // 2, (height + 1) // 2
    tiles = []
    for name, left, top, right, bottom in (
        ("upper-left", 0, 0, split_x, split_y),
        ("upper-right", split_x, 0, width, split_y),
        ("lower-left", 0, split_y, split_x, height),
        ("lower-right", split_x, split_y, width, height),
    ):
        # Blender stores bottom-up rows; metadata and imagegen use top-left UVs.
        crop = pixels[height - bottom : height - top, left:right]
        image = bpy.data.images.new(name, right - left, bottom - top, alpha=True)
        image.pixels.foreach_set(crop.ravel())
        image.filepath_raw = str(ROOT / f"bridge-{name}-source.png")
        image.file_format = "PNG"
        image.save()
        tiles.append(
            {
                "name": name,
                "bounds": [left / width, top / height, right / width, bottom / height],
            }
        )
    (ROOT / "bridge-tiles.json").write_text(
        json.dumps({"sourceExtent": [width, height], "tiles": tiles}, indent=2) + "\n"
    )


if __name__ == "__main__":
    prepare()
