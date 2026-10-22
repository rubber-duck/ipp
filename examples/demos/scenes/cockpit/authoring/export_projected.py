"""Export scene meshes, RGBA material inputs and cockpit runtime metadata.

blender -b cockpit-projected.blend --python export_projected.py -- OUTPUT_DIRECTORY
"""

import json
import sys
from pathlib import Path

import bpy
import numpy as np

AUTHORING = Path(__file__).resolve().parent
REPOSITORY = AUTHORING.parents[4]


def export():
    scene = bpy.data.scenes.get("Cockpit spatial bridge")
    if scene is None:
        raise RuntimeError(
            "Run project_cockpit.py before exporting the projected cockpit"
        )
    bpy.context.window.scene = scene
    output = Path(sys.argv[sys.argv.index("--") + 1])
    anchors = json.loads((AUTHORING / "cockpit-projected-anchors.json").read_text())
    carriers = []
    bodies = []
    exterior_carriers = []
    exterior_bodies = []
    material_copies = {}
    apertures = {}
    cutouts = set()
    lighting_keys = {}
    tile_catalog = json.loads((AUTHORING / "lighting-tiles.json").read_text())
    planetary_layers = {}
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
        elif obj.get("environment_role"):
            obj.hide_render = False
            exterior_bodies.append(obj.name)
        elif not obj.get("additive_layer"):
            bodies.append(obj.name)
        if obj.get("lighting_key"):
            lighting_keys[obj.name] = obj["lighting_key"]
        for slot in obj.material_slots:
            original = slot.material
            if original.name not in material_copies:
                bsdf = original.node_tree.nodes.get("Principled BSDF")
                image = bsdf.inputs["Base Color"].links[0].from_node.image
                if image.colorspace_settings.name != "sRGB" or image.is_float:
                    raise ValueError(f"Unsupported export image: {image.name}")
                if obj.get("cloud_map"):
                    width, height = image.size
                    pixels = np.empty(width * height * 4, np.float32)
                    image.pixels.foreach_get(pixels)
                    clouds = bpy.data.images.load(
                        str(AUTHORING / obj["cloud_map"]), check_existing=True
                    )
                    if tuple(clouds.size) != (width, height):
                        raise ValueError("Earth surface and cloud extents must match")
                    coverage = np.empty(width * height * 4, np.float32)
                    clouds.pixels.foreach_get(coverage)
                    pixels.reshape(-1, 4)[:, 3] = coverage.reshape(-1, 4)[:, 0]
                    apertures[obj.name] = (width, height, pixels)
                    planetary_layers[obj.name] = [width, height]
                    image = bpy.data.images.new("Earth export placeholder", 1, 1)
                    image.pixels[:] = (0.1, 0.1, 0.1, 1)
                elif obj.get("environment_role") == "atmosphere":
                    # The analytic atmosphere never samples its export carrier.
                    image = bpy.data.images.new("Atmosphere placeholder", 1, 1)
                    image.pixels[:] = (0.1, 0.1, 0.1, 1)
                elif obj.get("environment_role") == "bridge" or obj.get("alpha_cutout"):
                    # The standard Blender material is opaque. Preserve RGBA for
                    # the app-owned cutout shader in a separate texture input.
                    width, height = image.size
                    pixels = np.empty(width * height * 4, np.float32)
                    image.pixels.foreach_get(pixels)
                    apertures[obj.name] = (width, height, pixels.copy())
                    cutouts.add(obj.name)
                    image = bpy.data.images.new("Opaque export placeholder", 1, 1)
                    image.pixels[:] = (0.1, 0.1, 0.1, 1)
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
    sys.path.insert(0, str(REPOSITORY / "integrations/blender"))
    from ipp_blender.exporter import export_scene
    from ipp_blender.asset_store import AssetStore
    from ipp_blender.assets import texture_asset

    output.mkdir(parents=True, exist_ok=True)
    store = AssetStore(output / "assets")
    snapshot = export_scene(store.publish)
    unsupported = [d for d in snapshot["diagnostics"] if d["code"] != "modifiers-baked"]
    if unsupported:
        raise RuntimeError(f"Cockpit export diagnostics: {unsupported}")
    entities = {entity["id"]: entity for entity in snapshot["entities"]}

    def publish_pixels(width, height, pixels, cutout=False):
        rgba = np.rint(
            np.clip(pixels.reshape(height, width, 4)[::-1], 0, 1) * 255
        ).astype(np.uint8)
        if cutout:
            # The generated PNGs retain arbitrary RGB in transparent pixels.
            # Publish binary premultiplied coverage for linear GPU filtering;
            # otherwise neighboring quadrants pull that hidden RGB into joins.
            opaque = rgba[:, :, 3] >= 128
            rgba[~opaque] = 0
            rgba[opaque, 3] = 255
        return store.publish(texture_asset(width, height, rgba.tobytes()))

    aperture_sources = {
        name: publish_pixels(width, height, pixels, name in cutouts)
        for name, (width, height, pixels) in apertures.items()
        if name not in lighting_keys
    }
    tile_sources = {}
    for name, key in lighting_keys.items():
        tiles = []
        for tile in tile_catalog[key]:
            sources = {}
            for role in ("base", "contribution"):
                image = bpy.data.images.load(
                    str(AUTHORING / tile[role]), check_existing=False
                )
                if role == "contribution":
                    image.colorspace_settings.name = "Non-Color"
                width, height = image.size
                if width != height or [width, height] != tile["dimensions"]:
                    raise ValueError(
                        f"Lighting texture is not a registered square: {name}"
                    )
                pixels = np.empty(width * height * 4, np.float32)
                image.pixels.foreach_get(pixels)
                if role == "contribution":
                    rgba = pixels.reshape(-1, 4)
                    if not np.allclose(rgba[:, 0], rgba[:, 1]) or not np.allclose(
                        rgba[:, 0], rgba[:, 2]
                    ):
                        raise ValueError(
                            f"Light contribution must be grayscale: {name}"
                        )
                    # Alpha remains linear through the renderer's sRGB sampling.
                    rgba[:, 3] = rgba[:, 0]
                sources[role + "Texture"] = publish_pixels(
                    width, height, pixels, role == "base"
                )
            tiles.append(
                {
                    **sources,
                    "bounds": tile["bounds"],
                    "textureRect": tile["textureRect"],
                    "dimensions": tile["dimensions"],
                    "sourcePixels": tile["sourcePixels"],
                }
            )
        tile_sources[name] = tiles

    (output / "scene.json").write_text(
        json.dumps(
            {
                "type": "snapshot",
                "session": store.session,
                "revision": 1,
                "scene": snapshot,
            },
            indent=2,
        )
        + "\n"
    )
    (output / "catalog.json").write_text(
        json.dumps(
            {
                name: {"bytes": size, "contentType": content_type}
                for name, (_, size, content_type) in store.entries.items()
            },
            indent=2,
        )
        + "\n"
    )
    metadata = {
        "reference": {"width": 1672, "height": 941},
        "camera": {"entity": "cockpit-camera", "locked": True},
        "surfaces": anchors["surfaces"],
        "bridge": {
            "entity": anchors["exterior"]["bridge"],
            "carouselMount": anchors["exterior"]["mount"],
            "paneExtent": anchors["exterior"]["paneExtent"],
            "paneElevation": anchors["exterior"]["paneElevation"],
            "panoramaSplit": anchors["exterior"]["panoramaSplit"],
        },
        "lighting": {
            "uvOrigin": "top-left",
            "encoding": "sRGB unpowered fixtures; grayscale contribution in linear alpha multiplied by shader light RGB",
            "bodies": [
                {
                    "entity": name,
                    "tiles": tile_sources[name],
                }
                for name in bodies
            ],
            "zones": anchors["console"]["zones"],
            "carrierEntityNames": carriers,
        },
        "exterior": {
            "planetMap": {
                "dimensions": planetary_layers[anchors["exterior"]["planet"]],
                "surface": "NASA Blue Marble Next Generation, July 2004",
                "clouds": "NASA Blue Marble, 2002",
                "cities": "NASA Black Marble, 2016",
            },
            "bodies": [
                {
                    "entity": name,
                    "baseTexture": aperture_sources.get(
                        name, entities[name]["texture"]["source"]
                    ),
                    "role": scene.objects[name]["environment_role"],
                    **(
                        {
                            "bounds": list(scene.objects[name]["panorama_bounds"]),
                            "quadrant": scene.objects[name]["panorama_quadrant"],
                            "tiles": tile_sources[name],
                        }
                        if name in tile_sources
                        else {}
                    ),
                }
                for name in exterior_bodies
            ],
            "textures": {
                "cities": entities[anchors["exterior"]["cities"]]["texture"]["source"],
            },
            "carrierEntityNames": exterior_carriers,
            "uvOrigin": "top-left",
            "encoding": "square base/contribution tiles; alpha-cut bridge apertures; linear planet shading",
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
