"""Deformation and textured surfaces in the opt-in Blender stress scene."""

import math

import bpy
import numpy as np

from stress_authoring import action_bag, curve, identify, material, object_from_mesh


def animate_shape(obj, shape, frames, fps, phase):
    bag = action_bag(obj.data.shape_keys, f"{obj.name} vertex pose")
    curve(
        bag,
        shape.path_from_id("value"),
        0,
        frames,
        0.5 - 0.5 * np.cos((frames - 1) / fps * math.pi + phase),
    )


def add_pose_panels(scene, frames, fps, extent, count):
    image = bpy.data.images.new("Stress checker", width=4, height=4, alpha=False)
    image.pixels = [
        channel
        for y in range(4)
        for x in range(4)
        for channel in ((0.9, 0.12, 0.04, 1) if (x + y) % 2 else (0.04, 0.65, 0.9, 1))
    ]
    image.pack()
    materials = []
    for index in range(4):
        value = material(f"Pose surface {index}", (0.8, 0.6, 0.18))
        value["ipp_unlit"] = index < 2
        if index % 2:
            texture = value.node_tree.nodes.new("ShaderNodeTexImage")
            texture.image = image
            value.node_tree.links.new(
                texture.outputs["Color"],
                value.node_tree.nodes.get("Principled BSDF").inputs["Base Color"],
            )
        materials.append(value)
    parent = identify(
        bpy.data.objects.new("pose-affine-parent", None), "pose-affine-parent"
    )
    scene.collection.objects.link(parent)
    parent.scale = (1.15, 0.8, 1.05)
    parent.rotation_euler.z = 0.12
    result = []
    nx, nz = 24, 16
    for index in range(count):
        # Repeat four authoring variants; identical endpoint bytes are shared
        # across instances by the exporter's immutable asset catalog.
        if index < 4:
            mesh = bpy.data.meshes.new(f"Pose lattice {index}")
            vertices = [
                ((x / nx - 0.5) * 2.4, 0, z / nz * 3.0)
                for z in range(nz + 1)
                for x in range(nx + 1)
            ]
            faces = [
                (a, a + 1, a + nx + 2, a + nx + 1)
                for z in range(nz)
                for x in range(nx)
                for a in [z * (nx + 1) + x]
            ]
            mesh.from_pydata(vertices, [], faces)
            mesh.materials.append(materials[index])
            if index == 1:
                mesh.materials.append(materials[0])
                for polygon in mesh.polygons:
                    polygon.material_index = polygon.index % 2
            uv = mesh.uv_layers.new(name="UVMap")
            for loop in mesh.loops:
                point = mesh.vertices[loop.vertex_index].co
                uv.data[loop.index].uv = ((point.x / 2.4 + 0.5) * 3, point.z)
        else:
            mesh = result[index % 4].data
        obj = object_from_mesh(
            mesh,
            f"pose-panel-{index:02}",
            ((index % 8 - 3.5) * 3.2, -extent - 4 - index // 8 * 2, 0.1),
        )
        if index < 4:
            obj.shape_key_add(name="Basis")
            shape = obj.shape_key_add(name="Bend and ripple")
            for point in shape.data:
                point.co.x += 0.7 * (point.co.z / 3) ** 2
                point.co.y += 0.45 * math.sin(point.co.z * 2 + point.co.x)
            animate_shape(obj, shape, frames, fps, index * math.pi / 2)
        if index == 0:
            # Positive local TRS; composition creates nonuniform scale and shear.
            obj.parent = parent
            obj.rotation_euler.z = -0.22
        result.append(obj)
    return result


def sample_pose_probes(scene, frames, panels, humans):
    targets = [panels[0], panels[2]]
    if humans:
        targets.append(bpy.data.objects["walker-00-human"])
    probes = []
    try:
        for frame in (1, 13, 25, 37):
            if frame > frames[-1]:
                continue
            scene.frame_set(frame)
            graph = bpy.context.evaluated_depsgraph_get()
            for obj in targets:
                evaluated = obj.evaluated_get(graph)
                mesh = evaluated.to_mesh()
                try:
                    points = [evaluated.matrix_world @ v.co for v in mesh.vertices]
                    # Keep all extremal vertices plus a few interior probes. These
                    # come from Blender's evaluated mesh, including the armature.
                    indices = {0, len(points) // 2, len(points) - 1}
                    for axis in range(3):
                        indices.add(
                            min(range(len(points)), key=lambda i: points[i][axis])
                        )
                        indices.add(
                            max(range(len(points)), key=lambda i: points[i][axis])
                        )
                    probes.append(
                        {
                            "name": obj.name,
                            "frame": frame,
                            "weight": float(obj.data.shape_keys.key_blocks[1].value),
                            "vertices_blender": [
                                list(points[i]) for i in sorted(indices)
                            ],
                        }
                    )
                finally:
                    evaluated.to_mesh_clear()
    finally:
        scene.frame_set(1)
    return probes
