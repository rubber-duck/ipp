"""Spatial bridge shell, cubemap reprojection and oblate planet authoring.

Generated artwork stays unchanged. Cubemap faces are deterministic projections
of bridge-panorama.png, with matching border directions on adjacent faces.
"""

import math
import runpy
from pathlib import Path

import bpy
import numpy as np
from mathutils import Vector

ROOT = Path(__file__).resolve().parent
FACE_SIZE = 768
EARTH_QUALITY = "4k"
FACES = (
    ("front", (0, 1, 0), (1, 0, 0), (0, 0, 1)),
    ("right", (1, 0, 0), (0, -1, 0), (0, 0, 1)),
    ("back", (0, -1, 0), (-1, 0, 0), (0, 0, 1)),
    ("left", (-1, 0, 0), (0, 1, 0), (0, 0, 1)),
    ("top", (0, 0, 1), (1, 0, 0), (0, -1, 0)),
    ("bottom", (0, 0, -1), (1, 0, 0), (0, 1, 0)),
)


def image(name):
    value = bpy.data.images.load(str(ROOT / name), check_existing=True)
    value.pack()
    value.filepath = "//" + name
    return value


def textured(name, source, aperture=False):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material["ipp_unlit"] = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = source
    links.new(
        texture.outputs["Color"], nodes.get("Principled BSDF").inputs["Base Color"]
    )
    emission = nodes.new("ShaderNodeEmission")
    links.new(texture.outputs["Color"], emission.inputs["Color"])
    output = emission.outputs[0]
    if aperture:
        transparent = nodes.new("ShaderNodeBsdfTransparent")
        threshold = nodes.new("ShaderNodeMath")
        threshold.operation = "GREATER_THAN"
        threshold.inputs[1].default_value = 0.5
        links.new(texture.outputs["Alpha"], threshold.inputs[0])
        mix = nodes.new("ShaderNodeMixShader")
        links.new(threshold.outputs[0], mix.inputs[0])
        links.new(transparent.outputs[0], mix.inputs[1])
        links.new(emission.outputs[0], mix.inputs[2])
        output = mix.outputs[0]
    links.new(output, nodes.get("Material Output").inputs["Surface"])
    return material


def empty(name, location=(0, 0, 0)):
    obj = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(obj)
    obj["ipp_id"] = name
    obj.location = location
    return obj


def cubemap(camera):
    source = image("bridge-panorama.png")
    width, height = source.size
    data = np.empty(width * height * 4, np.float32)
    source.pixels.foreach_get(data)
    data = data.reshape(height, width, 4)
    # Pixel centers and extended half-texel edge samples share one spherical map.
    s, t = np.meshgrid(np.linspace(-1, 1, FACE_SIZE), np.linspace(-1, 1, FACE_SIZE))
    pitch = math.radians(22)
    faces = []
    for name, axis, right, up in FACES:
        direction = np.array(axis) + s[:, :, None] * right + t[:, :, None] * up
        direction /= np.linalg.norm(direction, axis=2, keepdims=True)
        x, y, z = direction.transpose(2, 0, 1)
        y, z = (
            y * math.cos(pitch) - z * math.sin(pitch),
            y * math.sin(pitch) + z * math.cos(pitch),
        )
        u = (0.5 + np.arctan2(x, y) / math.tau) % 1
        v = 0.5 + np.arcsin(np.clip(z, -1, 1)) / math.pi
        px, py = u * width - 0.5, np.clip(v * height - 0.5, 0, height - 1)
        ix, iy = np.floor(px).astype(int), np.floor(py).astype(int)
        fx, fy = (px - ix)[:, :, None], (py - iy)[:, :, None]
        a = data[iy, ix % width] * (1 - fx) + data[iy, (ix + 1) % width] * fx
        b = (
            data[np.minimum(iy + 1, height - 1), ix % width] * (1 - fx)
            + data[np.minimum(iy + 1, height - 1), (ix + 1) % width] * fx
        )
        pixels = a * (1 - fy) + b * fy
        face = bpy.data.images.new(
            "bridge-cube-" + name, FACE_SIZE, FACE_SIZE, alpha=True
        )
        face.pixels.foreach_set(pixels.astype(np.float32).ravel())
        face.filepath_raw = str(ROOT / (face.name + ".png"))
        face.file_format = "PNG"
        face.save()
        face.pack()
        face.filepath = "//bridge-cube-" + name + ".png"
        mesh = bpy.data.meshes.new("Bridge cubemap " + name)
        corners = [(-1, -1), (1, -1), (1, 1), (-1, 1)]
        mesh.from_pydata(
            [
                camera
                + Vector((24, 24, 12))
                * (Vector(axis) + a * Vector(right) + b * Vector(up))
                for a, b in corners
            ],
            [],
            [(0, 1, 2, 3)],
        )
        uv = mesh.uv_layers.new(name="Cubemap face")
        for loop in mesh.loops:
            a, b = corners[loop.vertex_index]
            uv.data[loop.index].uv = ((a + 1) / 2, (b + 1) / 2)
        mesh.materials.append(textured("Bridge " + name, face, aperture=True))
        obj = bpy.data.objects.new("cockpit-bridge-" + name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        obj["ipp_id"] = obj.name
        obj["environment_role"] = "bridge"
        faces.append(obj.name)
    return faces


def sphere(name, location, radius, material, role, scale=(1, 1, 1), inward=False):
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=96
        if role in ("planet", "atmosphere")
        else 32
        if role == "stars"
        else 8,
        ring_count=48
        if role in ("planet", "atmosphere")
        else 16
        if role == "stars"
        else 4,
        radius=radius,
        location=location,
    )
    obj = bpy.context.object
    obj.name = name
    obj["ipp_id"] = name
    obj["environment_role"] = role
    obj.scale = scale
    obj.data.materials.append(material)
    for face in obj.data.polygons:
        face.use_smooth = True
    if inward:
        import bmesh

        bm = bmesh.new()
        bm.from_mesh(obj.data)
        bmesh.ops.reverse_faces(bm, faces=list(bm.faces))
        bm.to_mesh(obj.data)
        bm.free()
    return obj


