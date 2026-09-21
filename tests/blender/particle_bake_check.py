"""Compare a standard disk export with independently simulated Blender particles.

blender -b SCENE.blend --python-exit-code 1 --python tests/blender/particle_bake_check.py -- EXPORT_DIR
"""

import json
import math
import sys
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(ROOT / "integrations/blender"))
from particle_oracle import assert_cache


def main():
    directory = Path(sys.argv[sys.argv.index("--") + 1])
    snapshot = json.loads((directory / "scene.json").read_text())["scene"]
    effects = {e["id"]: e for e in snapshot["entities"] if "particle_playback" in e}
    assert effects, "Export has no baked particle effects"
    results = {}
    for obj in bpy.context.scene.objects:
        for index, system in enumerate(obj.particle_systems):
            effect = effects.get(f"{obj.get('ipp_id')}:particles:{system.name}")
            if effect is None:
                continue
            cache = (
                directory
                / "assets"
                / effect["particle_playback"]["source"].removeprefix("/assets/")
            ).read_bytes()
            results[effect["name"]] = assert_cache(obj, index, cache)
            assert any(
                c["target"] == effect["id"] for c in snapshot.get("clips", [])
            ), "Baked playback must be reusable after clips-only import"
    assert len(results) == len(effects)
    # Associated light-data clips must keep the authored flash timing as well.
    scene = bpy.context.scene
    frame, subframe = scene.frame_current, scene.frame_subframe
    lights = {}
    exported_ids = {e["name"]: e["id"] for e in snapshot["entities"]}
    try:
        for obj in scene.objects:
            if obj.type != "LIGHT" or not obj.data.animation_data:
                continue
            action = obj.data.animation_data.action
            if action is None:
                continue
            for entry in snapshot.get("clips", []):
                if (
                    entry["target"] != exported_ids.get(obj.name)
                    or entry["name"] != action.name
                ):
                    continue
                clip = json.loads(
                    (
                        directory / "assets" / entry["source"].removeprefix("/assets/")
                    ).read_text()
                )
                checked = 0
                for track in clip["tracks"]:
                    if track.get("property", {}).get("component") != "Light":
                        continue
                    field = track["property"]["fields"][0]
                    for key in track["keys"]:
                        sample_frame = (
                            action.frame_range[0]
                            + key["time"] * scene.render.fps / scene.render.fps_base
                        )
                        scene.frame_set(
                            math.floor(sample_frame), subframe=sample_frame % 1
                        )
                        if field == "intensity":
                            expected = obj.data.get(
                                "ipp_intensity",
                                obj.data.energy
                                if obj.data.type == "SUN"
                                else obj.data.energy / (4 * math.pi),
                            )
                        else:
                            expected = obj.data.color["rgb".index(field)]
                        assert abs(key["value"]["value"] - expected) < 3e-4
                        checked += 1
                if checked:
                    lights[obj.name] = checked
    finally:
        scene.frame_set(frame, subframe=subframe)
    (directory / "particle-bake-validation.json").write_text(
        json.dumps({"particles": results, "lights": lights}, indent=2) + "\n"
    )
    print(
        "PASS: independent Blender identities, lifetimes, positions, velocities, rotations and sizes",
        results,
        lights,
    )


if __name__ == "__main__":
    main()
