"""Detached texture, material and world ambient extraction."""

import math

from ..assets import texture_asset
from .types import Unsupported


def texture(exporter, image, obj):
    if image.name in exporter.images:
        return exporter.images[image.name]
    if (
        image.source not in {"FILE", "GENERATED"}
        or image.is_float
        or image.colorspace_settings.name != "sRGB"
    ):
        raise Unsupported("Textures require a single byte sRGB image")
    width, height = image.size
    if min(width, height) < 1:
        raise Unsupported("Texture has no loaded pixels")
    pixels = list(image.pixels)
    channels = image.channels
    if channels not in {3, 4}:
        raise Unsupported("Textures require RGB or RGBA pixels")
    if channels == 4 and any(pixels[i] < 0.999 for i in range(3, len(pixels), 4)):
        raise Unsupported(
            "Texture alpha is not supported by the opaque runtime material"
        )
    rgba = bytearray()
    for y in range(height - 1, -1, -1):
        for x in range(width):
            offset = (y * width + x) * channels
            rgba.extend(
                round(max(0, min(1, value)) * 255)
                for value in pixels[offset : offset + 3]
            )
            rgba.append(255)
    source = exporter.asset(texture_asset, width, height, bytes(rgba), priority=1)
    exporter.images[image.name] = source
    return source


def material(exporter, material, obj, has_uv):
    if material is None:
        return {"material": {"type": "unlit", "r": 0.8, "g": 0.8, "b": 0.8}}
    color = material.diffuse_color
    metallic, roughness = material.metallic, material.roughness
    image = None
    if material.use_nodes:
        outputs = [
            n
            for n in material.node_tree.nodes
            if n.type == "OUTPUT_MATERIAL" and n.is_active_output
        ]
        if len(outputs) != 1 or len(outputs[0].inputs["Surface"].links) != 1:
            raise Unsupported("Material requires one connected surface shader")
        shader = outputs[0].inputs["Surface"].links[0].from_node
        if shader.type != "BSDF_PRINCIPLED":
            raise Unsupported(
                "Only Principled base color/factors or ipp_unlit are supported"
            )
        for socket_name in ("Metallic", "Roughness", "Normal", "Alpha"):
            if shader.inputs[socket_name].is_linked:
                raise Unsupported(f"Material {socket_name} connections are unsupported")
        for socket_name in (
            "Transmission Weight",
            "Subsurface Weight",
            "Coat Weight",
            "Sheen Weight",
        ):
            socket = shader.inputs[socket_name]
            if socket.is_linked or socket.default_value > 1e-6:
                raise Unsupported(f"Material {socket_name} is unsupported")
        emission = shader.inputs["Emission Color"]
        strength = shader.inputs["Emission Strength"]
        if (
            emission.is_linked
            or strength.is_linked
            or (strength.default_value > 0 and max(emission.default_value[:3]) > 1e-6)
        ):
            raise Unsupported("Emissive materials are unsupported")
        color = shader.inputs["Base Color"].default_value
        metallic, roughness = (
            shader.inputs["Metallic"].default_value,
            shader.inputs["Roughness"].default_value,
        )
        if shader.inputs["Alpha"].default_value < 0.999:
            raise Unsupported("Transparent materials are unsupported")
        links = shader.inputs["Base Color"].links
        if links:
            node = links[0].from_node
            if (
                node.type != "TEX_IMAGE"
                or node.image is None
                or node.inputs["Vector"].is_linked
                or not has_uv
            ):
                raise Unsupported(
                    "Base-color textures require the active UV layer without mapping nodes"
                )
            image = node.image
            color = (1, 1, 1, 1)
    if color[3] < 0.999:
        raise Unsupported("Transparent material factors are unsupported")
    unlit = bool(material.get("ipp_unlit", False))
    result = {
        "material": {
            "type": "unlit" if unlit else "pbr",
            "r": color[0],
            "g": color[1],
            "b": color[2],
        }
    }
    if not unlit:
        result["material"].update(
            metallic=metallic,
            roughness=roughness,
            cast_shadows=bool(material.get("ipp_cast_shadows", True)),
            receive_shadows=bool(material.get("ipp_receive_shadows", True)),
        )
    if image is not None:
        result["texture"] = {"source": texture(exporter, image, obj)}
    return result


def ambient_light(exporter):
    world = exporter.scene.world
    if world is None:
        return [0.0, 0.0, 0.0]
    if not world.use_nodes:
        color, strength = world.color, 1.0
    else:
        outputs = [
            node
            for node in world.node_tree.nodes
            if node.type == "OUTPUT_WORLD" and node.is_active_output
        ]
        if len(outputs) != 1 or not outputs[0].inputs["Surface"].is_linked:
            raise Unsupported(
                "World ambient lighting requires an active constant Background output"
            )
        if outputs[0].inputs["Volume"].is_linked:
            raise Unsupported("World volume shaders are unsupported")
        background = outputs[0].inputs["Surface"].links[0].from_node
        if (
            background.type != "BACKGROUND"
            or background.inputs["Color"].is_linked
            or background.inputs["Strength"].is_linked
        ):
            raise Unsupported(
                "World ambient lighting supports only constant Background color and strength"
            )
        color = background.inputs["Color"].default_value
        strength = background.inputs["Strength"].default_value
    ambient = [float(channel) * float(strength) for channel in color[:3]]
    if any(
        not math.isfinite(channel) or channel < 0 or channel > 3.4028234663852886e38
        for channel in ambient
    ):
        raise Unsupported(
            "World ambient lighting must be finite nonnegative RGB representable as float32"
        )
    return ambient
