"""Camera and light projection/parameter extraction."""

import math

from . import identity
from .types import Unsupported


def camera(exporter, obj, depsgraph):
    camera = obj.data
    if camera.type not in {"PERSP", "ORTHO"} or camera.shift_x or camera.shift_y:
        raise Unsupported(
            "Camera requires symmetric perspective or orthographic projection"
        )
    render = exporter.scene.render
    projection = obj.calc_matrix_camera(
        depsgraph,
        x=render.resolution_x,
        y=render.resolution_y,
        scale_x=render.pixel_aspect_x,
        scale_y=render.pixel_aspect_y,
    )
    result = identity.base(exporter, obj)
    result["camera"] = {
        "projection": 0 if camera.type == "PERSP" else 1,
        "fov_y": 2 * math.atan(1 / projection[1][1]),
        "near": camera.clip_start * exporter.unit,
        "far": camera.clip_end * exporter.unit,
        "ortho_height": 2 / projection[1][1] * exporter.unit,
        "focus_distance": max(0.001, camera.dof.focus_distance * exporter.unit),
    }
    return result


def light(exporter, obj):
    light = obj.data
    if light.type not in {"SUN", "POINT", "SPOT"}:
        raise Unsupported("Area lights require an explicit replacement or bake")
    result = identity.base(exporter, obj)
    radius = float(light.get("ipp_range", 20.0))
    if not math.isfinite(radius) or radius <= 0:
        raise Unsupported("Light ipp_range must be finite and positive")
    outer = (
        min(math.pi / 2 - 0.001, light.spot_size / 2)
        if light.type == "SPOT"
        else math.pi / 4
    )
    intensity = light.get("ipp_intensity")
    if intensity is None:
        intensity = (
            light.energy if light.type == "SUN" else light.energy / (4 * math.pi)
        )
        exporter.diagnostic(
            "light-units",
            "Blender energy approximated as runtime intensity; set light-data ipp_intensity for an explicit value",
            obj,
        )
    if not math.isfinite(intensity) or intensity < 0:
        raise Unsupported("Light intensity must be finite and nonnegative")
    shadow_near = float(light.get("ipp_shadow_near", min(0.1, radius / 10)))
    shadow_bias = float(light.get("ipp_shadow_bias", 0.001))
    if not math.isfinite(shadow_near) or not 0 < shadow_near < radius:
        raise Unsupported("Light ipp_shadow_near must be positive and below ipp_range")
    if not math.isfinite(shadow_bias) or shadow_bias < 0:
        raise Unsupported("Light ipp_shadow_bias must be finite and nonnegative")
    result["light"] = {
        "kind": {"SUN": 0, "POINT": 1, "SPOT": 2}[light.type],
        "r": light.color[0],
        "g": light.color[1],
        "b": light.color[2],
        "intensity": intensity,
        "range": radius,
        "outer_cone": outer,
        "inner_cone": min(outer - 0.0001, outer * (1 - light.spot_blend))
        if light.type == "SPOT"
        else 0,
        "cast_shadows": light.type == "SPOT" and light.use_shadow,
        "shadow_near": shadow_near,
        "shadow_bias": shadow_bias,
        "shadow_radius": light.shadow_soft_size if light.type == "SPOT" else 0.0,
    }
    return result
