"""Deterministic Blender scene and test-only edits used by the real integration runner."""

import math

import bpy
from mathutils import Vector


def identify(obj, identifier):
    obj.name = identifier
    obj["ipp_id"] = identifier
    return obj


def material(name, color, *, unlit=False):
    result = bpy.data.materials.new(name)
    result.use_nodes = True
    result.diffuse_color = (*color, 1)
    result.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = (
        *color,
        1,
    )
    result.node_tree.nodes.get("Principled BSDF").inputs[
        "Roughness"
    ].default_value = 0.65
    result["ipp_unlit"] = unlit
    return result


def aim(obj, point):
    obj.rotation_euler = (
        (Vector(point) - obj.location).to_track_quat("-Z", "Y").to_euler()
    )


def add_camera_lights():
    scene = bpy.context.scene
    camera = identify(
        bpy.data.objects.new("fixture-camera", bpy.data.cameras.new("Fixture camera")),
        "fixture-camera",
    )
    scene.collection.objects.link(camera)
    camera.location = (4.0, -10.0, 4.0)
    aim(camera, (0.0, 0.0, 1.0))
    camera.data.lens = 48
    camera.data.clip_start = 0.05
    camera.data.clip_end = 100
    camera.data.dof.focus_distance = 10
    scene.camera = camera
    sun = identify(
        bpy.data.objects.new("fixture-sun", bpy.data.lights.new("Fixture sun", "SUN")),
        "fixture-sun",
    )
    scene.collection.objects.link(sun)
    sun.rotation_euler = (0.4, -0.6, -0.3)
    sun.data.energy = 2
    sun.data["ipp_intensity"] = 2.0
    spot = identify(
        bpy.data.objects.new(
            "fixture-spot", bpy.data.lights.new("Fixture spot", "SPOT")
        ),
        "fixture-spot",
    )
    scene.collection.objects.link(spot)
    spot.location = (0, -4, 5)
    aim(spot, (1, 0, 1))
    spot.data.energy = 700
    spot.data.spot_size = math.radians(70)
    spot.data.spot_blend = 0.3
    spot.data["ipp_intensity"] = 45.0
    spot.data["ipp_range"] = 20.0
    return camera


def create_scene():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    scene = bpy.context.scene
    scene.unit_settings.scale_length = 1
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 1, 25
    scene.render.resolution_x, scene.render.resolution_y = 640, 480
    scene.render.resolution_percentage = 100
    scene.world.use_nodes = True
    scene.world.node_tree.nodes.get("Background").inputs["Color"].default_value = (
        0.0,
        0.0,
        0.0,
        1.0,
    )
    scene.world.node_tree.nodes.get("Background").inputs["Strength"].default_value = 1.0

    bpy.ops.mesh.primitive_cube_add(size=0.8, location=(-0.25, 0, 0.5))
    cube = identify(bpy.context.object, "fixture-cube")
    cube.data.vertices[7].co.x += 0.2
    cube.data.materials.append(material("Fixture orange", (1.0, 0.23, 0.025)))
    cube.location.z = 0.5
    cube.keyframe_insert("location", index=2, frame=1)
    cube.location.z = 1.0
    cube.keyframe_insert("location", index=2, frame=25)

    mesh = bpy.data.meshes.new("Fixture texture panel")
    mesh.from_pydata(
        [(-0.65, 0, 0), (0.65, 0, 0), (0.65, 0, 1.7), (-0.65, 0, 1.7)],
        [],
        [(0, 1, 2, 3)],
    )
    mesh.uv_layers.new(name="UVMap")
    for loop, uv in zip(
        mesh.uv_layers.active.data, [(0, 0), (1, 0), (1, 1), (0, 1)], strict=True
    ):
        loop.uv = uv
    panel = identify(bpy.data.objects.new("fixture-panel", mesh), "fixture-panel")
    scene.collection.objects.link(panel)
    panel.location = (-1.8, 0, 0.15)
    image = bpy.data.images.new("Fixture RGB quadrants", width=2, height=2, alpha=False)
    image.pixels = [0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1]
    image.pack()
    surface = material("Fixture texture", (1, 1, 1), unlit=True)
    texture = surface.node_tree.nodes.new("ShaderNodeTexImage")
    texture.image = image
    texture.interpolation = "Closest"
    surface.node_tree.links.new(
        texture.outputs["Color"],
        surface.node_tree.nodes.get("Principled BSDF").inputs["Base Color"],
    )
    mesh.materials.append(surface)

    armature = bpy.data.armatures.new("Fixture two bones")
    rig = identify(bpy.data.objects.new("fixture-rig", armature), "fixture-rig")
    scene.collection.objects.link(rig)
    rig.location = (1.3, 0, 0)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    root = armature.edit_bones.new("root")
    root.head, root.tail = (0, 0, 0), (0, 0, 1)
    child = armature.edit_bones.new("child")
    child.head, child.tail = (0, 0, 1), (0, 0, 2)
    child.parent, child.use_connect = root, True
    bpy.ops.object.mode_set(mode="OBJECT")
    vertices = [
        (x, y, z / 4)
        for z in range(9)
        for x, y in [(-0.2, -0.2), (0.2, -0.2), (0.2, 0.2), (-0.2, 0.2)]
    ]
    faces = [(3, 2, 1, 0), (32, 33, 34, 35)]
    faces += [
        (
            row * 4 + corner,
            row * 4 + (corner + 1) % 4,
            (row + 1) * 4 + (corner + 1) % 4,
            (row + 1) * 4 + corner,
        )
        for row in range(8)
        for corner in range(4)
    ]
    mesh = bpy.data.meshes.new("Fixture closed beam")
    mesh.from_pydata(vertices, [], faces)
    beam = identify(bpy.data.objects.new("fixture-beam", mesh), "fixture-beam")
    scene.collection.objects.link(beam)
    beam.location = rig.location
    mesh.materials.append(material("Fixture blue", (0.08, 0.3, 0.95)))
    root_group, child_group = (
        beam.vertex_groups.new(name="root"),
        beam.vertex_groups.new(name="child"),
    )
    for index, (_, _, z) in enumerate(vertices):
        child_weight = max(0, min(1, z - 0.5))
        if child_weight < 1:
            root_group.add([index], 1 - child_weight, "REPLACE")
        if child_weight > 0:
            child_group.add([index], child_weight, "REPLACE")
    modifier = beam.modifiers.new("Fixture skin", "ARMATURE")
    modifier.object = rig
    pose = rig.pose.bones["child"]
    pose.rotation_mode = "XYZ"
    pose.rotation_euler.z = 0
    pose.keyframe_insert("rotation_euler", index=2, frame=1)
    pose.rotation_euler.z = -0.7
    pose.keyframe_insert("rotation_euler", index=2, frame=25)
    add_camera_lights()
    scene.frame_set(1)
    bpy.context.view_layer.update()


