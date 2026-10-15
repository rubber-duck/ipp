"""Reproduce the editable bridge and export scene; run through Blender MCP.

The Preview collection illustrates GUI placement only. IPP provides live controls.
"""

import math
from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix, Vector

ROOT = Path(__file__).resolve().parent
STATIC = []
ANCHORS = []
COLLECTION = None
PREVIEW = None


def activate(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def put(obj, name, mat=None, preview=False, static=True):
    obj.name = name
    obj["ipp_id"] = name
    for collection in list(obj.users_collection):
        collection.objects.unlink(obj)
    (PREVIEW if preview else COLLECTION).objects.link(obj)
    if mat:
        obj.data.materials.append(mat)
    if preview:
        obj["cockpit_preview_only"] = True
    elif static and obj.type == "MESH":
        STATIC.append(obj)
        obj["cockpit_static"] = True
    return obj


def material(name, color, metal=0, rough=0.35, emission=0):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    for socket, value in (
        ("Base Color", (*color, 1)),
        ("Metallic", metal),
        ("Roughness", rough),
        ("Emission Color", (*color, 1)),
        ("Emission Strength", emission),
    ):
        bsdf.inputs[socket].default_value = value
    return mat


def box(name, center, size, mat, bevel=0.04, matrix=None, preview=False):
    bpy.ops.mesh.primitive_cube_add(size=1, location=center)
    obj = put(bpy.context.object, name, mat, preview)
    obj.dimensions = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = obj.modifiers.new("Continuous softened edge", "BEVEL")
        mod.width = bevel
        mod.segments = 5
        obj.modifiers.new("Weighted face normals", "WEIGHTED_NORMAL")
    if matrix:
        obj.matrix_world = matrix @ Matrix.Translation(center)
    return obj


def rounded(w, h, r):
    result = []
    for cx, cy, start in (
        (w / 2 - r, h / 2 - r, 0),
        (-w / 2 + r, h / 2 - r, 90),
        (-w / 2 + r, -h / 2 + r, 180),
        (w / 2 - r, -h / 2 + r, 270),
    ):
        for j in range(10):
            a = math.radians(start + j * 10)
            result.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return result


def line(name, points, radius, mat, preview=False, matrix=None):
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    curve.resolution_u = 1
    curve.bevel_depth = radius
    curve.bevel_resolution = 3
    spline = curve.splines.new("POLY")
    spline.points.add(len(points) - 1)
    for dest, xyz in zip(spline.points, points):
        dest.co = (*xyz, 1)
    obj = put(bpy.data.objects.new(name, curve), name, mat, preview, False)
    if matrix:
        obj.matrix_world = matrix
    if not preview:
        activate(obj)
        bpy.ops.object.convert(target="MESH")
        STATIC.append(obj)
        obj["cockpit_static"] = True
    return obj


def circle(
    name, radius, z, mat, matrix, thickness=0.007, start=0, end=math.tau, preview=False
):
    return line(
        name,
        [
            (radius * math.cos(a), radius * math.sin(a), z)
            for a in np.linspace(
                start, end, max(8, int(100 * (end - start) / math.tau))
            )
        ],
        thickness,
        mat,
        preview,
        matrix,
    )


def text(name, value, xy, size, mat, matrix, align="LEFT"):
    curve = bpy.data.curves.new(name, "FONT")
    curve.body = value
    curve.size = size
    curve.align_x = align
    curve.space_character = 1.05
    obj = put(bpy.data.objects.new(name, curve), name, mat, True, False)
    obj.matrix_world = matrix @ Matrix.Translation((xy[0], xy[1], 0.055))
    return obj


def anchor(name, matrix, width, height, role):
    obj = put(bpy.data.objects.new(name, None), name, static=False)
    # Export changes Blender Z-up coordinates to engine Y-up. This empty's
    # local X/Z plane therefore becomes the engine Surface's local X/Y plane.
    obj.matrix_world = matrix @ Matrix.Rotation(-math.pi / 2, 4, "X")
    obj.empty_display_type = "ARROWS"
    obj.empty_display_size = 0.3
    for key, value in (
        ("surface_width", width),
        ("surface_height", height),
        ("surface_role", role),
    ):
        obj[key] = value
    ANCHORS.append(obj)
    return obj


def image(name, pixels):
    h, w = pixels.shape[:2]
    im = bpy.data.images.new(name, width=w, height=h, alpha=False)
    rgba = np.ones((h, w, 4), dtype=np.float32)
    rgba[:, :, :3] = np.clip(pixels, 0, 1)
    im.pixels.foreach_set(rgba.ravel())
    im.filepath_raw = str(ROOT / (name + ".png"))
    im.file_format = "PNG"
    im.save()
    im.pack()
    im.filepath = "//" + name + ".png"
    return im


def noise(x, y, z):
    ix, iy, iz = np.floor(x), np.floor(y), np.floor(z)
    fx, fy, fz = x - ix, y - iy, z - iz
    fx, fy, fz = (v * v * (3 - 2 * v) for v in (fx, fy, fz))
    out = np.zeros_like(x)
    for dx in (0, 1):
        for dy in (0, 1):
            for dz in (0, 1):
                v = (
                    np.sin((ix + dx) * 127.1 + (iy + dy) * 311.7 + (iz + dz) * 74.7)
                    * 43758.5453
                )
                out += (
                    (v - np.floor(v))
                    * (fx if dx else 1 - fx)
                    * (fy if dy else 1 - fy)
                    * (fz if dz else 1 - fz)
                )
    return out


def fbm(x, y, z, octaves=6):
    total = np.zeros_like(x)
    norm, amp = 0, 1
    for _ in range(octaves):
        total += amp * noise(x, y, z)
        norm += amp
        x, y, z = x * 2.03 + 17.1, y * 2.03 - 8.7, z * 2.03 + 3.4
        amp *= 0.5
    return total / norm


def textured(name, im, emission=False):
    mat = material(name, (1, 1, 1), rough=0.7)
    nt = mat.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = im
    shader = nt.nodes.get("Principled BSDF")
    nt.links.new(
        tex.outputs["Color"],
        shader.inputs["Emission Color"] if emission else shader.inputs["Base Color"],
    )
    if emission:
        shader.inputs["Emission Strength"].default_value = 1
        shader.inputs["Base Color"].default_value = (0, 0, 0, 1)
    return mat


def space():
    h, w = 1536, 3072
    rng = np.random.default_rng(8976)
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    mist = fbm(xx / 180, yy / 180, xx * 0 + 7, 5)
    band = np.exp(-(((yy / h - 0.55 - 0.20 * np.sin(xx / w * 7)) / 0.15) ** 2))
    rgb = np.zeros((h, w, 3), np.float32)
    for c, level in enumerate((0.04, 0.08, 0.14)):
        rgb[:, :, c] = level + band * np.maximum(mist - 0.35, 0) * (0.10, 0.18, 0.30)[c]
    for _ in range(24000):
        x, y = int(rng.integers(4, w - 4)), int(rng.integers(4, h - 4))
        bright = 0.10 + float(rng.power(0.55)) * 0.90
        color = (
            np.array((0.75, 0.86, 1))
            if rng.random() < 0.8
            else np.array((1, 0.84, 0.63))
        )
        sigma = 0.48 if bright < 0.7 else 0.85
        for dy in range(-3, 4):
            for dx in range(-3, 4):
                rgb[y + dy, x + dx] += (
                    color
                    * bright
                    * math.exp(-(dx * dx + dy * dy) / (2 * sigma * sigma))
                )
    stars = image("cockpit-stars", rgb)
    bpy.ops.mesh.primitive_plane_add(size=2)
    backdrop = put(
        bpy.context.object,
        "cockpit-space",
        textured("Deep space packed stars", stars, True),
        static=False,
    )
    backdrop.location = (0, 110, 20)
    backdrop.rotation_euler = (math.pi / 2, 0, 0)
    backdrop.scale = (120, 60, 1)
    u, v = np.meshgrid(
        np.linspace(0, math.tau, 2048), np.linspace(-math.pi / 2, math.pi / 2, 1024)
    )
    x, y, z = np.cos(v) * np.cos(u), np.cos(v) * np.sin(u), np.sin(v)
    terrain = fbm(x * 4 + 1, y * 4 - 2, z * 4 + 4, 7)
    detail = fbm(x * 38, y * 38, z * 38, 4)
    land = np.clip((terrain - 0.515) * 40, 0, 1)
    sea = np.stack(
        (0.014 + detail * 0.015, 0.07 + detail * 0.045, 0.14 + detail * 0.085), axis=-1
    )
    ground = np.stack(
        (0.12 + detail * 0.12, 0.13 + detail * 0.12, 0.075 + detail * 0.085), axis=-1
    )
    rgb = sea * (1 - land[:, :, None]) + ground * land[:, :, None]
    curl = fbm(x * 5 + 13, y * 5, z * 5, 4)
    clouds = np.clip(
        (fbm(x * 13 + curl * 3, y * 13 + curl * 4, z * 13, 6) - 0.49) * 6, 0, 0.92
    )
    rgb = (
        rgb * (1 - clouds[:, :, None])
        + np.array((0.78, 0.83, 0.9)) * clouds[:, :, None]
    )
    ice = np.clip((abs(z) - 0.89) * 7, 0, 1)
    rgb = rgb * (1 - ice[:, :, None]) + np.array((0.7, 0.8, 0.9)) * ice[:, :, None]
    texture = image("cockpit-planet-albedo", rgb)
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=128, ring_count=64, radius=50, location=(-61, 60, -22)
    )
    planet = put(
        bpy.context.object,
        "cockpit-planet",
        textured("Pelagos ocean and cloud cover", texture),
        static=False,
    )
    planet.rotation_euler = (0.4, 0.2, -0.55)
    for face in planet.data.polygons:
        face.use_smooth = True
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=128, ring_count=64, radius=50.3833333, location=planet.location
    )
    atmosphere = put(bpy.context.object, "cockpit-atmosphere", static=False)
    for face in atmosphere.data.polygons:
        face.use_smooth = True
    mat = bpy.data.materials.new("Atmosphere grazing blue scattering")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    output = nt.nodes.new("ShaderNodeOutputMaterial")
    transparent = nt.nodes.new("ShaderNodeBsdfTransparent")
    emission = nt.nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = (0.12, 0.48, 1, 1)
    emission.inputs["Strength"].default_value = 2.1
    layer = nt.nodes.new("ShaderNodeLayerWeight")
    power = nt.nodes.new("ShaderNodeMath")
    power.operation = "POWER"
    power.inputs[1].default_value = 6
    nt.links.new(layer.outputs["Facing"], power.inputs[0])
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(power.outputs[0], mix.inputs[0])
    nt.links.new(transparent.outputs[0], mix.inputs[1])
    nt.links.new(emission.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], output.inputs[0])
    atmosphere.data.materials.append(mat)
    atmosphere["runtime_material"] = "animated-atmosphere"


