"""Build the Surface projector, bake static scenery, and export ordinary IPP assets.

Run with Blender 5.2: blender --background --factory-startup --python-exit-code 1
--python examples/world-gallery/worlds/gui/authoring/build_projector.py
"""

import hashlib
import argparse
import json
import math
import struct
import sys
from pathlib import Path

import bpy
from mathutils import Vector

AUTHORING = Path(__file__).resolve().parent
ROOT = AUTHORING.parents[4]
ASSETS = ROOT / "target" / "gallery-gui-assets" / "projector"
ARTIFACTS = ROOT / "target" / "projector-authoring"
PARTS = {}
PROJECTION = {
    "cubeScale": 2.2,
    "frustumScale": 1,
    "nearZ": 1.0714,
    "nearHalfSize": [0.242, 0.242],
    "farZ": 5.1,
    "farHalfSize": [2.569, 1.659],
    "nearCornerSpan": [0.119 / 2.569 * 0.242, 0.119 / 1.659 * 0.242],
    "farCornerSpan": [0.119, 0.119],
    "cornerCurve": "quadratic-bezier",
    "cornerSegments": 6,
    "uv": "U around the perimeter, V from 0 at the port to 1 at the panel",
}
COMPOSITION = {
    "projector": {
        "position": [0, 0.3, 0],
        "rotation": [
            -math.cos(0.15) * math.sin(0.02),
            math.sin(0.15) * math.cos(0.02),
            math.sin(0.15) * math.sin(0.02),
            math.cos(0.15) * math.cos(0.02),
        ],
    },
    "base": {
        "position": [0, 0, 0],
        "rotation": [0, math.sin(0.15), 0, math.cos(0.15)],
        "scale": 1.55,
    },
    "floor": {
        "position": [0, 0, 0],
        "rotation": [0, math.sin(0.15), 0, math.cos(0.15)],
        "scale": 1.55,
    },
    "surface": {"width": 7.4, "height": 4.8, "scale": 0.7},
    "camera": {
        "position": [-8.2, 3.2, 18.2],
        "target": [0.5, -0.03, 2.3],
        "fovY": math.radians(21),
        "near": 0.1,
        "far": 100,
    },
}


def blender_point(point):
    """Model in runtime coordinates, then author in Blender's Z-up basis."""
    x, y, z = point
    return (x, -z, y)


def active(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def material(name, color, metallic=0, roughness=0.4, emission=0):
    result = bpy.data.materials.new(name)
    result.use_nodes = True
    result.use_fake_user = True
    shader = result.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*color, 1)
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = roughness
    shader.inputs["Emission Color"].default_value = (*color, 1)
    shader.inputs["Emission Strength"].default_value = emission
    return result


def textured_material(name, dark, light, metallic, roughness, scale):
    result = material(name, dark, metallic, roughness)
    nodes, links = result.node_tree.nodes, result.node_tree.links
    shader = nodes.get("Principled BSDF")
    texcoord = nodes.new("ShaderNodeTexCoord")
    # Fine directional grain replaces broad cloud-like oxidation. This color
    # variation survives the ordinary color bake without hiding planar highlights.
    mapping = nodes.new("ShaderNodeVectorMath")
    mapping.operation = "MULTIPLY"
    mapping.inputs[1].default_value = (1, 1, 10)
    links.new(texcoord.outputs["Object"], mapping.inputs[0])
    noise = nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value = scale
    noise.inputs["Detail"].default_value = 2
    noise.inputs["Roughness"].default_value = 0.5
    links.new(mapping.outputs["Vector"], noise.inputs["Vector"])
    ramp = nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position = 0.22
    ramp.color_ramp.elements[0].color = (*dark, 1)
    ramp.color_ramp.elements[1].position = 0.78
    ramp.color_ramp.elements[1].color = (*light, 1)
    links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], shader.inputs["Base Color"])
    fine = nodes.new("ShaderNodeTexNoise")
    fine.inputs["Scale"].default_value = 175
    fine.inputs["Detail"].default_value = 2
    links.new(texcoord.outputs["Object"], fine.inputs["Vector"])
    bump = nodes.new("ShaderNodeBump")
    bump.inputs["Strength"].default_value = 0.06
    bump.inputs["Distance"].default_value = 0.0007
    links.new(fine.outputs["Fac"], bump.inputs["Height"])
    links.new(bump.outputs["Normal"], shader.inputs["Normal"])
    return result


