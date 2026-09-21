"""Real Blender extraction: native recipes, bake samples and timeline restoration."""

import hashlib
import pathlib
import struct
import sys

import bpy

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "integrations/blender"))
from ipp_blender.exporter import export_scene

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
bpy.ops.mesh.primitive_plane_add(size=2)
obj = bpy.context.object
obj.name = "ParticleFixture"
bpy.ops.object.particle_system_add()
system = obj.particle_systems[0]
settings = system.settings
settings.type = "EMITTER"
settings.physics_type = "NEWTON"
settings.emit_from = "FACE"
settings.count = 40
settings.frame_start = 1
settings.frame_end = 6
settings.lifetime = 20
settings.normal_factor = 1
settings.render_type = "HALO"
settings.use_rotations = False
bpy.context.scene.frame_start = 1
bpy.context.scene.frame_end = 12
bpy.context.scene.render.fps = 24
assets = {}


def publish(data, content_type):
    source = "/assets/" + hashlib.sha256(data).hexdigest()
    assets[source] = data
    return source


snapshot = export_scene(publish, animation=False)
effects = [e for e in snapshot["entities"] if "particle_emitter" in e]
assert len(effects) == 1, snapshot["diagnostics"]
assert effects[0]["particle_emitter"]["capacity"] == 40
settings.brownian_factor = 0.4
snapshot = export_scene(publish, animation=False)
assert any(d["code"] == "particle-unsupported" for d in snapshot["diagnostics"])
# Retain Brownian motion: the baked path must preserve simulation unavailable to recipes.
material = bpy.data.materials.new("Particle blue")
material.use_nodes = True
material.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = (
    0.1,
    0.4,
    1,
    1,
)
material.diffuse_color = (0.1, 0.4, 1, 1)
material["ipp_unlit"] = True
obj.data.materials.append(material)
settings.material = 1
obj.show_instancer_for_render = False
settings["ipp_blend"] = "ADDITIVE"
obj["ipp_particles"] = "BAKED"
bpy.context.scene.frame_set(7, subframe=0.25)
snapshot = export_scene(publish, animation=False)
assert (
    bpy.context.scene.frame_current == 7
    and abs(bpy.context.scene.frame_subframe - 0.25) < 1e-6
)
baked = [e for e in snapshot["entities"] if "particle_playback" in e]
assert len(baked) == 1, snapshot["diagnostics"]
assert baked[0]["particle_sprite"]["blend"] == 1
assert abs(baked[0]["particle_sprite"]["r"] - 0.1) < 1e-6
assert len(snapshot["clips"]) == len(snapshot["animations"]) == 1
assert snapshot["clips"][0]["source"] == snapshot["animations"][0]["source"]
assert all("mesh" not in e for e in snapshot["entities"] if e["name"] == obj.name)
cache = assets[baked[0]["particle_playback"]["source"]]
assert cache[:4] == b"IPPC"
assert struct.unpack_from("<III", cache, 4) == (1, 1, 12)
assert any(struct.unpack_from("<I", cache, 24 + 12 * i)[0] > 0 for i in range(12))
# Compare an independent Blender sample to the emitted portable cache.
for frame_number in range(0, 7):
    bpy.context.scene.frame_set(frame_number)
particles = (
    obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    .particle_systems[0]
    .particles
)
_, offset, count = struct.unpack_from("<fII", cache, 16 + 5 * 12)
from ipp_blender.exporter.types import BASIS

for i in range(count):
    identifier = struct.unpack_from("<Q", cache, offset + 60 * i)[0]
    expected = BASIS @ particles[identifier].location
    actual = struct.unpack_from("<3f", cache, offset + 60 * i + 16)
    assert max(abs(a - b) for a, b in zip(actual, expected)) < 1e-4, (
        actual,
        tuple(expected),
    )
output = ROOT / "target/integration-artifacts/particles/blender"
output.mkdir(parents=True, exist_ok=True)
(output / "cache.ippc").write_bytes(cache)
import json

