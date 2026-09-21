"""One shared world pass, with independent particle simulation and sample counts."""

import hashlib
import json
import math
import sys
import tempfile
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "integrations/blender"), str(ROOT / "tests/blender")]
from ipp_blender.exporter import export_scene
from particle_oracle import assert_cache

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
scene = bpy.context.scene
scene.frame_start, scene.frame_end = 4, 12

bpy.ops.mesh.primitive_cube_add()
obj = bpy.context.object
obj["ipp_id"] = "moving-shape"
obj.shape_key_add(name="Basis")
key = obj.shape_key_add(name="Bend")
key.data[0].co.z += 0.4
for frame, value in ((1, 0.1), (12, 0.8)):
    obj.location.x = value
    obj.keyframe_insert("location", frame=frame)
obj.data.shape_keys.animation_data_create().action = bpy.data.actions.new(
    "Fractional shape"
)
for frame, value in ((1.25, 0.2), (8.75, 0.7)):
    key.value = value
    key.keyframe_insert("value", frame=frame)

bpy.ops.object.light_add(type="POINT")
light = bpy.context.object
light["ipp_id"] = "animated-light"
for frame, value in ((3, 2.0), (9, 5.0)):
    light.data["ipp_intensity"] = value
    light.data.keyframe_insert('["ipp_intensity"]', frame=frame)

for index, start in enumerate((1, 3)):
    bpy.ops.mesh.primitive_plane_add()
    emitter = bpy.context.object
    emitter["ipp_id"] = f"baked-{index}"
    emitter["ipp_particles"] = "BAKED"
    emitter.location.x = index * 3
    emitter.show_instancer_for_render = False
    bpy.ops.object.particle_system_add()
    settings = emitter.particle_systems[0].settings
    settings.count = 16
    settings.frame_start, settings.frame_end = start, 5
    settings.lifetime = 20
    settings.normal_factor = 1
    settings.brownian_factor = 0.3
    settings.render_type = "HALO"
    settings.use_rotations = bool(index)

scene.frame_set(7, subframe=0.25)
with tempfile.TemporaryDirectory(prefix="ipp-world-sampling-") as directory:
    blend = str(Path(directory) / "source.blend")
    bpy.ops.wm.save_as_mainfile(filepath=blend)
    bpy.ops.wm.open_mainfile(filepath=blend)
    assets, frames = {}, []

    def publish(payload, _content_type):
        source = "/assets/" + hashlib.sha256(payload).hexdigest()
        assets[source] = payload
        return source

    def observed(scene, _depsgraph):
        frames.append(scene.frame_current + scene.frame_subframe)

    bpy.app.handlers.frame_change_post.append(observed)
    try:
        snapshot = export_scene(publish)
    finally:
        bpy.app.handlers.frame_change_post.remove(observed)
    assert not snapshot["diagnostics"], snapshot["diagnostics"]
    assert frames[-1] == 7.25

    # Derive the schedule independently from authored ranges. Particle simulation
    # owns the integer warmup pass; fractional actions run only after that pass.
    expected = list(range(0, 13))
    count = math.ceil(8.75 - 1.25) + 1
    expected.extend(1.25 + 7.5 * i / (count - 1) for i in range(count))
    assert frames[: len(expected)] == expected, (frames, expected)
    assert all(frame == 7.25 for frame in frames[len(expected) :])

    # Reopen for each oracle so exporter-populated simulation caches cannot supply
    # the reference particle values.
    particle_caches = [
        entity for entity in snapshot["entities"] if "particle_playback" in entity
    ]
    for entity in particle_caches:
        bpy.ops.wm.open_mainfile(filepath=blend)
        source_id = entity["id"].split(":particles:")[0]
        emitter = next(
            obj for obj in bpy.context.scene.objects if obj.get("ipp_id") == source_id
        )
        assert_cache(emitter, 0, assets[entity["particle_playback"]["source"]])
    print(
        json.dumps(
            {
                "sharedFrameSets": len(frames),
                "particleCaches": len(particle_caches),
            }
        )
    )