def crisp_planar_faces(obj):
    """Keep broad machined planes exact, including faces introduced by booleans."""
    normals = [normal.vector.copy() for normal in obj.data.corner_normals]
    for polygon in obj.data.polygons:
        if polygon.area > 0.002:
            polygon.use_smooth = False
            for index in polygon.loop_indices:
                normals[index] = polygon.normal.copy()
    # Explicit corner normals also survive joins and the exporter's normal stream;
    # Blender's flat-face flag alone does not replace inherited custom normals.
    obj.data.normals_split_custom_set(normals)


def finish(obj, name, mat, bevel=0):
    obj.name = name
    if mat:
        obj.data.materials.append(mat)
    active(obj)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    if bevel:
        modifier = obj.modifiers.new("Machined edge radius", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1 if bevel < 0.01 else 3
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    for polygon in obj.data.polygons:
        polygon.use_smooth = True
    modifier = obj.modifiers.new("Weighted face normals", "WEIGHTED_NORMAL")
    modifier.keep_sharp = True
    modifier.weight = 50
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    crisp_planar_faces(obj)
    return obj


def box(name, center, size, mat, bevel=0.01):
    bpy.ops.mesh.primitive_cube_add(size=1, location=blender_point(center))
    obj = bpy.context.object
    obj.dimensions = (size[0], size[2], size[1])
    return finish(obj, name, mat, bevel)


def mesh(name, vertices, faces, mat, bevel=0):
    data = bpy.data.meshes.new(name)
    data.from_pydata([blender_point(v) for v in vertices], [], faces)
    data.update()
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    active(obj)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    return finish(obj, name, mat, bevel)


def octagon(radius, cut=None):
    cut = radius * 0.35 if cut is None else cut
    return [
        (-radius + cut, -radius),
        (radius - cut, -radius),
        (radius, -radius + cut),
        (radius, radius - cut),
        (radius - cut, radius),
        (-radius + cut, radius),
        (-radius, radius - cut),
        (-radius, -radius + cut),
    ]


def ring(name, outer, inner, front, back, mat, axis="z", center=(0, 0, 0)):
    vertices = []
    for radius, depth in [(outer, back), (outer, front), (inner, front), (inner, back)]:
        for x, y in octagon(radius):
            point = (x, y, depth) if axis == "z" else (depth, y, x)
            vertices.append(tuple(point[i] + center[i] for i in range(3)))
    faces = []
    for layer in range(4):
        for i in range(8):
            j = (i + 1) % 8
            faces.append(
                (
                    layer * 8 + i,
                    layer * 8 + j,
                    ((layer + 1) % 4) * 8 + j,
                    ((layer + 1) % 4) * 8 + i,
                )
            )
    return mesh(name, vertices, faces, mat, 0.004)


def disk(name, radius, front, back, mat, axis="z", center=(0, 0, 0), sides=48):
    vertices = []
    for depth in (back, front):
        for i in range(sides):
            angle = 2 * math.pi * i / sides
            x, y = radius * math.cos(angle), radius * math.sin(angle)
            point = (x, y, depth) if axis == "z" else (depth, y, x)
            vertices.append(tuple(point[j] + center[j] for j in range(3)))
    faces = [tuple(reversed(range(sides))), tuple(range(sides, sides * 2))]
    faces.extend(
        (i, (i + 1) % sides, (i + 1) % sides + sides, i + sides) for i in range(sides)
    )
    return mesh(name, vertices, faces, mat, 0.002)


def combine(name, objects):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    obj = bpy.context.object
    obj.name = name
    obj["ipp_id"] = f"gui-projector-{name}"
    for index, mat in enumerate(obj.data.materials):
        if mat is None:
            obj.data.materials[index] = obj.data.materials[0]
    active(obj)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(68), island_margin=0.014)
    bpy.ops.object.mode_set(mode="OBJECT")
    crisp_planar_faces(obj)
    PARTS[name] = obj
    return obj


