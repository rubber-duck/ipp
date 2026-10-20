"""One mesh of compact star profiles from ESA Hipparcos photometry/positions.

UV integer cells carry magnitude and B-V; the interior of each cell carries the
point profile. No vertex-color contract or runtime catalogue download is needed.
"""

import csv
import math
from pathlib import Path

import bpy
from mathutils import Vector


def build(camera, material):
    vertices, faces, coordinates = [], [], []
    source = Path(__file__).resolve().parent / "stars/hipparcos.csv"
    with source.open() as stream:
        for star in csv.DictReader(stream):
            # A fixed celestial orientation, independent of viewport framing.
            ra = math.radians(float(star["ra_degrees"]) - 90)
            dec = math.radians(float(star["dec_degrees"]))
            direction = Vector(
                (
                    math.cos(dec) * math.sin(ra),
                    math.cos(dec) * math.cos(ra),
                    math.sin(dec),
                )
            )
            right = Vector((math.cos(ra), -math.sin(ra), 0))
            up = right.cross(direction)
            magnitude = round((float(star["magnitude_v"]) + 2) * 20)
            color = round((max(-0.4, min(2, float(star["b_minus_v"]))) + 0.4) * 20)
            start = len(vertices)
            for x, y in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                vertices.append(camera + direction * 790 + (right * x + up * y) * 1.2)
                # Export flips Blender's V coordinate. Stay within one integer
                # cell so interpolation cannot change a star's photometry.
                coordinates.append(
                    (magnitude + 0.5 + x * 0.4, 1 - (color + 0.5 + y * 0.4))
                )
            faces.append(tuple(start + i for i in range(4)))
    mesh = bpy.data.meshes.new("Hipparcos point profiles")
    mesh.from_pydata(vertices, [], faces)
    uv = mesh.uv_layers.new(name="Star photometry and profile")
    for loop in mesh.loops:
        uv.data[loop.index].uv = coordinates[loop.vertex_index]
    mesh.materials.append(material)
    obj = bpy.data.objects.new("cockpit-starfield", mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj["ipp_id"] = obj.name
    obj["environment_role"] = "starlight"
    return obj
