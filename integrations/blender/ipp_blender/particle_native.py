"""Supported native particle recipes and static instance geometry."""

from mathutils import Matrix

from .assets import mesh_asset
from .exporter.types import BASIS, Unsupported


def static_mesh(exporter, obj, *, instance=False):
    if (
        obj.type != "MESH"
        or obj.data.shape_keys
        or any(m.show_viewport and m.type != "PARTICLE_SYSTEM" for m in obj.modifiers)
    ):
        raise Unsupported(
            "Particle recipes require a static mesh; select ipp_particles='BAKED'"
        )
    mesh = obj.data
    mesh.calc_loop_triangles()
    scale = Matrix.Diagonal((*obj.scale, 1.0)) if instance else Matrix.Identity(4)
    matrix = BASIS @ scale
    normal_matrix = matrix.to_3x3().inverted().transposed()
    positions, normals, uvs = [], [], []
    uv = mesh.uv_layers.active
    for triangle in mesh.loop_triangles:
        for vertex, loop in zip(triangle.vertices, triangle.loops, strict=True):
            positions.append(tuple((matrix @ mesh.vertices[vertex].co) * exporter.unit))
            normals.append(tuple((normal_matrix @ triangle.normal).normalized()))
            if uv:
                point = uv.data[loop].uv
                uvs.append((point.x, 1 - point.y))
    return exporter.asset(mesh_asset, positions, normals, uvs, [], [])


def native(exporter, obj, system):
    settings = system.settings
    unsupported = []
    if (
        settings.type != "EMITTER"
        or settings.physics_type != "NEWTON"
        or settings.emit_from != "FACE"
    ):
        unsupported.append("only Newtonian face emitters are supported")
    if settings.child_type != "NONE" or settings.use_rotations:
        unsupported.append("children and authored rotations require baking")
    for name in (
        "brownian_factor",
        "damping",
        "tangent_factor",
        "object_factor",
        "factor_random",
    ):
        if abs(getattr(settings, name, 0.0)) > 1e-6:
            unsupported.append(name)
    if any(abs(v) > 1e-6 for v in settings.object_align_factor):
        unsupported.append("object_align_factor")
    if any(
        (o.field is not None and o.field.type != "NONE")
        or any(m.type == "COLLISION" for m in o.modifiers)
        for o in exporter.scene.objects
    ):
        unsupported.append("force fields or collisions")
    if settings.effector_weights.gravity != 1 or settings.effector_weights.all != 1:
        unsupported.append("effector weights")
    if obj.animation_data or obj.parent:
        unsupported.append("moving or parented emitters require baking")
    if unsupported:
        raise Unsupported(
            "Particle recipe: "
            + ", ".join(unsupported)
            + "; select ipp_particles='BAKED'"
        )
    fps = exporter.scene.render.fps / exporter.scene.render.fps_base
    duration = (settings.frame_end - settings.frame_start) / fps
    gravity = (
        BASIS @ exporter.scene.gravity * exporter.unit
        if exporter.scene.use_gravity
        else (0, 0, 0)
    )
    return {
        "source": static_mesh(exporter, obj),
        "shape": 3,
        "space": 1,
        "seed": system.seed,
        "capacity": max(1, settings.count),
        "rate": settings.count / duration if duration > 0 else 0,
        "burst": settings.count if duration <= 0 else 0,
        "duration": max(0, duration),
        "delay": max(0, (settings.frame_start - exporter.scene.frame_start) / fps),
        "lifetime": settings.lifetime / fps,
        "lifetime_random": settings.lifetime_random,
        "speed": settings.normal_factor * exporter.unit,
        "size": settings.particle_size,
        "size_random": settings.size_random,
        "acceleration_x": gravity[0],
        "acceleration_y": gravity[1],
        "acceleration_z": gravity[2],
    }
