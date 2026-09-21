"""Stable authoring identities and coordinate/parent transforms."""

import math
import uuid

from mathutils import Matrix

from .types import BASIS, Unsupported


def converted(exporter, matrix, directional=False):
    result = BASIS @ matrix if directional else BASIS @ matrix @ BASIS.inverted()
    result.translation *= exporter.unit
    return result


def trs(matrix):
    location, rotation, scale = matrix.decompose()
    if min(scale) <= 0 or matrix.determinant() <= 0:
        raise Unsupported("Negative or zero scale requires an explicit bake")
    rebuilt = Matrix.LocRotScale(location, rotation, scale)
    error = max(abs(matrix[r][c] - rebuilt[r][c]) for r in range(4) for c in range(4))
    if error > 1e-4 * max(1, max(abs(v) for row in matrix for v in row)):
        raise Unsupported("Sheared transforms require an explicit bake")
    values = [*location, rotation.x, rotation.y, rotation.z, rotation.w, *scale]
    if not all(math.isfinite(value) for value in values):
        raise Unsupported("Non-finite transform")
    return {
        "translation": list(location),
        "rotation": values[3:7],
        "scale": list(scale),
    }


def transform(exporter, obj):
    matrix = converted(exporter, obj.matrix_world, obj.type in {"CAMERA", "LIGHT"})
    if obj.parent and obj.parent_type in {"OBJECT", "BONE"}:
        parent_matrix = obj.parent.matrix_world
        if obj.parent_type == "BONE":
            if (
                obj.parent.name not in exporter.rigs
                or obj.parent_bone not in exporter.rigs[obj.parent.name]["indices"]
            ):
                raise Unsupported(
                    "Bone parent requires an exported skeleton and valid bone"
                )
            parent_matrix = (
                parent_matrix @ obj.parent.pose.bones[obj.parent_bone].matrix
            )
        parent = converted(
            exporter, parent_matrix, obj.parent.type in {"CAMERA", "LIGHT"}
        )
        matrix = parent.inverted() @ matrix
    values = trs(matrix)
    return dict(
        zip(
            ("x", "y", "z", "qx", "qy", "qz", "qw", "sx", "sy", "sz"),
            (*values["translation"], *values["rotation"], *values["scale"]),
            strict=True,
        )
    )


def base(exporter, obj, identifier=None):
    result = {
        "id": identifier or exporter.ids[obj.name],
        "name": obj.name,
        "transform": transform(exporter, obj),
    }
    if obj.parent and obj.parent_type in {"OBJECT", "BONE"}:
        if obj.parent.name not in exporter.ids:
            raise Unsupported("Parent has no export identity")
        result["parent"] = exporter.ids[obj.parent.name]
        if obj.parent_type == "BONE":
            result["parent_bone"] = exporter.rigs[obj.parent.name]["indices"][
                obj.parent_bone
            ]
    elif obj.parent:
        exporter.diagnostic(
            "parent-baked",
            "Vertex parenting is baked into the snapshot world transform",
            obj,
        )
    return result


def identify(exporter, objects):
    seen = set()
    for obj in objects:
        identifier = obj.get("ipp_id")
        if not isinstance(identifier, str) or not identifier or identifier in seen:
            duplicate = identifier in seen if isinstance(identifier, str) else False
            if obj.library is not None:
                exporter.diagnostic(
                    "linked-object",
                    "Link must be made local before assigning an export identity",
                    obj,
                )
                continue
            identifier = str(uuid.uuid4())
            obj["ipp_id"] = identifier
            if duplicate:
                exporter.diagnostic(
                    "duplicate-id",
                    "Duplicate ipp_id was replaced on this object",
                    obj,
                )
        seen.add(identifier)
        exporter.ids[obj.name] = identifier
