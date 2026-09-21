"""Compare shared action evaluation with isolated extraction in real Blender."""

import hashlib
import json
import math
import sys
import tempfile
from contextlib import nullcontext
from pathlib import Path
from unittest.mock import patch

import bpy

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "integrations/blender"), str(ROOT / "tests/blender")]
import fixture
from ipp_blender.exporter import export_scene

fixture.create_scene()
# Multiple active actions sharing a range exercise the batched path; the existing
# fixture also includes an armature. A stashed association forces isolated fallback.
for index in range(3):
    bpy.ops.mesh.primitive_cube_add(location=(index * 2, 0, 0))
    obj = bpy.context.object
    obj.name = f"sampling-{index}"
    obj["ipp_id"] = obj.name
    obj.location.z = 0.2
    obj.keyframe_insert("location", frame=1)
    obj.location.z = 1.3
    obj.keyframe_insert("location", frame=25)
    if index == 2:
        track = obj.animation_data.nla_tracks.new()
        track.mute = True
        track.strips.new("stashed", 1, obj.animation_data.action)
# Matching and fractional ranges, shared light data, custom intensities and a
# muted NLA association must preserve exact detached samples and associations.
for index in range(9):
    light = bpy.data.lights.new(
        f"sampling-light-{index}", "SUN" if index == 0 else "POINT"
    )
    obj = fixture.identify(bpy.data.objects.new(light.name, light), light.name)
    bpy.context.scene.collection.objects.link(obj)
    start, end = (1.25, 13.75) if index in (3, 4) else (1, 25)
    if index == 7:
        start, end = (2, 6)
    for frame, value in ((start, 2.0), (end, 5.0)):
        light.energy = value * (1 if index == 0 else 4 * math.pi)
        light.keyframe_insert("energy", frame=frame)
        light.color = (value / 10, 0.4, 0.6)
        light.keyframe_insert("color", frame=frame)
        if index == 1:
            light["ipp_intensity"] = value
            light.keyframe_insert('["ipp_intensity"]', frame=frame)
    if index == 2:
        track = light.animation_data.nla_tracks.new()
        track.mute = True
        track.strips.new("stashed", 1, light.animation_data.action)
        shared = fixture.identify(
            bpy.data.objects.new("sampling-shared-light", light),
            "sampling-shared-light",
        )
        bpy.context.scene.collection.objects.link(shared)
    if index == 5:
        # Rejected bindings retain the isolated path's diagnostic.
        light.driver_add("energy").driver.expression = "3.0"
    if index == 8:
        track = light.animation_data.nla_tracks.new()
        track.strips.new("active", 1, light.animation_data.action)
    if index == 6:
        # Failing only during sampling must restore the timeline and leave other
        # lights exportable. Snapshot intensity is valid at frame 7.25.
        light["ipp_intensity"] = 1.0
        light.keyframe_insert('["ipp_intensity"]', frame=1)
        light["ipp_intensity"] = -1.0
        light.keyframe_insert('["ipp_intensity"]', frame=25)
# Distinct datablocks/ranges, shared instances and rejected shape-key bindings.
for index in range(8):
    bpy.ops.mesh.primitive_cube_add(location=(index * 2, 4, 0))
    obj = fixture.identify(bpy.context.object, f"sampling-shape-{index}")
    obj.shape_key_add(name="Basis")
    shape = obj.shape_key_add(name="Bend")
    shape.data[0].co.z += 0.5
    start, end = (1.25, 13.75) if index in (2, 3) else (1, 25)
    for frame, value in ((start, 0.2), (end, 0.8)):
        shape.value = value
        shape.keyframe_insert("value", frame=frame)
    data = obj.data.shape_keys.animation_data
    if index == 0:
        shared = fixture.identify(
            bpy.data.objects.new("sampling-shared-shape", obj.data),
            "sampling-shared-shape",
        )
        bpy.context.scene.collection.objects.link(shared)
    if index in (1, 4):
        track = data.nla_tracks.new()
        track.mute = index == 1
        track.strips.new("shape-nla", 1, data.action)
    if index == 5:
        shape.driver_add("value").driver.expression = "0.5"
    if index == 6:
        shape.mute = True
    if index == 7:
        shape.slider_max = 2
        shape.value = 2
        shape.keyframe_insert("value", frame=25)
bpy.context.scene.frame_set(7, subframe=0.25)

