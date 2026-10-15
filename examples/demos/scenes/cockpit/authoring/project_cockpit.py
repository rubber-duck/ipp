"""Locked-camera reference projection on shallow, independently addressable geometry.

Pixel contours are measured against the immutable approved 1672 x 941 reference.
The cleaned plate removes pictured GUI; actual controls mount at the named empties.
"""

import json
import math
import runpy
from pathlib import Path

import bpy
from mathutils import Matrix, Vector
from mathutils.geometry import delaunay_2d_cdt

ROOT = Path(__file__).resolve().parent
WIDTH, HEIGHT = 1672, 941
CAMERA_POSITION = Vector((0, -8, 3.4))


def point(pixel, depth):
    x, y = pixel
    return Vector(
        (
            (x / WIDTH - 0.5) * 1.2 * depth,
            depth - 8,
            3.4 + (0.5 - y / HEIGHT) * 1.2 * depth * HEIGHT / WIDTH,
        )
    )


def inside(point, polygon):
    x, y = point
    result = False
    for a, b in zip(polygon, polygon[1:] + polygon[:1]):
        if (a[1] > y) != (b[1] > y) and x < (b[0] - a[0]) * (y - a[1]) / (
            b[1] - a[1]
        ) + a[0]:
            result = not result
    return result


def ellipse(cx, cy, rx, ry):
    return [
        (cx + rx * math.cos(t * math.tau / 80), cy + ry * math.sin(t * math.tau / 80))
        for t in range(80)
    ]


def projected_mesh(name, polygon, depth, material, holes=(), step=32):
    pixels = []
    edges = []
    for contour in [polygon, *holes]:
        first = len(pixels)
        pixels.extend(contour)
        edges.extend(
            (first + i, first + (i + 1) % len(contour)) for i in range(len(contour))
        )
    minx, maxx = min(p[0] for p in polygon), max(p[0] for p in polygon)
    miny, maxy = min(p[1] for p in polygon), max(p[1] for p in polygon)
    for x in range(math.ceil(minx), math.floor(maxx), step):
        for y in range(math.ceil(miny), math.floor(maxy), step):
            if inside((x, y), polygon) and not any(inside((x, y), h) for h in holes):
                pixels.append((x, y))
    vertices, _, triangles, _, _, _ = delaunay_2d_cdt(
        [Vector(p) for p in pixels], edges, [], 0, 1e-5, False
    )
    triangles = [
        f
        for f in triangles
        if inside(sum((vertices[i] for i in f), Vector((0, 0))) / len(f), polygon)
        and not any(
            inside(sum((vertices[i] for i in f), Vector((0, 0))) / len(f), h)
            for h in holes
        )
    ]
    used = sorted({i for f in triangles for i in f})
    mapping = {old: new for new, old in enumerate(used)}
    pixels = [vertices[i] for i in used]
    positions = [point(p, depth(*p) if callable(depth) else depth) for p in pixels]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(
        positions, [], [tuple(mapping[i] for i in reversed(f)) for f in triangles]
    )
    mesh.update()
    uv = mesh.uv_layers.new(name="Reference camera projection")
    for loop in mesh.loops:
        pixel = pixels[loop.vertex_index]
        uv.data[loop.index].uv = (pixel.x / WIDTH, 1 - pixel.y / HEIGHT)
    mesh.materials.append(material)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj["ipp_id"] = name
    obj["camera_locked"] = True
    return obj


def material(image):
    mat = bpy.data.materials.new("Reference cleanplate unlit")
    mat.use_nodes = True
    mat["ipp_unlit"] = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = image
    tex.interpolation = "Linear"
    bsdf = nodes.get("Principled BSDF")
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    # Presentation only: the export scene replaces this output with the supported
    # opaque Principled unlit declaration, retaining the same image and UVs.
    emission = nodes.new("ShaderNodeEmission")
    links.new(tex.outputs["Color"], emission.inputs["Color"])
    links.new(emission.outputs[0], nodes.get("Material Output").inputs["Surface"])
    return mat


