# World gallery

The gallery shares scene definitions between the browser shell and persistent native development sessions. Both use matching generated clients and the same React declarations, datasets, assets and controllers; the Host owns animation, particle simulation and runtime camera navigation. The chart demo additionally authors orientation for turning in place. Browser inspectors and physical input remain in the DOM shell.

Run from the repository root and open the printed URL:

```sh
python tools/ipp.py dev gallery
```

Use `python tools/ipp.py dev gallery --build` to build without serving.

For persistent offscreen GLES sessions, start the [shared development Host](../../docs/development/shared-host.md) with gallery assets and select a scene at startup:

```sh
node tools/shared-host/shared-host.mjs gallery charts --name gallery
```

The [native gallery guide](../../tools/shared-host/README.md) describes option changes, actions, inspection, completed-frame PNG capture and reload. Scene reload preserves high-level options and recreates handles; a failed rebuild leaves the active scene usable. Rust or generated-contract changes require restarting the Host.

## Reading the examples

Start with each world file; neighboring controls implement its HTML inspector.

| Example | Entry point | Demonstrates |
| --- | --- | --- |
| Geometry | [world.tsx](worlds/geometry/world.tsx) | Built-in resources, transforms, materials, textures and React-managed lifetime |
| Lighting, Picking & Animation | [world.tsx](worlds/lighting/world.tsx) | PBR lighting, picking/dragging, separate culling bounds and skeletal playback |
| Platformer | [world.tsx](worlds/platformer/world.tsx) | Blender-authored KayKit course, skinned gait transitions, reversible route playback and a following camera |
| Particles | [world.tsx](worlds/particles/world.tsx) | Live emission, sprite/mesh presentation, appearance changes and lifetime drain |
| GUI Demo | [scene.tsx](worlds/gui/scene.tsx) | A signal-station dashboard in an attached Canvas World on the runtime's default skin and every GUI kit component: panels, tabs, window controls, a data grid, a scene tree, nested scrolling over a virtual event log, value and selection controls, a colour picker, a canvas paint, feedback components, a tooltip, a popover, a context menu and a confirmation dialog, in-place re-theming, an exploded view of the overlay planes and an input shield on a Surface |
| Charts | [scene.ts](worlds/charts/scene.ts) | Animated inward-curved Canvas lines, bars and pies beside grid bars, a height surface, points and variable 3D pie slices in one camera scene, with Canvas charts on cylindrical segments of the arrangement ring; free camera navigation, two-second focus and source-row hover/selection |

The chart controls default to fixed samples and optionally switch all exhibits to a client-fed synthetic stream. Straight and smooth lines share one producer while requesting different count or elapsed-time windows; points retain recent marks, while bars, grids, surfaces and pies replace complete snapshots. Stream ingestion is bounded and waits for acknowledged writes. Pause stream holds the latest-data window independently of Pause animation, and expired rows clear hover and selection. Source switches preserve the camera and chart Worlds; leaving the scene drains ingestion and releases its producers. The [stream generator](worlds/charts/streaming.ts) owns sample recipes and window choices.

[scene-registry.ts](scene-registry.ts) exposes the same definitions to both runners. Each mount names its output, readiness, high-level options and actions and explicitly releases its roots, subscriptions and child Worlds. The runner owns the primary World and platform presentation. [main.tsx](main.tsx) binds these mounts to `IppCanvas` and retains the existing HTML inspectors; authored scenes share the primary session across navigation, while Charts and the saved Platformer scene use fresh sessions to select their required Systems. [gallery-controller.ts](gallery-controller.ts) coordinates scene teardown, presentation and camera controls.

The compact toolbar opens a searchable scene picker backed by [scene-catalog.ts](scene-catalog.ts). On wide screens, the current scene controls remain in a collapsible dock. On smaller screens, the Controls button below the canvas opens those same controls in a scrollable modal sheet, preserving their state as the layout changes.

[ReadyGeometry](shared/ready-geometry.tsx) retains displayed resources while replacements load. The [geometry catalog](shared/geometry-catalog.ts) owns recipes and control ranges, and [camera controls](shared/camera-controls.ts) translate pointer gestures into Host commands.

