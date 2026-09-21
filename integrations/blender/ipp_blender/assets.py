"""Target-independent immutable IPP asset encoders; no Blender objects escape."""

import json
import math
import struct

MAX_JOINTS = 32


def floats(values):
    values = tuple(values)
    if not all(math.isfinite(v) for v in values):
        raise ValueError("Non-finite asset value")
    return struct.pack(f"<{len(values)}f", *values)


def mesh_asset(positions, normals, uvs, joints, weights):
    """Expanded triangle corners preserve material, UV and hard-normal seams."""
    count = len(positions)
    if not 0 < count <= 65536 or count % 3:
        raise ValueError("Mesh requires 1..65536 triangle-corner vertices")
    streams = [(0, 1, floats(v for p in positions for v in p))]
    if uvs:
        streams.append((2, 2, floats(v for p in uvs for v in p)))
    streams.append((4, 1, floats(v for p in normals for v in p)))
    if joints:
        streams.extend(
            [
                (5, 4, bytes(v for p in joints for v in p)),
                (6, 5, floats(v for p in weights for v in p)),
            ]
        )
    header = b"IPPM" + struct.pack("<4I", 3, count, count, len(streams))
    descriptors = b"".join(
        struct.pack("<BBHI", sem, fmt, 0, len(data)) for sem, fmt, data in streams
    )
    indices = struct.pack(f"<{count}H", *range(count))
    return header + descriptors + b"".join(data for _, _, data in streams) + indices


def joint_values(trs):
    return (*trs["translation"], *trs["rotation"], *trs["scale"])


def skeleton_asset(parents, rests):
    if not 1 <= len(rests) <= MAX_JOINTS:
        raise ValueError("Skeleton requires 1..32 joints")
    return (
        b"IPPS"
        + struct.pack("<II", 1, len(rests))
        + b"".join(
            struct.pack("<I", 0xFFFFFFFF if parent is None else parent)
            + floats(joint_values(rest))
            for parent, rest in zip(parents, rests, strict=True)
        )
    )


def pose_asset(poses):
    return (
        b"IPPP"
        + struct.pack("<II", 1, len(poses))
        + b"".join(floats(joint_values(p)) for p in poses)
    )


def skin_asset(matrices):
    return (
        b"IPPB"
        + struct.pack("<II", 1, len(matrices))
        + b"".join(
            struct.pack("<I", index)
            + floats(matrix[row][column] for column in range(4) for row in range(4))
            for index, matrix in enumerate(matrices)
        )
    )


def texture_asset(width, height, rgba):
    return b"IPPT" + struct.pack("<III", 3, width, height) + rgba


def clip_asset(clip):
    return json.dumps(clip, allow_nan=False, separators=(",", ":")).encode()