def area(name, location, target, energy, color, size):
    data = bpy.data.lights.new(name, "AREA")
    data.energy, data.color, data.shape, data.size = energy, color, "DISK", size
    obj = put(bpy.data.objects.new(name, data), name, static=False)
    obj.location = location
    obj.rotation_euler = (
        (Vector(target) - obj.location).to_track_quat("-Z", "Y").to_euler()
    )


def glass_panel(index, center, width, height, yaw, mats):
    matrix = (
        Matrix.Translation(center)
        @ Matrix.Rotation(yaw, 4, "Z")
        @ Matrix.Rotation(math.pi / 2, 4, "X")
    )
    anchor(
        f"cockpit-screen-{index}",
        matrix,
        width,
        height,
        ("controls", "values", "telemetry")[index],
    )
    mat = bpy.data.materials.new(f"Preview glass {index}")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    transparent = nt.nodes.new("ShaderNodeBsdfTransparent")
    transparent.inputs["Color"].default_value = (0.62, 0.74, 0.80, 1)
    glass = nt.nodes.new("ShaderNodeBsdfGlass")
    glass.inputs["Color"].default_value = (0.46, 0.62, 0.72, 1)
    glass.inputs["Roughness"].default_value = 0.23
    glass.inputs["IOR"].default_value = 1.42
    mix = nt.nodes.new("ShaderNodeMixShader")
    mix.inputs[0].default_value = 0.48
    nt.links.new(transparent.outputs[0], mix.inputs[1])
    nt.links.new(glass.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs[0])
    box(
        f"Preview glass {index}",
        (0, 0, 0),
        (width, height, 0.018),
        mat,
        0.075,
        matrix,
        True,
    )
    outline = rounded(width, height, 0.075)
    rim = material(
        "Preview polished optical edge", (0.26, 0.43, 0.52), 0.85, 0.16, 0.04
    )
    line(
        "Glass bright perimeter",
        [(x, y, 0.012) for x, y in outline + outline[:1]],
        0.005,
        rim,
        True,
        matrix,
    )
    text(
        "Panel heading",
        ("Controls", "Values", "Telemetry")[index],
        (-width / 2 + 0.15, height / 2 - 0.23),
        0.13,
        mats["text"],
        matrix,
    )
    line(
        "Header rule",
        [
            (-width / 2 + 0.1, height / 2 - 0.36, 0.025),
            (width / 2 - 0.1, height / 2 - 0.36, 0.025),
        ],
        0.002,
        mats["muted"],
        True,
        matrix,
    )
    left, top = -width / 2 + 0.16, height / 2 - 0.55
    if index == 0:
        text(
            "Tabs",
            "Surfaces     Lighting     Geometry",
            (left, top),
            0.085,
            mats["text"],
            matrix,
        )
        text(
            "Surface type",
            "SURFACE PROJECTION",
            (left, top - 0.32),
            0.065,
            mats["muted"],
            matrix,
        )
        for j, label in enumerate(("Flat", "Curved")):
            box(
                "Projection choice",
                (left + 0.65 + j * 1.3, top - 0.52, 0.025),
                (1.25, 0.28, 0.008),
                mats["selected"] if j else mats["control"],
                0.035,
                matrix,
                True,
            )
            text(
                "Projection label",
                label,
                (left + 0.65 + j * 1.3, top - 0.55),
                0.09,
                mats["text"],
                matrix,
                "CENTER",
            )
        text(
            "Glass finish",
            "Glass finish",
            (left, top - 0.87),
            0.075,
            mats["text"],
            matrix,
        )
        box(
            "Glass selector",
            (0, top - 1.1, 0.025),
            (width - 0.32, 0.28, 0.008),
            mats["control"],
            0.025,
            matrix,
            True,
        )
        text(
            "Glass choice",
            "Natural glass                         v",
            (left + 0.1, top - 1.13),
            0.085,
            mats["text"],
            matrix,
        )
        for j, label in enumerate(("Show grid", "Live update")):
            yy = top - 1.47 - 0.3 * j
            text(label, label, (left, yy), 0.083, mats["text"], matrix)
            box(
                "Toggle track",
                (width / 2 - 0.35, yy + 0.035, 0.025),
                (0.26, 0.12, 0.012),
                mats["selected"],
                0.05,
                matrix,
                True,
            )
    elif index == 1:
        text("Exposure label", "Exposure", (left, top), 0.085, mats["text"], matrix)
        box(
            "Exposure rail",
            (-0.45, top - 0.22, 0.025),
            (2.1, 0.035, 0.012),
            mats["muted"],
            0.015,
            matrix,
            True,
        )
        box(
            "Exposure fill",
            (-0.8, top - 0.22, 0.04),
            (1.4, 0.045, 0.015),
            mats["selected"],
            0.02,
            matrix,
            True,
        )
        text("Exposure value", "0.75", (1, top - 0.25), 0.095, mats["text"], matrix)
        for j, label in enumerate(("Roughness", "Metallic", "Thickness")):
            xx = -0.99 + 0.99 * j
            text(
                "Knob label",
                label,
                (xx, top - 0.59),
                0.078,
                mats["text"],
                matrix,
                "CENTER",
            )
            mm = matrix @ Matrix.Translation((xx, top - 0.93, 0.028))
            circle("Preview dial rim", 0.22, 0, mats["muted"], mm, 0.009, preview=True)
            circle(
                "Preview dial value",
                0.235,
                0,
                mats["edge"],
                mm,
                0.012,
                0.3,
                3.6 + j * 0.3,
                True,
            )
            text(
                "Knob value",
                ("0.24", "0.60", "1.00")[j],
                (xx, top - 1.3),
                0.08,
                mats["text"],
                matrix,
                "CENTER",
            )
        text("Options", "OPTIONS", (left, top - 1.57), 0.064, mats["muted"], matrix)
        text(
            "Checks",
            "Cast shadows       Reflections       Hologram",
            (left, top - 1.8),
            0.078,
            mats["text"],
            matrix,
        )
    else:
        text(
            "Tabs",
            "Overview     Performance     Status",
            (left, top),
            0.083,
            mats["text"],
            matrix,
        )
        text(
            "Frame time",
            "FRAME TIME                         16.6 ms",
            (left, top - 0.32),
            0.075,
            mats["text"],
            matrix,
        )
        for j in range(4):
            yy = top - 0.49 - j * 0.14
            line(
                "Chart grid",
                [(left, yy, 0.025), (-left, yy, 0.025)],
                0.0018,
                mats["muted"],
                True,
                matrix,
            )
        line(
            "Telemetry trace",
            [
                (
                    left + j * (width - 0.32) / 20,
                    top - 0.75 + 0.10 * math.sin(j * 1.4) + j * 0.006,
                    0.03,
                )
                for j in range(21)
            ],
            0.008,
            mats["edge"],
            True,
            matrix,
        )
        for j, label in enumerate(("GPU", "VRAM", "DRAW CALLS")):
            xx = left + j * 0.94
            text("Metric", label, (xx, top - 1.18), 0.065, mats["text"], matrix)
            text(
                "Metric value",
                ("42%", "3.2 GB", "1.4 k")[j],
                (xx, top - 1.43),
                0.115,
                mats["edge"],
                matrix,
            )
        text("State", "SYSTEM STATUS", (left, top - 1.69), 0.065, mats["muted"], matrix)
        text(
            "Ready",
            "Ready             Paused             Offline",
            (left, top - 1.9),
            0.075,
            mats["text"],
            matrix,
        )