The [GUI Demo page](worlds/gui/README.md) declares its dashboard as ordinary Canvas and GUI entities, drawn by the runtime's default looks and the `@ipp/react/gui-kit` compositions, in an attached World that the gallery presents on a Surface, and converts its two waveform curve drawings during the gallery build. `IppCanvas` routes physical input through the runtime against the current camera. Authoritative routing keeps gestures that start on the panel in GUI, while pointer and wheel input that misses every panel drives the ordinary gallery camera controls; each gesture keeps its initial owner across panel boundaries. Its event log is a VirtualList over the whole history that declares only the entries the runtime asks for. Its open overlays lift off the whole panels along the Surface normal in an exploded view, an input shield of scene geometry stands in front of one control, and its panel presentation control compares distance-based whole-Surface caching with direct rendering.

The [Platformer page](worlds/platformer/README.md), also available through `#platformer`, loads a saved KayKit course and mannequin. Walk, Run and Crawl blend between exported skeletal clips while JavaScript declares a fixed route for Host animation playback. Reverse preserves the current position, turns the rig smoothly and keeps the gait playing forward; the side camera, overhead point light and twinkling orb follow the route root without flipping. The [packed Blender scene](worlds/platformer/authoring/platformer.blend), [builder](worlds/platformer/authoring/build_platformer.py) and [source provenance](worlds/platformer/authoring/PROVENANCE.md) support editing and regeneration. `python tools/ipp.py test gallery-platformer` checks the real scene, controls and rendered frames.

## Lighting, Picking & Animation

The lighting example's [animation controller](worlds/lighting/animation-controller.tsx) and [interaction logic](worlds/lighting/interaction.ts) connect playback, selection and dragging without writing evaluated poses from JavaScript.

The particle page demonstrates native simulation without JavaScript particle motion. Its decorative base supplies no collisions. See the [particle guide](../../crates/ipp-core/src/world/systems/particles/README.md).

### Rebuilding the saved scene

The gallery build generates its Platformer World and immutable assets under ignored `target/gallery-platformer-assets/` from the packed Blender source; upstream KayKit downloads are unnecessary for ordinary builds. Its [builder](worlds/platformer/authoring/build_platformer.py) and [source provenance](worlds/platformer/authoring/PROVENANCE.md) describe how to rebuild the Blender scene and route with the maintained tools.

## Validation and inspection

The maintained browser harness uses the actual DOM, generated client, worker, resources and renderer. It waits for acknowledged work and completed frames, then checks state and pixels. `window.ippWorldCanvas` exposes the live canvas and `window.ippGalleryScene` exposes the shared scene actions and inspection; [observation helpers](../../tests/render/viewer-observation.ts) belong to the tests.

Run `python tools/ipp.py test render` for gallery rendering and interaction, `python tools/ipp.py test animation` for playback, `python tools/ipp.py test gallery-platformer` for the saved course and gait controls, `python tools/ipp.py test gallery-particles` for the fountain, or `python tools/ipp.py test gallery-gui` for GUI control, overlay, re-theming, layer, scrolling and lifecycle behavior. The [suite registry](../../tools/pipeline/suites.json) and [render harness guide](../../docs/development/rendering.md) own exact coverage and environment setup.

For an independent comparison of a disk export with Blender's particle simulation, run:

```sh
blender -b SCENE.blend --python-exit-code 1 \
  --python tests/blender/particle_bake_check.py -- EXPORT_DIRECTORY
```

The Charts scene places ten inward-facing exhibits around a central camera, with generous gaps in a horizontal ring. Its five Canvas charts use existing subworlds presented through inward-facing CylinderSurface anchors at the same radius as the arrangement ring; panel widths are arc lengths and every horizontal point stays on that cylinder. Its five spatial charts are ordinary Plot geometry in the root World. At the center, drag to turn in place; after focusing a chart, drag to orbit it. Return to center restores exploration from the middle, while View whole ring gives an elevated view of the collection. Middle-drag pans and scrolling moves closer. Focus controls animate the current camera over two Host-clock seconds; a new focus or manual navigation interrupts that motion. Chart animations play by default and can pause independently of the camera. Hover feedback is transient, while selection keeps the exact series and source-row identity until an empty click or Clear selection. The previous standalone 2D and 3D content modules remain independent real-source fixtures for maintained Plot scenarios.