def area(name, center, target, color, energy, size):
    data = bpy.data.lights.new(name, "AREA")
    data.energy, data.color, data.shape, data.size = energy, color, "DISK", size
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    obj.location = blender_point(center)
    obj.rotation_euler = (
        (Vector(blender_point(target)) - obj.location)
        .to_track_quat("-Z", "Y")
        .to_euler()
    )
    return obj


def bake(obj, name, resolution, mode):
    image = bpy.data.images.new(name, width=resolution, height=resolution, alpha=False)
    image.generated_color = (0.003, 0.005, 0.008, 1)
    image.colorspace_settings.name = "sRGB"
    changed_outputs = []
    bake_targets = []
    for mat in obj.data.materials:
        nodes, links = mat.node_tree.nodes, mat.node_tree.links
        target = nodes.new("ShaderNodeTexImage")
        target.image = image
        nodes.active = target
        bake_targets.append((nodes, target))
        if mode == "EMIT":
            shader = nodes.get("Principled BSDF")
            output = nodes.get("Material Output")
            emission = nodes.new("ShaderNodeEmission")
            if shader.inputs["Base Color"].links:
                links.new(
                    shader.inputs["Base Color"].links[0].from_socket,
                    emission.inputs["Color"],
                )
            else:
                emission.inputs["Color"].default_value = shader.inputs[
                    "Base Color"
                ].default_value
            links.new(emission.outputs[0], output.inputs["Surface"])
            changed_outputs.append((mat, shader, output, emission))
    active(obj)
    scene = bpy.context.scene
    scene.render.bake.use_clear = True
    scene.render.bake.margin = 10
    scene.render.bake.use_pass_direct = True
    scene.render.bake.use_pass_indirect = True
    scene.render.bake.use_pass_color = True
    bpy.ops.object.bake(type=mode)
    path = ARTIFACTS / f"{name}.png"
    image.filepath_raw = str(path)
    image.file_format = "PNG"
    image.save()
    for mat, shader, output, emission in changed_outputs:
        mat.node_tree.links.new(shader.outputs[0], output.inputs["Surface"])
        mat.node_tree.nodes.remove(emission)
    loaded = bpy.data.images.load(str(path), check_existing=False)
    loaded.pack()
    for nodes, target in bake_targets:
        nodes.remove(target)
    bpy.data.images.remove(image)
    return loaded


def preview_camera():
    scene = bpy.context.scene
    bpy.ops.object.camera_add(location=blender_point((-2.9, 1.65, 3.6)))
    camera = bpy.context.object
    camera.rotation_euler = (
        (Vector(blender_point((0, -0.18, 0.1))) - camera.location)
        .to_track_quat("-Z", "Y")
        .to_euler()
    )
    camera.data.type = "PERSP"
    camera.data.lens = 46
    scene.camera = camera
    return camera


def runtime_texture(obj, name, image, metallic=0, roughness=0.5, unlit=False):
    mat = material(name, (1, 1, 1), metallic, roughness)
    mat["ipp_unlit"] = unlit
    texture = mat.node_tree.nodes.new("ShaderNodeTexImage")
    texture.image = image
    mat.node_tree.links.new(
        texture.outputs["Color"],
        mat.node_tree.nodes.get("Principled BSDF").inputs["Base Color"],
    )
    obj.data.materials.clear()
    obj.data.materials.append(mat)
    for face in obj.data.polygons:
        face.material_index = 0
    return mat


