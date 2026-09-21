"""Real Blender numerical extraction checks; run with --background --python."""

import hashlib
import json
import struct
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Quaternion, Vector

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "integrations/blender"), str(ROOT / "tests/blender")]
import capacity_fixture
import fixture
import hierarchy_pose_fixture
from ipp_blender.exporter import export_scene
from ipp_blender.exporter.types import BASIS


class Store:
    def __init__(self):
        self.data = {}

    def publish(self, payload, content_type):
        source = "/assets/" + hashlib.sha256(payload).hexdigest()
        self.data[source] = payload
        return source

    def export(self, *, animation=True):
        result = export_scene(self.publish, animation=animation)
        json.dumps(result, allow_nan=False)
        return result


def matrix(trs):
    t, q, s = trs[:3], trs[3:7], trs[7:]
    return Matrix.LocRotScale(Vector(t), Quaternion((q[3], *q[:3])), Vector(s))


def transform(entity):
    value = entity["transform"]
    return matrix(
        [value[f] for f in ("x", "y", "z", "qx", "qy", "qz", "qw", "sx", "sy", "sz")]
    )


def decode_mesh(data):
    version, vertices, indices, count = struct.unpack_from("<4I", data, 4)
    assert version == 3 and vertices == indices
    offset = 20 + 8 * count
    streams = {}
    for index in range(count):
        semantic, fmt, reserved, size = struct.unpack_from(
            "<BBHI", data, 20 + 8 * index
        )
        assert reserved == 0
        if fmt == 4:
            values, width = data[offset : offset + size], 4
        else:
            values = struct.unpack_from(f"<{size // 4}f", data, offset)
            width = {1: 3, 2: 2, 5: 4}[fmt]
        streams[semantic] = [
            values[i : i + width] for i in range(0, len(values), width)
        ]
        offset += size
    assert len(data) == offset + indices * 2
    return streams


def world_transform(entity, scene):
    local = transform(entity) if "transform" in entity else Matrix.Identity(4)
    if "parent" in entity:
        parent = next(e for e in scene["entities"] if e["id"] == entity["parent"])
        return world_transform(parent, scene) @ local
    return local


def skin_error(store, scene, object_name):
    obj = bpy.data.objects[object_name]
    entity = next(e for e in scene["entities"] if e["id"] == obj["ipp_id"])
    rig = next(e for e in scene["entities"] if e["id"] == entity["skin"]["skeleton"])
    skeleton = store.data[rig["skeleton"]["source"]]
    pose = store.data[rig["skeleton"]["pose_source"]]
    count = struct.unpack_from("<I", skeleton, 8)[0]
    global_pose = []
    for index in range(count):
        parent = struct.unpack_from("<I", skeleton, 12 + index * 44)[0]
        local = matrix(struct.unpack_from("<10f", pose, 12 + index * 40))
        global_pose.append(
            local if parent == 0xFFFFFFFF else global_pose[parent] @ local
        )
    skin = store.data[entity["skin"]["source"]]
    palette = []
    for index in range(count):
        joint = struct.unpack_from("<I", skin, 12 + index * 68)[0]
        values = struct.unpack_from("<16f", skin, 16 + index * 68)
        bind = Matrix([values[i : i + 4] for i in range(0, 16, 4)]).transposed()
        palette.append(world_transform(rig, scene) @ global_pose[joint] @ bind)
    streams = decode_mesh(store.data[entity["mesh"]["source"]])
    positions = streams[0]
    if "mesh_pose" in entity:
        pose = entity["mesh_pose"]
        targets = decode_mesh(store.data[pose["source"]])[0]
        positions = [
            Vector(a).lerp(Vector(b), pose["weight"])
            for a, b in zip(positions, targets, strict=True)
        ]
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = evaluated.to_mesh()
    try:
        mesh.calc_loop_triangles()
        expected = [
            BASIS @ obj.matrix_world @ mesh.vertices[index].co.to_4d()
            for tri in mesh.loop_triangles
            for index in tri.vertices
        ]
        errors = []
        for position, joints, weights, target in zip(
            positions, streams[5], streams[6], expected, strict=True
        ):
            point = Vector((*position, 1))
            actual = sum(
                (
                    palette[joint] @ point * weight
                    for joint, weight in zip(joints, weights, strict=True)
                ),
                Vector((0, 0, 0, 0)),
            )
            errors.append((actual.xyz - target.xyz).length)
        return max(errors)
    finally:
        evaluated.to_mesh_clear()


