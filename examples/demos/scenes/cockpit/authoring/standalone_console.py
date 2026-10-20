"""Complete imagegen console silhouette on a fixed, shallow projected mesh.

Image-space landmarks describe the source object, never the output viewport.
The texture's alpha supplies the rounded endcaps and finished front apron.
"""

from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parent
LEFT, TOP, WIDTH = 64, 587, 1544

# Normalized source-image regions constrain shader light extraction to lamps;
# bright metal and the blank instrument displays must never pulse.
ZONES = [
    ("centre-white", "dashboard", (0.43, 0.29, 0.57, 0.35)),
    ("port-white", "dashboard", (0.18, 0.47, 0.205, 0.56)),
    ("starboard-white", "dashboard", (0.795, 0.47, 0.82, 0.56)),
    ("port-buttons", "buttons", (0.185, 0.35, 0.375, 0.47)),
    ("starboard-buttons", "buttons", (0.625, 0.35, 0.815, 0.47)),
    ("left-dial", "dials", (0.31, 0.47, 0.375, 0.59)),
    ("centre-dial", "dials", (0.435, 0.405, 0.565, 0.615)),
    ("right-dial", "dials", (0.625, 0.47, 0.69, 0.59)),
    ("port-edge", "cabin", (0.0, 0.66, 0.065, 0.76)),
    ("starboard-edge", "cabin", (0.935, 0.66, 1.0, 0.76)),
]


def build(projected_mesh, material, anchor):
    image = bpy.data.images.load(
        str(ROOT / "console-standalone.png"), check_existing=False
    )
    image.pack()
    image.filepath = "//console-standalone.png"
    height = WIDTH * image.size[1] / image.size[0]

    def reference(uv):
        return LEFT + uv[0] * WIDTH, TOP + uv[1] * height

    def depth(x, y):
        return 8.4 - 0.006 * (y - 690) + 0.04 * ((x - 836) / 836) ** 2

    polygon = [reference(uv) for uv in [(0, 0), (1, 0), (1, 1), (0, 1)]]
    mat = material(image)
    obj = projected_mesh("cockpit-console-shell", polygon, depth, mat, step=32)
    obj["alpha_cutout"] = True
    # projected_mesh initially writes the original reference-camera UVs.
    for uv in obj.data.uv_layers.active.data:
        x, y = uv.uv.x * 1672, (1 - uv.uv.y) * 941
        uv.uv = ((x - LEFT) / WIDTH, 1 - (y - TOP) / height)
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    texture = next(node for node in nodes if node.type == "TEX_IMAGE")
    emission = next(node for node in nodes if node.type == "EMISSION")
    transparent = nodes.new("ShaderNodeBsdfTransparent")
    mix = nodes.new("ShaderNodeMixShader")
    links.new(texture.outputs["Alpha"], mix.inputs[0])
    links.new(transparent.outputs[0], mix.inputs[1])
    links.new(emission.outputs[0], mix.inputs[2])
    links.new(mix.outputs[0], nodes.get("Material Output").inputs["Surface"])

    left = [(0.09, 0.49), (0.18, 0.445), (0.173, 0.59), (0.075, 0.63)]
    right = [(1 - x, y) for x, y in [left[1], left[0], left[3], left[2]]]
    for role, corners in [("pulse", left), ("scanner", right)]:
        pixels = [reference(uv) for uv in corners]
        anchor(
            "cockpit-" + role + "-surface",
            pixels,
            [depth(x, y) - 0.04 for x, y in pixels],
            role,
        )
    return {
        "zones": [
            dict(name=name, group=group, rect=rect) for name, group, rect in ZONES
        ],
        "source": image.filepath,
        "referenceBounds": [LEFT, TOP, LEFT + WIDTH, TOP + height],
    }
