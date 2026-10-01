# Surface terminal

This example places a terminal-like Canvas on a 3D Surface. The Surface stays in its parent World; a `<CanvasWorld>` presents an attached child World whose canvas holds the keyed drawing, text, and bitmap entities. It includes font outlines, SVG drawings and an RGBA bitmap, and does not connect to a terminal process.

Run the strict Surface build, native lifecycle/persistence and worker rendering scenarios with `python tools/ipp.py regression --suite surfaces --only test:surface-cache:browser-surfaces`. The browser fixture supplies a Host and converted immutable assets to [the scene](scene.tsx). The same generated client contracts work with native Hosts. The Surface build has its own TypeScript target; GUI-owned fixture code and its existing scenarios remain under the separate `surface-gui-fixtures` build.

Canvas content uses logical units with a top-left origin, +X right and +Y down. Ordered entity children define paint order. Canvas styles and ordinary component fields support animation and direct field writes; the Surface anchor owns 3D placement.

A Surface presents directly unless its anchor entity also declares `<SurfaceCache>`, which opts it into [distance-based texture caching](../../docs/architecture/rendering.md#optional-surface-texture-caching). The [cache policy](../../crates/ipp-core/src/world/systems/surface/cache_policy.rs) documents the distance bands, limits and defaults.

Editable SVG sources live in [authoring/svg](authoring/svg) under [CC0](authoring/svg/CC0.txt). Fonts and their notices are maintained in [shared fonts](../../assets/fonts/README.md). The build downloads the pinned fonts automatically, converts the shared runtime font under `target/font-assets`, and writes this example's drawings and bitmap under `target/surface-assets`. [The format reference](../../crates/ipp-core/src/services/asset_management/SURFACE_FORMATS.md) defines the converter subset and approximation limits.

## Workload measurements

The positioned-text workload supports visible row/column counts, typing, blink, scrolling, full replacement and a sliding window of glyphs not shown before. It authors packed glyph rows as ordinary `CanvasGlyphRun` components. The asset build exports the printable ASCII and unseen glyph sets it draws from. See the [retained rendering measurements](../../tests/performance/retained-gui.md) for the maintained browser command, diagnostics and physical-device procedure.