def apply_command(command):
    action = command["action"]
    cube = next(
        (obj for obj in bpy.data.objects if obj.get("ipp_id") == "fixture-cube"), None
    )
    if action == "transform":
        cube.location.x = float(command.get("x", 0.3))
    elif action == "rename":
        cube.name = command["name"]
    elif action == "swap_names":
        beam = next(
            obj for obj in bpy.data.objects if obj.get("ipp_id") == "fixture-beam"
        )
        cube_name, beam_name = cube.name, beam.name
        cube.name = "temporary-name-swap"
        beam.name = cube_name
        cube.name = beam_name
    elif action == "mesh":
        cube.data.vertices[0].co.x += float(command.get("delta", 0.45))
        cube.data.update()
    elif action == "material":
        color = tuple(command.get("color", [0.1, 1.0, 0.2]))
        surface = cube.data.materials[0]
        surface.diffuse_color = (*color, 1)
        surface.node_tree.nodes.get("Principled BSDF").inputs[
            "Base Color"
        ].default_value = (*color, 1)
    elif action == "textured_pbr":
        surface = bpy.data.objects["fixture-panel"].data.materials[0]
        surface["ipp_unlit"] = False
        surface.node_tree.nodes.get("Principled BSDF").inputs[
            "Roughness"
        ].default_value = 0.9
    elif action == "ambient":
        world = bpy.context.scene.world
        world.use_nodes = True
        background = world.node_tree.nodes.get("Background")
        background.inputs["Color"].default_value = (*command["color"], 1.0)
        background.inputs["Strength"].default_value = command.get("strength", 1.0)
    elif action == "lighting":
        for obj in bpy.context.scene.objects:
            if obj.type == "LIGHT":
                obj.data["ipp_intensity"] = (
                    (2.0 if obj.data.type == "SUN" else 45.0)
                    if command.get("restore", False)
                    else float(command["intensity"])
                )
    elif action == "pose":
        rig = bpy.data.objects["fixture-rig"]
        # Keep the action, but change its first key so export frame restoration is stable.
        pose = rig.pose.bones["child"]
        pose.rotation_euler.z = float(command.get("angle", -0.6))
        pose.keyframe_insert(
            "rotation_euler", index=2, frame=bpy.context.scene.frame_current
        )
    elif action == "remove":
        bpy.data.objects.remove(cube, do_unlink=True)
    else:
        raise ValueError(f"Unknown fixture action: {action}")
    bpy.context.view_layer.update()