def anchor(name, pixels, depths, role):
    corners = [point(p, d) for p, d in zip(pixels, depths)]
    tl, tr, br, bl = corners
    right = (tr + br - tl - bl) / 2
    up = (tl + tr - bl - br) / 2
    width, height = right.length, up.length
    right.normalize()
    normal = right.cross(up).normalized()
    up = normal.cross(right).normalized()
    pose = Matrix((right, up, normal)).transposed().to_4x4()
    pose.translation = sum(corners, Vector()) / 4
    obj = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(obj)
    obj.matrix_world = pose @ Matrix.Rotation(-math.pi / 2, 4, "X")
    obj["ipp_id"] = name
    obj["surface_width"] = width
    obj["surface_height"] = height
    obj["surface_role"] = role
    obj["reference_corners"] = json.dumps(pixels)
    obj.empty_display_type = "ARROWS"
    obj.empty_display_size = 0.2
    if role in ("pulse", "scanner"):
        obj["surface_width"] *= 0.85
        obj["surface_height"] *= 0.88
        obj.location.x += 0.06 if role == "pulse" else -0.06
    return obj


def placement_preview(scene):
    mat = bpy.data.materials.new("GUI placement preview only")
    mat.use_nodes = True
    emit = mat.node_tree.nodes.new("ShaderNodeEmission")
    emit.inputs["Color"].default_value = (0.15, 0.65, 1, 1)
    mat.node_tree.links.new(
        emit.outputs[0], mat.node_tree.nodes.get("Material Output").inputs["Surface"]
    )
    collection = bpy.data.collections.new("Preview native GUI placement only")
    scene.collection.children.link(collection)
    for anchor in [o for o in scene.objects if o.get("surface_role")]:
        width, height = anchor["surface_width"], anchor["surface_height"]
        pose = anchor.matrix_world @ Matrix.Rotation(math.pi / 2, 4, "X")
        curve = bpy.data.curves.new("GUI placement outline", "CURVE")
        curve.dimensions = "3D"
        curve.bevel_depth = 0.003
        curve.bevel_resolution = 2
        spline = curve.splines.new("POLY")
        spline.points.add(4)
        corners = [
            (-width / 2, -height / 2),
            (width / 2, -height / 2),
            (width / 2, height / 2),
            (-width / 2, height / 2),
            (-width / 2, -height / 2),
        ]
        for point, corner in zip(spline.points, corners):
            point.co = (*corner, 0.005, 1)
        obj = bpy.data.objects.new("Preview " + anchor["surface_role"], curve)
        collection.objects.link(obj)
        obj.matrix_world = pose
        curve.materials.append(mat)
        obj["cockpit_preview_only"] = True
        if anchor["surface_role"] in ("pulse", "scanner"):
            continue
        font = bpy.data.curves.new("Native GUI placement label", "FONT")
        font.body = "LIVE GUI / " + anchor["surface_role"].upper()
        font.size = 0.11
        label = bpy.data.objects.new("Preview label", font)
        collection.objects.link(label)
        label.matrix_world = pose @ Matrix.Translation(
            (-width / 2 + 0.1, height / 2 - 0.2, 0.006)
        )
        font.materials.append(mat)
        label["cockpit_preview_only"] = True


