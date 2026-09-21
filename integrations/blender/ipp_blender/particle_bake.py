"""Sequential Blender particle sampling and portable IPPC encoding."""

import math
import struct

import bpy
from mathutils import Quaternion

from .assets import floats
from .exporter.types import BASIS, Unsupported
from .sampling import SamplingSchedule


def cache_asset(frames, space=1):
    """IPPC v1 has an explicit frame directory and stable per-particle identities."""
    offset = 16 + 12 * len(frames)
    directory = []
    blocks = []
    for time, samples in frames:
        directory.append(struct.pack("<fII", time, offset, len(samples)))
        block = b"".join(
            struct.pack("<Q", identifier) + floats(values)
            for identifier, values in samples
        )
        blocks.append(block)
        offset += len(block)
    return (
        b"IPPC"
        + struct.pack("<III", 1, space, len(frames))
        + b"".join(directory + blocks)
    )


def prepare_particles(exporter, objects, schedule):
    result = {}
    for obj in objects:
        if obj.get("ipp_particles", "NATIVE") == "BAKED":
            for index, system in enumerate(obj.particle_systems):
                if system.settings.type == "EMITTER":
                    schedule.add(
                        _bake_job(exporter, obj, index, result), simulation=True
                    )
    return result


def bake(exporter, obj, system_index):
    result = {}
    schedule = SamplingSchedule(exporter)
    schedule.add(_bake_job(exporter, obj, system_index, result), simulation=True)
    schedule.run()
    value = result[(obj.name, system_index)]
    if isinstance(value, Exception):
        raise value
    return value


def _bake_job(exporter, obj, system_index, result):
    try:
        yield from _sample_particles(exporter, obj, system_index, result)
    except (Unsupported, ValueError) as error:
        # Keep the ordinary per-system diagnostic and continue other sample jobs.
        result[(obj.name, system_index)] = error


def _sample_particles(exporter, obj, system_index, result):
    scene = exporter.scene
    fps = scene.render.fps / scene.render.fps_base
    start, end = scene.frame_start, scene.frame_end
    system = obj.particle_systems[system_index]
    warmup = min(start, math.floor(system.settings.frame_start))
    frames = []
    rotation_basis = BASIS.to_quaternion()
    yield warmup - 1
    for number in range(warmup, end + 1):
        yield number
        evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
        particles = evaluated.particle_systems[system_index].particles
        if number < start:
            continue
        samples = []
        for index, particle in enumerate(particles):
            if particle.alive_state != "ALIVE":
                continue
            location = BASIS @ particle.location * exporter.unit
            velocity = BASIS @ particle.velocity * exporter.unit
            # Blender leaves rotation storage unspecified when rotations are disabled.
            rotation = (
                (
                    rotation_basis @ particle.rotation @ rotation_basis.inverted()
                ).normalized()
                if system.settings.use_rotations
                else Quaternion()
            )
            samples.append(
                (
                    index,
                    (
                        (particle.birth_time - start) / fps,
                        (particle.die_time - start) / fps,
                        *location,
                        *velocity,
                        rotation.x,
                        rotation.y,
                        rotation.z,
                        rotation.w,
                        particle.size,
                    ),
                )
            )
        frames.append(((number - start) / fps, samples))
    result[(obj.name, system_index)] = (
        exporter.source(cache_asset(frames), key=("particles", obj.name, system_index)),
        (end - start) / fps,
    )
