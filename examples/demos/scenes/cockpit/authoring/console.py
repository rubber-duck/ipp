"""A single machined console solid with live Boolean wells and swept curvature.

All dark well floors are faces of the shell itself. Hidden cutters remain editable;
only the three lathed dial assemblies and flush light inserts are separate hardware.
"""

import math

import bpy
import bmesh
import numpy as np
from mathutils import Matrix, Vector
from mathutils.geometry import delaunay_2d_cdt


def rounded_polygon(points, radius=0.10, steps=8):
    points = [Vector(p) for p in points]
    area = sum(a.x * b.y - b.x * a.y for a, b in zip(points, points[1:] + points[:1]))
    if area < 0:
        points.reverse()
    result = []
    for i, p in enumerate(points):
        before, after = points[i - 1] - p, points[(i + 1) % len(points)] - p
        distance = min(radius, before.length * 0.3, after.length * 0.3)
        a, b = p + before.normalized() * distance, p + after.normalized() * distance
        for t in np.linspace(0, 1, steps):
            result.append(tuple((1 - t) ** 2 * a + 2 * (1 - t) * t * p + t * t * b))
    return result


def grid_solid(collection, materials):
    nx, ny = 112, 26
    xs, ys = np.linspace(-5.7, 5.7, nx + 1), np.linspace(-1.18, 1.35, ny + 1)
    layer = (nx + 1) * (ny + 1)
    vertices = [(x, y, z) for z in (0, -0.38) for y in ys for x in xs]
    faces = []
    for j in range(ny):
        for i in range(nx):
            k = j * (nx + 1) + i
            face = (k, k + 1, k + nx + 2, k + nx + 1)
            faces.append(face)
            faces.append(tuple(v + layer for v in reversed(face)))
    perimeter = list(range(nx + 1))
    perimeter += [j * (nx + 1) + nx for j in range(1, ny + 1)]
    perimeter += [ny * (nx + 1) + i for i in range(nx - 1, -1, -1)]
    perimeter += [j * (nx + 1) for j in range(ny - 1, 0, -1)]
    # A real rolled fascia profile, continuing around the entire solid.
    rings = [perimeter]
    for z, offset in (
        (-0.015, 0.045),
        (-0.055, 0.085),
        (-0.115, 0.105),
        (-0.25, 0.095),
        (-0.32, 0.06),
    ):
        ring = []
        for index in perimeter:
            x, y, _ = vertices[index]
            dx = math.copysign(offset, x) if abs(x) > 5.69 else 0
            dy = -offset if y < -1.17 else offset * 0.4 if y > 1.34 else 0
            ring.append(len(vertices))
            vertices.append((x + dx, y + dy, z))
        rings.append(ring)
    rings.append([v + layer for v in perimeter])
    for lower, upper in zip(rings[1:], rings[:-1]):
        for i in range(len(perimeter)):
            j = (i + 1) % len(perimeter)
            faces.append((lower[i], lower[j], upper[j], upper[i]))
    mesh = bpy.data.meshes.new("Continuous console structural solid")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("cockpit-console-shell", mesh)
    collection.objects.link(obj)
    obj["ipp_id"] = obj.name
    obj["cockpit_static"] = True
    obj["construction"] = (
        "One connected solid; Boolean recesses, chamfers, continuous sweep."
    )
    for mat in materials:
        mesh.materials.append(mat)
    for p in mesh.polygons:
        p.use_smooth = True
    mod = obj.modifiers.new("01 Outer fascia chamfer", "BEVEL")
    mod.width = 0.015
    mod.segments = 3
    mod.limit_method = "ANGLE"
    mod.angle_limit = 0.5
    return obj