def preview_reflections():
    """Softbox cards affect optical glass but are absent from runtime assets."""
    mat = material(
        "Preview soft reflected architecture",
        (0.025, 0.035, 0.045),
        rough=0.5,
        emission=15,
    )
    mat.node_tree.nodes.get("Principled BSDF").inputs[
        "Emission Color"
    ].default_value = (0.5, 0.65, 0.75, 1)
    for i, (x, z, width, height, tilt) in enumerate(
        (
            (-3.4, 4.8, 2.1, 7.0, -0.34),
            (2.8, 4.2, 1.4, 7.0, 0.27),
            (0.1, 6.8, 7.0, 0.45, 0),
        )
    ):
        matrix = (
            Matrix.Translation((x, -4, z))
            @ Matrix.Rotation(tilt, 4, "Y")
            @ Matrix.Rotation(math.pi / 2, 4, "X")
        )
        obj = box(
            f"Preview reflection card {i}",
            (0, 0, 0),
            (width, height, 0.01),
            mat,
            0.2,
            matrix,
            True,
        )
        obj.visible_camera = False
        obj.visible_diffuse = False
        obj.visible_transmission = True
        obj.visible_shadow = False


def presentation_lighting(scene):
    """Keep bright ship lights out of glass; optical cards supply reflections."""
    receivers = bpy.data.collections.new("Physical lighting receivers")
    for obj in scene.objects:
        if obj.get("cockpit_static") or obj.name == "cockpit-planet":
            receivers.objects.link(obj)
        if obj.type == "LIGHT":
            obj.light_linking.receiver_collection = receivers
            obj.visible_glossy = False
            obj.data.specular_factor = 0.12
        elif obj.get("cockpit_static") and any(
            mat and mat.name.startswith("Architectural white")
            for mat in obj.data.materials
        ):
            obj.visible_glossy = False


