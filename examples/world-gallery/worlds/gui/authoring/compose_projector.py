"""Preview the exact runtime composition with a placeholder Surface and beam.

Run with Blender --background --python-exit-code 1 --python this_file.py.
The placeholder only establishes layout; React owns the actual GUI and shaders.
"""

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Quaternion, Vector

AUTHORING = Path(__file__).resolve().parent
sys.path.insert(0, str(AUTHORING))
import build_projector as author

BASIS = Matrix.Rotation(math.pi / 2, 4, "X")


def rotation(values):
    x, y, z, w = values
    return Quaternion((w, x, y, z))


def place(obj, position, quaternion, scale=1):
    basis = BASIS.to_quaternion()
    obj.location = author.blender_point(position)
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = basis @ quaternion @ basis.inverted()
    obj.scale = (scale,) * 3


def unlit_preview(obj, strength=1):
    mat = obj.data.materials[0]
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    shader = nodes.get("Principled BSDF")
    emission = nodes.new("ShaderNodeEmission")
    emission.inputs["Strength"].default_value = strength
    color = shader.inputs["Base Color"]
    if color.links:
        links.new(color.links[0].from_socket, emission.inputs["Color"])
    else:
        emission.inputs["Color"].default_value = color.default_value
    links.new(emission.outputs[0], nodes.get("Material Output").inputs["Surface"])


def beam_preview(obj):
    mat = obj.data.materials[0]
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    transparent = nodes.new("ShaderNodeBsdfTransparent")
    emission = nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = (0.04, 0.65, 0.9, 1)
    mix = nodes.new("ShaderNodeMixShader")
    mix.inputs[0].default_value = 0.025
    links.new(transparent.outputs[0], mix.inputs[1])
    links.new(emission.outputs[0], mix.inputs[2])
    links.new(mix.outputs[0], nodes.get("Material Output").inputs["Surface"])
    obj.hide_render = False


def placeholder_panel(center, quaternion, projection):
    width, height = [value * 2 for value in projection["farHalfSize"]]
    dark = author.material("Placeholder dark panel", (0.004, 0.02, 0.03), 0, 0.6)
    cyan = author.material("Placeholder cyan", (0.04, 0.7, 0.85), 0, 0.4, 1.8)
    white = author.material("Placeholder text", (0.3, 0.85, 0.93), 0, 0.4, 1)

    def box(name, local_center, size, mat):
        obj = author.box(
            name, local_center, size, mat, 0.015 if min(size) > 0.03 else 0
        )
        place(obj, center, quaternion)

    def text(body, x, y, size):
        bpy.ops.object.text_add()
        obj = bpy.context.object
        obj.name = body
        obj.data.body = body
        obj.data.size = size
        obj.data.extrude = 0.0003
        obj.data.materials.append(white)
        position = center + quaternion @ Vector((x, y, 0.04))
        obj.matrix_world = (
            Matrix.Translation(Vector(author.blender_point(position)))
            @ BASIS
            @ quaternion.to_matrix().to_4x4()
        )

    profile = author.rounded_profile(
        projection["farHalfSize"], projection["farCornerSpan"]
    )
    panel = author.mesh(
        "Placeholder Surface",
        [(x, y, 0) for x, y in profile],
        [tuple(range(len(profile)))],
        dark,
    )
    place(panel, center, quaternion)
    border = bpy.data.curves.new("Rounded panel border", "CURVE")
    border.dimensions = "3D"
    border.bevel_depth = 0.008
    border.bevel_resolution = 2
    spline = border.splines.new("POLY")
    spline.points.add(len(profile) - 1)
    for point, (x, y) in zip(spline.points, profile):
        point.co = (*author.blender_point((x, y, 0.02)), 1)
    spline.use_cyclic_u = True
    border.materials.append(cyan)
    obj = bpy.data.objects.new("Panel rounded border", border)
    bpy.context.collection.objects.link(obj)
    place(obj, center, quaternion)
    for y in (0.91, 0.12, -0.77):
        box("Section separator", (0, y, 0.025), (width - 0.45, 0.008, 0.006), cyan)
    box("Slider track", (-0.15, 0.47, 0.025), (3.7, 0.04, 0.01), cyan)
    box("Slider knob", (0.55, 0.47, 0.04), (0.1, 0.23, 0.02), white)
    text("GUI DEMO", -2.32, 1.16, 0.23)
    text("GAIN", -2.25, 0.65, 0.16)
    text("64%", 1.7, 0.55, 0.28)
    text("SCAN", -2.25, -0.21, 0.17)
    text("ONLINE", 0.45, -0.21, 0.16)
    text("PULSE", -2.25, -1.13, 0.24)
    text("TELEMETRY", 0.32, -1.12, 0.16)


def main():
    manifest = json.loads((author.ASSETS / "projector.json").read_text())
    layout, projection = manifest["composition"], manifest["projection"]
    bpy.ops.wm.open_mainfile(filepath=str(AUTHORING / "projector.blend"))
    bpy.context.preferences.filepaths.save_version = 0
    scene = bpy.context.scene
    center = Vector(layout["projector"]["position"])
    quaternion = rotation(layout["projector"]["rotation"])
    for name in ("shell", "trim", "aperture", "lens"):
        place(bpy.data.objects[name], center, quaternion, projection["cubeScale"])
    place(bpy.data.objects["frustum"], center, quaternion)
    for name in ("base", "floor"):
        part = layout[name]
        place(
            bpy.data.objects[name],
            part["position"],
            rotation(part["rotation"]),
            part["scale"],
        )
        unlit_preview(bpy.data.objects[name])
    unlit_preview(bpy.data.objects["lens"], 2)
    beam_preview(bpy.data.objects["frustum"])
    panel_center = center + quaternion @ Vector((0, 0, projection["farZ"]))
    placeholder_panel(panel_center, quaternion, projection)

    view, camera = layout["camera"], scene.camera
    camera.location = author.blender_point(view["position"])
    camera.rotation_euler = (
        (Vector(author.blender_point(view["target"])) - camera.location)
        .to_track_quat("-Z", "Y")
        .to_euler()
    )
    camera.data.sensor_fit = "VERTICAL"
    camera.data.sensor_height = 32
    camera.data.lens = 32 / (2 * math.tan(view["fovY"] / 2))
    camera.data.clip_start, camera.data.clip_end = view["near"], view["far"]
    scene.cycles.samples = 48
    scene.render.resolution_x, scene.render.resolution_y = 1100, 760
    scene.render.resolution_percentage = 100
    author.ARTIFACTS.mkdir(parents=True, exist_ok=True)
    scene.render.filepath = str(author.ARTIFACTS / "projector-scene-preview.png")
    bpy.ops.render.render(write_still=True)
    bpy.ops.wm.save_as_mainfile(
        filepath=str(author.ARTIFACTS / "projector-preview.blend")
    )
    print("PROJECTOR_COMPOSITION " + json.dumps(layout))


if __name__ == "__main__":
    main()