def generated_checks():
    fixture.create_scene()
    store = Store()
    scene = store.export()
    assert not scene["diagnostics"], scene["diagnostics"]
    assert len(scene["entities"]) == 7 and len(scene["animations"]) == 2
    assert bpy.context.scene.frame_current == 1
    assert scene["ambient_light"] == [0.0, 0.0, 0.0]
    fixture.apply_command(
        {"action": "ambient", "color": [0.25, 0.5, 1.0], "strength": 2.0}
    )
    assert store.export(animation=False)["ambient_light"] == [0.5, 1.0, 2.0]
    world = bpy.context.scene.world
    noise = world.node_tree.nodes.new("ShaderNodeTexNoise")
    link = world.node_tree.links.new(
        noise.outputs["Color"], world.node_tree.nodes.get("Background").inputs["Color"]
    )
    unsupported_world = store.export(animation=False)
    assert unsupported_world["ambient_light"] == [0.0, 0.0, 0.0]
    assert any(
        d["code"] == "world-unsupported" for d in unsupported_world["diagnostics"]
    )
    world.node_tree.links.remove(link)
    fixture.apply_command({"action": "ambient", "color": [0, 0, 0]})
    initial = {e["id"]: e for e in scene["entities"]}
    spot = bpy.data.objects["fixture-spot"].data
    old_radius = spot.shadow_soft_size
    spot.shadow_soft_size = 0.18
    spot["ipp_shadow_near"], spot["ipp_shadow_bias"] = 1.0, 0.00002
    exported_spot = next(
        e
        for e in store.export(animation=False)["entities"]
        if e["id"] == "fixture-spot"
    )
    assert abs(exported_spot["light"]["shadow_radius"] - 0.18) < 1e-6
    assert exported_spot["light"]["shadow_near"] == 1.0
    assert exported_spot["light"]["shadow_bias"] == 0.00002
    for name, invalid in (("ipp_shadow_near", 20), ("ipp_shadow_bias", -1)):
        original = spot[name]
        spot[name] = invalid
        rejected = store.export(animation=False)
        assert not any(e["id"] == "fixture-spot" for e in rejected["entities"])
        assert any(name in d["message"] for d in rejected["diagnostics"])
        spot[name] = original
    del spot["ipp_shadow_near"], spot["ipp_shadow_bias"]
    spot.shadow_soft_size = old_radius
    first_error = skin_error(store, scene, "fixture-beam")
    assert first_error < 1e-5, first_error
    texture = store.data[initial["fixture-panel"]["texture"]["source"]]
    assert texture[16:] == bytes(
        [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]
    )
    panel_surface = bpy.data.objects["fixture-panel"].data.materials[0]
    panel_surface["ipp_unlit"] = False
    textured = {e["id"]: e for e in store.export(animation=False)["entities"]}[
        "fixture-panel"
    ]
    assert textured["material"]["type"] == "pbr"
    assert (
        textured["texture"]["source"] == initial["fixture-panel"]["texture"]["source"]
    )
    panel_surface["ipp_unlit"] = True

    assert decode_mesh(store.data[initial["fixture-panel"]["mesh"]["source"]])[2][
        0
    ] == (0.0, 1.0)
    fixture.apply_command({"action": "transform", "x": 0.7})
    transformed = {e["id"]: e for e in store.export(animation=False)["entities"]}
    assert transformed["fixture-cube"]["transform"]["x"] > 0.69
    assert transformed["fixture-cube"]["mesh"] == initial["fixture-cube"]["mesh"]
    fixture.apply_command({"action": "mesh"})
    changed = {e["id"]: e for e in store.export(animation=False)["entities"]}
    assert changed["fixture-cube"]["mesh"] != initial["fixture-cube"]["mesh"]
    bpy.context.scene.frame_set(25)
    scene = store.export(animation=False)
    bent = {e["id"]: e for e in scene["entities"]}
    assert bent["fixture-beam"]["mesh"] == initial["fixture-beam"]["mesh"]
    assert bent["fixture-beam"]["skin"] == initial["fixture-beam"]["skin"]
    assert (
        bent["fixture-rig"]["skeleton"]["source"]
        == initial["fixture-rig"]["skeleton"]["source"]
    )
    assert (
        bent["fixture-rig"]["skeleton"]["pose_source"]
        != initial["fixture-rig"]["skeleton"]["pose_source"]
    )
    bent_error = skin_error(store, scene, "fixture-beam")
    assert bent_error < 1e-5, bent_error
    # The inverse bind must account for distinct mesh and skeleton spaces.
    bpy.data.objects["fixture-beam"].location.x += 0.2
    bpy.context.view_layer.update()
    separate_error = skin_error(store, store.export(animation=False), "fixture-beam")
    assert separate_error < 1e-5, separate_error
    # An invalid pose must not leave a Skin referring to an omitted skeleton.
    bpy.data.objects["fixture-rig"].pose.bones["child"].scale.x = -1
    bpy.context.view_layer.update()
    invalid = store.export(animation=False)
    assert not any("skeleton" in e or "skin" in e for e in invalid["entities"])
    assert invalid["diagnostics"]
    fixture.create_scene()
    bpy.data.objects["fixture-panel"]["ipp_id"] = "fixture-cube"
    duplicate = store.export(animation=False)
    assert len({e["id"] for e in duplicate["entities"]}) == len(duplicate["entities"])
    assert any(d["code"] == "duplicate-id" for d in duplicate["diagnostics"])
    return {
        "rest_error": first_error,
        "bent_error": bent_error,
        "distinct_space_error": separate_error,
    }