with tempfile.TemporaryDirectory(prefix="ipp-action-sampling-") as directory:
    blend = str(Path(directory) / "source.blend")
    bpy.ops.wm.save_as_mainfile(filepath=blend)
    outputs = []
    for shared in (False, True):
        bpy.ops.wm.open_mainfile(filepath=blend)
        assets = {}

        def publish(payload, content_type):
            source = "/assets/" + hashlib.sha256(payload).hexdigest()
            assets[source] = payload
            return source

        actions = {
            obj.name: obj.animation_data.action.name
            for obj in bpy.data.objects
            if obj.animation_data and obj.animation_data.action
        }
        sampling = (
            nullcontext()
            if shared
            else patch(
                "ipp_blender.animation.prepare_actions",
                return_value=({}, {}, {}),
            )
        )
        with sampling:
            result = export_scene(publish)
        assert (bpy.context.scene.frame_current, bpy.context.scene.frame_subframe) == (
            7,
            0.25,
        )
        assert actions == {
            obj.name: obj.animation_data.action.name
            for obj in bpy.data.objects
            if obj.animation_data and obj.animation_data.action
        }
        for collection in ("animations", "clips"):
            for association in result[collection]:
                association["source"] = json.loads(assets[association["source"]])
        light_clips = [
            clip for clip in result["clips"] if ":light-action:" in clip["id"]
        ]
        assert len(light_clips) == 7, len(light_clips)
        for clip in light_clips:
            intensity = next(
                track
                for track in clip["source"]["tracks"]
                if track["property"]["fields"] == ["intensity"]
            )
            assert abs(intensity["keys"][0]["value"]["value"] - 2) < 1e-6
            assert abs(intensity["keys"][-1]["value"]["value"] - 5) < 1e-6
        rejected = {
            item.get("entity")
            for item in result["diagnostics"]
            if item["code"] == "light-animation-unsupported"
        }
        assert rejected == {
            "sampling-light-5",
            "sampling-light-6",
            "sampling-light-8",
        }, rejected
        shape_clips = [
            clip
            for clip in result["clips"]
            if clip["target"].startswith("sampling-") and ":shape-action:" in clip["id"]
        ]
        assert len(shape_clips) == 5, len(shape_clips)
        for clip in shape_clips:
            samples = clip["source"]["tracks"][0]["keys"]
            assert abs(samples[0]["value"]["value"] - 0.2) < 1e-6
            assert abs(samples[-1]["value"]["value"] - 0.8) < 1e-6
        rejected_shapes = {
            item.get("entity")
            for item in result["diagnostics"]
            if item["code"] == "mesh-animation-unsupported"
        }
        assert rejected_shapes == {
            "sampling-shape-4",
            "sampling-shape-5",
            "sampling-shape-7",
        }, rejected_shapes
        outputs.append(result)

maximum = 0.0


def compare(a, b):
    global maximum
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        error = abs(a - b)
        maximum = max(maximum, error)
        assert error < 1e-5, (a, b)
    elif isinstance(a, dict):
        assert a.keys() == b.keys()
        for key in a:
            compare(a[key], b[key])
    elif isinstance(a, (list, tuple)):
        assert len(a) == len(b)
        for x, y in zip(a, b, strict=True):
            compare(x, y)
    else:
        assert a == b, (a, b)


compare(*outputs)


# A stream can be cancelled while a shared timeline pass is in progress. Keep
# this independent of the networking harness: assert actual Blender state after
# cancellation at a changed frame, including the isolated NLA/action fallback.
class CancelSampling:
    def __init__(self):
        from ipp_blender.asset_store import AssetStore

        self.asset_store = AssetStore()

    def asset_index(self, _entries):
        pass

    def checkpoint(self):
        scene = bpy.context.scene
        if (scene.frame_current, scene.frame_subframe) != (7, 0.25):
            raise RuntimeError("cancel sampling")

    def entities(self, _entities, **_options):
        pass

    def complete_entities(self):
        pass

    def asset(self, _source, _content_type):
        pass


actions = {
    obj.name: obj.animation_data.action
    for obj in bpy.data.objects
    if obj.animation_data
}
stream = CancelSampling()
try:
    export_scene(publish, stream=stream)
    raise AssertionError("Cancellation did not reach a sampling checkpoint")
except RuntimeError as error:
    assert str(error) == "cancel sampling"
finally:
    assert not stream.asset_store.has_pending()
    stream.asset_store.close()
assert (bpy.context.scene.frame_current, bpy.context.scene.frame_subframe) == (
    7,
    0.25,
)
assert actions == {
    obj.name: obj.animation_data.action
    for obj in bpy.data.objects
    if obj.animation_data
}

print(
    "Shared/isolated action samples match; timeline and actions restored; max error",
    maximum,
)