def build():
    scene = bpy.data.scenes.new("Cockpit projected presentation")
    bpy.context.window.scene = scene
    for other in list(bpy.data.scenes):
        if other != scene:
            bpy.data.scenes.remove(other)
    bpy.ops.outliner.orphans_purge(do_recursive=True)
    scene.name = "Cockpit projected presentation"
    plate = ROOT / "cockpit-unlit-all.png"
    image = bpy.data.images.load(str(plate), check_existing=False)
    if tuple(image.size) != (WIDTH, HEIGHT):
        image.scale(WIDTH, HEIGHT)
    image.pack()
    image.filepath = "//cockpit-unlit-all.png"
    mat = material(image)
    full = [(0, 0), (WIDTH, 0), (WIDTH, HEIGHT), (0, HEIGHT)]
    window = [
        (83, 0),
        (1591, 0),
        (1573, 50),
        (1556, 100),
        (1524, 200),
        (1507, 300),
        (1482, 400),
        (1459, 500),
        (1449, 544),
        (1033, 544),
        (836, 542),
        (704, 544),
        (222, 544),
        (211, 500),
        (187, 400),
        (162, 300),
        (146, 200),
        (116, 100),
        (101, 50),
    ]
    side_windows = (
        [(0, 415), (43, 573), (0, 573)],
        [(WIDTH, 415), (WIDTH, 573), (1630, 573)],
    )
    dashboard = [
        (0, 744),
        (180, 718),
        (420, 699),
        (650, 689),
        (840, 686),
        (1040, 690),
        (1260, 700),
        (1490, 720),
        (WIDTH, 744),
        (WIDTH, HEIGHT),
        (0, HEIGHT),
    ]
    dials = [
        ellipse(562, 848, 62, 31),
        ellipse(835, 840, 117, 51),
        ellipse(1111, 848, 63, 31),
    ]
    exterior = runpy.run_path(str(ROOT / "projected_exterior.py"))["build"](
        projected_mesh, material
    )
    projected_mesh(
        "cockpit-window-shell",
        full,
        12,
        mat,
        holes=(window, *side_windows, dashboard),
        step=80,
    )
    deck = lambda x, y: 8.4 - 0.008 * (y - 690) + 0.04 * ((x - 836) / 836) ** 2
    projected_mesh("cockpit-console-shell", dashboard, deck, mat, holes=dials, step=20)
    for name, polygon in zip(("left", "centre", "right"), dials):
        projected_mesh(
            "cockpit-dial-" + name,
            polygon,
            lambda x, y: deck(x, y) - 0.018,
            mat,
            step=12,
        )
    lights, registration = runpy.run_path(str(ROOT / "projected_lights.py"))["build"](
        projected_mesh, deck, mat
    )
    anchor(
        "cockpit-screen-0",
        [(34, 181), (517, 237), (510, 646), (29, 666)],
        [7.4, 8.65, 8.65, 7.4],
        "controls",
    )
    anchor(
        "cockpit-screen-1",
        [(535, 235), (1140, 235), (1140, 651), (535, 651)],
        [8.65] * 4,
        "values",
    )
    anchor(
        "cockpit-screen-2",
        [(1158, 237), (1640, 181), (1646, 667), (1158, 647)],
        [8.65, 7.4, 7.4, 8.65],
        "telemetry",
    )
    anchor(
        "cockpit-pulse-surface",
        [(91, 804), (246, 787), (230, 866), (54, 883)],
        [7.20, 7.20, 6.55, 6.55],
        "pulse",
    )
    anchor(
        "cockpit-scanner-surface",
        [(1418, 787), (1574, 804), (1619, 883), (1443, 866)],
        [7.20, 7.20, 6.55, 6.55],
        "scanner",
    )
    data = bpy.data.cameras.new("Locked reference camera")
    data.lens = 30
    data.sensor_width = 36
    data.sensor_fit = "HORIZONTAL"
    data.clip_end = 100
    cam = bpy.data.objects.new("cockpit-camera", data)
    scene.collection.objects.link(cam)
    cam["ipp_id"] = "cockpit-camera"
    cam.location = CAMERA_POSITION
    cam.rotation_euler = (math.pi / 2, 0, 0)
    scene.camera = cam
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 16
    scene.cycles.use_denoising = False
    scene.render.resolution_x = WIDTH
    scene.render.resolution_y = HEIGHT
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.world = bpy.data.worlds.new("Projection black")
    scene.world.use_nodes = True
    scene.world.node_tree.nodes.get("Background").inputs["Strength"].default_value = 0
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0
    placement_preview(scene)
    scene["fixed_camera"] = True
    scene["reference_texture"] = "//cockpit-reference.png"
    scene["cleanplate"] = "//cockpit-cleanplate.png"
    scene["presentation_notice"] = (
        "Pictured GUI removed; all controls are supplied by native GUI at named empties."
    )
    # Save only this prototype and its packed image, retaining the previous physical
    # model in its own unchanged cockpit.blend and verified recovery archive.
    for other in list(bpy.data.scenes):
        if other != scene:
            bpy.data.scenes.remove(other)
    bpy.ops.outliner.orphans_purge(do_recursive=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT / "cockpit-projected.blend"))
    result = {
        "scene": scene.name,
        "meshes": [
            (o.name, len(o.data.polygons)) for o in scene.objects if o.type == "MESH"
        ],
        "anchors": [
            {
                "id": o.name,
                "width": o["surface_width"],
                "height": o["surface_height"],
                "matrix_blender": [list(r) for r in o.matrix_world],
            }
            for o in scene.objects
            if o.type == "EMPTY"
        ],
    }
    result["surfaces"] = {
        a["id"]: {"width": a["width"], "height": a["height"]} for a in result["anchors"]
    }
    result["lights"] = lights
    result["exterior"] = exterior
    result["mask_registration"] = registration
    (ROOT / "cockpit-projected-anchors.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    return result


if __name__ == "__main__":
    build()
