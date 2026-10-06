# Chart study captures

This native capture wrapper presents the gallery’s [Charts 2D](../world-gallery/worlds/charts2d/content.ts) and [Charts 3D](../world-gallery/worlds/charts3d/content.ts) content through the runtime's seven Plot components, using the [public React Plot API](../../packages/ipp-react/src/plots/README.md) and real Data Service sources. The canonical content lives in the [shared gallery](../world-gallery/README.md); this wrapper and maintained integration scenarios consume those same samples, column definitions and authored labels. React supplies no chart geometry.

The 2D study shows straight and smooth lines with a missing-data gap, bars, supplied histogram bins and row-labelled pie sectors. A paused Host animation controller targets the smooth series' column parameter, and changed capture seeks its pinned state through the existing controller API. The 3D study includes grid and single-row bars, a triangulated height surface with a hole, disconnected point groups and pie slices with independent share, radius and height. Bars and points share one source with independent binding projections.

Start a compatible [shared native Host](../../docs/development/shared-host.md), then run:

```sh
node tools/shared-host/shared-host.mjs session start examples/chart-showcase/native.ts --name charts
node tools/shared-host/shared-host.mjs session capture charts --out target/chart-study/baseline
node tools/shared-host/shared-host.mjs session capture charts changed --out target/chart-study/changed
node tools/shared-host/shared-host.mjs session capture charts changed rotated --out target/chart-study/rotated
node tools/shared-host/shared-host.mjs session stop charts
```

Captures contain actual GLES output: `charts-2d.png`, `charts-3d.png` and the standalone `single-row.png`. The 3D sheet borrows existing Camera Worlds through SurfaceCamera attachments, clearing their standalone root bindings first. Removing the sheet releases its attachments; closing the example releases roots, sessions, Worlds and sources. Native development screenshots complement the maintained suite; they are not a regression run.

The canonical [2D content](../world-gallery/worlds/charts2d/content.ts) and [3D content](../world-gallery/worlds/charts3d/content.ts) expose `openPlot2d` and `openPlot3d`, which accept a connected Host, its generated module, font bytes and a unique name, independently of process launch. Their ready/change/close operations let native and browser drivers share scenario behavior. The [React scenario](../../tests/data/scenarios/react-plots.ts) exercises producer declarations and parameter animation independently of the study samples.