def hierarchy_pose_checks():
    hierarchy_pose_fixture.create_scene()
    store = Store()
    scene = store.export()
    assert not scene["diagnostics"], scene["diagnostics"]
    entities = {e["id"]: e for e in scene["entities"]}
    panel = entities["fixture-panel"]
    cube = entities["fixture-cube"]
    assert panel["parent"] == cube["parent"] == "fixture-parent"
    parts = [
        e for e in scene["entities"] if e["id"].startswith("fixture-cube:material:")
    ]
    assert len(parts) == 2 and all(e["parent"] == cube["id"] for e in parts)
    assert all(transform(e) == Matrix.Identity(4) for e in parts)
    errors = []
    for name in ("fixture-cube", "fixture-panel", "fixture-parent"):
        obj = bpy.data.objects[name]
        expected = BASIS @ obj.matrix_world @ BASIS.inverted()
        actual = world_transform(entities[name], scene)
        errors.append(
            max(abs(actual[r][c] - expected[r][c]) for r in range(4) for c in range(4))
        )
    assert max(errors) < 1e-5, errors
    base = decode_mesh(store.data[panel["mesh"]["source"]])[0]
    target = decode_mesh(store.data[panel["mesh_pose"]["source"]])[0]
    animation = next(a for a in scene["animations"] if a["target"] == "fixture-panel")
    assert any(
        entry["target"] == animation["target"]
        and entry["source"] == animation["source"]
        for entry in scene["clips"]
    ), "Rigid mesh-pose actions must remain reusable"
    clip = json.loads(store.data[animation["source"]])
    assert clip["tracks"][0]["property"] == {
        "component": "MeshPose",
        "fields": ["weight"],
    }
    assert clip["tracks"][0]["keys"][12]["value"]["value"] == 0.5
    assert bpy.context.scene.frame_current == 1
    pose_errors = []
    for frame in (1, 13, 25):
        bpy.context.scene.frame_set(frame)
        updated = store.export(animation=False)
        current = next(e for e in updated["entities"] if e["id"] == "fixture-panel")
        assert current["mesh"] == panel["mesh"]
        assert current["mesh_pose"]["source"] == panel["mesh_pose"]["source"]
        assert skin_error(store, updated, "fixture-beam") < 1e-5
        weight = current["mesh_pose"]["weight"]
        obj = bpy.data.objects["fixture-panel"]
        evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
        mesh = evaluated.to_mesh()
        try:
            mesh.calc_loop_triangles()
            expected = [
                BASIS.to_3x3() @ mesh.vertices[i].co
                for tri in mesh.loop_triangles
                for i in tri.vertices
            ]
            actual = [
                Vector(a).lerp(Vector(b), weight)
                for a, b in zip(base, target, strict=True)
            ]
            pose_errors.append(
                max((a - b).length for a, b in zip(actual, expected, strict=True))
            )
        finally:
            evaluated.to_mesh_clear()
    assert max(pose_errors) < 1e-5, pose_errors
    # Rejected mesh animation cannot discard independent parent/rig clips, and
    # failed sampling restores the original subframe and authored key values.
    obj = bpy.data.objects["fixture-panel"]
    key = obj.data.shape_keys.key_blocks[1]
    key.slider_max = 2
    key.value = 2
    key.keyframe_insert("value", frame=25)
    bpy.context.scene.frame_set(1, subframe=0.25)
    rejected = store.export()
    assert any(
        d["code"] == "mesh-animation-unsupported" for d in rejected["diagnostics"]
    )
    assert any(a["target"] == "fixture-parent" for a in rejected["animations"])
    assert (
        bpy.context.scene.frame_current == 1
        and bpy.context.scene.frame_subframe == 0.25
    )
    obj.shape_key_add(name="Unsupported second target")
    rejected = store.export(animation=False)
    fallback = next(e for e in rejected["entities"] if e["id"] == "fixture-panel")
    assert "mesh" in fallback and "mesh_pose" not in fallback
    assert any(
        "one relative shape key" in d["message"] for d in rejected["diagnostics"]
    )
    return {"hierarchy_errors": errors, "mesh_pose_errors": pose_errors}