def build_projector():
    metal = textured_material(
        "Brushed titanium", (0.058, 0.076, 0.088), (0.074, 0.094, 0.108), 0.8, 0.28, 90
    )
    trim = material("Brushed bevels", (0.27, 0.34, 0.38), 0.85, 0.27)
    dark = material("Port ceramic", (0.009, 0.018, 0.024), 0.5, 0.36)
    cyan = material("Preview live cyan", (0.055, 0.78, 0.92), 0.2, 0.23, 3)
    body = box("Monocoque", (0, 0, 0), (0.97, 0.97, 0.97), metal, 0.065)
    cutter = disk("Port recess cutter", 0.262, 0.64, 0.345, None, sides=8)
    active(body)
    modifier = body.modifiers.new("Recessed projector port", "BOOLEAN")
    modifier.operation = "DIFFERENCE"
    modifier.object = cutter
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    bpy.data.objects.remove(cutter, do_unlink=True)
    shell = [body]
    trim_parts, dark_parts, glow = [], [], []
    # Restrained side cassettes replace the reference's intricate exposed machinery.
    for sign in (-1, 1):
        shell.append(
            box("Side cassette", (sign * 0.48, 0, 0), (0.03, 0.74, 0.74), metal, 0.038)
        )
        dark_parts.append(
            ring("Side gasket", 0.355, 0.335, sign * 0.498, sign * 0.477, dark, "x")
        )
        trim_parts.append(
            ring("Side inset bevel", 0.28, 0.267, sign * 0.499, sign * 0.488, trim, "x")
        )
        dark_parts.append(
            disk("Side induction well", 0.19, sign * 0.499, sign * 0.49, dark, "x")
        )
        trim_parts.append(
            ring(
                "Side induction frame",
                0.208,
                0.191,
                sign * 0.499,
                sign * 0.489,
                trim,
                "x",
            )
        )
        for y in (-0.295, 0.295):
            for z in (-0.29, 0.29):
                trim_parts.append(
                    disk(
                        "Recessed fastener",
                        0.012,
                        sign * 0.5,
                        sign * 0.494,
                        trim,
                        "x",
                        (0, y, z),
                        sides=8,
                    )
                )
        for y in (-0.24, 0.24):
            glow.append(
                box(
                    "Side status slit",
                    (sign * 0.498, y, 0),
                    (0.003, 0.012, 0.18),
                    cyan,
                    0.002,
                )
            )
    shell.append(box("Top cassette", (0, 0.48, 0), (0.77, 0.029, 0.75), metal, 0.022))
    for x in (-0.36, 0.36):
        trim_parts.append(
            box("Top rail", (x, 0.497, 0), (0.012, 0.005, 0.60), trim, 0.002)
        )
    for y in (-0.36, 0.36):
        shell.append(
            box("Front armor plate", (0, y, 0.484), (0.69, 0.20, 0.025), metal, 0.016)
        )
        for x in (-0.29, 0.29):
            trim_parts.append(
                disk(
                    "Front fastener", 0.012, 0.5, 0.496, trim, center=(x, y, 0), sides=8
                )
            )
        for x in (-0.18, -0.12, -0.06, 0, 0.06, 0.12, 0.18):
            dark_parts.append(
                box("Vent inset", (x, y, 0.499), (0.022, 0.06, 0.002), dark, 0.008)
            )
    trim_parts.append(ring("Port machined shoulder", 0.285, 0.243, 0.5, 0.465, trim))
    dark_parts.append(ring("Port recessed throat", 0.242, 0.177, 0.496, 0.392, dark))
    trim_parts.append(ring("Port inner bevel", 0.192, 0.176, 0.461, 0.431, trim))
    glow.append(ring("Port luminous seal", 0.226, 0.214, 0.487, 0.482, cyan))
    glow.append(disk("Projector optical lens", 0.163, 0.435, 0.420, cyan))
    combine("shell", shell)
    combine("trim", trim_parts)
    combine("aperture", dark_parts)
    combine("lens", glow)