def cutter(collection, name, points, materials, bottom=-0.07, top=0.5, radius=0.22):
    corners = [Vector(p) for p in rounded_polygon(points, radius)]
    points = []
    for a, b in zip(corners, corners[1:] + corners[:1]):
        for t in np.linspace(0, 1, max(2, math.ceil((b - a).length / 0.12) + 1))[:-1]:
            points.append(a.lerp(b, t))
    n = len(points)
    inset = 0.055 if bottom < -0.05 and top > 0 else 0
    lower = []
    for i, p in enumerate(points):
        a = (p - points[i - 1]).normalized()
        b = (points[(i + 1) % n] - p).normalized()
        na, nb = Vector((-a.y, a.x)), Vector((-b.y, b.x))
        lower.append(p + (na + nb) * (inset / max(0.2, 1 + na.dot(nb))))
    grid = [
        Vector((x, y))
        for x in np.arange(
            min(p.x for p in lower) + 0.07, max(p.x for p in lower), 0.12
        )
        for y in np.arange(
            min(p.y for p in lower) + 0.07, max(p.y for p in lower), 0.12
        )
    ]
    coords, _, cap, orig, _, _ = delaunay_2d_cdt(
        lower + grid, [], [list(range(n))], 1, 1e-7, True
    )
    boundary = {
        source: i for i, sources in enumerate(orig) for source in sources if source < n
    }
    verts = [(p.x, p.y, bottom) for p in coords]
    start = len(verts)
    verts += [
        (p.x, p.y, z) for z in ((0.005, top) if inset else (top,)) for p in points
    ]
    faces = [tuple(reversed(face)) for face in cap]
    floors = len(faces)
    faces += [
        (boundary[i], boundary[(i + 1) % n], start + (i + 1) % n, start + i)
        for i in range(n)
    ]
    if inset:
        faces += [
            (start + i, start + (i + 1) % n, start + n + (i + 1) % n, start + n + i)
            for i in range(n)
        ]
    faces += [tuple(range(len(verts) - n, len(verts)))]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    for mat in materials:
        mesh.materials.append(mat)
    for i, face in enumerate(mesh.polygons):
        face.material_index = 1 if i < floors or i == len(faces) - 1 else 2
        face.use_smooth = i >= floors
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.delete(
        bm, geom=[v for v in bm.verts if not v.link_edges], context="VERTS"
    )
    assert all(e.is_manifold for e in bm.edges), name + " cutter must be watertight"
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    collection.objects.link(obj)
    obj.display_type = "WIRE"
    obj.hide_render = True
    obj.hide_set(True)
    obj["cockpit_construction_only"] = True
    return obj


def deform_modifier(obj):
    group = bpy.data.node_groups.new(
        "Console continuous deck sweep", "GeometryNodeTree"
    )
    group.interface.new_socket(
        name="Geometry", in_out="INPUT", socket_type="NodeSocketGeometry"
    )
    group.interface.new_socket(
        name="Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry"
    )
    nodes, links = group.nodes, group.links
    source = nodes.new("NodeGroupInput")
    target = nodes.new("NodeGroupOutput")
    pos = nodes.new("GeometryNodeInputPosition")
    xyz = nodes.new("ShaderNodeSeparateXYZ")
    links.new(pos.outputs["Position"], xyz.inputs[0])
    square = nodes.new("ShaderNodeMath")
    square.operation = "MULTIPLY"
    links.new(xyz.outputs["X"], square.inputs[0])
    links.new(xyz.outputs["X"], square.inputs[1])
    sweep = nodes.new("ShaderNodeMath")
    sweep.operation = "MULTIPLY_ADD"
    links.new(square.outputs[0], sweep.inputs[0])
    sweep.inputs[1].default_value = -0.04
    sweep.inputs[2].default_value = -0.7
    slope = nodes.new("ShaderNodeMath")
    slope.operation = "MULTIPLY_ADD"
    links.new(xyz.outputs["Y"], slope.inputs[0])
    slope.inputs[1].default_value = 0.12
    slope.inputs[2].default_value = 1.03
    ridge = nodes.new("ShaderNodeMapRange")
    ridge.interpolation_type = "SMOOTHSTEP"
    ridge.clamp = True
    links.new(xyz.outputs["Y"], ridge.inputs["Value"])
    ridge.inputs["From Min"].default_value = 0.5
    ridge.inputs["From Max"].default_value = 1.35
    ridge.inputs["To Min"].default_value = 0
    ridge.inputs["To Max"].default_value = 0.08
    height = nodes.new("ShaderNodeMath")
    height.operation = "ADD"
    links.new(slope.outputs[0], height.inputs[0])
    links.new(ridge.outputs[0], height.inputs[1])
    offset = nodes.new("ShaderNodeCombineXYZ")
    links.new(sweep.outputs[0], offset.inputs["Y"])
    links.new(height.outputs[0], offset.inputs["Z"])
    setpos = nodes.new("GeometryNodeSetPosition")
    links.new(source.outputs["Geometry"], setpos.inputs["Geometry"])
    links.new(offset.outputs[0], setpos.inputs["Offset"])
    links.new(setpos.outputs["Geometry"], target.inputs["Geometry"])
    mod = obj.modifiers.new("12 Continuous arc and sculpted rear ridge", "NODES")
    mod.node_group = group