def fox_checks():
    path = ROOT / "tests/fixtures/blender/fox.blend"
    bpy.ops.wm.open_mainfile(filepath=str(path))
    assert {a.name for a in bpy.data.actions} == {
        "Run",
        "Survey",
        "Walk",
        "FoxBreathing",
    }
    assert len(bpy.data.objects["root"].data.bones) == 24
    assert "Icosphere" not in bpy.data.objects
    image = bpy.data.images["Image_0"]
    assert image.packed_file and image.size[:] == (1024, 1024)
    assert not Path(bpy.path.abspath(image.filepath)).exists()
    assert max(image.pixels[:]) > 0.5
    store = Store()
    rig = bpy.data.objects["root"]
    active, slot = rig.animation_data.action, rig.animation_data.action_slot
    muted = [track.mute for track in rig.animation_data.nla_tracks]
    first = store.export()
    assert {clip["name"] for clip in first["clips"]} >= {"Walk", "Run", "Survey"}
    assert (
        rig.animation_data.action == active and rig.animation_data.action_slot == slot
    )
    assert [track.mute for track in rig.animation_data.nla_tracks] == muted
    assert bpy.context.scene.frame_current == 0
    fox = next(e for e in first["entities"] if e["id"] == "fox-fox")
    assert "skin" in fox and "mesh_pose" in fox
    assert {a["target"] for a in first["animations"]} == {"fox-fox", "fox-root"}
    shape_clip = next(a for a in first["animations"] if a["target"] == "fox-fox")
    assert any(
        clip["target"] == shape_clip["target"]
        and clip["source"] == shape_clip["source"]
        for clip in first["clips"]
    ), "Mesh-pose actions must survive clips-only disk import"
    track = json.loads(store.data[shape_clip["source"]])["tracks"][0]
    assert track["property"] == {"component": "MeshPose", "fields": ["weight"]}
    assert track["keys"][12]["value"]["value"] == 0.5
    assert track["keys"][24]["value"]["value"] == 1
    errors, weights = [], []
    for frame in (0, 12, 24, 36, 48, 50):
        bpy.context.scene.frame_set(frame)
        current = store.export(animation=False)
        mesh = next(e for e in current["entities"] if e["id"] == "fox-fox")
        assert mesh["mesh"] == fox["mesh"]
        assert mesh["mesh_pose"]["source"] == fox["mesh_pose"]["source"]
        assert mesh["skin"] == fox["skin"]
        weights.append(mesh["mesh_pose"]["weight"])
        errors.append(skin_error(store, current, "fox"))
    assert weights == [0, 0.5, 1, 0.5, 0, 0], weights
    assert max(errors) < 2e-4, errors
    bpy.context.scene.frame_set(12)
    bpy.data.objects["fox"].modifiers.new("Unsupported post-skin smoothing", "SMOOTH")
    rejected = store.export(animation=False)
    assert not any(e["id"] == "fox-fox" for e in rejected["entities"])
    assert any(
        "Unsupported shape deformation with skin" in d["message"]
        for d in rejected["diagnostics"]
    )
    return {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "bytes": path.stat().st_size,
        "skin_errors": errors,
        "mesh_pose_weights": weights,
        "diagnostics": first["diagnostics"],
        "packed_texture_bytes": len(image.packed_file.data),
        "actions": [a.name for a in bpy.data.actions],
    }


