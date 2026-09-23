# World gallery

The gallery demonstrates React scene authoring through the generated client, worker Host and WebGL renderer. Ordinary React state drives controls; the Host owns animation, particle simulation and camera mathematics.

Run from the repository root and open the printed URL:

```sh
python tools/ipp.py dev gallery
```

Use `python tools/ipp.py dev gallery --build` to build without serving.

## Reading the examples

Start with each world file; neighboring controls implement its HTML inspector.

| Example | Entry point | Demonstrates |
| --- | --- | --- |
| Geometry | [world.tsx](worlds/geometry/world.tsx) | Built-in resources, transforms, materials, textures and React-owned lifetime |
| Lighting, Picking & Animation | [world.tsx](worlds/lighting/world.tsx) | PBR lighting, picking/dragging, separate culling bounds and skeletal playback |
| Platformer | [world.tsx](worlds/platformer/world.tsx) | Blender-authored KayKit course, skinned gait transitions, reversible route playback and a following camera |
| Particles | [world.tsx](worlds/particles/world.tsx) | Live emission, sprite/mesh presentation, appearance changes and lifetime drain |
| GUI Demo | [scene.tsx](worlds/gui/scene.tsx) | React GUI controls, runtime-owned values/focus/scrolling, vector assets and animated skins on a Surface |

[main.tsx](main.tsx) composes worlds and inspectors with `IppCanvas`. Geometry, lighting, particles and the GUI demo share the expanded-render session; changing to or from the saved platformer scene creates a fresh session. Controls retain ordinary React state across navigation. [gallery-controller.ts](gallery-controller.ts) coordinates presentation and camera controls.

The compact toolbar opens a searchable scene picker backed by [scene-catalog.ts](scene-catalog.ts). On wide screens, the current scene controls remain in a collapsible dock. On smaller screens, the Controls button below the canvas opens those same controls in a scrollable modal sheet, preserving their state as the layout changes.

[ReadyGeometry](shared/ready-geometry.tsx) retains displayed resources while replacements load. The [geometry catalog](shared/geometry-catalog.ts) owns recipes and control ranges, and [camera controls](shared/camera-controls.ts) translate pointer gestures into Host commands.

The [GUI Demo page](worlds/gui/README.md) draws controls as retained rounded boxes with font-glyph icons, converts its three waveform curve drawings during the gallery build and mounts a real `@ipp/react/gui` application with `IppCanvas.guiInput`. Core layout and input route controls against the current camera. Authoritative routing keeps gestures that start on the panel in GUI, while pointer and wheel input that misses every panel drives the ordinary gallery camera controls; each gesture keeps its initial owner across panel boundaries. Its panel presentation control compares distance-based whole-Surface caching with direct rendering.

The [Platformer page](worlds/platformer/README.md), also available through `#platformer`, loads a saved KayKit course and mannequin. Walk, Run and Crawl blend between exported skeletal clips while JavaScript declares a fixed route for Host animation playback. Reverse preserves the current position, turns the rig smoothly and keeps the gait playing forward; the side camera, overhead point light and twinkling orb follow the route root without flipping. The [packed Blender scene](worlds/platformer/authoring/platformer.blend), [builder](worlds/platformer/authoring/build_platformer.py) and [source provenance](worlds/platformer/authoring/PROVENANCE.md) support editing and regeneration. `python tools/ipp.py test gallery-platformer` checks the real scene, controls and rendered frames.

## Lighting, Picking & Animation

The lighting example's [animation controller](worlds/lighting/animation-controller.tsx) and [interaction logic](worlds/lighting/interaction.ts) connect playback, selection and dragging without writing evaluated poses from JavaScript.

The particle page demonstrates native simulation without JavaScript particle motion. Its decorative base supplies no collisions. See the [particle guide](../../crates/ipp-core/src/world/systems/particles/README.md).

### Rebuilding the saved scene

The gallery build generates its Platformer World and immutable assets under ignored `target/gallery-platformer-assets/` from the packed Blender source; upstream KayKit downloads are unnecessary for ordinary builds. Its [builder](worlds/platformer/authoring/build_platformer.py) and [source provenance](worlds/platformer/authoring/PROVENANCE.md) describe how to rebuild the Blender scene and route with the maintained tools.

## Validation and inspection

The maintained browser harness uses the actual DOM, generated client, worker, resources and renderer. It waits for acknowledged work and completed frames, then checks state and pixels. `window.ippWorldCanvas` exposes the live canvas for inspection; [observation helpers](../../tests/render/viewer-observation.ts) belong to the tests.

Run `python tools/ipp.py test render` for gallery rendering and interaction, `python tools/ipp.py test animation` for playback, `python tools/ipp.py test gallery-platformer` for the saved course and gait controls, `python tools/ipp.py test gallery-particles` for the fountain, or `python tools/ipp.py test gallery-gui` for GUI control, skin, scrolling and lifecycle behavior. The [suite registry](../../tools/pipeline/suites.json) and [render harness guide](../../docs/development/rendering.md) own exact coverage and environment setup.

For an independent comparison of a disk export with Blender's particle simulation, run:

```sh
blender -b SCENE.blend --python-exit-code 1 \
  --python tests/blender/particle_bake_check.py -- EXPORT_DIRECTORY
```