def build_base():
    plinth = textured_material(
        "Plinth brushed alloy",
        (0.033, 0.047, 0.057),
        (0.045, 0.061, 0.073),
        0.45,
        0.37,
        95,
    )
    black = material("Recesses", (0.006, 0.012, 0.015), 0.4, 0.5)
    bar_cyan = material(
        "Internal continuous square lightbar", (0.04, 0.82, 1), 0.2, 0.2, 14
    )
    parts = []
    vertices = [
        (x, y, z)
        for radius, y in [(0.875, -0.895), (0.82, -0.73)]
        for x, z in octagon(radius, 0.04)
    ]
    faces = [tuple(reversed(range(8))), tuple(range(8, 16))]
    faces.extend((i, (i + 1) % 8, (i + 1) % 8 + 8, i + 8) for i in range(8))
    base = mesh(
        "Levitation plinth with recessed light openings", vertices, faces, plinth, 0.009
    )
    light_height = -0.841

    def cut_opening(cutter):
        active(base)
        modifier = base.modifiers.new("Internal lightbar opening", "BOOLEAN")
        modifier.operation = "DIFFERENCE"
        modifier.object = cutter
        bpy.ops.object.modifier_apply(modifier=modifier.name)
        bpy.data.objects.remove(cutter, do_unlink=True)

    # An enclosed cavity houses the complete square ring. Only the machined
    # horizontal slots expose it; no luminous bar is attached outside the shell.
    cavity_vertices = [
        (x, y, z)
        for y in (light_height - 0.022, light_height + 0.022)
        for x, z in octagon(0.845, 0.05)
    ]
    cut_opening(
        mesh("Internal square light cavity cutter", cavity_vertices, faces, None)
    )
    for side in (-1, 1):
        for along in (-0.55, 0.55):
            cut_opening(
                box(
                    "Front/back slit cutter",
                    (along, light_height, side * 0.865),
                    (0.29, 0.032, 0.13),
                    None,
                    0.003,
                )
            )
            cut_opening(
                box(
                    "Side slit cutter",
                    (side * 0.865, light_height, along),
                    (0.13, 0.032, 0.29),
                    None,
                    0.003,
                )
            )
        parts.append(
            box(
                "Internal square lightbar X side",
                (side * 0.815, light_height, 0),
                (0.025, 0.026, 1.655),
                bar_cyan,
                0.002,
            )
        )
        parts.append(
            box(
                "Internal square lightbar Z side",
                (0, light_height, side * 0.815),
                (1.605, 0.026, 0.025),
                bar_cyan,
                0.002,
            )
        )
    for side in (-1, 1):
        for along in (-0.55, 0.55):
            for origin, direction in [
                ((along, light_height, side * 1.1), (0, 0, -side)),
                ((side * 1.1, light_height, along), (-side, 0, 0)),
            ]:
                hit, point, _, _ = base.ray_cast(
                    Vector(blender_point(origin)), Vector(blender_point(direction))
                )
                assert not hit or (point - Vector(blender_point(origin))).length > 0.5
        origin = Vector(blender_point((0, light_height, side * 1.1)))
        hit, point, _, _ = base.ray_cast(origin, Vector(blender_point((0, 0, -side))))
        assert hit and (point - origin).length < 0.4
    crisp_planar_faces(base)
    parts.append(base)
    # The recessed circular mount belongs to the fixed plinth, not to the floating body.
    bpy.ops.mesh.primitive_torus_add(
        major_radius=0.52,
        minor_radius=0.009,
        major_segments=64,
        minor_segments=8,
        location=blender_point((0, -0.725, 0)),
    )
    parts.append(finish(bpy.context.object, "Mount groove", black))
    return combine("base", parts)


def build_floor():
    stone = textured_material(
        "Quiet graphite tiles",
        (0.018, 0.026, 0.032),
        (0.025, 0.034, 0.040),
        0.12,
        0.62,
        110,
    )
    tiles = []
    for x in range(9):
        for z in range(10):
            tiles.append(
                box(
                    "Floor tile",
                    (-3.3 + (x + 0.5) * 6.6 / 9, -0.922, -2.5 + (z + 0.5) * 7.3 / 10),
                    (6.6 / 9 - 0.009, 0.044, 7.3 / 10 - 0.009),
                    stone,
                    0.008,
                )
            )
    return combine("floor", tiles)


def rounded_profile(half_size, corner_span):
    """Sample quadratic corners of the rounded panel outline, keeping straight edges."""
    width, height = half_size
    rx, ry = corner_span
    corners = [
        ((width - rx, -height), (width, -height), (width, -height + ry)),
        ((width, height - ry), (width, height), (width - rx, height)),
        ((-width + rx, height), (-width, height), (-width, height - ry)),
        ((-width, -height + ry), (-width, -height), (-width + rx, -height)),
    ]
    points = []
    for start, control, end in corners:
        for step in range(PROJECTION["cornerSegments"] + 1):
            t = step / PROJECTION["cornerSegments"]
            points.append(
                tuple(
                    (1 - t) ** 2 * start[i]
                    + 2 * (1 - t) * t * control[i]
                    + t**2 * end[i]
                    for i in range(2)
                )
            )
    return points


