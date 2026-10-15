# Cockpit authoring

[cockpit-projected.blend](cockpit-projected.blend) is the active scene. [project_cockpit.py](project_cockpit.py) projects the approved [reference](cockpit-reference.png) onto shallow cabin, dashboard, three dial, planet and sky meshes. Its fixed Camera is part of the composition; orbiting exposes the intentional shallow geometry. Five oriented empties locate actual native GUI surfaces. Preview outlines and labels are authoring aids and never export.

The original reference and image-generation prompts remain beside the blend. `cockpit-cleanplate.png` removes pictured GUI, and `cockpit-unlit-all.png` removes artificial cabin and dashboard illumination while preserving ambient light and space. [projected_lights.py](projected_lights.py) registers the generated emission mask to those images, classifies its saturated cores spatially, and produces three sRGB RGB emission atlases. The preview adds those light layers to the opaque base; separate gains control cabin white, dashboard white and cyan lamps. Runtime uses the same textures with Host-driven per-zone gains. Source images remain unchanged.

The exterior-only generated plate and unlit/mask variants keep the same camera coordinates. [projected_exterior.py](projected_exterior.py) separates positive linear radiance into city lights, atmosphere and stars using the generated mask’s smooth limb boundary. Faint source stars remain included even where the generated mask omits them. Traced mesh apertures expose this exterior through the main and side windows; the optional foreground alpha source is retained for provenance, without using its ragged edge. All working images normalize to the original 1672 × 941 extent; generated source files remain unchanged.

From the repository root, with the pinned Blender installation:

```sh
blender -b --factory-startup --python-exit-code 1 --python examples/demos/scenes/cockpit/authoring/project_cockpit.py
blender -b examples/demos/scenes/cockpit/authoring/cockpit-projected.blend --python-exit-code 1 --python examples/demos/scenes/cockpit/authoring/export_projected.py -- target/cockpit-projected-export
```

The builder also runs through Blender MCP with `runpy.run_path(...)["build"]()`. Preserve unrelated unsaved scenes first. No new lighting bake is needed for this path. Regenerate after changing image sources, contours or anchors.

[cockpit-projected-anchors.json](cockpit-projected-anchors.json) records authored surface dimensions, Blender matrices and mask registration. Adopt the standard export's converted transforms: surface local X/Y spans the GUI plane and +Z faces the reader. [export_projected.py](export_projected.py) delegates the standard exporter using supported opaque unlit image materials, then writes `cockpit-runtime.json` beside `scene.json`. That metadata identifies exact exported texture sources, body meshes, independent UV light zones, carrier meshes to hide, and the separate exterior bodies. Runtime UVs have a top-left origin. Texture sampling already decodes sRGB; add the RGB layers in linear space. Carrier meshes publish genuine texture assets and are hidden after runtime material adoption. The exterior has its own unlit base and three original-color radiance textures, so no lit sky remains behind a disabled layer.

The earlier Boolean model in [cockpit.blend](cockpit.blend) and its build/bake scripts are preserved as an alternate editable study. They are not the active runtime source. Their sphere-atmosphere shader and old anchor dimensions do not apply to the projected scene.