def embedded_previews(mats):
    """Composition proxies only; runtime replaces these with native GUI controls."""
    for role in ("pulse", "scanner"):
        empty = bpy.data.objects["cockpit-" + role + "-surface"]
        matrix = empty.matrix_world @ Matrix.Rotation(math.pi / 2, 4, "X")

        def label(value, x, y, size=0.05):
            text(
                "Embedded " + role + " label",
                value,
                (x, y),
                size,
                mats["text"],
                matrix @ Matrix.Translation((0, 0, -0.049)),
            )

        label(role.upper(), -0.48, 0.43, 0.065)
        if role == "pulse":
            for j, (name, value) in enumerate(
                (("FLOW", 0.82), ("RATE", 0.63), ("GAIN", 0.72), ("BIAS", 0.48))
            ):
                y = 0.22 - j * 0.22
                label(name, -0.48, y)
                line(
                    "Pulse track",
                    [(-0.15, y, 0.005), (0.48, y, 0.005)],
                    0.003,
                    mats["muted"],
                    True,
                    matrix,
                )
                line(
                    "Pulse signal",
                    [(-0.15, y, 0.006), (-0.15 + 0.63 * value, y, 0.006)],
                    0.006,
                    mats["edge"],
                    True,
                    matrix,
                )
        else:
            for r in (0.14, 0.27, 0.41):
                circle(
                    "Scanner range",
                    r,
                    0.005,
                    mats["muted"],
                    matrix @ Matrix.Translation((0, -0.04, 0)),
                    0.0025,
                    preview=True,
                )
            line(
                "Scanner axes",
                [(-0.44, -0.04, 0.005), (0.44, -0.04, 0.005)],
                0.002,
                mats["muted"],
                True,
                matrix,
            )
            line(
                "Scanner axes",
                [(0, -0.48, 0.005), (0, 0.4, 0.005)],
                0.002,
                mats["muted"],
                True,
                matrix,
            )
            line(
                "Scanner sweep",
                [(0, -0.04, 0.007), (0.29, 0.25, 0.007)],
                0.005,
                mats["edge"],
                True,
                matrix,
            )
            for x, y in ((-0.10, 0.17), (0.19, 0.04), (-0.22, -0.24)):
                circle(
                    "Scanner return",
                    0.013,
                    0.008,
                    mats["edge"],
                    matrix @ Matrix.Translation((x, y, 0)),
                    0.003,
                    preview=True,
                )