def build_frustum():
    near, far = PROJECTION["nearZ"], PROJECTION["farZ"]
    assert math.hypot(*PROJECTION["nearHalfSize"]) < 0.163 * PROJECTION["cubeScale"]
    near_profile = rounded_profile(
        PROJECTION["nearHalfSize"], PROJECTION["nearCornerSpan"]
    )
    far_profile = rounded_profile(
        PROJECTION["farHalfSize"], PROJECTION["farCornerSpan"]
    )
    count = len(far_profile)
    vertices = [
        (x, y, z)
        for profile, z in [(near_profile, near), (far_profile, far)]
        for x, y in profile
    ]
    faces = [
        (i, (i + 1) % count, (i + 1) % count + count, i + count) for i in range(count)
    ]
    perimeter = [0.0]
    for i, point in enumerate(far_profile):
        other = far_profile[(i + 1) % count]
        perimeter.append(perimeter[-1] + math.dist(point, other))
    mat = material("Runtime projection volume", (0.04, 0.6, 0.8))
    mat["ipp_unlit"] = True
    obj = mesh("frustum", vertices, faces, mat)
    obj["ipp_id"] = "gui-projector-frustum"
    layer = obj.data.uv_layers.new(name="Projection coordinates")
    for face in obj.data.polygons:
        for loop_index in face.loop_indices:
            vertex = obj.data.vertices[obj.data.loops[loop_index].vertex_index]
            next_corner = vertex.index % count != face.index
            u = perimeter[face.index + int(next_corner)] / perimeter[-1]
            # The maintained exporter flips Blender V to runtime's top-left basis.
            layer.data[loop_index].uv = (u, 1 - (-vertex.co.y - near) / (far - near))
    PARTS["frustum"] = obj


def mesh_report(data, projection=False):
    version, count, indices, attributes = struct.unpack_from("<4I", data, 4)
    assert data[:4] == b"IPPM" and version == 3 and count == indices
    offset = 20 + attributes * 8
    streams = {}
    for index in range(attributes):
        semantic, fmt, reserved, size = struct.unpack_from(
            "<BBHI", data, 20 + index * 8
        )
        assert reserved == 0 and fmt in (1, 2)
        streams[semantic] = struct.unpack_from(f"<{size // 4}f", data, offset)
        offset += size
    assert set(streams) == {0, 2, 4}
    assert len(data) == offset + indices * 2
    assert all(math.isfinite(v) for stream in streams.values() for v in stream)
    normals = streams[4]
    assert all(
        abs(sum(normals[i + j] ** 2 for j in range(3)) - 1) < 2e-4
        for i in range(0, len(normals), 3)
    )
    assert all(-1e-5 <= value <= 1.00001 for value in streams[2])
    positions = streams[0]
    if projection:
        assert count == 24 * (PROJECTION["cornerSegments"] + 1)
        for triangle in range(0, count, 3):
            # Every triangle spans the two endpoint planes: there are no caps.
            assert (
                len({round(positions[(triangle + i) * 3 + 2], 5) for i in range(3)})
                == 2
            )
        for index in range(count):
            distance = (positions[index * 3 + 2] - PROJECTION["nearZ"]) / (
                PROJECTION["farZ"] - PROJECTION["nearZ"]
            )
            assert abs(streams[2][index * 2 + 1] - distance) < 1e-6
    return {
        "vertices": count,
        "triangles": indices // 3,
        "bounds": [
            [min(positions[axis::3]) for axis in range(3)],
            [max(positions[axis::3]) for axis in range(3)],
        ],
        "attributes": ["position", "uv", "normal"],
    }


