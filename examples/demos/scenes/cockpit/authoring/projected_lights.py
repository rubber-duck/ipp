"""Deterministic registration and channel cleanup of the authored emission mask."""

from pathlib import Path

import bpy
import numpy as np

ROOT = Path(__file__).resolve().parent
WIDTH, HEIGHT = 1672, 941
ZONES = [
    ("white-cabin-port", 0, (0, 70, 836, 740), (1.0, 0.96, 0.90), "cabin"),
    ("white-cabin-starboard", 0, (836, 70, 1672, 740), (1.0, 0.96, 0.90), "cabin"),
    ("white-dashboard", 1, (250, 725, 1410, 862), (0.9, 0.97, 1.0), "deck"),
    ("cyan-port-bank", 2, (260, 754, 646, 812), (0.55, 0.80, 1.0), "deck"),
    ("cyan-starboard-bank", 2, (1035, 754, 1403, 812), (0.55, 0.80, 1.0), "deck"),
    ("cyan-dial-left", 2, (495, 810, 630, 888), (0.55, 0.80, 1.0), "dial"),
    ("cyan-dial-centre", 2, (710, 785, 960, 901), (0.55, 0.80, 1.0), "dial"),
    ("cyan-dial-right", 2, (1040, 810, 1180, 888), (0.55, 0.80, 1.0), "dial"),
]
WHITE_DASH_RECTS = [(720, 730, 949, 773), (260, 809, 307, 858), (1369, 808, 1405, 858)]


def linear(pixels):
    return np.where(
        pixels <= 0.04045, pixels / 12.92, ((pixels + 0.055) / 1.055) ** 2.4
    )


def load(name):
    image = bpy.data.images.load(str(ROOT / name), check_existing=False)
    if tuple(image.size) != (WIDTH, HEIGHT):
        image.scale(WIDTH, HEIGHT)
    pixels = np.empty(WIDTH * HEIGHT * 4, np.float32)
    image.pixels.foreach_get(pixels)
    return image, pixels.reshape(HEIGHT, WIDTH, 4)[::-1, :, :3].copy()


def save(name, pixels):
    image = bpy.data.images.new(name, width=WIDTH, height=HEIGHT, alpha=False)
    image.colorspace_settings.name = "sRGB"
    encoded = np.where(
        pixels <= 0.0031308,
        pixels * 12.92,
        1.055 * np.maximum(pixels, 0) ** (1 / 2.4) - 0.055,
    )
    rgba = np.ones((HEIGHT, WIDTH, 4), np.float32)
    rgba[:, :, :3] = encoded[::-1]
    image.pixels.foreach_set(rgba.ravel())
    image.filepath_raw = str(ROOT / name)
    image.file_format = "PNG"
    image.save()
    image.pack()
    image.filepath = "//" + name
    return image


def prepare():
    _, mask = load("cockpit-emission-mask.png")
    _, original = load("cockpit-cleanplate.png")
    _, base = load("cockpit-unlit-all.png")
    # Blender byte Image.pixels exposes encoded source bytes; texture nodes
    # decode them. Subtract radiance in linear space and encode exported PNGs.
    original, base = linear(original), linear(base)
    target = np.max(np.maximum(original - base, 0), axis=2)
    output = np.zeros_like(mask)
    colors = [np.zeros_like(mask) for _ in range(3)]
    delta = np.maximum(original - base, 0)
    reports = []
    parts = []
    for name, channel, rect, _, _ in ZONES:
        for bounds in WHITE_DASH_RECTS if channel == 1 else [rect]:
            parts.append((name, channel, bounds))
    for name, channel, (x0, y0, x1, y1) in parts:
        intensity = np.max(mask[y0:y1, x0:x1, :], axis=2)
        core = np.maximum(intensity - 0.4, 0)
        best = (-1, 0, 0)
        for dy in range(-7, 8):
            for dx in range(-7, 8):
                if x0 + dx < 0 or y0 + dy < 0 or x1 + dx > WIDTH or y1 + dy > HEIGHT:
                    continue
                score = float(
                    np.sum(core * target[y0 + dy : y1 + dy, x0 + dx : x1 + dx])
                )
                if score > best[0]:
                    best = (score, dx, dy)
        _, dx, dy = best
        dest = output[y0 + dy : y1 + dy, x0 + dx : x1 + dx, channel]
        # Saturated white cores are classified by the spatial zone, never copied
        # into multiple channels. Suppress imagegen's oversized colored halos.
        support = np.clip((intensity - 0.035) * 6, 0, 1)
        yy, xx = np.mgrid[0 : y1 - y0, 0 : x1 - x0]
        edge = np.minimum.reduce((xx, yy, x1 - x0 - 1 - xx, y1 - y0 - 1 - yy))
        support *= np.clip(edge / 6, 0, 1)
        contribution = delta[y0 + dy : y1 + dy, x0 + dx : x1 + dx] * support[:, :, None]
        np.maximum(dest, np.max(contribution, axis=2), out=dest)
        color_dest = colors[channel][y0 + dy : y1 + dy, x0 + dx : x1 + dx]
        np.maximum(color_dest, contribution, out=color_dest)
        reports.append(
            {
                "zone": name,
                "source_bounds": (x0, y0, x1, y1),
                "registration_pixels": (dx, dy),
            }
        )
    save("cockpit-emission-zones.png", output)
    emission = [
        save("cockpit-emission-" + name + ".png", color)
        for name, color in zip(("white-cabin", "white-dashboard", "cyan"), colors)
    ]
    return emission, reports


