"""Fixed inward-facing bridge panorama shell and oblate planet authoring."""

import json
import math
import runpy
from pathlib import Path

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parent
EARTH_QUALITY = "4k"
BRIDGE_RADIUS = 12
BRIDGE_FORWARD_OFFSET = 8


def image(name):
    value = bpy.data.images.load(str(ROOT / name), check_existing=True)
    value.pack()
    value.filepath = "//" + name
    return value


def textured(name, source, aperture=False, lamps=None):
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
    color = texture.outputs["Color"]
    if lamps is not None:
        lamp_texture = nodes.new("ShaderNodeTexImage")
        lamp_texture.image = lamps
        combined = nodes.new("ShaderNodeMixRGB")
        combined.blend_type = "ADD"
        combined.inputs[0].default_value = 1
        links.new(color, combined.inputs[1])
        links.new(lamp_texture.outputs["Color"], combined.inputs[2])
        color = combined.outputs[0]
    links.new(color, emission.inputs["Color"])
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


def bridge_interior(camera):
    # Four independent high-detail textures cover one fixed spherical shell.
    # Use the original pixel split latitude (the source has an odd height), so
    # the neighboring mesh boundaries coincide without shifting the artwork.
    center = camera + Vector((0, BRIDGE_FORWARD_OFFSET, 0))
    tiles = json.loads((ROOT / "bridge-tiles.json").read_text())["tiles"]
    parts = []
    for quadrant, tile in enumerate(tiles):
        name = tile["name"]
        left, top, right, bottom = tile["bounds"]
        segments, rings = 64, 32
        positions, coordinates, faces = [], [], []
        for ring in range(rings + 1):
            v = ring / rings
            latitude = (0.5 - (bottom + (top - bottom) * v)) * math.pi
            for segment in range(segments + 1):
                u = segment / segments
                longitude = (left + (right - left) * u - 0.5) * math.tau
                positions.append(
                    center
                    + BRIDGE_RADIUS
                    * Vector(
                        (
                            math.cos(latitude) * math.sin(longitude),
                            math.cos(latitude) * math.cos(longitude),
                            math.sin(latitude),
                        )
                    )
                )
                coordinates.append((u, v))
        for ring in range(rings):
            for segment in range(segments):
                a = ring * (segments + 1) + segment
                b, c, d = a + 1, a + segments + 2, a + segments + 1
                # Longitude then latitude winds inward. Only the sphere's poles
                # need triangles; the quadrant boundaries remain ordinary quads.
                faces.append(
                    (a, c, d)
                    if ring == 0 and bottom == 1
                    else (a, b, c)
                    if ring == rings - 1 and top == 0
                    else (a, b, c, d)
                )
        mesh = bpy.data.meshes.new("Bridge spherical quadrant " + name)
        mesh.from_pydata(positions, [], faces)
        mesh.update()
        uv = mesh.uv_layers.new(name="Quadrant-local panorama")
        for loop in mesh.loops:
            uv.data[loop.index].uv = coordinates[loop.vertex_index]
        contribution_map = "bridge-" + name + "-contribution.png"
        mesh.materials.append(
            textured(
                "Bridge " + name,
                image("bridge-" + name + "-fixtures-base.png"),
                aperture=True,
                lamps=image(contribution_map),
            )
        )
        obj = bpy.data.objects.new("cockpit-bridge-" + name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        obj["ipp_id"] = obj.name
        obj["environment_role"] = "bridge"
        obj["lighting_key"] = "bridge-" + name
        obj["panorama_bounds"] = tile["bounds"]
        obj["panorama_quadrant"] = quadrant
        parts.append(obj)
    return parts


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
    interior = bridge_interior(camera)
    for part in interior:
        part.parent = bridge
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
        "interior": [part.name for part in interior],
        "panoramaSplit": json.loads((ROOT / "bridge-tiles.json").read_text())["tiles"][
            0
        ]["bounds"][3],
        "interiorRadius": BRIDGE_RADIUS,
        "interiorForwardOffset": BRIDGE_FORWARD_OFFSET,
        "planet": planet.name,
        "atmosphere": atmosphere.name,
        "stars": stars.name,
        "cities": carrier.name,
        "radius": 183,
        "polarRatio": 0.985,
    }
