"""Export the currently loaded blend through the standard IPP exporter.

blender -b SCENE.blend --python export_scene.py -- OUTPUT_DIRECTORY
"""

import argparse
import cProfile
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ipp_blender.exporter import export_scene
from ipp_blender.asset_store import AssetStore


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--profile", type=Path, help="Write cProfile export statistics")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])
    args.output.mkdir(parents=True, exist_ok=True)
    store = AssetStore(args.output / "assets")

    started = time.perf_counter()
    if args.profile:
        profile = cProfile.Profile()
        try:
            scene = profile.runcall(export_scene, store.publish)
        finally:
            args.profile.parent.mkdir(parents=True, exist_ok=True)
            profile.dump_stats(str(args.profile))
    else:
        scene = export_scene(store.publish)
    export_seconds = time.perf_counter() - started
    snapshot = {
        "type": "snapshot",
        "session": store.session,
        "revision": 1,
        "scene": scene,
    }
    (args.output / "scene.json").write_text(json.dumps(snapshot, indent=2) + "\n")
    catalog = {
        name: {"bytes": size, "contentType": content_type}
        for name, (_, size, content_type) in store.entries.items()
    }
    (args.output / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
    print(
        json.dumps(
            {
                "entities": len(scene["entities"]),
                "clips": len(scene.get("clips", [])),
                "assets": len(catalog),
                "exportSeconds": export_seconds,
                "profiled": args.profile is not None,
                "diagnostics": scene["diagnostics"],
            }
        )
    )


if __name__ == "__main__":
    main()
