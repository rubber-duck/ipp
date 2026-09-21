"""Independent Blender oracle for portable particle cache samples."""

import math
import struct
import sys
from pathlib import Path

import bpy
from mathutils import Quaternion

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "integrations/blender"))
from ipp_blender.exporter.types import BASIS


def assert_cache(obj, system_index, cache):
    scene = bpy.context.scene
    frame, subframe = scene.frame_current, scene.frame_subframe
    start, end = scene.frame_start, scene.frame_end
    fps = scene.render.fps / scene.render.fps_base
    unit = scene.unit_settings.scale_length
    assert cache[:4] == b"IPPC"
    assert struct.unpack_from("<III", cache, 4) == (1, 1, end - start + 1)
    rotation_basis = BASIS.to_quaternion()
    checked = 0
    system = obj.particle_systems[system_index]
    warmup = min(start, math.floor(system.settings.frame_start))
    try:
        scene.frame_set(warmup - 1)
        for number in range(warmup, end + 1):
            scene.frame_set(number)
            particles = (
                obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
                .particle_systems[system_index]
                .particles
            )
            if number < start:
                continue
            time, offset, count = struct.unpack_from(
                "<fII", cache, 16 + (number - start) * 12
            )
            assert abs(time - (number - start) / fps) < 1e-5
            alive = [
                (i, p) for i, p in enumerate(particles) if p.alive_state == "ALIVE"
            ]
            assert count == len(alive), (obj.name, number, count, len(alive))
            for sample, (identifier, particle) in enumerate(alive):
                actual_id, *actual = struct.unpack_from(
                    "<Q13f", cache, offset + sample * 60
                )
                assert actual_id == identifier
                position = BASIS @ particle.location * unit
                velocity = BASIS @ particle.velocity * unit
                rotation = (
                    (
                        rotation_basis @ particle.rotation @ rotation_basis.inverted()
                    ).normalized()
                    if system.settings.use_rotations
                    else Quaternion()
                )
                expected = [
                    (particle.birth_time - start) / fps,
                    (particle.die_time - start) / fps,
                    *position,
                    *velocity,
                    rotation.x,
                    rotation.y,
                    rotation.z,
                    rotation.w,
                    particle.size,
                ]
                error = max(abs(a - b) for a, b in zip(actual, expected, strict=True))
                assert error < 3e-4, (
                    obj.name,
                    number,
                    identifier,
                    error,
                    actual,
                    expected,
                )
                checked += 1
    finally:
        scene.frame_set(frame, subframe=subframe)
    assert checked > 0, f"{obj.name} produced no living samples"
    return checked
