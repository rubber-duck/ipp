"""Rig, mesh, skin and shape-key extraction."""

import math

import bpy

from ..assets import (
    MAX_JOINTS,
    mesh_asset,
    pose_asset,
    skeleton_asset,
    skin_asset,
)
from . import appearance, identity
from .types import BASIS, Unsupported


def rig(exporter, obj):
    bones = []

    def visit(bone):
        bones.append(bone)
        for child in sorted(bone.children, key=lambda b: b.name):
            visit(child)

    for root in sorted(
        (b for b in obj.data.bones if b.parent is None), key=lambda b: b.name
    ):
        visit(root)
    if not 1 <= len(bones) <= MAX_JOINTS:
        raise Unsupported("Skeleton must contain 1..32 bones")
    indices = {bone.name: i for i, bone in enumerate(bones)}
    rests = []
    for bone in bones:
        matrix = bone.matrix_local.copy()
        if bone.parent:
            matrix = bone.parent.matrix_local.inverted() @ matrix
        rests.append(identity.trs(identity.converted(exporter, matrix)))
    exporter.rigs[obj.name] = {"object": obj, "bones": bones, "indices": indices}
    result = identity.base(exporter, obj)
    result["skeleton"] = {
        "source": exporter.asset(
            skeleton_asset,
            [indices[b.parent.name] if b.parent else None for b in bones],
            rests,
        ),
        "pose_source": exporter.asset(pose_asset, poses(exporter, obj)),
    }
    return result


def poses(exporter, obj):
    poses = []
    for bone in exporter.rigs[obj.name]["bones"]:
        pose = obj.pose.bones[bone.name]
        matrix = pose.matrix.copy()
        if pose.parent:
            matrix = pose.parent.matrix.inverted() @ matrix
        poses.append(identity.trs(identity.converted(exporter, matrix)))
    return poses


def weights(exporter, obj, mesh, rig):
    index = exporter.rigs[rig.name]["indices"]
    groups = {g.index: index[g.name] for g in obj.vertex_groups if g.name in index}
    result = []
    for vertex in mesh.vertices:
        values = [
            (groups[g.group], g.weight)
            for g in vertex.groups
            if g.group in groups and g.weight > 0
        ]
        values.sort(key=lambda pair: (-pair[1], pair[0]))
        if not 1 <= len(values) <= 4:
            raise Unsupported(
                "Every skinned vertex requires 1..4 nonzero bone influences"
            )
        total = sum(weight for _, weight in values)
        values = [(joint, weight / total) for joint, weight in values]
        values += [(0, 0)] * (4 - len(values))
        result.append(
            ([joint for joint, _ in values], [weight for _, weight in values])
        )
    return result