def export(output_directory=ASSETS, verify=False, projection_only=False):
    sys.path.insert(0, str(ROOT / "integrations" / "blender"))
    from ipp_blender.exporter import export_scene

    payloads = {}

    def write_asset(path, data):
        if verify or (projection_only and path.name != "frustum.ippm"):
            assert path.read_bytes() == data, f"Regenerated asset differs: {path.name}"
        else:
            path.write_bytes(data)

    def publish(data, _content_type):
        key = hashlib.sha256(data).hexdigest()
        payloads[key] = data
        return key

    for obj in bpy.context.scene.objects:
        obj.hide_render = obj.name not in PARTS
    for obj in PARTS.values():
        for polygon in obj.data.polygons:
            if polygon.area > 0.002:
                assert all(
                    polygon.normal.dot(obj.data.corner_normals[index].vector)
                    > math.cos(math.radians(0.05))
                    for index in polygon.loop_indices
                ), f"Nonplanar corner normal on {obj.name} face {polygon.index}"
    snapshot = export_scene(publish, animation=False)
    assert not snapshot["diagnostics"], snapshot["diagnostics"]
    manifest = {
        "generator": "build_projector.py",
        "blender": bpy.app.version_string,
        "projection": PROJECTION,
        "composition": COMPOSITION,
        "staticLighting": {
            "plinthLightHeight": -0.841,
            "plinthOpenings": 8,
            "excluded": ["projector", "frustum", "panel"],
        },
        "parts": {},
    }
    for entity in snapshot["entities"]:
        name = entity["name"]
        assert [entity["transform"][axis] for axis in ("x", "y", "z")] == [
            0.0,
            0.0,
            0.0,
        ]
        assert [entity["transform"][axis] for axis in ("sx", "sy", "sz")] == [
            1.0,
            1.0,
            1.0,
        ]
        assert [entity["transform"][axis] for axis in ("qx", "qy", "qz", "qw")] == [
            0.0,
            0.0,
            0.0,
            1.0,
        ]
        data = payloads[entity["mesh"]["source"]]
        path = output_directory / f"{name}.ippm"
        write_asset(path, data)
        report = mesh_report(data, projection=name == "frustum")
        report.update(
            mesh=path.name,
            bytes=len(data),
            sha256=hashlib.sha256(data).hexdigest(),
            material=entity["material"],
        )
        if "texture" in entity:
            data = payloads[entity["texture"]["source"]]
            filename = (
                f"{name}-baked.ippt" if name in ("base", "floor") else "metal-base.ippt"
            )
            write_asset(output_directory / filename, data)
            version, width, height = struct.unpack_from("<3I", data, 4)
            assert (
                data[:4] == b"IPPT"
                and version == 3
                and len(data) == 16 + width * height * 4
            )
            assert all(v == 255 for v in data[19::4])
            report["texture"] = {
                "source": filename,
                "bytes": len(data),
                "width": width,
                "height": height,
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        manifest["parts"][name] = report
    if verify:
        assert json.loads((output_directory / "projector.json").read_text()) == manifest
        print(
            "PROJECTOR_VERIFY all seven meshes and three textures match the saved blend"
        )
    else:
        (output_directory / "projector.json").write_text(
            json.dumps(manifest, indent=2) + "\n"
        )
    print("PROJECTOR_EXPORT " + json.dumps(manifest))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--export-only", action="store_true")
    modes.add_argument("--verify-export", action="store_true")
    modes.add_argument("--refresh-projection", action="store_true")
    parser.add_argument("--output-directory", type=Path, default=ASSETS)
    args = parser.parse_args(
        sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    )
    if not args.verify_export:
        args.output_directory.mkdir(parents=True, exist_ok=True)
    if args.refresh_projection:
        bpy.context.preferences.filepaths.save_version = 0
        bpy.data.objects.remove(bpy.data.objects["frustum"], do_unlink=True)
        build_frustum()
        for name in ("shell", "trim", "aperture", "lens", "base", "floor"):
            PARTS[name] = bpy.data.objects[name]
        hidden = {obj.name: obj.hide_render for obj in bpy.context.scene.objects}
        export(args.output_directory, projection_only=True)
        for obj in bpy.context.scene.objects:
            obj.hide_render = hidden[obj.name] or obj.name == "frustum"
        bpy.ops.wm.save_as_mainfile(filepath=str(AUTHORING / "projector.blend"))
        return
    if args.verify_export or args.export_only:
        for name in ("shell", "trim", "aperture", "lens", "base", "floor", "frustum"):
            PARTS[name] = bpy.data.objects[name]
        export(args.output_directory, verify=args.verify_export)
        return
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.preferences.filepaths.save_version = 0
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 48
    scene.cycles.use_denoising = True
    scene.cycles.seed = 19
    scene.world = bpy.data.worlds.new("Dark studio")
    scene.world.use_nodes = True
    background = scene.world.node_tree.nodes.get("Background")
    background.inputs["Color"].default_value = (0.045, 0.072, 0.105, 1)
    background.inputs["Strength"].default_value = 0.22
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGB"
    build_projector()
    base = build_base()
    floor = build_floor()
    area("Soft key", (-2.4, 3.2, 1.2), (0, -0.25, 0), (0.62, 0.79, 1), 120, 3)
    area("Edge light", (1.3, 1.8, -2), (0, 0, 0), (0.22, 0.57, 0.75), 100, 2)
    area("Warm base fill", (2, 1.3, 1.8), (0, -0.9, 0.5), (1, 0.69, 0.43), 35, 2)
    preview_camera()
    scene.cycles.samples = 24
    scene.render.resolution_x, scene.render.resolution_y = 900, 720
    scene.render.resolution_percentage = 100
    preview = ROOT / "target" / "projector-authoring" / "material-preview.png"
    preview.parent.mkdir(parents=True, exist_ok=True)
    scene.render.filepath = str(preview)
    bpy.ops.render.render(write_still=True)
    scene.cycles.samples = 48
    for name, obj in PARTS.items():
        obj.hide_render = name not in ("base", "floor")
    # Bake soft ambient fill into fixed scenery. The live body is lit by the app;
    # restoring the studio world afterward also preserves the preview backdrop.
    background.inputs["Strength"].default_value = 2.2
    base_image = bake(base, "base-baked", 1024, "COMBINED")
    floor_image = bake(floor, "floor-baked", 1024, "COMBINED")
    background.inputs["Strength"].default_value = 0.22
    base_image.scale(512, 512)
    base_image.pack()
    base_material = runtime_texture(base, "Baked static base", base_image, unlit=True)
    floor_material = runtime_texture(
        floor, "Baked static floor", floor_image, unlit=True
    )
    for obj in PARTS.values():
        obj.hide_render = False
    metal_image = bake(PARTS["shell"], "metal-base", 1024, "EMIT")
    runtime_texture(PARTS["shell"], "Textured titanium", metal_image, 0.8, 0.28)
    # Keep glow authored as an ordinary factor material; runtime supplies gain/pulse.
    live = material("Runtime cyan", (0.055, 0.78, 0.92))
    live["ipp_unlit"] = True
    PARTS["lens"].data.materials.clear()
    PARTS["lens"].data.materials.append(live)
    for face in PARTS["lens"].data.polygons:
        face.material_index = 0
    build_frustum()
    export(args.output_directory)
    for obj in scene.objects:
        obj.hide_render = obj.name == "frustum"
    # Preview uses the baked scenery as emission so it is not lit twice.
    for mat in (base_material, floor_material, live):
        nodes, links = mat.node_tree.nodes, mat.node_tree.links
        shader = nodes.get("Principled BSDF")
        emission = nodes.new("ShaderNodeEmission")
        if shader.inputs["Base Color"].links:
            links.new(
                shader.inputs["Base Color"].links[0].from_socket,
                emission.inputs["Color"],
            )
        else:
            emission.inputs["Color"].default_value = shader.inputs[
                "Base Color"
            ].default_value
        links.new(emission.outputs[0], nodes.get("Material Output").inputs["Surface"])
    scene.render.resolution_x, scene.render.resolution_y = 1200, 960
    scene.render.resolution_percentage = 100
    scene.render.filepath = str(ARTIFACTS / "projector-preview.png")
    bpy.ops.render.render(write_still=True)
    # Save the exportable material graph, retaining preview lighting/camera for inspection.
    for mat in (base_material, floor_material, live):
        shader = mat.node_tree.nodes.get("Principled BSDF")
        mat.node_tree.links.new(
            shader.outputs[0],
            mat.node_tree.nodes.get("Material Output").inputs["Surface"],
        )
        mat.node_tree.nodes.remove(
            next(node for node in mat.node_tree.nodes if node.type == "EMISSION")
        )
    bpy.ops.wm.save_as_mainfile(filepath=str(AUTHORING / "projector.blend"))


if __name__ == "__main__":
    main()
