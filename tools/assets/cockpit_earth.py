"""Prepare the cockpit's NASA Earth maps using the pinned Blender image tools.

Run with Blender --background --factory-startup --python-exit-code 1 --python
tools/assets/cockpit_earth.py. Source downloads stay in target/cockpit-earth-sources;
checked-in derivatives let normal demo builds run without external downloads.
"""

import hashlib
import json
from pathlib import Path
import shutil
import urllib.request

import bpy

ROOT = Path(__file__).resolve().parents[2]
DESTINATION = ROOT / "examples/demos/scenes/cockpit/authoring/earth"
CACHE = ROOT / "target/cockpit-earth-sources"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def download(source):
    path = CACHE / source["file"]
    if path.is_file() and digest(path) == source["sha256"]:
        return path

    CACHE.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".part")
    with (
        urllib.request.urlopen(source["url"], timeout=60) as response,
        temporary.open("wb") as output,
    ):
        shutil.copyfileobj(response, output)
    if digest(temporary) != source["sha256"]:
        raise ValueError(f"Earth source checksum mismatch: {source['url']}")
    temporary.replace(path)
    return path


def prepare():
    catalog = json.loads((DESTINATION / "sources.json").read_text())
    outputs = {}
    for name, source in catalog["sources"].items():
        image = bpy.data.images.load(str(download(source)), check_existing=False)
        try:
            if list(image.size) != source["dimensions"]:
                raise ValueError(f"Unexpected Earth source dimensions: {name}")
            # Keep the source's common plate-carree registration and north-up
            # orientation. Scale the large original in bounded half-size steps.
            for width in source["widths"]:
                while image.size[0] > width * 2:
                    image.scale(image.size[0] // 2, image.size[1] // 2)
                image.scale(width, width // 2)
                image.file_format = "JPEG" if name == "surface" else "PNG"
                suffix = "jpg" if name == "surface" else "png"
                output = DESTINATION / f"{name}-{width // 1024}k.{suffix}"
                image.save(filepath=str(output), quality=95, save_copy=True)
                outputs[output.name] = {
                    "dimensions": list(image.size),
                    "bytes": output.stat().st_size,
                    "sha256": digest(output),
                }
        finally:
            bpy.data.images.remove(image)

    (DESTINATION / "prepared.json").write_text(
        json.dumps({"blender": bpy.app.version_string, "images": outputs}, indent=2)
        + "\n"
    )
    return outputs


if __name__ == "__main__":
    print(json.dumps(prepare(), indent=2))