def build():
    global COLLECTION, PREVIEW
    STATIC.clear()
    ANCHORS.clear()
    scene = bpy.context.scene
    for obj in list(scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for collection in list(scene.collection.children):
        bpy.data.collections.remove(collection)
    scene.name = "Cockpit presentation"
    COLLECTION = bpy.data.collections.new("Bridge editable architecture")
    PREVIEW = bpy.data.collections.new("Preview live GUI placement only")
    scene.collection.children.link(COLLECTION)
    scene.collection.children.link(PREVIEW)
    pale = material("Pearl ceramic satin", (0.46, 0.46, 0.44), 0.12, 0.38)
    trim = material("Porcelain polished edge", (0.56, 0.60, 0.62), 0.3, 0.27)
    dark = material("Graphite glass instrument wells", (0.012, 0.022, 0.032), 0.5, 0.24)
    charcoal = material("Charcoal structural alloy", (0.035, 0.045, 0.055), 0.65, 0.3)
    silver = material("Cool machined titanium", (0.46, 0.52, 0.56), 0.70, 0.27)
    glow = material("Ice blue light guides", (0.18, 0.50, 0.85), 0.1, 0.24, 1.4)
    white = material("Architectural white strips", (0.8, 0.88, 1), 0.05, 0.3, 2.5)
    seam = material("Fine panel seams", (0.07, 0.09, 0.10), 0.3, 0.5)
    floor = material("Bridge deck", (0.025, 0.035, 0.045), 0.2, 0.45)
    box("Bridge deck", (0, 0, -0.6), (15, 16, 0.2), floor)
    import runpy

    console = runpy.run_path(str(ROOT / "console.py"))
    api = {"box": box, "line": line, "circle": circle, "anchor": anchor}
    console["build_console"](api, COLLECTION, pale, dark, silver, glow, white)
    console["build_aperture"](api, COLLECTION, pale, charcoal, white)
    space()
    mats = {
        "text": material(
            "Preview warm white typography", (0.67, 0.86, 0.94), emission=1.3
        ),
        "edge": material("Preview cyan edge", (0.37, 0.73, 0.95), emission=1.6),
        "muted": material("Preview subtle rules", (0.13, 0.25, 0.31), emission=0.5),
        "control": material("Preview control wells", (0.04, 0.09, 0.12), 0.3, 0.4, 0.2),
        "selected": material(
            "Preview selected glass", (0.10, 0.32, 0.43), 0.3, 0.28, 0.5
        ),
    }
    embedded_previews(mats)
    glass_panel(0, (-3.26, 1, 2.79), 3.12, 2.65, 0.18, mats)
    glass_panel(1, (0, 1.26, 2.68), 3.35, 2.65, 0, mats)
    glass_panel(2, (3.26, 1, 2.79), 3.12, 2.65, -0.18, mats)
    preview_reflections()
    data = bpy.data.cameras.new("Cockpit view")
    data.lens, data.clip_end = 30, 500
    camera = put(
        bpy.data.objects.new("cockpit-camera", data), "cockpit-camera", static=False
    )
    camera.location = (0, -7.8, 3.4)
    camera.rotation_euler = (
        (Vector((0, 1.3, 2.45)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    )
    scene.camera = camera
    area("Soft ceiling key", (-3, -2, 5.5), (0, 0, 0.8), 1400, (0.93, 0.96, 1), 7)
    area("Starboard bounce", (4, -0.8, 3.5), (0, -0.5, 1), 400, (0.72, 0.85, 1), 4)
    area("Warm port rim", (-6, 3, 4), (0, -1, 1), 1100, (1, 0.87, 0.73), 5)
    area("Console soft frontal fill", (0, -4, 4), (0, 0, 1), 350, (0.93, 0.96, 1), 6)
    area("Fascia grazing softbox", (-2, -4, 2.7), (0, -1, 1), 300, (1, 0.97, 0.93), 5)
    data = bpy.data.lights.new("Distant stellar light", "SUN")
    data.energy, data.angle, data.color = 1.5, 0.025, (0.83, 0.91, 1)
    sun = put(
        bpy.data.objects.new("Distant stellar light", data),
        "Distant stellar light",
        static=False,
    )
    sun.rotation_euler = Vector((0.3, 0.4, -1)).to_track_quat("-Z", "Y").to_euler()
    presentation_lighting(scene)
    scene.world = bpy.data.worlds.new("Orbital night")
    scene.world.use_nodes = True
    bg = scene.world.node_tree.nodes.get("Background")
    bg.inputs["Color"].default_value = (0.012, 0.02, 0.035, 1)
    bg.inputs["Strength"].default_value = 0.3
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 96
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = 1600, 900
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.view_settings.view_transform = "AgX"
    scene.view_settings.look = "AgX - Medium High Contrast"
    scene.view_settings.exposure = -0.65
    scene["cockpit_reference"] = "//cockpit-reference.png"
    scene["runtime_bake_pending"] = True
    scene["preview_notice"] = (
        "Glass and labels show composition only; IPP supplies real interactive GUI at named empties."
    )
    bpy.ops.outliner.orphans_purge(do_recursive=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT / "cockpit.blend"))
    return {
        "objects": len(scene.objects),
        "static_meshes": sum(bool(o.get("cockpit_static")) for o in scene.objects),
        "anchors": [o.name for o in ANCHORS],
    }


if __name__ == "__main__":
    build()