def build(camera):
    bridge = empty("cockpit-bridge")
    mount = empty("cockpit-carousel-mount", camera)
    # Blender Y forward / Z up -> IPP -Z forward / Y up; this frame is authored
    # with the bridge and is deliberately independent of the Camera entity.
    mount.parent = bridge
    mount["pane_width"] = 4.8
    mount["pane_height"] = 4.0
    mount["pane_elevation"] = 0.65
    faces = cubemap(camera)
    for name in faces:
        bpy.data.objects[name].parent = bridge
    # NASA's registered maps replace the generated globe. Keep the prepared 8K
    # master on disk; the demo uses 4K albedo/clouds and 2K night emission.
    base = image(f"earth/surface-{EARTH_QUALITY}.jpg")
    cities = image(f"earth/cities-{'4k' if EARTH_QUALITY == '8k' else '2k'}.png")
    center = Vector((-200, 280, -110))
    planet = sphere(
        "cockpit-planet",
        center,
        183,
        textured("Planet albedo", base),
        "planet",
        scale=(1, 1, 0.985),
    )
    # Pack weather coverage into the unused albedo alpha at export. The shader
    # reads that channel at independently drifting UVs, without another RGBA map.
    planet["cloud_map"] = f"earth/clouds-{EARTH_QUALITY}.png"
    atmosphere = sphere(
        "cockpit-atmosphere",
        center,
        183 * 1.018,
        textured("Atmosphere export carrier", base),
        "atmosphere",
        scale=(1, 1, 0.985),
    )
    atmosphere.hide_render = True
    dark = bpy.data.images.new("Space black", 1, 1)
    dark.pixels[:] = (0, 0, 0, 1)
    dark.pack()
    sky_material = textured("Unlit space", dark)
    stars = sphere(
        "cockpit-space",
        camera,
        800,
        sky_material,
        "stars",
        inward=True,
    )
    runpy.run_path(str(ROOT / "starfield.py"))["build"](camera, sky_material)
    carrier = sphere(
        "cockpit-planet-cities",
        center,
        1,
        textured("City emission export carrier", cities),
        "carrier",
    )
    carrier["export_texture_carrier"] = True
    carrier["exterior_layer"] = "cities"
    carrier.hide_render = True
    return {
        "bridge": bridge.name,
        "mount": mount.name,
        "paneExtent": [mount["pane_width"], mount["pane_height"]],
        "paneElevation": mount["pane_elevation"],
        "cubemap": faces,
        "planet": planet.name,
        "atmosphere": atmosphere.name,
        "stars": stars.name,
        "cities": carrier.name,
        "radius": 183,
        "polarRatio": 0.985,
    }