def material(image, channel, tint, name, gain=1):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    mat["ipp_unlit"] = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = image
    links.new(tex.outputs["Color"], nodes.get("Principled BSDF").inputs["Base Color"])
    separate = nodes.new("ShaderNodeSeparateColor")
    links.new(tex.outputs["Color"], separate.inputs["Color"])
    scale = nodes.new("ShaderNodeMath")
    scale.operation = "MULTIPLY"
    scale.name = "Emission gain"
    scale.inputs[1].default_value = gain
    if channel >= 0:
        links.new(separate.outputs[channel], scale.inputs[0])
    else:
        scale.inputs[0].default_value = 1
    emission = nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = (*tint, 1)
    links.new(scale.outputs[0], emission.inputs["Strength"])
    if channel < 0:
        links.new(tex.outputs["Color"], emission.inputs["Color"])
    transparent = nodes.new("ShaderNodeBsdfTransparent")
    add = nodes.new("ShaderNodeAddShader")
    links.new(emission.outputs[0], add.inputs[0])
    links.new(transparent.outputs[0], add.inputs[1])
    links.new(add.outputs[0], nodes.get("Material Output").inputs["Surface"])
    return mat


def compose_preview(mat, images, names=("Cabin", "Dashboard white", "Cyan")):
    """Match runtime's opaque composition without coplanar transparent layers."""
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    emission = next(node for node in nodes if node.type == "EMISSION")
    color = emission.inputs["Color"].links[0].from_socket
    for name, image in zip(names, images):
        texture = nodes.new("ShaderNodeTexImage")
        texture.image = image
        scale = nodes.new("ShaderNodeVectorMath")
        scale.operation = "SCALE"
        scale.name = name + " lights gain"
        scale.inputs["Scale"].default_value = 1
        links.new(texture.outputs["Color"], scale.inputs[0])
        add = nodes.new("ShaderNodeVectorMath")
        add.operation = "ADD"
        links.new(color, add.inputs[0])
        links.new(scale.outputs[0], add.inputs[1])
        color = add.outputs[0]
    links.new(color, emission.inputs["Color"])


def build(api, deck, body_material):
    mask, registration = prepare()
    compose_preview(body_material, mask)
    records = []
    for name, channel, bounds, tint, surface in ZONES:
        x0, y0, x1, y1 = bounds
        # Expand only geometry to retain registered halo pixels; channel values
        # remain exactly zero outside the classified emitter footprints.
        polygon = [
            (max(0, x0 - 10), max(0, y0 - 10)),
            (min(WIDTH, x1 + 10), max(0, y0 - 10)),
            (min(WIDTH, x1 + 10), min(HEIGHT, y1 + 10)),
            (max(0, x0 - 10), min(HEIGHT, y1 + 10)),
        ]
        mat = material(mask[channel], -1, (1, 1, 1), "Light layer " + name)
        depth = 11.98 if surface == "cabin" else lambda x, y: deck(x, y) - 0.04
        obj = api(
            "cockpit-light-" + name,
            polygon,
            depth,
            mat,
            step=20 if surface != "cabin" else 120,
        )
        obj["light_channel"] = -1
        obj["light_category"] = channel
        obj["light_tint"] = (1, 1, 1)
        obj["light_gain"] = 1.0
        obj["additive_layer"] = True
        obj.visible_shadow = False
        obj.hide_render = True
        obj["export_texture_carrier"] = True
        records.append(
            {
                "id": obj.name,
                "channel": -1,
                "category": channel,
                "tint": (1, 1, 1),
                "default_gain": 1.0,
                "texture": mask[channel].name,
            }
        )
    return records, registration
