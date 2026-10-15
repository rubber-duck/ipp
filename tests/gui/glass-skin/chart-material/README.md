# Glass chart materials

The production material now shares the natural GUI glass optics. Use the [native family lab](#native-plot-family-baseline) below for current references. Earlier isolated studies in this directory retain their historical shaders and are not the production style.

## Historical angular study

One 2D bar chart and one 3D grid-bar chart exercise a shared procedural glass-face vocabulary through native CanvasPaint and CustomMaterial. Opaque faces combine blue gradients, corner facets, soft reflections and narrow illuminated rims. Palette and point-source position/intensity are ordinary shader properties. No texture, scene refraction or blur pass is used. The 3D halo uses one small, depth-tested billboard per bar, discarding its interior; GPU performance has not been measured.

This is an isolated visual prototype using the application's sample values. A single fixture mapping places bars, grids, labels and annotation anchors. The 2D sheet uses Canvas primitives. The 3D bars are opaque cube meshes with native depth testing; their Camera output is attached to a Canvas sheet with matching panel and annotation decoration. Category cells provide full bar widths and clearance inside the enclosure. Decorative corner overlays compose above the camera rectangle.

Axes and annotations in this study are authored fixture geometry, not runtime Plot output. The isolated study does not exercise native Plot geometry. Optional production siblings are documented with the [public chart authoring API](../../../../packages/ipp-react/src/gui-glass/README.md#native-charts); the native family lab below exercises those declarations on real Plot output. The mesh shader adapter assumes axis-aligned box faces.

Start a compatible [shared native Host](../../../../docs/development/shared-host.md), then run:

```sh
node tools/shared-host/shared-host.mjs session start tests/gui/glass-skin/chart-material/lab.tsx --name glass-chart-material
node tools/shared-host/shared-host.mjs session capture glass-chart-material
node tools/shared-host/shared-host.mjs session capture glass-chart-material right
node tools/shared-host/shared-host.mjs session capture glass-chart-material violet
node tools/shared-host/shared-host.mjs session capture glass-chart-material contrast
node tools/shared-host/shared-host.mjs session capture glass-chart-material verify
node tools/shared-host/shared-host.mjs session stop glass-chart-material
```

Captures are actual completed native frames at 1480 × 920, with the sheet at 2× device scale. `right` moves the source, `violet` changes the palette, and `contrast` substitutes a bright background behind the opaque bars. Shader edits reload on capture. Output size and World setup changes require restarting the session.

`verify` captures both dimensions under default light, moved light, alternate palette and contrasting background. Background-difference masks measure all six rendered 2D widths, heights, baselines and end margins. Checks also cover 3D footprint containment, tooltip collision, concept-derived color/glow bounds, source response, palette response, opaque interiors and shader fallback. Negative controls reject the original out-of-grid cube and overlapping callout. These are prototype checks on the shared Host, not production chart integration coverage. Independent side-by-side visual review is still needed: the approved concept is not a pixel oracle for native typography or correct data mapping.

## Stylized reflection close-ups

`reflection-lab.tsx` compares the current finish with an opt-in stylized surface on one 2D bar and one 3D column. `reflections.ts` adds a few smooth light bands, fading corner highlights and a painted contact sheen. It does not sample the scene, render a mirror view, trace rays or add a reflection/blur render pass. The 2D bands run in its existing paint; the 3D contact sheen adds one blended floor patch. Performance remains unmeasured.

The material's `profile.z` enables the added finish and `profile.w` controls band softness; existing chart fixtures leave it disabled. Palette and light source parameters remain shared with the original shader. The floor patch is an authored graphic tied to the specimen footprint. Captured Camera pixels and a separate native Canvas caption sheet are composed into the close-up PNG; this isolates the material study from nested-view presentation. The original chart lab still exercises the attached-Camera arrangement.

```sh
node tools/shared-host/shared-host.mjs session start tests/gui/glass-skin/chart-material/reflection-lab.tsx --name glass-reflections
node tools/shared-host/shared-host.mjs session capture glass-reflections verify
node tools/shared-host/shared-host.mjs session capture glass-reflections motion
node tools/shared-host/shared-host.mjs session stop glass-reflections
```

`verify` captures the original finish, added finish, opposite source position, contrasting background and violet palette. Assertions check visible finish/light/palette response, opaque interiors and absence of shader fallback. `motion` captures nine actual parameter positions for a presentation sequence; this is sampled material behavior, not a measurement of animation frame rate or GPU timing.

## Native Plot family baseline

`native-lab.ts` captures the existing application-owned [2D](../../../../examples/demos/scenes/charts2d/content.ts) and [3D](../../../../examples/demos/scenes/charts3d/content.ts) fixtures through their native Plot components, Data Service sources, generated React declarations and completed GLES frames. It includes straight/smooth lines, bars and supplied bins, pie/donut, grid/single-row bars, surface edges/solid faces, cube/octahedron points and fixed or variable radius/height pies. Axis text, annotations and data geometry come from the runtime. Standalone cameras use the maintained scenario's framing so titles remain visible.

```sh
node tools/shared-host/shared-host.mjs run tests/gui/glass-skin/chart-material/native-lab.ts --name chart-native-families
node tools/shared-host/shared-host.mjs run tests/gui/glass-skin/chart-material/native-lab.ts --name chart-native-families-baseline baseline
node tools/shared-host/shared-host.mjs run tests/gui/glass-skin/chart-material/native-lab.ts --name chart-native-families-response verify
```

The default run captures native defaults, optional glass finishes and individual comparisons. `baseline` leaves the default presentation intact; `verify` additionally captures opposite light and alternate sheen hue. Maintained `glass-2d`, `glass-3d` and `glass-lifecycle` cases in the Plot suite assert real pixel responses, backed 2D interiors, transparent 3D interiors, opposing-camera triangle ordering, source identity/picking, data updates and transferred shader failure/recovery. The run closes its roots, sessions, Worlds and dataset producers after capture.

The native contour study exercises wide smooth lines and curved boundaries at three scales:

```sh
node tools/shared-host/shared-host.mjs run tests/gui/glass-skin/chart-material/chart-contour-lab.tsx --host /home/dev/ipp
```

For fast material iteration on native 2D marks, start `chart-detail-lab.tsx` as a shared Host session. Each capture returns a full chart sheet plus lossless enlarged bar and pie crops; pass `right` for the opposing light.
