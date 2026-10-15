"""Export supported opaque textures, meshes and the cockpit runtime metadata.

blender -b cockpit-projected.blend --python export_projected.py -- OUTPUT_DIRECTORY
"""

import json
import runpy
import sys
from pathlib import Path

import bpy

AUTHORING = Path(__file__).resolve().parent
REPOSITORY = AUTHORING.parents[4]


def export():
    scene = bpy.data.scenes.get("Cockpit projected presentation")
    if scene is None:
        raise RuntimeError(
            "Run project_cockpit.py before exporting the projected cockpit"
        )
    bpy.context.window.scene = scene
    output = Path(sys.argv[sys.argv.index("--") + 1])
    anchors = json.loads((AUTHORING / "cockpit-projected-anchors.json").read_text())
    lights = runpy.run_path(str(AUTHORING / "projected_lights.py"))
    carriers = []
    bodies = []
    exterior_carriers = []
    exterior_bodies = []
    material_copies = {}
    for obj in scene.objects:
        if obj.get("cockpit_preview_only"):
            obj.hide_render = True
            continue
        if obj.type != "MESH":
            continue
        if obj.get("export_texture_carrier"):
            # Actual supported inputs publish images through the standard path.
            # Runtime hides these meshes and samples their images in body shaders.
            obj.hide_render = False
            (exterior_carriers if obj.get("exterior_layer") else carriers).append(
                obj.name
            )
        elif not obj.get("additive_layer"):
            (exterior_bodies if obj.get("exterior_body") else bodies).append(obj.name)
        for slot in obj.material_slots:
            original = slot.material
            if original.name not in material_copies:
                bsdf = original.node_tree.nodes.get("Principled BSDF")
                image = bsdf.inputs["Base Color"].links[0].from_node.image
                if image.colorspace_settings.name != "sRGB" or image.is_float:
                    raise ValueError(f"Unsupported export image: {image.name}")
                material = bpy.data.materials.new(original.name + " export")
                material.use_nodes = True
                material["ipp_unlit"] = True
                nodes, links = material.node_tree.nodes, material.node_tree.links
                texture = nodes.new("ShaderNodeTexImage")
                texture.image = image
                links.new(
                    texture.outputs["Color"],
                    nodes.get("Principled BSDF").inputs["Base Color"],
                )
                material_copies[original.name] = material
            slot.material = material_copies[original.name]
    runpy.run_path(
        str(REPOSITORY / "integrations/blender/export_scene.py"), run_name="__main__"
    )
    snapshot = json.loads((output / "scene.json").read_text())["scene"]
    if snapshot["diagnostics"]:
        raise RuntimeError(f"Cockpit export diagnostics: {snapshot['diagnostics']}")
    entities = {entity["id"]: entity for entity in snapshot["entities"]}
    textures = {}
    for record in anchors["lights"]:
        if "category" in record:
            category = ("cabin", "dashboard", "cyan")[record["category"]]
            textures[category] = entities[record["id"]]["texture"]["source"]
    zones = []
    for name, category, (x0, y0, x1, y1), _, surface in lights["ZONES"]:
        zones.append(
            {
                "name": name,
                "rect": [
                    x0 / lights["WIDTH"],
                    y0 / lights["HEIGHT"],
                    x1 / lights["WIDTH"],
                    y1 / lights["HEIGHT"],
                ],
                "group": "cabin"
                if category == 0
                else "dashboard"
                if category == 1
                else "dials"
                if surface == "dial"
                else "buttons",
                "texture": ("cabin", "dashboard", "cyan")[category],
            }
        )
    metadata = {
        "reference": {"width": lights["WIDTH"], "height": lights["HEIGHT"]},
        "camera": {"entity": "cockpit-camera", "locked": True},
        "surfaces": anchors["surfaces"],
        "lighting": {
            "uvOrigin": "top-left",
            "encoding": "sRGB-encoded positive linear RGB emission",
            "bodies": [
                {"entity": name, "baseTexture": entities[name]["texture"]["source"]}
                for name in bodies
            ],
            "textures": textures,
            "zones": zones,
            "carrierEntityNames": carriers,
        },
        "exterior": {
            "bodies": [
                {"entity": name, "baseTexture": entities[name]["texture"]["source"]}
                for name in exterior_bodies
            ],
            "textures": {
                record["layer"]: entities[record["id"]]["texture"]["source"]
                for record in anchors["exterior"]["layers"]
            },
            "carrierEntityNames": exterior_carriers,
            "uvOrigin": "top-left",
            "encoding": anchors["exterior"]["encoding"],
            "defaultGains": {"cities": 1.0, "atmosphere": 1.0, "stars": 1.0},
        },
    }
    (output / "cockpit-runtime.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(
        json.dumps(
            {
                "cockpitMetadata": str(output / "cockpit-runtime.json"),
                "bodyMeshes": len(bodies),
                "carrierMeshes": len(carriers),
            }
        )
    )


if __name__ == "__main__":
    export()