def frame(x, y, z=0):
    t = max(0, min(1, (y - 0.5) / 0.85))
    h = 0.08 * t * t * (3 - 2 * t)
    slope = 0.12 + (0.08 * 6 * t * (1 - t) / 0.85 if 0 < t < 1 else 0)
    xx = Vector((1, -0.08 * x, 0)).normalized()
    yy = Vector((0, 1, slope)).normalized()
    zz = xx.cross(yy).normalized()
    yy = zz.cross(xx).normalized()
    matrix = Matrix((xx, yy, zz)).transposed().to_4x4()
    matrix.translation = Vector((x, y - 0.04 * x * x - 0.7, z + 1.03 + 0.12 * y + h))
    return matrix


def dial(collection, name, radius, matrix, materials):
    # One spun solid, with a shallow dark face and a turned metallic perimeter.
    profile = [
        (0, 0),
        (radius * 0.96, 0),
        (radius, 0.012),
        (radius, 0.030),
        (radius * 0.98, 0.045),
        (radius * 0.91, 0.050),
        (radius * 0.89, 0.044),
        (0, 0.044),
    ]
    n = 96
    verts = [
        (r * math.cos(a * math.tau / n), r * math.sin(a * math.tau / n), z)
        for r, z in profile
        for a in range(n)
    ]
    faces = [
        (k * n + j, k * n + (j + 1) % n, (k + 1) * n + (j + 1) % n, (k + 1) * n + j)
        for k in range(len(profile) - 1)
        for j in range(n)
    ]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    for mat in materials:
        mesh.materials.append(mat)
    for i, p in enumerate(mesh.polygons):
        p.material_index = 1 if 1 <= i // n <= 4 else 0
        p.use_smooth = True
    obj = bpy.data.objects.new(name, mesh)
    obj.matrix_world = matrix
    obj["ipp_id"] = name
    obj["cockpit_static"] = True
    collection.objects.link(obj)
    return obj