def meshes(exporter, obj, depsgraph):
    modifiers = [m for m in obj.modifiers if m.show_viewport]
    armatures = [m for m in modifiers if m.type == "ARMATURE"]
    rig = None
    evaluated = None
    shape = None
    try:
        shape = shape_key(exporter, obj)
        if shape is not None and (
            any(modifier.type != "ARMATURE" for modifier in modifiers)
            or obj.show_only_shape_key
        ):
            raise Unsupported(
                "Mesh poses require no modifiers except a supported armature, and no isolated shape-key display"
            )
    except Unsupported as error:
        if (
            armatures
            and obj.data.shape_keys
            and any(abs(key.value) > 1e-6 for key in obj.data.shape_keys.key_blocks[1:])
        ):
            raise Unsupported(
                f"Unsupported shape deformation with skin: {error}"
            ) from error
        exporter.diagnostic(
            "mesh-pose-baked",
            f"{error}; {'inactive shape keys omitted' if armatures else 'evaluated snapshot baked instead'}",
            obj,
        )
        shape = None
    temporary = []
    target_mesh = None
    try:
        if armatures:
            modifier = armatures[0]
            rig = modifier.object
            if (
                len(modifiers) != 1
                or len(armatures) != 1
                or rig is None
                or rig.name not in exporter.rigs
            ):
                raise Unsupported(
                    "Skin requires exactly one supported armature modifier and no other enabled modifiers"
                )
            if (
                modifier.use_deform_preserve_volume
                or not modifier.use_vertex_groups
                or modifier.use_bone_envelopes
            ):
                raise Unsupported("Skin supports linear vertex-group deformation only")
            mesh = obj.data
        if shape is not None:
            # Copies preserve corner correspondence without changing authored key values.
            for key in (obj.data.shape_keys.reference_key, shape):
                copy = obj.data.copy()
                temporary.append(copy)
                for vertex, point in zip(copy.vertices, key.data, strict=True):
                    vertex.co = point.co
                copy.update()
            mesh, target_mesh = temporary
        elif rig is None:
            evaluated = obj.evaluated_get(depsgraph)
            mesh = evaluated.to_mesh(preserve_all_data_layers=True, depsgraph=depsgraph)
            if modifiers:
                exporter.diagnostic(
                    "modifiers-baked",
                    "Rigid modifier result baked into mesh; modifier animation is not exported",
                    obj,
                )
        mesh.calc_loop_triangles()
        if len(mesh.loop_triangles) * 3 > 65536:
            raise Unsupported("Mesh exceeds 65536 expanded triangle corners")
        influence = weights(exporter, obj, mesh, rig) if rig else None
        uv_layer = mesh.uv_layers.active
        slots = sorted({triangle.material_index for triangle in mesh.loop_triangles})
        results = [identity.base(exporter, obj)] if len(slots) > 1 else []
        pose_targets = []
        skin_source = None
        if rig:
            bind = identity.converted(
                exporter, rig.matrix_world
            ).inverted() @ identity.converted(exporter, obj.matrix_world)
            skin_source = exporter.source(
                skin_asset(
                    [
                        identity.converted(exporter, b.matrix_local).inverted() @ bind
                        for b in exporter.rigs[rig.name]["bones"]
                    ]
                )
            )
        for slot in slots:
            positions, normals, uvs, joints, corner_weights = [], [], [], [], []
            target_positions, target_normals = [], []
            for triangle in mesh.loop_triangles:
                if triangle.material_index != slot:
                    continue
                for loop_index in triangle.loops:
                    vertex_index = mesh.loops[loop_index].vertex_index
                    positions.append(
                        list(
                            BASIS.to_3x3()
                            @ mesh.vertices[vertex_index].co
                            * exporter.unit
                        )
                    )
                    normals.append(
                        list(BASIS.to_3x3() @ mesh.corner_normals[loop_index].vector)
                    )
                    if target_mesh is not None:
                        target_positions.append(
                            list(
                                BASIS.to_3x3()
                                @ target_mesh.vertices[vertex_index].co
                                * exporter.unit
                            )
                        )
                        target_normals.append(
                            list(
                                BASIS.to_3x3()
                                @ target_mesh.corner_normals[loop_index].vector
                            )
                        )
                    if uv_layer:
                        uv = uv_layer.data[loop_index].uv
                        uvs.append([uv.x, 1 - uv.y])
                    if influence is not None:
                        joint, weight = influence[vertex_index]
                        joints.append(joint)
                        corner_weights.append(weight)
            identifier = (
                exporter.ids[obj.name]
                if len(slots) == 1
                else f"{exporter.ids[obj.name]}:material:{slot}"
            )
            result = identity.base(exporter, obj, identifier)
            if len(slots) > 1:
                result["name"] = f"{obj.name} [material {slot}]"
                result["parent"] = exporter.ids[obj.name]
                result["transform"] = dict(
                    zip(
                        ("x", "y", "z", "qx", "qy", "qz", "qw", "sx", "sy", "sz"),
                        (0, 0, 0, 0, 0, 0, 1, 1, 1, 1),
                        strict=True,
                    )
                )
            material = (
                obj.material_slots[slot].material
                if slot < len(obj.material_slots)
                else None
            )
            result.update(
                appearance.material(exporter, material, obj, uv_layer is not None)
            )
            result["mesh"] = {
                "source": exporter.asset(
                    mesh_asset, positions, normals, uvs, joints, corner_weights
                )
            }
            if target_mesh is not None:
                result["mesh_pose"] = {
                    "source": exporter.asset(
                        mesh_asset, target_positions, target_normals, [], [], []
                    ),
                    "weight": shape_weight(shape),
                }
                pose_targets.append(identifier)
            if rig:
                result["skin"] = {
                    "source": skin_source,
                    "skeleton": exporter.ids[rig.name],
                }
            results.append(result)
        if pose_targets:
            exporter.mesh_poses[obj.name] = (shape, pose_targets)
        return results
    finally:
        if evaluated is not None:
            evaluated.to_mesh_clear()
        for copy in temporary:
            bpy.data.meshes.remove(copy)


def shape_key(exporter, obj):
    keys = obj.data.shape_keys
    if keys is None or len(keys.key_blocks) <= 1:
        return None
    if not keys.use_relative or len(keys.key_blocks) != 2:
        raise Unsupported("Mesh poses support one relative shape key plus Basis")
    key = keys.key_blocks[1]
    if key.relative_key != keys.reference_key or key.vertex_group:
        raise Unsupported(
            "Mesh pose shape key must be relative to Basis without a vertex group"
        )
    shape_weight(key)
    return key


def shape_weight(key):
    value = 0.0 if key.mute else key.value
    if not math.isfinite(value) or not 0 <= value <= 1:
        raise Unsupported("Mesh pose shape-key weight must be within [0, 1]")
    return value