(output / "scene.json").write_text(json.dumps(snapshot, indent=2))
print(
    "PASS: native recipe, explicit rejection, portable bake, independent positions and timeline restoration"
)

# Check every sample, including velocities and disabled-rotation identity.
sys.path.insert(0, str(ROOT / "tests/blender"))
from particle_oracle import assert_cache

assert_cache(obj, 0, cache)
settings.render_type = "COLLECTION"
unsupported = export_scene(publish, animation=False)
assert not unsupported.get("animations")
assert not unsupported.get("clips")
assert any(d["code"] == "particle-unsupported" for d in unsupported["diagnostics"])
print(
    "PASS: reusable bake clips, sprite color/blend, hidden emitter and no orphan controllers"
)

# The same export carries a light-data action for an authored arc flash.
bpy.ops.object.light_add(type="POINT")
light = bpy.context.object
light.name = "ArcLight"
scene = bpy.context.scene
for frame_number, intensity, color in [
    (1, 0.0, (0.2, 0.4, 1)),
    (4, 5.0, (1, 0.8, 0.4)),
    (12, 0.0, (0.2, 0.4, 1)),
]:
    light.data["ipp_intensity"] = intensity
    light.data.color = color
    light.data.keyframe_insert(data_path='["ipp_intensity"]', frame=frame_number)
    light.data.keyframe_insert(data_path="color", frame=frame_number)
scene.frame_set(7, subframe=0.25)
original_intensity = light.data["ipp_intensity"]
snapshot = export_scene(publish)
assert scene.frame_current == 7 and abs(scene.frame_subframe - 0.25) < 1e-6
assert light.data["ipp_intensity"] == original_intensity
identifier = light["ipp_id"]
clips = [c for c in snapshot["clips"] if c["target"] == identifier]
assert len(clips) == 1, clips
assert any(
    a["target"] == identifier and a["source"] == clips[0]["source"]
    for a in snapshot["animations"]
)
clip = json.loads(assets[clips[0]["source"]])
assert abs(clip["duration"] - 11 / 24) < 1e-7
for track in clip["tracks"]:
    assert track["property"]["component"] == "Light"
    field = track["property"]["fields"][0]
    assert len(track["keys"]) == 12
    for index, key in enumerate(track["keys"]):
        scene.frame_set(index + 1)
        expected = (
            light.data["ipp_intensity"]
            if field == "intensity"
            else light.data.color["rgb".index(field)]
        )
        assert abs(key["value"]["value"] - expected) < 1e-6
assert len(clip["tracks"]) == 4
# Without an explicit intensity, the energy conversion matches static export.
light.data.animation_data_clear()
del light.data["ipp_intensity"]
for frame_number, energy in [(1, 0), (12, 4 * 3.141592653589793)]:
    light.data.energy = energy
    light.data.keyframe_insert(data_path="energy", frame=frame_number)
snapshot = export_scene(publish)
clip = json.loads(
    assets[next(c["source"] for c in snapshot["clips"] if c["target"] == identifier)]
)
assert abs(clip["tracks"][0]["keys"][-1]["value"]["value"] - 1) < 1e-6
# A failed sample cannot leak partial clips or leave the timeline displaced.
light.data.animation_data_clear()
for frame_number, intensity in [(1, 1.0), (4, -1.0), (12, 1.0)]:
    light.data["ipp_intensity"] = intensity
    light.data.keyframe_insert(data_path='["ipp_intensity"]', frame=frame_number)
scene.frame_set(12, subframe=0.25)
snapshot = export_scene(publish)
assert all(c["target"] != identifier for c in snapshot.get("clips", []))
assert any(
    d["code"] == "light-animation-unsupported" for d in snapshot["diagnostics"]
), snapshot["diagnostics"]
assert scene.frame_current == 12 and abs(scene.frame_subframe - 0.25) < 1e-6
print("PASS: authored light intensity/color clips, energy conversion and restoration")
