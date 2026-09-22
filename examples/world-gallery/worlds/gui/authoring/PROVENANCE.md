# Surface projector authoring

The projector, lit base and tiled floor are original procedural Blender geometry and materials made for this example. The user-approved holographic projector reference informed the silhouette, metallic finish, recessed port and cyan lighting; no reference image pixels or external models are included. The side cassette deliberately uses restrained detail while retaining the beveled housing and layered optical port.

[build_projector.py](build_projector.py) is the reproducible source. [projector.blend](projector.blend) retains the generated meshes, packed textures, procedural material datablocks, lighting and preview camera. `target/projector-authoring/projector-preview.png` demonstrates the authored material and baked base, and is not evidence of IPP rendering. [compose_projector.py](compose_projector.py) applies the manifest's exact camera and transforms to produce `target/projector-authoring/projector-preview.blend` and `target/projector-authoring/projector-scene-preview.png`. Its sparse panel and translucent beam are composition placeholders. React owns the actual Surface controls, live lens and transparent projection shader.

Ordinary gallery builds export the packed scene without rebaking. Meshes, textures and `projector.json` are generated under ignored `target/gallery-gui-assets/projector/` alongside drawings converted from every original SVG source in `svg/`, currently the three waveform curves. Generate that product directly with `python tools/ipp.py build gallery-gui-assets`.

## Regeneration

Use the repository's [Blender environment](../../../../../docs/development/blender.md), currently Blender 5.2.1. From the repository root:

```sh
blender --background --factory-startup --python-exit-code 1 \
  --python examples/world-gallery/worlds/gui/authoring/build_projector.py

blender --background examples/world-gallery/worlds/gui/authoring/projector.blend \
  --python-exit-code 1 \
  --python examples/world-gallery/worlds/gui/authoring/build_projector.py \
  -- --verify-export

blender --background --python-exit-code 1 \
  --python examples/world-gallery/worlds/gui/authoring/compose_projector.py
```

On the development VM, `/home/dev/.local/bin/blender-xvfb` can replace `blender`. The first command builds geometry, bakes material color and static illumination, exports through the maintained [Blender exporter](../../../../../integrations/blender/ipp_blender/EXPORTER.md), and saves the blend and preview. The second reopens that blend and compares every exported mesh and texture byte with the generated assets. The third renders the complete composition from the manifest and saves its separate Blender scene. It does not rebuild or rebake. The script verifies finite unit normals, UV ranges, identity transforms, payload lengths, format headers and the actual boolean openings with geometry ray queries. The generated `projector.json` manifest records authoritative bounds, counts, material hints and SHA-256 identities.

Raw bake PNGs and composition previews are generated under ignored `target/projector-authoring/` for inspection. The editable `projector.blend` retains packed textures and stays in source control. The base is baked at 1024 pixels and resampled to a packed 512-pixel export; metal uses 1024 pixels. The separate floor bake uses 1024 pixels. These three ordinary opaque IPPT textures occupy approximately 9 MiB together. The base has its own UV atlas so surrounding tiles do not reduce the texel density of its recessed lights and chamfers. Broad planes use exact flat normals; the retained bevel geometry and subtle directional color variation define the machined finish. Only the generated `target/gallery-gui-assets/projector/` assets belong in the gallery's served asset build; authoring sources and previews are not runtime downloads.

## Geometry and placement

All mesh origins are identity transforms. The script models runtime coordinates explicitly and converts to Blender `(x, -z, y)` before the existing exporter converts back. The housing, trim, dark aperture and lens remain inside the centered unit cube. The emitting face is local +Z, with the optical disk at Z = 0.435 and luminous port seal at Z = 0.487. Shell, trim and aperture are separate for their material treatment; the lens includes the small side status slits and remains a live runtime material.

The authored frustum is an open rounded-rectangle loft with 56 triangles. It already accounts for the scene's 2.2 body scale: its near cross-section is at Z = 1.0714 with half-width/height 0.242, and its far cross-section is at Z = 5.1 with half-width 2.569 and half-height 1.659. The far edge follows the original authored panel outline, including its quadratic Bézier corners with 0.119-metre horizontal and vertical spans, sampled with six segments per corner. The frustum retains the sampled quadratic corner profile of the authored projector; the GUI now uses analytic rounded-box corners with the same outer dimensions. The near corner spans scale by the corresponding half-size ratios, preserving the same normalized contour at both ends for straight shader rays. The near profile fits within the scaled optical disk. Apply the common projector rotation/translation to the frustum at scale 1; apply that rotation/translation to the body at scale 2.2. The target Surface is 7.4 × 4.8 at scale 0.70, centered at the frustum's far plane with the same rotation; the GUI frame begins 0.021 metres inside the Surface bounds. UV U follows distance around the far perimeter, with corresponding values at the near perimeter. UV V is 0 at the port and 1 at the Surface after exporter V conversion. No cap obscures the lens or panel. Changing these scene dimensions or the GUI frame profile requires regenerating this authored geometry.

The manifest also owns the accepted camera and scene placement. The projector is centered at (0, 0.3, 0), with yaw 0.30 radians and upward pitch 0.04 radians. The base and floor stay horizontal: scale 1.55, yaw 0.30, no pitch. The perspective camera looks from (-8.2, 3.2, 18.2) toward (0.5, -0.03, 2.3), with a 21° vertical field of view. This longer view and projection distance keep the cube fully visible beside the panel.

The separate static base is a beveled plinth; the surrounding floor has 90 subdued graphite tiles across a 6.6 × 7.3-metre local footprint. Neither includes decorative beacons or their light sources. The base's local top is Y = -0.73, and tile tops are Y = -0.90. Eight boolean slots expose a continuous square lightbar inside the opaque shell, all centered at Y = -0.841. The ring has no exterior attached strip geometry. Static lighting, recessed-ring illumination, floor spill and the base's fixed shadow are baked with the base and floor together, into separate UV atlases. Soft ambient fill lifts the darker graphite surfaces while the fixed key lights and recessed cyan illumination retain their contrast. The floating projector, projection frustum and interactive panel are excluded from both bakes so their changing state does not leave baked shadows. Render `base.ippm` with `base-baked.ippt` and `floor.ippm` with `floor-baked.ippt`, using ordinary unlit materials; do not light the baked illumination a second time. These bakes assume the documented shared base/floor transform and fixed studio lighting, and do not respond to runtime gain. Gain affects the runtime projector; SCAN scrolls the GUI sine wave, and PULSE adds an independent graph trace.