def capacity_checks():
    capacity_fixture.create_scene()
    store = Store()
    scene = store.export()
    assert len(scene["entities"]) > 300
    panel = next(e for e in scene["entities"] if e["id"] == "fixture-panel")
    texture = store.data[panel["texture"]["source"]]
    assert struct.unpack_from("<III", texture, 4) == (3, 1024, 1536)
    assert len(texture) > 4 * 1024 * 1024
    grid = next(e for e in scene["entities"] if e["id"] == "capacity-dense-grid")
    mesh = store.data[grid["mesh"]["source"]]
    assert len(mesh) > 1024 * 1024
    clip = next(a for a in scene["animations"] if a["target"] == "fixture-rig")
    tracks = json.loads(store.data[clip["source"]])["tracks"]
    assert all(len(track["keys"]) == 241 for track in tracks)
    assert all(track["keys"][-1]["time"] == 10 for track in tracks)
    assert bpy.context.scene.frame_current == 1
    assert not scene["diagnostics"], scene["diagnostics"]
    return {
        "entities": len(scene["entities"]),
        "texture_bytes": len(texture),
        "mesh_bytes": len(mesh),
        "animation_samples": 241,
    }


def bone_parent_checks():
    fixture.create_scene()
    scene = bpy.context.scene
    rig = bpy.data.objects["fixture-rig"]
    attachment = bpy.data.objects.new("bone-tool", None)
    scene.collection.objects.link(attachment)
    attachment.parent = rig
    attachment.parent_type = "BONE"
    attachment.parent_bone = "child"
    attachment.matrix_parent_inverse = Matrix.Translation((0.12, 0.03, -0.2))
    attachment.location = (0.15, -0.1, 0.25)
    attachment.rotation_euler = (0.2, 0.1, -0.3)
    store = Store()
    results = []
    local = None
    for frame in (1, 7, 13, 19, 25):
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        exported = store.export()
        item = next(e for e in exported["entities"] if e["name"] == "bone-tool")
        assert item["parent"] == rig["ipp_id"] and item["parent_bone"] == 1
        actual_local = transform(item)
        if local is None:
            local = actual_local.copy()
        assert (
            max(
                abs(local[r][c] - actual_local[r][c])
                for r in range(4)
                for c in range(4)
            )
            < 2e-5
        )
        expected = BASIS @ attachment.matrix_world @ BASIS.inverted()
        parent = (
            BASIS @ rig.matrix_world @ rig.pose.bones["child"].matrix @ BASIS.inverted()
        )
        actual = parent @ actual_local
        error = max(
            abs(expected[r][c] - actual[r][c]) for r in range(4) for c in range(4)
        )
        assert error < 2e-5, error
        assert not any(a["target"] == item["id"] for a in exported["animations"])
        assert not any(d.get("entity") == item["id"] for d in exported["diagnostics"])
        assert scene.frame_current == frame
        results.append(error)
    # Failure while sampling a stashed clip must restore authoring state too.
    data = rig.animation_data
    action, slot = data.action, data.action_slot
    track = data.nla_tracks.new()
    strip = track.strips.new("Saved action", 1, action)
    strip.action_slot = slot
    track.mute = True
    data.action = None
    before = {bone.name: bone.matrix_basis.copy() for bone in rig.pose.bones}

    def fail_clip(data_bytes, content_type):
        if content_type == "application/json" and data.action == action:
            assert track.mute
            raise RuntimeError("Intentional clip publication failure")
        return store.publish(data_bytes, content_type)

    try:
        export_scene(fail_clip)
        raise AssertionError("Clip failure was hidden")
    except RuntimeError as error:
        assert str(error) == "Intentional clip publication failure"
    assert data.action is None and track.mute and scene.frame_current == 25
    assert all(
        max(
            abs(before[bone.name][r][c] - bone.matrix_basis[r][c])
            for r in range(4)
            for c in range(4)
        )
        < 2e-5
        for bone in rig.pose.bones
    )
    return {"frames": 5, "attachment_errors": results, "failed_export_restored": True}


def main():
    report = {
        "blender": bpy.app.version_string,
        "generated": generated_checks(),
        "capacity": capacity_checks(),
        "bone_parent": bone_parent_checks(),
        "hierarchy_mesh_pose": hierarchy_pose_checks(),
        "fox": fox_checks(),
    }
    destination = ROOT / "target/integration-artifacts/blender-export"
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "checks.json").write_text(json.dumps(report, indent=2) + "\n")
    print("IPP_EXPORT_CHECKS " + json.dumps(report))


if __name__ == "__main__":
    main()