def build_console(api, collection, pale, dark, metal, glow, white):
    cutters = bpy.data.collections.new(
        "Console Boolean tooling — edit to reshape wells"
    )
    bpy.context.scene.collection.children.link(cutters)
    materials = [pale, dark, metal]
    body = grid_solid(collection, materials)
    outer = [
        (-5.30, -0.98),
        (-2.62, -0.98),
        (-2.41, 0.15),
        (-2.14, 0.48),
        (-1.03, 0.48),
        (-0.94, 1.04),
        (-4.90, 1.04),
        (-5.31, 0.36),
    ]
    sidewell = [(-2.43, -1.0), (-1.20, -1.0), (-1.06, 0.26), (-2.09, 0.26)]
    openings = [
        ("Port joined display and recessed button bank", outer),
        (
            "Starboard joined display and recessed button bank",
            [(-x, y) for x, y in outer],
        ),
        ("Port dial recess", sidewell),
        ("Starboard dial recess", [(-x, y) for x, y in sidewell]),
        (
            "Central trapezoid instrument recess",
            [(-1.02, -1.0), (1.02, -1.0), (0.79, 0.27), (-0.79, 0.27)],
        ),
        (
            "Central integrated status neck",
            [(-0.73, 0.47), (0.73, 0.47), (0.60, 1.04), (-0.60, 1.04)],
        ),
    ]
    for i, (name, polygon) in enumerate(openings):
        tool = cutter(cutters, name, polygon, materials)
        mod = body.modifiers.new(f"{i + 2:02d} Cut {name}", "BOOLEAN")
        mod.operation = "DIFFERENCE"
        mod.solver = "EXACT"
        mod.object = tool
        mod.material_mode = "INDEX"
    reveal = cutter(
        cutters,
        "Front fascia continuous undercut",
        [(-5.65, -1.40), (5.65, -1.40), (5.65, -1.24), (-5.65, -1.24)],
        materials,
        bottom=-0.295,
        top=-0.275,
        radius=0.02,
    )
    mod = body.modifiers.new("08 Cut fascia shadow reveal", "BOOLEAN")
    mod.operation = "DIFFERENCE"
    mod.solver = "EXACT"
    mod.object = reveal
    mod = body.modifiers.new("09 Instrument opening precision chamfers", "BEVEL")
    mod.width = 0.022
    mod.segments = 3
    mod.limit_method = "ANGLE"
    mod.angle_limit = 0.45
    mod.material = 0
    deform_modifier(body)
    # The instrument hardware remains below the pale deck's upper plane.
    for name, x, r in (
        ("left", -1.67, 0.27),
        ("centre", 0, 0.55),
        ("right", 1.67, 0.27),
    ):
        matrix = frame(x, -0.44, -0.055)
        created = set(bpy.context.scene.objects)
        dial(collection, "cockpit-dial-" + name, r, matrix, [dark, metal])
        for a, b in ((0.25, 2.65), (3.4, 5.75)):
            api["circle"](
                "Dial luminous arc", r * 0.88, 0.048, glow, matrix, 0.0045, a, b
            )
        for fraction in (0.72, 0.47):
            api["circle"](
                "Dial precision engraving", r * fraction, 0.046, metal, matrix, 0.0018
            )
        if name == "centre":
            for j in range(48):
                a = j * math.tau / 48
                api["line"](
                    "Dial etched index",
                    [
                        (r * 0.78 * math.cos(a), r * 0.78 * math.sin(a), 0.047),
                        (r * 0.82 * math.cos(a), r * 0.82 * math.sin(a), 0.047),
                    ],
                    0.0015,
                    metal,
                    False,
                    matrix,
                )
        pieces = [o for o in bpy.context.scene.objects if o not in created]
        bpy.ops.object.select_all(action="DESELECT")
        for o in pieces:
            o.select_set(True)
        bpy.context.view_layer.objects.active = pieces[0]
        bpy.ops.object.join()
        assembly = bpy.context.object
        assembly.name = "cockpit-dial-" + name
        assembly["ipp_id"] = assembly.name
        assembly["cockpit_static"] = True
    for side in (-1, 1):
        for j in range(7):
            x = side * (1.35 + j * 0.49)
            api["box"](
                "Flush light key",
                (0, 0, 0),
                (0.19, 0.075, 0.008),
                glow,
                0.012,
                frame(x, 0.78, -0.060),
            )
        role = "pulse" if side < 0 else "scanner"
        api["anchor"](
            "cockpit-" + role + "-surface",
            frame(side * 3.22, -0.28, -0.050),
            1.10,
            1.14,
            role,
        )
    api["box"](
        "Recessed central status light",
        (0, 0, 0),
        (1.03, 0.065, 0.012),
        white,
        0.018,
        frame(0, 0.76, -0.060),
    )
    return body


def build_aperture(api, collection, pale, dark, white):
    # A single tapered aperture rim, with a thin stepped inner chamfer.
    inner = [(-6.5, 0.45), (6.5, 0.45), (6.45, 6.1), (-6.45, 6.1)]
    rings = []
    for spread, depth in (
        (0.43, 4.45),
        (0.43, 4.15),
        (0.30, 4.04),
        (0.02, 4.04),
        (0, 4.16),
    ):
        rings.extend(
            [
                (
                    x + math.copysign(spread, x),
                    depth,
                    z + (-spread if z < 1 else spread),
                )
                for x, z in inner
            ]
        )
    faces = [
        (k * 4 + j, k * 4 + (j + 1) % 4, (k + 1) * 4 + (j + 1) % 4, (k + 1) * 4 + j)
        for k in range(4)
        for j in range(4)
    ]
    mesh = bpy.data.meshes.new("Tapered continuous viewing aperture")
    mesh.from_pydata(rings, [], faces)
    mesh.update()
    mesh.materials.append(pale)
    obj = bpy.data.objects.new("cockpit-viewing-aperture", mesh)
    collection.objects.link(obj)
    obj["ipp_id"] = obj.name
    obj["cockpit_static"] = True
    bevel = obj.modifiers.new("Thin aperture edge chamfers", "BEVEL")
    bevel.width = 0.025
    bevel.segments = 3
    obj.modifiers.new("Aperture panel normals", "WEIGHTED_NORMAL")
    for side in (-1, 1):
        api["line"](
            "Aperture inset light",
            [(side * 6.67, 4.025, 1.0), (side * 6.62, 4.025, 5.5)],
            0.012,
            white,
        )
    api["line"](
        "Distant window sill light",
        [(x, 4.06 - 0.008 * x * x, 0.47) for x in np.linspace(-6.5, 6.5, 80)],
        0.012,
        white,
    )
    return obj
